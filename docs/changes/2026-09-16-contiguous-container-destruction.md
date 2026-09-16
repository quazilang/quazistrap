# Contiguous generic-container destruction

Audience: language users and standard-library implementers.

## Change

Generic ordinary structs may declare
`@contiguous_elements(element=T, pointer=ptr, length=len)`. The compiler
validates the named element parameter and physical fields, then emits
exact-once cleanup for the initialized dense range `[0, len)`: owned elements
are destroyed in reverse order before the existing consuming `free(self)`
method releases backing storage. Explicit `.free()` and automatic scope cleanup
share this sequence.

`Array[T]` now declares this contract. The implementation is metadata-driven;
it has no compiler branch for `Array`, `Headers`, or a particular field name.

## Compatibility and migration

Existing containers are unchanged unless they opt in. An opted-in `free` method
must only release its backing storage; it must not also walk initialized
elements. The declaration is rejected for unions, `@repr(C)` structs, missing
or mistyped fields, unknown generic parameters, and types without a consuming
`free` method.

This change does not enable by-value reads of owned `Array` elements. Such
reads remain rejected until borrowed-element provenance, exclusive replacement,
and removal semantics are available. Consequently it does not by itself make
the HTTP `Headers` migration safe.

## Verification

Focused semantic regressions validate the declaration contract. Bytecode
regressions prove owned elements use a reverse loop and that automatic cleanup
and direct `.free()` both invoke the element cleanup followed by the release
hook.
