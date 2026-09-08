# Guide: Understanding diagnostics

Audience: Quazi users.

This guide helps you understand and fix common compiler errors and warnings.

## Error format

Quazi diagnostics include a code, location, and message:

```
src/main.qz:5:12: error[S04]: cannot access private declaration 'internal_fn'
```

The format is `file:line:column: level[code]: message`.

## Semantic errors (S-codes)

### S04 — Visibility or access error

```
error[S04]: cannot access private declaration 'helper'
```

The declaration is not `pub` and you are importing it from another module.

**Fix**: Add `pub` to the declaration, or move the code into the same module.

### S08 — Arity or signature mismatch

```
error[S08]: expected 2 arguments, found 3
```

The function call has the wrong number of arguments.

**Fix**: Check the function signature and adjust your call.

### S11 — Unsafe operation in safe context

```
error[S11]: dereference of raw pointer requires unsafe context
```

You are using a pointer dereference, calling an `unsafe fn`, or invoking an
`@api` function outside an `unsafe` block.

**Fix**: Wrap the operation in `unsafe { ... }`.

### S12 — Unsafe function missing `unsafe` keyword

```
error[S12]: function with pointer parameter must be declared 'unsafe fn'
```

A function accepting `*T` parameters must be declared `unsafe fn`.

**Fix**: Add `unsafe` to the function declaration.

### S14 — Unsupported type layout

```
error[S14]: multi-slot value cannot be stored in this position
```

A value type is too wide for the current storage position (enum payload,
struct field outside `@repr(C)`, or fixed-array element).

**Fix**: Use `Box[T]` to heap-allocate the value, or restructure the type.

## Type errors

### Type mismatch

```
error: expected 'i32', found 'str'
```

The expression type does not match what is expected.

**Fix**: Use explicit `as` conversions for numeric types, or adjust the
expression.

### Move after use

```
error: use of moved value 'buffer'
```

An owned value was used after being moved to another binding or function.

**Fix**: Use the value before moving it, clone if possible, or restructure
to avoid the move.

### Cannot borrow

```
error: cannot mutate 'x' while it is borrowed
```

A shared reference `&x` exists and you are trying to modify `x`.

**Fix**: Complete the borrow before mutating, or restructure the code.

## Warnings (W-codes)

### W01 / W02 — Unused variable

```
warning[W01]: unused variable 'result'
```

A `var` or `const` binding is declared but never used.

**Fix**: Use the variable, remove it, or suppress with `@ignore(unused_vars)`.

### W03 / W07 — Dead code

```
warning[W03]: function 'helper' is never called
```

A function or declaration is defined but never referenced.

**Fix**: Use it, remove it, or suppress with `@ignore(dead_code)`.

## Parse errors

### Unterminated string

```
error: unterminated string literal
```

A quoted string is missing its closing `"`, or a raw backtick string is
missing its closing `` ` ``.

### Invalid escape

```
error: invalid escape sequence '\\q'
```

A quoted string contains an unrecognized escape. Valid escapes include `\n`,
`\t`, `\\`, `\"`, `\xNN`, `\uNNNN`, `\u{H...}`, and others listed in the
[lexical specification](../language/lexical.md).

**Fix**: Use a valid escape or switch to a raw backtick string.

### Missing semicolon

```
error: expected ';' after statement
```

Quazi statements must end with `;`.

## Linker errors

### Unresolved symbol

```
error: unresolved symbol 'pthread_create'
```

The native linker cannot find a required symbol.

**Fix**: Add the library to `[link].libraries` in `quazi.toml`, or use an
external linker with the appropriate `-l` flag.

### Unsupported section

```
error: unsupported loaded section '.tls'
```

A native object file contains a section type not supported by the built-in
linker (e.g., thread-local storage).

**Fix**: Use an external linker for complex native dependencies.

## Suppressing warnings

### Per-declaration

```quazi
@ignore
fn unused_helper() void { ... }

@ignore(unused_vars)
fn example(x: i32) void { ... }

@ignore(dead_code)
fn reserved() void { ... }
```

### Build-wide

There is no build-wide warning suppression. Address warnings individually.

## Getting help

- [Language specification](../language/README.md) — authoritative syntax and
  semantics.
- [Standard-library API](../api/README.md) — function signatures and error
  types.
- [Examples](../../examples/README.md) — 35 runnable programs.
- [Testing guide](testing.md) — writing and running tests.
