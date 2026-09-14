# String receiver capabilities

Non-consuming `String` methods now use a shared `&String` receiver. This
includes length and view accessors, lookup, parsing, and transformations that
allocate a new `String` without modifying the receiver. `push_str` also
creates a fresh combined value and therefore borrows its receiver.

## Why

These methods only inspect the owner's UTF-8 allocation. Making the shared
loan explicit lets safe code call them through an immutable owner or `&String`
view while keeping future mutation and release operations exclusive or
consuming.

## Compatibility and migration

Existing calls remain source-compatible, and read-only calls now work through
shared views. `free` and `Index.index` retain their legacy by-value receiver
contracts: consuming receiver effects and the `Index` ownership contract are
separate D-014 work. This change does not make returned `String` values
aliases of the receiver.

## Verification

Semantic regressions cover shared-view dispatch for accessors, transformations,
and parsing. The `22-quazifetch` example is lowered using the changed prelude,
and the compiler suite covers receiver dispatch and source loading.
