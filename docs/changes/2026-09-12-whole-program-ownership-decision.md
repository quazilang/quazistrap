# Whole-program ownership and QZI escape-analysis decision

Quazi has resolved D-014 before implementing the next ownership layer.

## Decision

Safe code will use affine owners, shared `&T` loans, and exclusive `&mut T`
loans. Whole-program analysis derives their regions from the concrete resolved
call graph, so source does not need lifetime parameters. It is a precision
mechanism, not permission to alias mutable state or let a reference outlive
its root.

The decision also resolves the policy portions of D-002 and D-003: receivers
are explicitly shared, exclusive, or consuming; destruction is structural with
a Drop hook followed by reverse-order owned members; moves and explicit close
consume the owner; panic does not unwind.

## Library compatibility

QZI-only distribution remains supported. New-format QZI libraries must carry
compiler-checked ownership summaries that describe parameter capabilities,
result/reference provenance, escape edges, indirect effects, and exported type
move/drop/layout facts. Missing or old summaries will require a source/current
compiler rebuild at a safe boundary. The implementation will introduce the
corresponding QZI/QZC versions; this decision does not alter current artifacts.

## Scope boundary

The existing direct Linux `Child` lifecycle remains documented, but D-011
platform work and resource-retaining/asynchronous process features now wait for
the D-014 implementation. Safe concurrency remains blocked for the same
reason; its scheduling and synchronization choices stay with D-007.

## Compatibility and verification

This is a documentation and language-design decision only. Implementations
must add source and QZI-only regressions for aliasing, reference escape,
structural destruction, recursion/indirect calls, and process/thread transfers
before dependent APIs are stabilized.
