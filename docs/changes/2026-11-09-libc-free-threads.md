# Linux threads without libc

Audience: language users and compiler developers.

`quazi.thread.spawn` / `spawn_arg` / `join` called `pthread_create` and
`pthread_join` on Linux, so any program using `std.thread` had to link libc
(`[link] libc = true`) and an external linker. Plain programs, which use the
built-in linker, and `qz run --jit` could not start threads at all. The M:N
qoroutine scheduler of [D-019](../decisions/qoroutines.md) needs OS worker
threads in exactly those programs.

Linux programs without `[link] libc` now get two embedded runtime routines
(`src/backend/x86_64/runtime.rs`), emitted only when used:

- `__quazi_thread_spawn(callback, arg)` maps an 8 MiB region (PROT_NONE guard
  page, stack, 64-byte control block) and calls `clone` with
  `CLONE_VM | FS | FILES | SIGHAND | THREAD | SYSVSEM | PARENT_SETTID |
  CHILD_CLEARTID`. The child calls `callback(arg)` on a 16-byte aligned stack
  and ends with the thread-only `exit`. The handle is the control block.
- `__quazi_thread_join(handle)` waits with `futex(FUTEX_WAIT)` until the kernel
  clears the thread-id word at thread exit, then unmaps the region. Handle 0
  stays a no-op.

`TargetSpec.libc` records `[link] libc`; with libc the encoder keeps the
pthread lowering, because a raw `clone` thread would share the main thread's
libc thread control block (errno, stdio locks). NetBSD and Windows are
unchanged. Returning from `main` uses `exit_group`, so it still ends threads
that are running.

Verification: `scripts/check_thread_atomics.py` now also builds the std
`Mutex`/`AtomicI64` contention program without libc (static, no `pthread`
reference; native and `qz run --jit`), runs 2000 spawn/join pairs while
checking the process's mapped pages (fails with about 4 million pages when
join does not unmap), and checks that `main` returning ends a spinning thread.
Encoder tests cover both lowerings.
