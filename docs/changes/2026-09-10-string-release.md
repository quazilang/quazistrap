# Repeat-safe owned string release

Date: 2026-09-10

Audience: Quazi users and runtime maintainers.

`String.free()` now replaces released allocation-backed state with the canonical
empty-string state. A second explicit `free()` is a no-op, while `bytes_len()`,
`is_empty()`, and `as_str()` observe the invalidated empty value.

Previously, a released `String` retained its former data view, length, and
capacity. Releasing it twice could free the same allocation twice, and normal
queries could still report stale content length.

Compatibility: no source migration. Borrowed views and raw pointers obtained
before release remain invalid after the first `free()`. Direct `free()` on a
named local continues to suppress that local's automatic cleanup.

Verification:

```bash
qz test string_ownership --no-color
```

The source-level regression checks the observable empty state and a repeated
explicit release.
