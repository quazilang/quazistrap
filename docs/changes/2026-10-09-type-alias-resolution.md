# Checked transparent aliases

Audience: language users and compiler developers.

Transparent alias expansion previously used several unchecked recursive walkers.
Unused direct or indirect cycles could overflow the stack during semantic layout
or ownership inspection. Generic alias substitution could duplicate large type
trees before specialization limits ran; lowering and standalone header generation
had separate behavior and could leave a wrong-arity alias unresolved.

The compiler now shares a bounded resolver across semantic analysis, codegen,
C ABI type lowering, and C-header validation. Cycles, exact-arity violations,
applied type parameters, and resource-limit failures are explicit errors.
Analysis checks target-active declaration types before other consumers, including
unused definitions and dependency declarations. Read-only semantic helpers retain
resolver failures and publish mandatory S14 diagnostics with the final report.

Explicit generic binders shadow same-named global aliases during signatures and
bodies. Alias bodies resolve their own parameters and global declarations rather
than capturing the caller's bindings. Nested identity-alias uses remain valid.
C callback aliases retain their ABI representation; header rendering preserves
callback typedefs and target-dependent C integer names.

## Compatibility

Finite well-formed aliases keep their transparent representation. Previously
accepted wrong generic arity and recursive transparent aliases are now rejected.
Named aggregates remain available for recursive data. No QZI version or ownership
certificate policy changes. See the [resolver contract](../internals/type-alias-resolution.md)
for exact limits and cancellation boundaries.

## Verification

Focused tests cover direct, mutual, pointer, and expanding cycles; exact alias
arity; lexical binders; caller-independent alias bodies; nested applications;
constructor traversal; substitution cycles; depth, width, work, and text budgets;
raw-report codegen rejection; and target-aware standalone header validation.
The complete compiler suite passes 690/690. Pinned-toolchain build and clippy
checks succeed with existing warnings. Linux native execution, QZI emission,
Windows COFF, both target headers, and negative unused-cycle CLI probes pass.
Exact environment and the transient test-runner retry are recorded in RESUME.md.
