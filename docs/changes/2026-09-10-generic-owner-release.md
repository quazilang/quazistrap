# Generic owner release and dependency closure

Date: 2026-09-10

Audience: Quazi users and compiler/runtime maintainers.

`Array[T].free()` and `Box[T].free()` now invalidate their raw pointers after
releasing storage. `Array` also clears its length and capacity. A second
explicit `free()` is therefore a no-op; a released `Array` reports length zero.
As with the existing collection contract, invalidation does not make the owner
safe to reuse for reads or mutation.

This change also fixes generic-specialization dependency closure. Concrete
generic bodies now retain calls to ordinary helpers and intrinsics, so an
`Array.push()` specialization retains the realloc intrinsic it uses. This is a
general reachability correction, not an Array-specific exception.

Compatibility: no migration is required. Code that explicitly releases an
owner gains repeat-safe behavior; it must continue to avoid using it after the
first release.

Verification:

```bash
cargo test --offline generic_specialization
qz test raw_owner_release --no-color
```

The compiler regression checks the concrete dependency edge. The source
regressions construct, release, and release again each of `Array`, `Box`,
`Map`, and `Set`.
