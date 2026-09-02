# `std.collections`

Audience: Quazi application developers.

Status: experimental. `std.collections` currently exports integer-keyed
`Map` and `Set` only. It is not a generic collection framework: keys and values
are `usize`, and it does not provide iterators, ordering, stable hashing,
serialization, or concurrent access.

**Safety status:** the former update/cleanup aliasing crash was fixed on
2026-09-02 by making updates mutate the existing owner in place. The containers
remain experimental and limited to `usize` keys and values.

## `Map`

`Map.new()` returns `Result[Map, MapError]`; `MapError` is either
`AllocationFailed` or `CapacityOverflow`. `insert(self, key, value)` mutates
the map and returns `Result[bool, MapError]`: `true` means a new key was added,
and `false` means an existing value was replaced. `get(self, key)` returns
`Option[usize]`, `contains(self, key)` reports membership, `remove(self, key)`
returns whether a key was removed, and `len(self)` returns the number of
entries.

The implementation is open-addressed linear probing with tombstones, initial
capacity 16, and a resize threshold of 75%. Insertion replaces an existing
value for an equal key. Iteration order is not exposed and must not be inferred
from storage behavior.

## `Set`

`Set.new()` returns `Result[Set, SetError]`, with the same allocation and
capacity errors. `insert(self, key)` mutates the set and returns
`Result[bool, SetError]`; `true` means a new key was added and duplicates return
`false`. `contains`, `remove`, and `len` mirror the matching map operations.
Set storage uses the same probing, tombstone, initial capacity, and resize
policy.

## Ownership and limitations

Method receivers are borrowed by the language, so `insert` and `remove` mutate
the one owning container rather than returning an alias. Normal scope cleanup
releases the backing allocations once. `free(self)` is available only when an
earlier release is necessary; do not use the map or set after calling it.

The current surface remains limited until generic element bounds and fully
audited drop-aware storage exist; do not treat it as a substitute for a general
`Map[K, V]` or `Set[T]` API.
