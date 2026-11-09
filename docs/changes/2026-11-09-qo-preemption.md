# Timer preemption for qoroutines (Linux)

Audience: language users and compiler developers.

D-019 round 2 (В3Б) decided on full timer-based preemption. A qoroutine that
loops without waiting used to keep its worker forever; with four workers,
four such loops starved every other qoroutine.

- Every worker arms a `timer_create(CLOCK_MONOTONIC, SIGEV_THREAD_ID)` timer
  that sends it SIGURG every 10 ms.
- `quazi.signal.install` (intrinsic 47) installs the prelude handler
  `__qo_preempt` with raw `rt_sigaction` and the embedded `__quazi_sigreturn`
  restorer. The handler runs on the interrupted qoroutine's stack; unless the
  qoroutine holds a preemption hold, it switches to the worker from inside
  the signal frame. The worker re-queues the task and unblocks SIGURG. When
  the task resumes (possibly on another worker), the handler returns and
  `rt_sigreturn` restores every register and the signal mask.
- Holds: runtime and channel locks, voluntary switches and the preempting
  switch take a hold, and fresh tasks start with one that the task entry
  drops. A task that is not running therefore always holds one, so a signal
  in the instruction window of the switch into a task cannot re-enter it.
- `quazi.sleep_ms` on Linux now retries `nanosleep` with the remaining time
  after `EINTR`; before, a preemption tick cut sleeps short.
- Not preempted: Windows and NetBSD (no signal-based mechanism yet) and
  executables linking libc (`quazi.signal.install` returns -1 there), because
  a preempted libc call could deadlock on libc locks or move its thread state
  to another worker. Those schedule cooperatively.

An independent review found the switch-in race, the shortened sleeps, the
libc hazard and unprotected channel locks before commit; all are fixed.

Verification: `scripts/check_qoroutines.py` runs eight qoroutines that spin
without yielding until all eight have started (impossible on four workers
without preemption; 5 native runs and `--jit`, plus 30 extra local runs),
a 200 ms sleep inside a qoroutine, and nested joins in a libc build with
pthread workers.
