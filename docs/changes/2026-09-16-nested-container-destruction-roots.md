# Nested contiguous-container destruction roots

## Summary

Implicit cleanup of a nested `@contiguous_elements` container now retains every
concrete storage-release specialization required by generated recursive
destruction, including a specialization reached through a generic caller.

## Why

The initial destruction-root collector retained only the outer release. For
example, cleaning up `Buffer[Buffer[Token]]` retained
`Buffer.free<Buffer[Token]>` but could omit `Buffer.free<Token>`, even though
the generated outer loop invokes the latter while destroying each element.
Code generation correctly failed rather than dispatching a concrete value to
an unresolved generic `free` template, but that made a valid nested container
program fail to compile.

## Behavior and compatibility

The compiler now recursively records contiguous-element releases and their
call-graph edges. Generic template dependencies remain symbolic until normal
monomorphization substitutes them for a real concrete instantiation.
This is a compiler correctness fix with no source migration.

## Verification

Semantic regressions assert the complete nested specialization set and reject
unresolved template roots. Bytecode coverage compiles nested owned container
cleanup and verifies the inner release chunk survives reachability pruning.
