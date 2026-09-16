# Compiler-derived callable ownership signatures

Audience: compiler maintainers.

## Change

Semantic analysis now exposes a deterministic ownership signature for every
declared callable with a representable signature in `SemanticReport`. Each
record identifies its receiver, fixed parameters, optional variadic element,
and result as a plain copy, an owned move, a shared borrow, or an exclusive
borrow. It also records whether the declaration has a body and whether it is a
generic template.

This metadata is derived from resolved compiler signatures; users cannot write
or override it. It is not a public language feature and does not change source
syntax or safe-reference behavior.

The existing source-only direct-call borrow rule now reads this compiler-owned
metadata rather than separately reinterpreting every callee signature. Calls
without an eligible local body, including unsafe, variadic, and intrinsic
targets, remain opaque and consuming exactly as before.

## Why

D-014 requires compiler-generated ownership summaries before a QZI-only
dependency can safely participate in reference-bearing calls. Previously, the
only capability facts lived transiently in the borrow checker. The report-level
schema gives the later effect solver and QZI serializer one canonical,
deterministically ordered input.

## Boundary

`transitive_effects_verified` is false for every current record. This release
does not infer body effects, provenance, escapes, indirect-call targets, or
QZI certificates. QZI v9 and QZC v7 therefore remain insufficient for safe
borrowed values across an artifact boundary; existing conservative rejection
rules remain in force.

## Verification

`semantic::tests::records_deterministic_callable_ownership_signatures` covers
copy, move, shared/exclusive borrow, receiver classification, variadics,
generic templates, body availability, and stable ordering.
`semantic::tests::direct_call_signatures_preserve_named_borrow_capabilities`
also verifies that positional and named source-call borrows continue to use
their distinct signature capabilities and end when the call returns.
