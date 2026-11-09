# `std.thread`

Audience: language users.

Status: experimental. The callback ABI and zero-on-failure low-level contract
are implemented; safe concurrency is blocked on implementation of
[D-014](../decisions/whole-program-ownership.md). Typed creation errors, panic
propagation, cancellation, and structured concurrency are not yet available.

## Callback type

`ThreadCallback` is a target-specific `@repr(C)` function-pointer alias:

- Linux: `fn(*u8) *u8`
- Windows: `fn(*u8) u32`

The callback must be a signature-compatible `@export` function. Capturing
closures and ordinary Quazi function values do not use the operating system's
C callback representation and cannot be passed as thread callbacks.

## `unsafe fn thread_spawn(f: ThreadCallback) usize`

Starts one native operating-system thread. The callback receives a null context
pointer because this provisional API does not yet accept user context.

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
  Linux uses `sched_yield`, while Windows uses `Sleep(0)`. It is not a
  synchronization primitive.
- `current_tid()` returns the Linux kernel thread identifier. Despite its
  current public shape, this helper is not portable: it returns `-1` on
  Windows rather than issuing a Linux syscall. Portable thread identifiers are
  not yet provided.

## Unsupported behavior

The module does not currently provide cancellation, synchronization primitives,
result or panic propagation, join timeouts, detached threads, portable thread
identifiers, or automatic joining. Programs requiring those guarantees should
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
plain scalar result). A panic on the thread ends the program. On Linux without
libc the thread is a `clone` thread; this works with the built-in linker and
`qz run --jit`. Channels between threads and qoroutines are planned on the
same built-in `Channel[T]`.

## Synchronization (D-019)

`AtomicI64.new(value)` owns a heap cell whose operations are atomic and
sequentially consistent: `load`, `store`, `swap`, `fetch_add` (returns the
previous value) and `compare_exchange(expected, desired)` (stores only on a
match and returns the value it saw). `Mutex.new()` is a non-reentrant,
non-fair spin lock that yields the processor while waiting: `lock`,
`try_lock` (true on success) and `unlock` (panics if the lock is not held).

Both free their cell when the owner goes out of scope. Until `JoinHandle[T]`
can move values into threads, share them by passing `unsafe as_ptr()` to a
thread (for example with `thread_spawn_arg`) and opening it there with
`unsafe AtomicI64.at(ptr)` / `Mutex.at(ptr)`, which return non-owning views
with the same operations. Join every thread that uses a view before the owner
is dropped.

`thread_spawn_arg(callback, arg)` passes `arg` to the C-ABI callback instead of
`NULL`. See [D-019](../decisions/qoroutines.md) for the planned `JoinHandle[T]`,
`qo` qoroutines and `Channel[T]`.
