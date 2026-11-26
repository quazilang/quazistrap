# `std.collections`

Audience: Quazi application developers.

Status: experimental. This module exports the generic `HashMap` and
`HashSet` alongside the original `usize` collections `Map` and `Set`.

## `HashMap[K, V]` and `HashSet[K]`

Both collections accept keys that implement `Hash` and `Eq`. The prelude
provides implementations for integers, `bool`, `str`, and `String`.
Custom implementations must give equal keys the same hash and keep their
equality and hash stable while they are in the collection. Hashes are
deterministic and intended for in-memory lookup.

`new()` makes an empty collection and backing arrays allocate on first
insertion. `insert` consumes its arguments and returns
`Result[bool, ArrayReserveError]`. `Ok(true)` marks a new entry.
`Ok(false)` marks replacement of an existing map value or a duplicate set
value. On a capacity or allocation error, existing entries are preserved.
The map retains its original key on replacement and drops the previous value.

Both types provide `contains(&key)`, `remove(&key)`, `len()`, `is_empty()`,
and `clear()`. Removing a key drops its entry and reports whether it existed.
For maps, `get(&key)` returns a copy of a `Copy` value as `Option[V]` (owned
values are rejected; borrow them instead), and `find(&key)` returns `Option[usize]`. A successful index can be passed
to `key_at(index)` or `value_at(index)` to borrow the stored element.
`HashSet.value_at(index)` borrows a stored key. These indexed accessors panic
for an out-of-range index. Removing an entry can move the last entry into its
position, so use indices only while the collection has no intervening mutation.

Shared results borrow the collection. Mutation requires exclusive access.
Scope exit and `clear()` release stored owners and backing storage. The
collections do not define a concurrent access or serialization interface.

```quazi
import std.collections.HashMap;
import std.io.println;

fn main() void {
    var counts: HashMap[String, i32] = HashMap.new();
    counts.insert(String.from("apples"), 3).unwrap();
    const apples = String.from("apples");
    const index = counts.find(&apples).unwrap();
    println("{}", *counts.value_at(index));
}
```

## `Map`

`Map.new()` returns `Result[Map, MapError]`; `MapError` is either
`AllocationFailed` or `CapacityOverflow`. `insert(self: &Map!, key, value)`
mutates the map and returns `Result[bool, MapError]`: `true` means a new key
was added, and `false` means an existing value was replaced. `get(self: &Map,
key)` returns `Option[usize]`, `contains(self: &Map, key)` reports membership,
`remove(self: &Map!, key)` returns whether a key was removed, and
`len(self: &Map)` returns the number of entries.

The implementation is open-addressed linear probing with tombstones, initial
capacity 16, and a resize threshold of 75%. Insertion replaces an existing
value for an equal key. Iteration order is not exposed and must not be inferred
from storage behavior.

## `Set`

`Set.new()` returns `Result[Set, SetError]`, with the same allocation and
capacity errors. `insert(self: &Set!, key)` mutates the set and returns
`Result[bool, SetError]`; `true` means a new key was added and duplicates return
`false`. `contains(self: &Set)`, `remove(self: &Set!, key)`, and
`len(self: &Set)` mirror the matching map operations. Set storage uses the
same probing, tombstone, initial capacity, and resize policy.

## Ownership and limitations

Read-only collection methods use shared receivers (`self: &Map` or
`self: &Set`). Mutation and `clear()` use exclusive receivers (`self: &Map!`
or `self: &Set!`), so a call cannot overlap another loan of that container.
`clear()` releases backing storage while retaining an empty reusable container.
Normal scope cleanup calls consuming `free(self)` once; direct `free()` also
consumes the owner and must be its final use.
