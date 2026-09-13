# Exact Windows file-handle sentinel

Date: 2026-09-10

Audience: users of `std.fs` and runtime maintainers.

`File.close()` now recognizes only the exact `-1` invalid-handle bit pattern.
The previous signed less-than-zero test could skip `CloseHandle` for a valid
Windows `HANDLE` whose high bit is set. The constructors already used the exact
Windows invalid-handle sentinel, so close now follows the same contract.

Compatibility: no source migration. `File.raw_handle()` remains target-specific
FFI; callers must not use its signed interpretation as a validity check.

Verification:

```bash
qz build -c --target x86_64-windows
```

The temporary smoke imports `std.fs.File`, opens a path, and invokes
`close()`; it produces a Windows x86-64 COFF object. Windows runtime execution
and a forced high-bit-handle test are unavailable in this Linux workspace.
