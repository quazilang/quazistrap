# Generic free-function calls infer their type arguments

Audience: language users and compiler developers.

The language reference said generic calls infer their arguments, but a free
generic function without explicit arguments failed: `id(5 as i64)` for
`fn id[T](x: T) T` reported `arg 1: expected T, got i64`. Every example wrote
`max[i32](3, 7)`.

Analysis now infers each binder from the positional argument types
(`infer_type_subst`, which also matches `fn(...)` parameter and result types)
and checks the call as if the arguments were explicit. The inferred arguments
are recorded on the call's expression annotation (`inferred_type_args`), and
codegen selects that specialization. For a `fn(...) T` parameter whose `T` is
still unknown, a closure argument is checked without an expected result, so its
body's type infers `T`: `apply(|| square(n))`. This is the form the planned
`qo f(args)` lowering needs (D-019).

Not inferred (explicit arguments still required): binders that occur only in
the result or in named arguments, module-qualified calls (`module.f(...)`),
generic methods, and generic `@intrinsic`/`@api`/`@syscall` declarations,
which dispatch on their arguments directly.

Verification: semantic and codegen regressions (`id<i64>`/`apply<i64>`
specializations; a result-only binder is still rejected); all Rust tests, e2e
scripts, examples and the std suite pass.
