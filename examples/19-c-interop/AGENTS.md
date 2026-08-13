# FFI round-trip example

This example exercises both directions of the initial Linux x86-64 C ABI:

- `[cc].sources` compiles `native/helper.c` through `$CC` or `cc`.
- `@api("c_roundtrip")` imports the C symbol and requires `unsafe` at the call.
- `@export("quazi_multiply") pub fn` exports an unmangled C ABI symbol.
- `std.ffi.c_int` documents the platform C integer type.

Run with `qz run` from this directory. Success exits with status zero after C
calls back into the exported Quazi function.
