# Affine Closure Environments

Date: 2026-08-29

## Motivation

Every Quazi closure and named function value allocated a heap environment, but
no language cleanup path owned it. Escaping closures could also shallow-capture
an owned value that the defining scope later destroyed. Captured closure chunks
allocated capture registers on top of hidden ABI parameters, and nested closure
symbols could collide.

## Behavior

- `fn` is an affine owner; passing, returning, and assigning transfer it.
- Scope exit, replacement, discarded temporaries, immediately called
  temporaries, and consumed parameters destroy exactly one environment.
- Calling a function value borrows it.
- Closure chunks reserve r0 for the environment and r1 onward for user
  parameters before capture loads.
- Nested closure symbols are unique and named-function forwarders are
  deduplicated.
- Callable signatures require exact runtime parameter and result shapes.
- Safe closures currently accept only immutable plain-scalar captures,
  parameters, and results. Aggregate/generic storage of owned `fn` values is
  rejected until recursive cleanup exists.
- Moves from conditionally executed paths, function-valued match results, and
  consuming assignment expressions are rejected until cleanup becomes
  path-sensitive.
- QZI v7 establishes the affine ownership contract. Older artifacts with
  public function-value APIs or legacy closure/forwarder chunks require a
  source rebuild.
- QZC v4 invalidates stale generated chunks.

## Compatibility

Programs that copied function values, mutated captures, captured owners or
references, used representation-changing callable coercions, or nested `fn`
inside containers now receive semantic diagnostics. Follow the
[closure migration guide](../migrations/closures.md).

## Verification

Compiler regressions cover hidden ABI registers, unique nested symbols,
forwarder deduplication, exact signatures, capture restrictions, self-assignment,
generic/aggregate rejection, parameter cleanup, replacement, temporary cleanup,
and returned-owner transfer. The native closure pressure program runs on Linux,
round-trips through QZI v7, and emits a valid Windows COFF object. The full Rust
suite passes 425 tests.
