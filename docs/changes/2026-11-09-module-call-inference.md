# Module-qualified generic calls infer their type arguments

Audience: language users and compiler developers.

Generic inference for free functions (same day) did not cover calls through
an imported module such as `thread.spawn(|| square(n))`; the closure was
checked against an unbound `T` (`closure return type T is unavailable`).

The module call path now uses the same inference: closure arguments for
`fn(...) T` parameters are checked without an expected result, binders are
inferred from the positional arguments, the inferred arguments are recorded
on the call annotation, and codegen's module-call path selects that
specialization.

Verification: the `std` test `thread.spawned_threads_return_their_results`
and `std.thread.spawn`/`JoinHandle` programs natively, with `--jit`, and as
Windows COFF and NetBSD objects.
