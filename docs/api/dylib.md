# `std.dylib`

Audience: Quazi developers maintaining a deliberately small native-plugin or
system-library adapter.

`std.dylib` loads a Windows DLL or Linux shared object at runtime. It exposes
only a library handle and untyped symbol addresses; it does not validate ABI,
calling convention, ownership, thread safety, or unload safety. Keep all use
inside a narrow `unsafe` integration boundary.

## Opening and closing

`DynamicLibrary.open(path)` copies the UTF-8 path into a NUL-terminated
`CString` and returns a library owner. Embedded NULs become
`DynamicLibraryError.InvalidName(index)`, allocation failures become
`AllocationFailed`, and load failures become `OpenFailed` on Linux or
`Native(code)` on Windows.

`close()` releases an open library and returns `Ok(true)`. It returns
`Ok(false)` when the handle was already closed. A native close failure becomes
`Native(code)` and leaves the owner open so normal cleanup can retry it.
`free()` invokes `close()` and intentionally discards that error; ordinary
scope cleanup calls `free()` once. Use `close()` when an unload failure must be
handled explicitly, and do not use the library or any address obtained from it
after a successful close.

`raw_handle()` exposes the platform handle as `usize` for a foreign API that
explicitly requires it. It remains valid only while the `DynamicLibrary` owner
is open.

## Symbols

`unsafe symbol(name)` returns a raw `usize` address or `SymbolNotFound`; NUL in
the symbol name follows the same `InvalidName` rule as a path. A non-null
address does not prove that its type matches the declaration the caller wants.
Cast it only inside `unsafe` code to an exact exported `@repr(C)` callback type,
then call it according to the native ABI. The address is borrowed from the
library and becomes invalid after close; Quazi cannot enforce that lifetime.

```quazi
import std.dylib;

fn load_plugin(path: str) Result[DynamicLibrary, DynamicLibraryError] {
    ret DynamicLibrary.open(path);
}
```

Do not use this module to load an arbitrary library and guess signatures. A
wrong callback declaration, an unloaded library, or a callback that retains
Quazi-owned pointers is undefined at the foreign boundary.
