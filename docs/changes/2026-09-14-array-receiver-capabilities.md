# Array receiver capabilities

`Array[T]` now declares the borrowing capability of its non-consuming API.
Queries (`get`, `len`, `is_empty`, and `unsafe as_ptr`) use a shared
`&Array[T]` receiver. Updates (`push`, `set`, and indexed assignment) use an
exclusive `&Array[T]!` receiver. Read indexing remains on the `Index` trait's
legacy by-value receiver until that trait contract is redesigned.

## Motivation

The prelude must express whether a call merely observes an owner or may update
its backing storage. In particular, `push` can reallocate an array, while
`get` only inspects the existing storage.

## Compatibility and migration

Read-only calls now work with immutable owners and shared views. Mutation
calls require a mutable owner, which makes an invalid update through an
immutable owner or a `const` aggregate field a compile-time error. Existing
mutable `Array` call sites need no source changes. `free` remains a legacy
by-value operation until the
language implements consuming receivers.

This receiver change does not define ownership transfer for a returned owned
element: `get` and indexing retain their existing raw-load behavior. That
move-or-borrow and structural-destruction work remains tracked by D-014.

## Verification

Semantic regressions cover shared queries on immutable and shared Array views,
reject exclusive updates through immutable owners, shared views, and `const`
aggregate fields, and preserve generic receiver substitution. Documentation
fixtures compile with the prelude, and the full compiler test suite covers
generic array dispatch and bytecode.
