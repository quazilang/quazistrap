# Waiting qoroutines yield their worker

Audience: language users and compiler developers.

`join` inside a qoroutine spun its OS worker thread. With four workers, five
qoroutines waiting on each other (for example a recursive `qo` chain) blocked
every worker and the program hung.

Qoroutine stacks are now 256 KiB slots of one arena (1024 slots, 256 on
Windows); a slot's first word points to its task. Two intrinsics let runtime
code find the slot it runs on without thread-local storage:
`quazi.stack.pointer()` (id 45, rsp) and `quazi.ptr.offset_from(p, base)`
(id 46, `p - base` as `isize`). Pointer<->integer casts stay rejected, so no
pointer is rebuilt from an integer. `__qo_current()` returns the running task
or null, and `__qo_yield()` switches a qoroutine back to its worker, which
re-queues it; outside qoroutines it yields the OS thread. `join` waits with
`__qo_yield()`. The arena and the workers are initialized once (`init state`
0/1/2) before the first slot is taken.

Verification: `scripts/check_qoroutines.py` adds 20 chains of 40 nested joins
on four workers (hung before this change) next to the earlier cases; QZI
validation accepts intrinsic ids up to 46.
