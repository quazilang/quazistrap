# Source statement control-flow foundation

Audience: language and tooling developers.

Ownership analysis now retains a bounded graph for each active source callable
body, including leaves without calls. Concrete specializations share their
declaration's graph. It distinguishes structured branches, loop setup and
back edges, C-style updates, abrupt transfers, lexical scope exits and deferred
closure bodies. A structural validator rejects broken graph construction before
publication. See [the canonical contract](../internals/source-control-flow.md).

The previous argument walker visits syntax without representing execution
paths. Reusing that order would misrepresent early returns, loop updates,
named arguments and assignment operands. Existing bytecode control flow also
lacks the declaration identities needed for source ownership. The new graph
therefore records statement structure separately, leaving whole expression
regions opaque. A possible `?` failure has a conservative return edge, with
closure bodies isolated from the enclosing callable's return paths.

This is a prerequisite of D-014, not its completed effect solver. It grants no
new borrow, escape, move, destruction, indirect-call or artifact permissions.
Typed memory accesses, normalized expression sequencing, alias flow and effect
composition remain outstanding. Public structural validation does not verify
expression effects. Resource counters bound flow work and retained metadata;
they do not bound all frontend allocations, including canonical-name helpers.

Construction is iterative, cooperatively cancellable and globally budgeted.
S14 reports exhaustion and no partial flow map is published. The default scope
and deferred-closure depth limit is 64. Opaque expression scanning has no
separate depth limit; a 200-node arithmetic chain remains accepted by this pass.
Programs exceeding the documented resource limits may now receive S14.
QZI13 remains Unverified. QZC17 invalidates older exact hits and reusable units
so accepted source is rebuilt through the new mandatory pass.

Verification uses the existing stable Rust toolchain through
`/tmp/quazi-named-rust-bin` ELF-loader wrappers, with
`RUSTC=/tmp/quazi-named-rust-bin/rustc` and
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`:

- `cargo test --offline -q`: 849 passed. Fourteen new flow tests cover abrupt
  termination, else-if chaining, nested-loop targets, scope exits, loop-region
  `?`, deferred closures, cfg filtering, graph corruption, fan-out/deferred
  budgets, cancellation and deep iterative scanning. A further integration
  test checks source leaves, unavailable declarations and shared generic graphs.
- `cargo test --offline -q source_control_flow`: 14 passed after final
  structural-validator cleanup.
- `cargo build --offline -q`: passes with existing warnings.
- `RUSTC_WORKSPACE_WRAPPER=/tmp/quazi-named-rust-bin/clippy-driver cargo check --offline -q`:
  Clippy passes with existing warnings. The direct driver wrapper avoids the
  unavailable Nix loader used by normal Cargo-Clippy subprocess startup.
- `python3 scripts/check_named_arguments.py --qz target/debug/qz`: native,
  QZI-only, Windows COFF and source-removed aliased QZI imports pass.
- `python3 scripts/check_result_retention.py target/debug/qz`: 12 rejected and
  seven valid cases pass, including source/QZI execution and Windows COFF.
- `python3 scripts/check_dynamic_vtables.py target/debug/qz`: native/QZI/COFF,
  source-removed imports and cold/exact/partial/updated cache paths pass.
- Matching-compiler `qz build` twice and `qz run` in `examples/22-quazifetch`
  pass, including a verified v17 cache hit. `qz build -c --target x86_64-windows
  --no-incremental -o /tmp/quazifetch-flow-windows.o` passes with four existing
  target-specific warnings. The six isolated STD memory tests pass against the
  matching compiler; no STD sources change.
- Independent read-only design and implementation review, new-file formatting,
  changed Markdown links and `git diff --check` pass.
- `bash scripts/graphify.sh update .` refreshes the local AST index. It does
  not cover Quazi source semantics or certify the new ownership evidence.

No Windows-host runtime, full STD validation, completed D-014/D-015 or overall
workspace readiness is claimed. No STD or editor sources change in this step.
