# std.collections (`std/src/collections/`)

Hash-map and hash-set backed by open-addressing tables with linear probing.

## Map (`map.void`)

- `Map[K, V]` — generic hash map
- `new()`, `insert(k, v)`, `get(k)`, `remove(k)`, `contains(k)`, `len()`, `keys()`, `vals()`
- `__grow()` resizes at 75 % load factor
- `__hash()` is integer multiplicative (`key * 2654435761 % cap`); caller must use integer or pointer-stable keys
- Tombstone deletion (flag = 2) so probes remain valid

## Set (`set.void`)

- `Set[T]` — generic hash set, wraps `Map[T, bool]`
- `new()`, `insert(v)`, `remove(v)`, `contains(v)`, `len()`
- Same probing / resize / tombstone strategy as Map

## Internals

- `__map_store` / `__map_load` — `@intrinsic` raw slot access
- Keys, vals, and flags are raw `*u8` blocks allocated with `core.malloc`
- Initial capacity 16, doubles on resize
