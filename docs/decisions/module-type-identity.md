# D-018: module-qualified type identities

Audience: compiler developers.

Status: proposed 2026-11-08; not implemented.

## Problem

Functions in non-entry files are registered as `module.name`, but structs,
enums, traits and aliases are registered under their bare name in one global
table. Per-file *visibility* is enforced (`semantic/type_visibility.rs`), yet
two files cannot *declare* the same type name: a user `struct Box` or
`struct File` collides with the prelude `Box` or `std.fs.File` (`S05 duplicate
declaration`), and two libraries cannot both have a private `Node`. The error is
explicit, not a miscompilation, but it makes common names unavailable.

## Scope of the change

Bare-name keys are used by roughly 50 `struct_defs`, 26 `enums`, 19
`trait_impls` and many other analyzer tables, by method symbols spelled
`Type.method`, by codegen chunk and layout names, by QZI interfaces and vtable
descriptors, by `qz header`, and by the LSP. A local rename in one consumer
would desynchronize the others.

## Proposed migration

1. Give every type declaration a canonical identity: its bare name for the
   entry file and the selected prelude package, and `module.Name` for other
   files, mirroring function namespacing.
2. Resolve every raw spelling once, in its source file, to that identity:
   extend `SourceTypeContexts` (already used for imports and alias bodies) so
   same-file declarations map to their canonical identity, and publish the
   result as expression/annotation facts instead of re-reading raw AST names.
3. Key analyzer tables, method symbols (`identity.method`), layouts and codegen
   by canonical identity; display the source spelling in diagnostics.
4. Version QZI interfaces (type names cross the artifact boundary) and bump QZC.
5. Regression matrix: user `Box`/`File` beside prelude/std, two private `Node`
   types in different modules (native, QZI, COFF), imported aliases,
   generics, trait impls and dynamic dispatch on same-named types, headers, LSP.

Until then, `S05` remains the documented behavior.
