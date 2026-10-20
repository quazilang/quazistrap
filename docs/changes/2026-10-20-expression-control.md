# Shared expression control and canonical logical bools

Audience: language users and compiler developers.

`&&` and `||` now produce canonical boolean results for integer operands.
Previously, an evaluated RHS was copied directly: `logical_and(2, 3)` could
produce a Bool-typed value unequal to `true`, while the equivalent boolean
operation returned `true`. Shortcut paths already produced 0/1. Both paths
now obey the same boolean contract and preserve lazy evaluation. See
[migration notes](../migrations/logical-bool-results.md).

`src/evaluation.rs` supplies operand-control classification consumed by codegen
and the [source control-flow graph](../internals/source-control-flow.md).
Grouping, casts and unary operations evaluate a child once; eager binary
operations evaluate left before right. Short-circuit operators branch with
shared polarity. An exposed `?` has an operand evaluation, a success-discriminant
branch, payload extraction and a failure return with source scope exits.

Result events distinguish the enclosing expression from its operands. Unary
`!` is therefore not confused with its child's truth value, and logical paths
define the same expression identity through normalization or a shortcut constant.
The iterative builder does not rescan completed wrappers for duplicate failure
edges. Opaque calls, assignments, fields/indexes, match expressions, aggregates,
increment/decrement and multi-input iteration regions retain conservative
nested-`?` scanning. Closure bodies remain separate deferred graphs.

This advances D-014 control evidence. It does not establish typed accesses,
aliases, moves, escapes, destruction effects or artifact correctness. Implicit
automatic reads and runtime operations remain unverified. Try failure wrapper
compatibility with the enclosing callable remains a separate type-checking
audit; these graph edges do not certify it. No new ownership permission is
enabled. QZI13 remains Unverified; QZC18 rejects older exact hits and reusable
chunks so source caches cannot retain the logical-result bug.

Verification uses the existing stable toolchain through temporary ELF-loader
wrappers in `/tmp/quazi-named-rust-bin`, with
`RUSTC=/tmp/quazi-named-rust-bin/rustc` and
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`:

- `cargo test --offline -q`: 854 passed. Five new flow tests cover skipped RHS
  failure paths, eager failure cutoff, enclosing result identities, opaque and
  deferred boundaries, iterative expansion, budgets and cancellation. Existing
  loop-region tests now cover explicit as well as conservative failure edges.
- `cargo test --offline -q lsp::`: 60 passed, including the three failures in
  the older README progress audit. This does not prove real-client coverage.
- `cargo build --offline -q`: passes with existing warnings.
- `RUSTC_WORKSPACE_WRAPPER=/tmp/quazi-named-rust-bin/clippy-driver cargo check --offline -q`:
  passes with existing warnings; the direct driver avoids the unavailable
  original Nix loader used by Cargo-Clippy subprocess startup.
- `python3 scripts/check_expression_control.py --qz target/debug/qz --before-qz /tmp/quazi-expression-control-before`:
  the preceding compiler builds the valid fixture but exits 3 at the canonical
  Bool comparison. The new compiler invalidates its source cache and passes
  both the rebuilt source and subsequent exact hit. All 36 checks pass through
  native execution, QZI-only execution and Windows COFF emission. Cases include
  integer booleans, closures, lazy Result/Option propagation, eager operand
  cutoff, wrappers and opaque named-call arguments.
- Existing named-argument, result-retention and dynamic-vtable/cache integration
  runners pass. Their source/QZI native and Windows COFF paths remain covered.
- Independent read-only design and implementation review found no blocking
  control defect and required the included QZC invalidation. New Rust-file
  formatting, changed-region formatting and `git diff --check` pass.

No Windows-host execution, full STD validation, completed D-014/D-015 or overall
workspace readiness is claimed. No STD or editor sources change here.
