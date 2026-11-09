# `qo` moves owned arguments

Audience: language users and compiler developers.

`qo f(args)` lowers to `__qo_spawn(|| f(args))`, and closures may capture only
plain scalars, so `qo measure(text)` with a `String`, or any struct argument,
was rejected (`S10 closure cannot capture ...`).

The lowering's closure is now marked `once` in the AST (`ExprKind::Closure {
once: true }`): it is called exactly once and its body is the call itself.
For such closures:

- analysis accepts captured variables of named owned types (structs, enums,
  `String`); references and `str` views stay rejected because they cannot
  outlive the spawning frame;
- the borrow checker marks those captures moved at the `qo` expression, so a
  later use is `use of moved value`;
- codegen clears the outer owner's initialization flag when it copies the
  value into the closure environment, so only the callee frees it.

Ordinary closures keep the scalar-only rule.

Also fixed: the type-erased runner closure stored in each task through raw
memory was never released (one leaked mapping per qoroutine); the task entry
frees it after the run. The struct handle of `Qoroutine` itself still leaks,
like every struct without a `free` hook (general structural destruction is
pending).

Verification: `scripts/check_qoroutines.py` runs 1000 qoroutines with moved
`String` and struct arguments and checks the use-after-move diagnostic; a
semantic regression covers `qo` versus ordinary closures.
