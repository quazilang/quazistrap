# Guide: C Interoperability

Audience: Quazi users.

This guide covers calling C functions from Quazi, exporting Quazi functions to
C, and working with C data types.

## Importing C functions

Use `@api` to declare a C function import:

```quazi
import std.ffi.{c_char, c_int};

@api("puts")
unsafe fn puts(text: *c_char) c_int;

fn main() i32 {
    unsafe {
        puts("Hello from C\0".as_ptr() as *c_char);
    };
    ret 0;
}
```

- `@api("symbol")` specifies the native symbol name.
- Bare `@api` uses the Quazi function name.
- All `@api` calls require an `unsafe` context.
- The ABI is Win64 on Windows and SysV on Linux.

## Exporting Quazi functions

Use `@export` to expose a `pub` function with C ABI:

```quazi
@export("quazi_add")
pub fn add(left: c_int, right: c_int) c_int {
    ret left + right;
}
```

Generate C header declarations:

```bash
qz header src/lib.qz -o build/mylib.h
qz header src/lib.qz --target x86_64-windows -o build/mylib_win.h
```

## C-compatible types

### Scalar aliases

`std.ffi` provides C-compatible type aliases:

```quazi
import std.ffi.{c_char, c_int, c_uint, c_long, c_ulong, c_size};
```

### `@repr(C)` structs

```quazi
@repr(C)
struct Point {
    x: f64,
    y: f64,
}
```

`@repr(C)` structs use target C layout and can be passed by value across the
FFI boundary.

### Packed and aligned structs

```quazi
@repr(C, packed)
struct Header {
    magic: u32,
    flags: u8,
}

@repr(C, align=16)
struct AlignedData {
    values: [f64; 4],
}
```

### Unions

```quazi
@repr(C)
union Value {
    integer: i64,
    floating: f64,
    pointer: *u8,
}
```

Union field access is unsafe because reading the wrong variant is undefined.

### Bitfields

```quazi
@repr(C)
struct Flags {
    read: u8 : 1,
    write: u8 : 1,
    execute: u8 : 1,
}
```

Named integer bitfields pack bits according to C rules.

### Opaque handles

```quazi
@opaque pub struct FileHandle {}
```

Opaque types cannot be constructed in Quazi. They serve as typed pointer
targets for foreign handles.

## Callbacks

### Declaring callback types

```quazi
@repr(C)
type CompareFunc = fn(i32, i32) i32;
```

### Passing callbacks to C

Only `@export` functions coerce to callback pointers:

```quazi
@export("my_compare")
pub fn my_compare(a: i32, b: i32) i32 {
    ret a - b;
}

@api("qsort")
unsafe fn qsort(base: *u8, count: usize, size: usize, cmp: CompareFunc) void;
```

Invoking a raw callback is unsafe. Ordinary Quazi closures cannot cross the
C boundary.

## C variadic functions

Bare `...` marks C variadic calling convention:

```quazi
@api("printf")
unsafe fn printf(fmt: *c_char, ...) c_int;
```

This is distinct from Quazi's typed variadics (`...name: Type`).

## Foreign globals

Import mutable C global variables:

```quazi
@api("some_global")
var some_global: c_int;
```

Reading and writing require `unsafe`. Macro/TLS pseudo-globals like `errno`
should use accessor functions instead.

## Strings

### Borrowed C strings

`CStr` borrows a NUL-terminated foreign pointer without ownership:

```quazi
import std.ffi.CStr;

// In an unsafe context with a known-valid pointer:
const borrowed = CStr.from_ptr(foreign_ptr);
```

### Owned C strings

`CString` owns a NUL-terminated buffer:

```quazi
import std.ffi.CString;

const result = CString.try_from(b"hello");
match result {
    Ok(cstr) => {
        // Use cstr.as_ptr() for C calls
    },
    Err(e) => { /* embedded NUL or allocation failure */ },
};
```

## Linking

### Built-in linker

Plain executables use the built-in ELF/PE linker. No implicit libc:

```bash
qz build
```

### External linker with native libraries

```bash
qz build --linker /usr/bin/ld.lld
```

Or in `quazi.toml`:

```toml
[link]
libraries = ["sqlite3"]
library-paths = ["/usr/local/lib"]
libc = true
```

### Native C sources

Compile C sources alongside Quazi:

```toml
[cc]
sources = ["native/helper.c"]
include-paths = ["native/include"]
```

### Shared libraries

```bash
qz build src/lib.qz --shared-lib -o build/libacme.so
qz build src/lib.qz --shared-lib --target x86_64-windows -o build/acme.dll
```

Only `@export` symbols enter the shared library export table.

### Runtime loading

```quazi
import std.dylib.DynamicLibrary;

fn main() i32 {
    const lib = DynamicLibrary.open("libm.so");
    match lib {
        Ok(handle) => {
            // Use handle.symbol("function_name") for raw addresses
            handle.close();
        },
        Err(e) => io.eprintln("Load error: {}", e.message()),
    };
    ret 0;
}
```

Casting a raw symbol address to a callback and calling it both require
`unsafe`.

## Safety responsibilities

When using FFI, you are responsible for:

- Pointer validity, alignment, and lifetime.
- Correct type sizes and calling conventions.
- Thread safety of foreign functions.
- NUL termination of C strings.
- Proper cleanup of foreign resources.
