# Types, Ownership, and Memory

Quazi combines value semantics with compiler-managed lexical destruction.
Primitive scalars copy. Owned standard-library values move unless their type is
copyable. The semantic analyzer rejects use after move, unsafe raw access, and
incompatible assignments before QZI generation.

## Numeric model

Integer widths are fixed except `isize`/`usize`, which follow target pointer
width. Signedness affects comparisons, shifts, extension, parsing, and C ABI
classification. `f16`, `f32`, and `f64` are distinct source types; Quazi's
internal slots preserve required target conversion at C boundaries.

Conversions that may change representation are explicit with `as`. Integer
overflow checking is not a substitute for input validation; parsing returns
`Result` and rejects malformed/out-of-range values.

Fallible public APIs use typed error enums. Match variants for recovery logic;
use each enum's `message()` method for display. Raw numeric error codes remain
inside low-level unsafe/platform modules or an explicit `Native(code)` fallback.

## Text and bytes

`str` is borrowed immutable UTF-8. `String` owns writable UTF-8 storage with
pointer, byte length, and capacity. `String.as_str()` borrows it. Rune indexing
never splits UTF-8; `bytes_len()` exposes encoded length when protocols need it.

`bytes` is immutable arbitrary data. Unlike `str`, it carries no UTF-8 promise.
FFI conversion to `CString` is explicit and fallible because embedded NUL and
allocation failure must be handled.

## Arrays and slices

`[T; N]` is a fixed-size value. `[T]` is an unsized borrowed slice. `Array[T]`
is an owned growable collection from the prelude. Indexing is checked by safe
APIs; raw pointer access moves responsibility to an unsafe block.

## References and pointers

`&T` is a safe shared reference. `*T` is a raw native pointer: it may be null,
dangling, misaligned, or point to invalid storage. Dereferencing it is unsafe.
Integer zero is the raw null-pointer constant. Raw pointer types interoperate at
FFI boundaries, but typed wrappers remain preferred.

Until lifetime parameters and mutable references are designed, safe references
are deliberately lexical and restrictive. Address-of accepts a named local or
parameter, pointee types are invariant, and a reference binding cannot be
rebound. Shared aggregate references cannot be dereferenced into owned-looking
values because current aggregate storage is address-based and that would create
a mutable alias. Scalar and representation-identical string reads remain
available.
Non-string references cannot be returned, stored in owned aggregates,
or captured by closures. Once a local is address-taken, safe code cannot mutate,
move, or invoke methods on it for the rest of that function. `str`/`&str` is the
representation-identical string-view exception. See the
[reference migration guide](migrations/references.md).

## Scope cleanup and moves

Owned locals are destroyed in reverse lexical order on fallthrough and early
return. Assignment only destroys a previously initialized value. Returning an
owner transfers it to the caller. Passing by value may move; ordinary method
receivers borrow unless the API explicitly consumes ownership.

A Quazi `fn` value owns a heap-allocated environment. Passing, returning, or
assigning it moves that ownership. Calling it borrows the environment; replacing
the binding, discarding a temporary, or leaving the owning scope frees it.
Function values cannot currently be placed inside arrays, structs, enums, or
generic type arguments because those containers do not yet perform recursive
destruction. Closure captures are limited to immutable plain scalars for the
same reason. A function-pointer alias declared with `@repr(C)` is a non-owning
native code pointer and does not use this cleanup rule.

`free()` methods are idempotent resource operations for APIs needing early
release. Normal application code should rely on scope cleanup where possible.
Foreign pointers and borrowed `CStr` never gain ownership automatically.

## Layout

Ordinary Quazi structs use compiler-defined layout. Never expose that layout to
C. `@repr(C)` gives target C layout for supported scalar fields and aggregates.
`packed`, `align=N`, unions, bitfields, and final `[T; ..]` flexible array
members exist for foreign declarations and carry extra safety restrictions.
