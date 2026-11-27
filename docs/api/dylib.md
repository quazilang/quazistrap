# `std.dylib`

Audience: Quazi developers maintaining a deliberately small native-plugin or
system-library adapter.

`std.dylib` loads a Windows DLL or a Linux or NetBSD shared object at runtime. It exposes
only a library handle and untyped symbol addresses; it does not validate ABI,
calling convention, ownership, thread safety, or unload safety. Keep all use
inside a narrow `unsafe` integration boundary.

## Opening and closing

`DynamicLibrary.open(path)` copies the UTF-8 path into a NUL-terminated
`CString` and returns a library owner. Embedded NULs become
`DynamicLibraryError.InvalidName(index)`, allocation failures become
`AllocationFailed`, and load failures become `OpenFailed(code)` on every
target (the `GetLastError` code on Windows, 0 where the loader reports none).

`close()` consumes the owner, unloads the library and returns `Ok(true)`
(`Ok(false)` for an owner without a handle). A native unload failure becomes
`Native(code)`; the owner is consumed either way and the unload is not
retried. `free()`, run once by ordinary scope cleanup, unloads directly and
ignores a failure. Use `close()` when an unload failure must be reported, and
do not use any address obtained from the library after it is closed.

`raw_handle()` exposes the platform handle as `usize` for a foreign API that
explicitly requires it. It remains valid only while the `DynamicLibrary` owner
is open. Both `raw_handle()` and `unsafe symbol(name)` use a shared receiver:
they may inspect an open library but cannot overlap `close()`. `DynamicLibraryError.message()` provides display text for diagnostics;
callers should match error variants when behavior depends on the failure kind.

## Symbols

`unsafe symbol(name)` returns a raw `usize` address or `SymbolNotFound`; NUL in
the symbol name follows the same `InvalidName` rule as a path. A non-null
address does not prove that its type matches the declaration the caller wants.
Cast it only inside `unsafe` code to an exact exported `@repr(C)` callback type,
then call it according to the native ABI. The address is borrowed from the
library and becomes invalid after close; Quazi cannot enforce that lifetime.

```quazi
import std.dylib.{DynamicLibrary, DynamicLibraryError};

fn load_plugin(path: str) Result[DynamicLibrary, DynamicLibraryError] {
    ret DynamicLibrary.open(path);
}
```

Do not use this module to load an arbitrary library and guess signatures. A
wrong callback declaration, an unloaded library, or a callback that retains
Quazi-owned pointers is undefined at the foreign boundary.
