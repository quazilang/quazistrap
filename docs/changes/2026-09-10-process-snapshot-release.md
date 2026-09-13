# Repeat-safe Windows process-snapshot release

Date: 2026-09-10

Audience: users of `std.os` and runtime maintainers.

Windows `std.os` parent-process scanning now treats both `0` and the exact
all-ones snapshot sentinel as non-closeable. Its private snapshot owner clears
the handle after release, so repeated cleanup no longer sends the cleared `0`
value to `CloseHandle`.

Compatibility: no source migration. This only corrects best-effort `shell()`
and `terminal()` cleanup during their bounded Windows parent-process scan.

Verification:

```bash
qz check --target x86_64-windows
```

The system-information example imports `std.os` and type-checks its Windows
path. Windows runtime execution is unavailable in this Linux workspace.
