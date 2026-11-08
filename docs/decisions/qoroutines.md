# D-019: qoroutines, channels and OS threads

Audience: language users and compiler developers.

Status: maintainer discussion of 2026-11-08 recorded; decided points are marked
**decided**, recommendations still awaiting confirmation are marked
**proposed** (open questions are listed in the workspace `asks.txt`).

## Decided

| # | Topic | Decision |
| --- | --- | --- |
| 1 | Starting a qoroutine | **decided**: keyword `qo`, e.g. `qo worker(4);`. No parallel `std.qroutines.spawn` API: one way to do it. |
| 2 | Channel operations | **decided**: methods `channel.send(value)` / `channel.receive()`. No `<-` / `->` operators. |
| 3 | `select` | **decided** (leaning): not in the first version. |
| 4 | Scheduler | **decided**: M:N — qoroutines run on a pool of OS worker threads. |
| 5 | Preemption | **decided** (tentative): preemptive, see the proposal below. |
| 6 | Waiting | **decided**: `qo` yields a handle; `handle.join()` waits and returns the result. |
| 7 | `main` returns while qoroutines run | **decided**: the program exits; running qoroutines stop (as in Go). |
| 9 | Passing data | **decided**: owned values can be moved into a qoroutine. |
| 10a | OS threads module | **decided**: keep the name `std.thread` (no `std.threading`). |
| 10b | OS thread API | **decided**: `thread.spawn(...) -> JoinHandle[T]`, `handle.join() -> Result[T, ThreadError]`; the unsafe C-callback layer stays underneath. |
| 10c | OS thread synchronization | **decided**: `Mutex`, `AtomicI64`, and channels. |
| 11 | Names | **decided**: types `Qoroutine[T]` and `Channel[T]` are built in (spelling `Qoroutine` vs `Qroutine` to confirm). |

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
- **Preemption.** Recommended staging: (1) the compiler inserts cheap
  safe-point checks at loop back-edges and function entries, so a busy loop
  yields when its time slice expires; (2) later, if needed, timer/signal based
  asynchronous preemption. Stage 1 gives fairness without signal handling and
  works on Linux, NetBSD and Windows alike.
- **Panics (open, answer 8).** Recommendation: a panic in any qoroutine ends
  the whole program with the usual panic report (Go's behavior). Returning it
  from `join()` would require unwinding, which Quazi does not implement.

## Implementation phases

1. Runtime groundwork (done): atomics, `quazi.context.switch`,
   `quazi.thread.spawn_arg`.
2. `std.thread`: `JoinHandle[T]`, `Mutex`, `AtomicI64` on the atomics.
3. Runtime scheduler: per-qoroutine stacks, global run queue, worker pool,
   parking/unparking, `Channel[T]`.
4. Language: `qo` keyword (lexer, parser, Tree-sitter, LSP), typing to
   `Qoroutine[T]`, lowering to the runtime spawn with moved arguments.
5. Safe-point preemption, then closure capture moves (answer 9).
