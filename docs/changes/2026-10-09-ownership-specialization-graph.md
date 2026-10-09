# Ownership specialization evidence and generic-call diagnostics

Audience: language users and compiler maintainers.

Semantic analysis now exposes a deterministic concrete source call graph for
the future ownership solver. It distinguishes generic specializations by
structural, alias-resolved types and recomputes their ownership capabilities.
This does not relax safe-reference rules or validate QZI ownership envelopes.
See the [implementation contract](../internals/ownership-call-graph.md).

Three previously unreliable cases now have explicit diagnostics:

- `S14` rejects distinct specialization identities that collide at one emitted
  bytecode symbol. The old punctuation-replacing mangling could select a
  specialization with the wrong layout or ownership. Alias-equivalent types
  remain compatible; bytecode symbol spelling and QZI/QZC versions are unchanged.
- `S14` rejects specialization types exceeding 64 levels or 4,096 type nodes,
  and specialization/ownership graphs exceeding their callable budgets. This
  prevents unbounded recursive expansion from silently proceeding. Type values
  are never truncated. Alias expansion itself still needs separate hardening.
- `S08` rejects an explicit generic argument list with the wrong arity,
  including type arguments supplied to a non-generic function. Remove excess
  arguments or supply the declaration's exact explicit type-argument count.

Some symbolic helper chunks still participate in the bytecode namespace.
They are covered by the collision diagnostic too; code whose symbolic helper
name collides with another specialization may require a different type name
until helper emission and symbol mangling receive a coordinated redesign.

Resolved source sites now supplement transitive generic dependency closure,
and impl generic parameters are recorded at their declarations rather than
guessed from dotted names. Automatic dense-container release records layouts
and uses the same specialization identity checks as explicit calls.
Renamed/reordered impl binders now drive method substitution in analysis and
codegen consistently, including multi-slot parameter/result lowering.
Declared concrete target types/aliases are not binders. A receiver that does
not match the substituted impl target is rejected with S08, including a
zero-argument method on the wrong nominal specialization.

Verified compiler commands use the stable toolchain directory on `PATH` and
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'` in this Nix workspace (the bundled Rust
lld wrapper references a missing store path). `cargo test --offline -q`
passes 666 tests, including 20 focused ownership graph cases. Repeated test
runs used a fresh `TMPDIR` because a pre-existing LSP fixture can collide with
an abandoned PID-named directory. `cargo build --offline -q` and
`git diff --check` pass; the new ownership module is rustfmt-clean. Existing
compiler warnings remain outside this checkpoint.

Verification includes focused semantic regressions, the complete compiler
suite, a Linux native scalar/multi-slot round trip, Windows COFF and portable
QZI emission, and CLI rejection probes for colliding names, expanding recursion,
and wrong generic arity. No Windows runtime behavior is claimed by a COFF build.
