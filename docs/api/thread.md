# `std.thread`

Audience: language users.

Status: experimental. The callback ABI and zero-on-failure low-level contract
are implemented. `spawn` returns a `JoinHandle[T]` that follows the D-007
contract chosen in maintainer answer В29А: `join` yields the result or
`ThreadError.Panicked`, a dropped handle waits for its thread, and
cancellation is cooperative. Moving owned values into threads and channels
between threads still depend on
[D-014](../decisions/whole-program-ownership.md).

## Callback type

`ThreadCallback` is the `@repr(C)` function-pointer alias `fn(*u8) *u8` on
every target (Windows uses the low 32 bits of the result as the exit code), so
a QZI can share the same caller code across targets.

The callback must be a signature-compatible `@export` function. Capturing
closures and ordinary Quazi function values do not use the operating system's
C callback representation and cannot be passed as thread callbacks.

## `unsafe fn thread_spawn(f: ThreadCallback) usize`

Starts one native operating-system thread. The callback receives a null context
pointer; `thread_spawn_arg(f, arg)` passes `arg` instead.

Returns an opaque nonzero handle on success and `0` on failure. On Linux
without `[link] libc`, the embedded runtime starts the thread with `clone` on
its own 8 MiB stack (no libc is needed, so threads also work with the built-in
linker and `qz run --jit`). With `libc = true`, Linux uses `pthread_create` and
frees its temporary `pthread_t` storage if creation fails. Windows returns the
null handle produced by `CreateThread`.

The operation is unsafe because the caller must uphold the callback ABI,
lifetime, shared-state, and synchronization requirements. The returned zero
value may be passed to `thread_join`, where it is handled as a no-op.

## `unsafe fn thread_join(handle: usize) void`

Waits for a successful thread handle to finish. On Linux it then releases the
thread's stack (embedded runtime) or its temporary `pthread_t` storage (libc);
on Windows it closes the native thread handle. Returning from `main` ends the
whole process, including threads that are still running.

Handle `0` is accepted as a no-op so a failed spawn cannot trigger a null-handle
crash. Any nonzero handle must have been returned by `thread_spawn`, must belong
to the current process, and must not have been joined already. Violating those
nonzero-handle requirements is invalid use of this experimental unsafe API.

## `Thread`

`unsafe Thread.spawn(f)` returns `Result[Thread, ThreadError]`. A native
creation failure is `Err(ThreadError.CreationFailed)`; no zero-handle `Thread`
is exposed through this safe wrapper. `unsafe Thread.join(self)` invalidates
its stored handle before performing the native join, so calling it again is a
no-op through the zero-handle low-level contract. `handle()` exposes the opaque
nonzero value for low-level interoperability until the wrapper is joined; using
an extracted handle remains an unsafe low-level operation.

## Scheduling helpers

- `sleep(ms: u64)` suspends the current thread for approximately the requested
  number of milliseconds using `std.core`.
- `yield_now()` asks the operating-system scheduler to yield the current thread:
  Linux and NetBSD use `sched_yield`, while Windows uses `Sleep(0)`. It is not a
  synchronization primitive.
- `current_tid()` returns the kernel thread identifier: Linux `gettid`,
  NetBSD `_lwp_self`; Windows returns `-1`.

## Unsupported behavior

The module does not currently provide join timeouts, detached threads,
forced (non-cooperative) cancellation, or portable thread identifiers. Programs requiring those guarantees should
not treat this API as production-stable.

## `spawn` and `JoinHandle[T]` (D-019)

`thread.spawn(|| work(n))` runs a Quazi closure on a new OS thread and returns
`Result[JoinHandle[T], ThreadError]` (`CreationFailed` if the thread cannot be
started); `T` is inferred from the closure. `handle.join()` waits and returns
`Result[T, ThreadError]`:

```quazi
import std.thread;

fn square(n: i64) i64 { n * n }

fn main() i32 {
    const n: i64 = 7;
    const handle = thread.spawn(|| square(n)).unwrap();
    const value = handle.join().unwrap(); // 49
    0
}
```

The closure follows the ordinary capture rules (plain scalar captures and a
plain scalar result). On Linux without libc the thread is a `clone` thread;
this works with the built-in linker and `qz run --jit`.

### Panics

A panic on a `spawn`ed thread is reported (`thread panicked at '...'` and the
location) and ends only that thread; `join()` then returns
`Err(ThreadError.Panicked)`. The thread's block, closure and native stack are
still released by `join`. Quazi does not unwind: destructors pending in the
frames the panic abandoned do not run, so values they owned leak. A custom
`@panic_handler` is not called for such a panic; it runs only for fatal
panics (main thread, raw `thread_spawn` callbacks, qoroutine workers), which
still end the process ([panic](../language/panic.md)). A panic raised while a
thread reports its own panic is fatal.

### Dropping a handle

A `JoinHandle` that goes out of scope without `join()` waits for its thread
(`JoinHandle.free`); the result of a finished thread is dropped. There are no
detached threads.

### Cooperative cancellation

`handle.cancel()` sets a flag that code running on that thread reads with
`thread.cancel_requested()`; the thread chooses where to check and how to
finish, and `join` still waits for it. `cancel_requested()` is false on any
thread not started by `spawn`.

```quazi
import std.thread;

fn poll() i32 {
    for (!thread.cancel_requested()) { thread.yield_now(); }
    0
}

fn main() i32 {
    const worker = thread.spawn(|| poll()).unwrap();
    worker.cancel();
    ret worker.join().unwrap();
}
```

The isolation, drop and cancellation behavior is checked by
`std/scripts/check_thread_isolation.py` (clone runtime, pthreads, `--jit`,
concurrent panics, channels); Windows and NetBSD build but are not run here.

### Communication

The prelude's `Channel[T]` ([reference](../reference/prelude/qoroutine.md#channel))
is the communication surface between threads, as between qoroutines: create
it with `Channel.new(capacity)`, give each thread its own `clone()` (moved
into the closure, which borrows it as `&Channel[T]` because a closure may run
more than once), `send` owned values and `receive` until `None` after
`close()`. A blocked `send`/`receive` on an OS thread yields its time slice.
The channel is freed when the last clone is dropped.

```quazi
import std.thread;

fn produce(out: &Channel[i64]) i64 {
    for i : 0..100 { out.send(i as i64); }
    100
}

fn main() i32 {
    const channel: Channel[i64] = Channel.new(8);
    const sender = channel.clone();
    const worker = thread.spawn(|| produce(&sender)).unwrap();
    var received: i64 = 0;
    for (received < 100) {
        channel.receive().unwrap();
        received = received + 1;
    }
    ret worker.join().unwrap() as i32 - 100;
}
```

Values still inside a channel or captured by a thread that panicked are
leaked, not destroyed (no unwinding).

## Synchronization (D-019)

`AtomicI64.new(value)` owns a heap cell whose operations are atomic and
sequentially consistent: `load`, `store`, `swap`, `fetch_add` (returns the
previous value) and `compare_exchange(expected, desired)` (stores only on a
match and returns the value it saw). `Mutex.new()` is a non-reentrant,
non-fair spin lock that yields the processor while waiting: `lock`,
`try_lock` (true on success) and `unlock` (panics if the lock is not held).

Both free their cell when the owner goes out of scope. Moving owned values
into threads waits for D-014, so share them by passing `unsafe as_ptr()` to a
thread (for example with `thread_spawn_arg`) and opening it there with
`unsafe AtomicI64.at(ptr)` / `Mutex.at(ptr)`, which return non-owning views
with the same operations. Join every thread that uses a view before the owner
is dropped.

`thread_spawn_arg(callback, arg)` passes `arg` to the C-ABI callback instead of
`NULL`. See [D-019](../decisions/qoroutines.md) for `qo` qoroutines and
`Channel[T]`.
