# Lexical cleanup across loop exits

Audience: Quazi users and compiler developers.

`break` and `continue` now destroy active owners in the body scopes they exit,
in reverse declaration order. Previously they emitted only a jump: a native
program with a counting `free(self)` hook left the count at zero after `break`.
The pre-fix executable is retained locally as `/tmp/quazi-loop-cleanup-before`;
the isolated reproducer built successfully and exited 7 when its destructor
did not run.

Each loop now has a persistent cleanup scope for its initializer and any
compiler-owned iterable. Every emitted loop body records the depth immediately
inside that scope. An abrupt edge reads active cleanup actions from that depth
through the innermost scope without changing compile-time state. This preserves
cleanup for alternate branches. `continue` retains the persistent loop scope
and reaches the existing update/increment target; `break` and condition
exhaustion reach a common epilogue that cleans that scope once. Early return
already walks all enclosing scopes.

Block and loop exit restore lexical register/type bindings so a shadowed owner
does not replace its outer binding after the inner owner is destroyed. Return
generation restores its compile-time cleanup snapshot after emitting the
terminal path: transferring an initializer on a return path must not disable
cleanup on the zero-iteration path.

The borrow checker now checks C-style initialization once, at the ambient loop
and control depth, inside a dedicated lexical scope. Condition, body and update
are checked at the repeated depth. This permits a once-only initializer move,
rejects repeated consumption of an initializer owner, and ends initializer
loans when the loop ends. The existing immediate reassignment policy remains.
Scope removal precedes declaration-identity-based merging into the outer state.

The native return case exposed a second defect in constant propagation:
`Ret` did not end a basic block, so the return constant could flow through a
dead loop backedge and replace an unknown pointer parameter with zero. `Ret`
and `Trap` now terminate CFG blocks. Only blocks reachable from the callable
entry participate in propagation and rewriting, and only reachable predecessor
facts are merged. Both analysis and rewriting use the same input helper.
The callable-entry boundary marks all encoded registers as runtime-unknown,
including when block zero also has a live backedge. A backedge cannot invent a
constant for the first invocation.

**QZC20** rejects older exact and reusable caches. QZI13 and the physical ABI
are unchanged. Rebuild old QZI dependencies from source: native re-emission
cannot insert missing cleanup calls or recover already folded incorrect
constants. See [migration guidance](../migrations/loop-cleanup.md),
[scope cleanup](../language/safety.md#scope-cleanup-raii) and
[loop control](../language/statements.md#break-and-continue).

Verification uses the installed stable toolchain through the existing
`/tmp/quazi-named-rust-bin` ELF-loader wrappers, with
`RUSTC=/tmp/quazi-named-rust-bin/rustc` and
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`:

- `cargo test --offline -q`: **872 passed**, zero failed/ignored, including
  eleven new regressions. The four codegen and three semantic regressions
  failed before the cleanup/scope fix; the initializer call-order fixture was
  corrected to retain a foreign-call marker and then failed on the wrong order.
  Four separate constprop regressions failed before the optimizer fix.
- `cargo test --offline -q loop_`: **14 passed**; independently repeated by a
  read-only reviewer. `cargo test --offline -q bytecode::constprop::`:
  **16 passed** after the optimizer fix.
- `cargo build --offline -q` and
  `RUSTC_WORKSPACE_WRAPPER=/tmp/quazi-named-rust-bin/clippy-driver cargo check --offline -q`:
  pass with existing warnings.
- `python3 scripts/check_loop_cleanup.py --qz target/debug/qz --before-qz /tmp/quazi-loop-cleanup-before`:
  **23 destructor-trace cases** pass natively and from QZI after source removal;
  Windows x86-64 COFF emission passes. Cases cover reverse order, both branches,
  nested loops, cleanup before update, zero iterations, once-only moves,
  initializer loans, returned initializers, shadowing, ranges, fixed arrays,
  slices, transferred locals and supported structural cleanup. The old compiler
  builds the isolated cache fixture but exits 7; the new compiler misses that
  cache, fixes execution and runs the subsequent exact hit correctly.

The existing typed-propagation, expression-control, result-retention,
named-argument and dynamic-vtable/cache integration runners also pass. The
isolated STD memory checker passes six tests and ordinary quazifetch builds
and runs with the matching compiler.

Independent design/implementation and regression review covered both the cleanup
and CFG changes. Graphify queries/AST update, changed-region formatting and
Markdown links were checked. No performance benchmark or Windows-host execution
is claimed. Ordinary quazifetch and the isolated STD memory fixtures remain
smoke checks rather than a claim of full STD validation.

This completes the flagged abrupt-loop audit for supported initialized owners.
It does not complete path-dependent initialization, general structural
destruction, inferred interprocedural regions, retained artifact verification
or D-014/D-015. Named iterator paths still rejected by the frontend have not
gained source support. No new loan or QZI verification permission is enabled.
