# D-019: qoroutines, channels and OS threads

Audience: language users and compiler developers.

Status: maintainer discussions of 2026-11-08 (rounds 1 and 2) recorded; decided
points are marked **decided**, design proposals are marked **proposed**.

## Decided

| # | Topic | Decision |
| --- | --- | --- |
| 1 | Starting a qoroutine | **decided**: keyword `qo`, e.g. `qo worker(4);`. No parallel `std.qroutines.spawn` API: one way to do it. |
| 2 | Channel operations | **decided**: methods `channel.send(value)` / `channel.receive()`. No `<-` / `->` operators. |
| 3 | `select` | **decided** (leaning): not in the first version. |
| 4 | Scheduler | **decided**: M:N — qoroutines run on a pool of OS worker threads. |
| 6 | Waiting | **decided**: `qo` yields a handle; `handle.join()` waits and returns the result. |
| 7 | `main` returns while qoroutines run | **decided**: the program exits; running qoroutines stop (as in Go). |
| 9 | Passing data | **decided**: owned values can be moved into a qoroutine. |
| 10a | OS threads module | **decided**: keep the name `std.thread` (no `std.threading`). |
| 10b | OS thread API | **decided**: `thread.spawn(...) -> JoinHandle[T]`, `handle.join() -> Result[T, ThreadError]`; the unsafe C-callback layer stays underneath. |
| 10c | OS thread synchronization | **decided**: `Mutex`, `AtomicI64`, and channels. |
| 11 | Names | **decided**: built-in types `Qoroutine[T]` and `Channel[T]` (spelling `Qoroutine`, round 2 В1). |
| 8 | Panics | **decided** (round 2 В2): a panic inside a qoroutine is reported to the joiner as an error (`join()` returns a `Result`), not a whole-program abort. |
| 5 | Preemption | **decided** (round 2 В3): full timer-based preemption, not only compiler safe points. |
| 12 | Closures | **decided** (round 2 В4): `qo || ...` is allowed once closures can move owned captures. |
| 13 | Channel type | **decided** (round 2 В6): OS threads and qoroutines share the one built-in `Channel[T]`. |
| 14 | JIT | **decided** (round 2 В5): a real minimal JIT, experimental behind `qz run --jit`; a later API may import/load code dynamically (example idea: `target = "xtensa32-esp32"`, "dynamic import"). |

## Proposed design (reconciles "compiler = base, std = library" with "built-in `qo`")

- **Where it lives.** The keyword `qo`, the scheduler runtime and the types
  `Qoroutine[T]` and `Channel[T]` ship with the compiler: parser + semantic
  analysis for `qo`, the types and scheduler in the compiler's prelude/runtime
  (like `String`, `Array`, `Option`). `std` does not depend on them and the
  compiler does not depend on `std`. There is no second spawn API.
- **`qo` semantics.** The operand must be a call: `qo f(a, b)`. Arguments are
  evaluated and moved exactly like an ordinary call, in the spawning context;
  the call then runs in a new qoroutine. The expression has type
  `Qoroutine[T]` where `T` is `f`'s return type; `join()` blocks until it
  finishes and returns `T`. Because arguments move like call arguments, owned
  values (`String`, structs) reach the qoroutine without first extending
  closure captures; closure capture moves (answer 9) remain useful for
  `qo || ...` and for `thread.spawn`.
- **One `Channel[T]`.** The same thread-safe channel serves qoroutines and OS
  threads (10c): `Channel[T].new(capacity)`, `send`, `receive` (returns
  `Option[T]`, `None` once closed and drained), `close`. A blocking operation
  parks only the current qoroutine, not its worker thread.
- **Preemption (decided: timer-based).** A per-worker timer interrupts a
  qoroutine that exceeds its time slice: a signal handler (Linux/NetBSD) or a
  suspended-thread context switch (Windows) saves the full register state
  and resumes the scheduler. Quazi has no moving GC and VM registers live in
  the frame, so any instruction outside runtime critical sections is a safe
  interruption point; critical sections (scheduler, allocator, channel locks)
  mask preemption.
- **Panics (decided: reported through `join()`).** Proposed mechanism without
  unwinding: each qoroutine owns its stack, so the panic path records the
  message on the qoroutine, abandons that stack (its pending destructors do
  not run; the leak is documented) and switches to the scheduler; `join()`
  then returns `Err(QoroutinePanic)`. A panic outside any qoroutine still ends
  the program.

## Implementation phases

1. Runtime groundwork (done): atomics, `quazi.context.switch`,
   `quazi.thread.spawn_arg`.
2. `std.thread`: `JoinHandle[T]`, `Mutex`, `AtomicI64` on the atomics.
3. Runtime scheduler: per-qoroutine stacks, global run queue, worker pool,
   parking/unparking, `Channel[T]`.
4. Language: `qo` keyword (lexer, parser, Tree-sitter, LSP), typing to
   `Qoroutine[T]`, lowering to the runtime spawn with moved arguments.
5. Safe-point preemption, then closure capture moves (answer 9).
