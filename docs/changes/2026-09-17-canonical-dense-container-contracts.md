# Canonical dense-container contracts

Date: 2026-09-17

Audience: compiler maintainers.

`@contiguous_elements` now produces a semantic
`ContiguousElementContract`: the declared generic element parameter plus the
validated pointer and initialized-length field identities. Ownership analysis,
replacement/removal validation, and code generation consume that one contract.
Code generation translates its field names through the semantic field-layout
report; it no longer parses or validates source attributes a second time.

## Compatibility

This is an internal compiler-model consolidation. Source syntax, current QZI
v9/QZC v7 artifacts, and the public collection API are unchanged. It does not
expose D-015 element borrowing; that remains contingent on provenance and
verified D-014 ownership summaries.

## Verification

Semantic regressions cover both first and non-first generic element parameters,
including the recorded field identities. Existing generic cleanup, replacement,
removal, and native checked-address regressions verify code generation consumes
the semantic contract without an `Array`-specific path.
