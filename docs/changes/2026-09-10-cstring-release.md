# Repeat-safe C-string release

Date: 2026-09-10

Audience: users of `std.ffi` and runtime maintainers.

`CString.free()` now invalidates its owned pointer and clears its stored byte
length after releasing the allocation. Calling `free()` again is therefore a
no-op. The compiler continues to suppress automatic cleanup after a direct
explicit `free()` call.

Previously, an explicit `free()` left the owner pointing at released storage,
so a repeated explicit release could release that allocation again.

Compatibility: no source migration. Code must still not use an earlier
`CStr` view or raw pointer after the first release.

Verification:

```bash
qz test ffi --no-color
```

The source-level regression verifies invalidation, the cleared length, and a
second `free()` call.
