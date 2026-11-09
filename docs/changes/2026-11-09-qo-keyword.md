# `qo` starts qoroutines

Audience: language users and compiler developers.

[D-019](../decisions/qoroutines.md) decided on the keyword `qo` and the
built-in `Qoroutine[T]` type for green threads scheduled M:N on OS threads.

- **Syntax.** `qo f(args)` is a prefix expression whose operand must be a call;
  anything else is `E01`. The parser lowers it, like D-017 tail values, to
  `__qo_spawn(|| f(args))`. The synthetic callee, closure and call get distinct
  spans because analysis annotations are keyed by span.
- **Typing.** The prelude declares `__qo_spawn[T](f: fn() T) Qoroutine[T]`;
  generic inference from closure results ([change record](2026-11-09-generic-call-inference.md))
  gives `T`. `Qoroutine[T].join(self) T` waits and returns the result.
- **Runtime.** `prelude/src/qoroutine.qz` holds the scheduler (the reference
  prototype is `scripts/check_qoroutine_runtime.py`): state in
  `quazi.runtime.state`, a spin-locked run queue, a 256 KiB stack and a
  context per qoroutine entered with `quazi.context.switch`, and four worker
  threads (libc-free clone threads on Linux) started on first use. Workers
  publish completion only after leaving the qoroutine's stack, so `join` can
  free it.
- **Reachability fix.** Library (prelude/std) `@export` functions were always
  compiled, and a reference to an export used its bare table name while its
  body's call edges used the module-qualified name, so helpers called only by
  an export were dropped ("missing from the function table"). Library exports
  are now compiled only when reachable, the project's exports stay roots, and
  reachability follows from an export's table name to its body's edges.
  Programs that never use `qo` contain no scheduler code.

Limits (next D-019 phases): closure capture rules apply to the arguments
(plain scalars, evaluated in the qoroutine) and to the result; no preemption,
parking or channels yet; `join` yields its OS thread while waiting; a panic in
a qoroutine ends the program.

Verification: `scripts/check_qoroutines.py` (600 qoroutines over 4 workers,
5 runs, `--jit`, source-removed QZI, Windows COFF and NetBSD objects emit, an
unused scheduler is absent from QZI, non-call operand rejected) and a parser
regression for the lowering and spans.
