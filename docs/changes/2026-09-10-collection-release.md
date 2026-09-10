# Repeat-safe collection release

Date: 2026-09-10

Audience: users of `std.collections` and runtime maintainers.

`Map.free()` and `Set.free()` now clear their raw allocation pointers, length,
and capacity after releasing storage. Repeated explicit calls are no-ops, and
`len()` reports zero after the first release.

Compatibility: no source migration. A released container remains invalid for
lookup or mutation; the new empty observable state is not permission to reuse
it.

Verification:

```bash
qz test raw_owner_release --no-color
```

The source regression constructs each table, performs an insertion, checks the
post-release length, and calls `free()` again.
