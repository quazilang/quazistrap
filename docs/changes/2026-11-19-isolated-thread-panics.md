# Isolated thread panics, joining on drop, cooperative cancellation

Date: 2026-11-19

Audience: Quazi users and compiler developers.

## Behavior (D-007, maintainer answer В29А)

A panic on an OS thread started by `std.thread.spawn` no longer ends the
process. It is reported (`thread panicked at '...'`), only that thread ends,
and `join()` returns `Err(ThreadError.Panicked)`; `ThreadError` gained the
`Panicked` variant, so exhaustive matches on it need an arm for it. The custom
`@panic_handler` runs only for fatal panics. A `JoinHandle` dropped without
`join()` waits for its thread. `JoinHandle.cancel()` sets a flag that the
thread reads with `thread.cancel_requested()`. See [std.thread](../api/thread.md).

## Mechanism

- New intrinsic `quazi.thread.current() u64` (id 69): a nonzero id of the
  calling OS thread (Linux `gettid`, Windows TEB `ClientId.UniqueThread`,
  NetBSD `_lwp_self`).
- Prelude `__qo_isolate(entry, arg, tag)` captures the thread's machine state
  with a no-op `quazi.context.switch`, aims a copy at `entry(arg)` on the same
  OS stack 512 bytes below its frame (so the thread keeps its guard page), and
  registers a record keyed by the thread id in the runtime state (+72 lock,
  +80 list head). `panic` asks `__qo_panic_is_isolated()`; on an isolated
  thread it reports and switches back to the isolation point, which returns
  true. `__qo_isolation_arg(tag)` gives the innermost isolation's argument to
  its owner (std uses it for `cancel_requested`).
- `std.thread` runs every spawned closure through `__qo_isolate`.

Verified by `std/scripts/check_thread_isolation.py` with the clone runtime,
pthreads and `qz run --jit`, including 1200 concurrent panics; Windows and
NetBSD objects build but were not run (no Wine/NetBSD here).
