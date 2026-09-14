# Collection receiver capabilities

`std.collections.Map` and `Set` now declare the capabilities their operations
require. Lookup operations (`get`, `contains`, and `len`) use shared receivers;
operations that mutate, rehash, or release raw storage (`insert`, `remove`,
and `free`) use exclusive receivers.

## Why

The collection tables own raw allocations. An insertion can rehash the table,
and removal or early release changes that allocation-backed state. Declaring
those operations as `self: &T!` lets the compiler's current call-local
exclusive-loan checking reject overlapping safe uses at their boundary instead
of treating every collection method as the legacy implicit receiver form.

## Compatibility and migration

Ordinary calls on a mutable local remain source-compatible:

```quazi
var map = Map.new()?;
map.insert(7, 42)?;
```

A caller holding a shared `&Map` / `&Set` loan cannot invoke a mutating method
until that loan ends. This is an intentional enforcement of the documented
exclusive receiver contract; no replacement container is returned.

This is still the current local receiver foundation, not the D-014
whole-program ownership implementation. Consuming receiver effects, structural
destruction, and QZI ownership summaries remain incomplete.

## Verification

`std/tests/raw_owner_release.qz` covers insert, shared lookup, exclusive
removal, and post-removal lookup for both collections. The source is checked
with the contained compiler; runtime execution of allocation-owning standard
library tests remains blocked by the pre-existing allocator-free test harness.
