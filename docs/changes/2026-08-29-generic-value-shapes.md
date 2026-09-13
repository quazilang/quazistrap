# Generic Value-Shape Validation

Date: 2026-08-29

## Motivation

Quazi's current internal function ABI passes one eight-byte virtual-register
slot per ordinary parameter and result. Generic specialization previously did
not revalidate that contract after substituting a concrete type. A fixed array
could therefore enter `Array.push`, `Array.get`, `Box.new`, or any generic
function as if its first register represented the complete value. Incremental
QZC hits could preserve the unsafe bytecode after the compiler changed.

## Behavior

- The compiler has one target-neutral model for internal runtime value shapes.
- Scalars and indirect handles occupy one slot. Fixed arrays occupy a contiguous
  register block, slices use a pointer and length, and flexible arrays have no
  standalone value representation.
- Every concrete generic function and method specialization validates its
  substituted parameters, variadic element, and result before code generation.
- An annotated binding supplies generic constructor context, so declarations
  such as `var values: Array[i32] = Array.new();` no longer need a cast.
- Unsupported multi-register specializations now produce `S14` instead of
  silently truncating the value.
- QZC v5 rejects caches produced before this validation boundary.

Named structs, enums, and `String` are currently indirect one-slot handles, so
this check intentionally does not reject them by width. Their move, borrowed
access, replacement, and recursive-destruction contract remains an active
ownership milestone; one slot does not imply that a value is copyable.

## Compatibility

Programs that instantiated generic functions or containers with fixed arrays or
slice-shaped values may now fail during analysis. See the
[generic value-shape migration guide](../migrations/generic-value-shapes.md).
This is a safety correction to previously miscompiled code. QZI's long-term
generic ABI remains under design; public generic libraries still ship as source.

## Verification

Focused runtime-layout tests distinguish handles, register blocks, nested fixed
arrays, and slices. Semantic regressions cover direct generic functions,
generic receiver methods, and contextual associated constructors. The dynamic
array example now builds and runs without an `Array.new()` cast. An end-to-end
`Array[[i32; 3]]` probe reports both the invalid `push` parameter and `get`
result before bytecode generation. A QZC regression proves that a cache marked
as v4 is ignored by the v5 reader.
