# Attributes

Audience: language users and implementers.

## Syntax

Attributes use the form `@name` or `@name(arguments...)`. Arguments are
string or integer literals, identifiers, or named values (`name="value"`).
The parser preserves attributes it does not recognize; it does not maintain a
whitelist.

## Conditional compilation

### `@cfg(key="value")`

Conditionally includes declarations based on the build target:

```quazi
@cfg(target_os="windows") fn separator() str { ret "\\"; }
@cfg(target_os="linux") fn separator() str { ret "/"; }
```

Block form:

```quazi
@cfg(target_os="windows") {
    const platform_name: str = "Windows";
}
```

Supported keys:

| Key | Values |
| --- | --- |
| `target_os` | `"linux"`, `"windows"` |
| `target_arch` | `"x86_64"` |
| `target_abi` | `"sysv"`, `"win64"` |

`@cfg` on imports excludes disabled dependencies from target-aware discovery.

## C interoperability

### `@api("Symbol")`

Imports a C function or mutable global. The call uses Win64 ABI on Windows
and SysV ABI on Linux. All calls require unsafe context:

```quazi
@api("puts") unsafe fn puts(text: *c_char) c_int;
@api("errno") var errno_val: c_int;
```

Bare `@api` (without a string argument) uses the Quazi function name as the
native symbol. Explicit `@api("Symbol")` is recommended.

### `@export("Symbol")`

Exports a `pub` function under a stable C ABI symbol:

```quazi
@export("quazi_add")
pub fn add(left: c_int, right: c_int) c_int { ret left + right; }
```

Bare `@export` uses the function name. Only `@export` functions coerce to
raw `@repr(C)` callback pointers.

### `@repr(C)`

Declares C-compatible layout for structs, unions, and function-pointer
aliases:

```quazi
@repr(C) struct Point { x: f64, y: f64, }
@repr(C, packed) struct Header { magic: u32, flags: u8, }
@repr(C, align=16) struct Aligned { data: [u8; 64], }
@repr(C) type Callback = fn(i32, i32) i32;
```

Supports:

- Scalar and pointer fields.
- `packed` — no padding between fields.
- `align=N` — power-of-two alignment override.
- `union` — overlapping field storage.
- Named integer bitfields.
- Final `[T; ..]` flexible array members (pointer-only, unsafe).
- By-value FFI for arguments and return values.

Empty and generic `@repr(C)` forms are rejected.

### `@opaque`

Declares an empty, non-generic foreign handle type:

```quazi
@opaque pub struct NativeHandle {}
```

Quazi cannot construct opaque types. They exist as pointer targets for FFI.

### `@syscall("name"/num)`

Declares a raw syscall wrapper. The body becomes a `Syscall + Ret`
instruction sequence. Implicitly unsafe:

```quazi
@syscall("write") unsafe fn sys_write(fd: i32, buf: *u8, count: usize) isize;
```

## Code generation

### `@intrinsic("quazi.X")`

Declares a safe standard-library wrapper around a compiler-internal
operation. Dispatched by encoder case number. Unsafety is handled internally;
callers do not need `unsafe`:

```quazi
@intrinsic("quazi.alloc") fn alloc(size: usize) *u8;
```

### `@inline`

Requests inlining. Recursive functions remain excluded:

```quazi
@inline
fn fast_add(a: i32, b: i32) i32 { ret a + b; }
```

## Trait derivation

### `@derive(Trait, ...)`

Registers derived traits for a struct:

```quazi
@derive(Serialize)
struct Config {
    name: String,
    count: i64,
    enabled: bool,
}
```

Currently generates implementations for:

- `Serialize` — JSON serialization for non-generic structs with `bool`,
  `i64`, and owned `String` fields. Respects `@json(name="...")` field
  attributes. Unsupported field types produce an `S14` diagnostic.

Other derived trait names are recorded as metadata but do not generate code.

## Warning control

### `@ignore`

Suppresses all warnings on a declaration:

```quazi
@ignore
fn unused_helper() void { ... }
```

### `@ignore(category)`

Suppresses specific warning categories:

- `@ignore(unused_vars)` — suppresses W01/W02 unused-variable warnings.
- `@ignore(dead_code)` — suppresses W03/W07 dead-code warnings.

## Testing

### `@test`

Marks a zero-argument `void` function for `qz test`:

```quazi
@test
fn addition_works() void {
    if (20 + 22 != 42) {
        panic("addition produced the wrong value");
    }
}
```

Tests must use `fn name() void`, take no attribute arguments, and have a
body. They cannot be named `main`. See [testing](../TESTING.md).

## Panic handling

### `@panic_handler`

Declares a terminal panic handler:

```quazi
@panic_handler
fn handle_panic(info: PanicInfo) ! {
    core.exit(101);
}
```

Exactly one non-generic, non-variadic `fn(PanicInfo) !` handler per program.
See [panic handling](panic.md).

## Field attributes

Struct and union fields accept postfix attributes after their type:

```quazi
struct User {
    name: String @ini("username") @json(name="user_name"),
    age: u32 @ini("age"),
}
```

Field attributes are opaque language metadata. Quazi assigns no built-in
meaning to them. A library, derive implementation, or external tool defines
the interpretation. No attribute registration is needed: choose an identifier,
document its arguments, and apply it.

## Package settings (not source attributes)

Standard-library inclusion, crash-handler registration, and native symbol
mangling are configured in `quazi.toml`, not through source attributes:

```toml
[package]
std = true           # inject prelude and resolve std
crash_handler = true # register crash handler
mangling = true      # module-qualify function names
```

The removed `@no_std`, `@no_crash`, and `@no_mangle`/`@no_mangling` source
attributes are replaced by these package settings.
