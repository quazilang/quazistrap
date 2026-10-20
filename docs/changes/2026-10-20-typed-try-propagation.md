# Typed failure propagation and wrapper aliases

Audience: language users and compiler developers.

`?` now checks its failure return against the nearest function or closure.
Previously, a function returning `i32` could evaluate a failing
`Result[i32, i32]?` and return an enum-object address as an integer. Different
Result error types were also accepted even though lowering copies the error
payload without conversion. Both cases now produce `S14` during analysis.

`Option[T]?` requires `Option[U]`; `Result[T, E]?` requires `Result[U, E]`.
The success types may differ. Error compatibility uses bidirectional structural
runtime-shape checking after checked alias resolution, rather than ordinary
assignment compatibility. This rejects numeric conversions, changed pointer
pointees and unrelated generic binders. The nearest callable return context is
replaced for every closure, including an inferred closure with an unknown return
type, then restored after body checking. Function checking restores it before
propagating cancellation; analyzer reuse clears it.

Untyped qualified constructor operands now receive a controlled diagnostic.
Their result type has not yet been resolved, so accepting them could make
lowering guess the wrong wrapper. The existing closure-result ownership gate
still rejects wrapper-returning closures. See the
[language contract](../language/expressions.md#error-propagation) and
[migration guidance](../migrations/typed-try-propagation.md).

The native alias fixture also exposed `match` rejecting an alias such as
`type Output = Result[bool, i32]` as an unknown enum. Match now resolves the
whole scrutinee before reference peeling, pattern validation, payload typing
and exhaustiveness. Borrowed-enum detection uses the same resolved type, so an
alias to a reference permits discriminant inspection while still rejecting
payload projection. Existing codegen already resolves those aliases.

No ABI or bytecode representation changes. **QZC19** rejects older exact and
reusable caches so invalid source cannot bypass mandatory analysis. The cache
regression first compiles invalid source using the preceding compiler, removes
only its temporary binary, then verifies that the new compiler misses that
same cache and rejects the source. A corrected source and its subsequent exact
hit both execute successfully. Old QZI artifacts are not re-typechecked by this
change; their ownership envelopes remain Unverified. No artifact certificate,
complete destruction proof or new loan permission is introduced.

Verification uses the installed stable toolchain through the existing
`/tmp/quazi-named-rust-bin` ELF-loader wrappers, with
`RUSTC=/tmp/quazi-named-rust-bin/rustc` and
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`:

- `cargo test --offline -q`: **861 passed**. Seven new tests cover callable
  return wrappers, invariant errors, closure boundaries and restoration,
  aliases/generic binders/methods, unknown operands and alias match restrictions.
  The five initial propagation tests failed against the preceding implementation;
  the alias-match regression separately failed before normalization.
- `cargo build --offline -q`: passes with existing warnings.
- `RUSTC_WORKSPACE_WRAPPER=/tmp/quazi-named-rust-bin/clippy-driver cargo check --offline -q`:
  passes with existing warnings.
- `python3 scripts/check_try_propagation.py --qz target/debug/qz --before-qz /tmp/quazi-try-before`:
  nine invalid programs rejected by `check` and before native/QZI/COFF emission;
  eight success/failure/discriminant runtime checks pass natively and from QZI
  with source removed. Windows x86-64 COFF emission and old-cache rejection pass.
- `python3 scripts/check_expression_control.py --qz target/debug/qz`:
  all 36 expression-control checks pass through native/QZI/COFF paths.
- `python3 scripts/check_result_retention.py target/debug/qz`:
  12 unsafe programs rejected and seven valid programs execute successfully.
- STD `python3 scripts/check_os_memory.py <matching-qz>`: six tests pass;
  ordinary quazifetch `qz build` with the matching compiler passes.
- Independent read-only design and implementation review, Graphify queries and
  AST update, changed-region Rust formatting, Markdown links and
  `git diff --check` were checked.

Windows-host execution, complete D-014/D-015, full STD validation and overall
workspace readiness remain outside this checkpoint. Abrupt-loop cleanup is
still an open native-runtime audit. STD and editor sources are unchanged.
