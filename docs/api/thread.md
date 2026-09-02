# `std.thread`

Audience: language users.

Status: experimental. The callback ABI and zero-on-failure low-level contract
are implemented; typed creation errors, panic propagation, cancellation, and
structured concurrency are not yet available.

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

Returns an opaque nonzero handle on success and `0` on failure. Linux frees its
temporary `pthread_t` storage when `pthread_create` fails. Windows returns the
null handle produced by `CreateThread`.

The operation is unsafe because the caller must uphold the callback ABI,
lifetime, shared-state, and synchronization requirements. The returned zero
value may be passed to `thread_join`, where it is handled as a no-op.

## `unsafe fn thread_join(handle: usize) void`

Waits for a successful thread handle to finish. On Linux it then frees the
temporary `pthread_t` storage; on Windows it closes the native thread handle.

Handle `0` is accepted as a no-op so a failed spawn cannot trigger a null-handle
crash. Any nonzero handle must have been returned by `thread_spawn`, must belong
to the current process, and must not have been joined already. Violating those
nonzero-handle requirements is invalid use of this experimental unsafe API.

## `Thread`

`unsafe Thread.spawn(f)` returns `Result[Thread, ThreadError]`. A native
creation failure is `Err(ThreadError.CreationFailed)`; no zero-handle `Thread`
is exposed through this safe wrapper. `unsafe Thread.join(self)` consumes a
successful handle, and `handle()` exposes its opaque nonzero value for
low-level interoperability.

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
