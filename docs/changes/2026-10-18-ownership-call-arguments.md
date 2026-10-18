# Declaration-backed ownership call arguments

The semantic ownership graph now retains source argument storage facts for
each concrete caller. Previously its call sites retained targets and spans
without the operands needed by later ownership analysis. Snapshots preserve
declaration identities, storage types, receiver/positional/named roles,
parameter slots, field/dereference/builtin-index paths and deferred closures.
Result retention and snapshots share one declared-slot mapper, including
legacy receivers and variadic grouping.

Concrete caller substitutions apply to snapshot types. Global declaration
types resolve outside caller binders, including grouped and dereferenced
globals. Unknown storage, indirect targets and overloaded indexes remain
explicitly conservative. Budgets and cancellation bound the new analysis.
See the [implementation contract](../internals/ownership-call-arguments.md).

These are analysis inputs. Existing lexical loan permissions, runtime ABI,
source result-retention behavior, QZI v13 and QZC v14 remain unchanged. No source
migration is needed. Excessive argument evidence now produces an explicit S14
compiler-limit diagnostic. D-014 effects, structural destruction, D-015 element
loans and QZI certification remain open.

## Verification

The staged candidate was checked in a detached local worktree at
`/tmp/quazi-argument-facts-verify`, excluding the pre-existing, unfinished
raw-memory retention refinement. With the pinned stable toolchain in PATH,
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'` and
`CARGO_TARGET_DIR=/tmp/quazi-argument-facts-target`:

- `cargo test --offline semantic::places`: 18 focused tests pass.
- `cargo test --offline -q`: all 808 tests pass with a fresh temporary TMPDIR.
- `cargo build --offline -q`: passes with existing warnings.
- `cargo clippy --offline -q`: passes with existing warnings; the new pass's
  type-complexity warning was corrected.
- `python3 scripts/check_result_retention.py /tmp/quazi-argument-facts-target/debug/qz`:
  10 unsafe cases reject during check/build/QZI emission and six safe cases
  execute from source and QZI and emit Windows COFF.
- `python3 scripts/check_dynamic_vtables.py /tmp/quazi-argument-facts-target/debug/qz`:
  source/QZI native execution, Windows COFF, incremental cache paths and a
  source-removed library pass.
- Owned new Rust files and ownership graph integration are rustfmt-clean;
  `git diff --cached --check` passes.

Two read-only reviewers challenged declaration context, temporary projections,
copy budgets and retention regressions. Their findings were reconciled. The
initial inherited depth test overflowed the recursive parser's test-thread
stack; its replacement constructs the deep operand after ordinary analysis and
tests the new pass's S14 boundary directly. No parser recursion guarantee is
claimed. Reinstating the pre-review expression typing and temporary projection
behavior makes both new regression tests fail; restoring the fixes passes the
full suite. Evidence is `/tmp/quazi-argument-facts-before-fixes.log`.

The separate raw-memory refinement is preserved unstaged. Review proved it
loses parameter retention when `malloc` is called through a source helper and
`memcpy` subsequently writes a view through that pointer. The same regression
passes with the committed retention semantics and fails with that refinement.
It is not part of this checkpoint; do not infer full-worktree correctness from
the isolated candidate's tests.

Graphify query and AST update were used. The refreshed local graph contains
4,878 nodes and 10,731 edges; it excludes Quazi
source semantics. Its diagnostic reports 63 dangling endpoint edges and 40
undirected same-endpoint collisions. The graph remains a navigation aid.
No full STD, editor-client or Windows-host execution claim is made.
