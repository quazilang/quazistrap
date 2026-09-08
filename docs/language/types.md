# Type system

Audience: language users and implementers.

## Primitive types

### Signed integers

`i8`, `i16`, `i32`, `i64` — fixed-width two's complement integers.
`isize` — pointer-width signed integer (64 bits on x86-64).

### Unsigned integers

`u8`, `u16`, `u32`, `u64` — fixed-width unsigned integers.
`usize` — pointer-width unsigned integer (64 bits on x86-64).

Signedness affects comparisons, right shifts (sign-preserving for signed,
zero-extending for unsigned), extension, parsing, and C ABI classification.

### Floating point

`f16`, `f32`, `f64` — IEEE-754 floating-point types. `f16` is a source type;
the compiler preserves required target conversions at C boundaries. Division by
zero produces signed infinity; `0.0 / 0.0` produces NaN. These values are
ordinary floats that propagate through calculations.

### Boolean

`bool` — `true` or `false`. Used in conditions and logical operations.

### Void and never

`void` — the unit type. Functions with no meaningful return value use `void`.

`!` — the never type. Functions that never return (such as `@panic_handler`
handlers or infinite loops that always exit through `core.exit`) use `!`.

## Text types

### `str` and `&str`

Immutable borrowed UTF-8 text. `str` and `&str` are interchangeable. Indexes
count Unicode scalar values (`Rune`, an alias of `u32`). Negative indexes
count from the end. `text[start:end:step]` follows Python's clamped,
end-exclusive slice rules. `len()` counts scalars; `bytes_len()` reports
encoded UTF-8 byte length.

### `String`

Owned growable UTF-8 text with pointer, byte length, and capacity.
`String.as_str()` borrows it as `str`. Local variables are cleaned
automatically at scope exit. Methods returning a new `String` allocate.

### `Rune`

Alias of `u32`. Represents a single Unicode codepoint. Returned by string
indexing.

## Byte types

### `bytes`

Immutable, length-carrying arbitrary byte data. Unlike `str`, it carries no
UTF-8 promise. Provides `len()`, indexing, and `as_ptr()` for FFI. `b"..."`
decodes byte escapes; `br"..."` preserves content exactly.

## Container types

### Fixed arrays

`[T; N]` — a fixed-size value containing exactly `N` elements of type `T`.
Indexing is bounds-checked in safe code.

```quazi
const rgb: [u8; 3] = [255, 128, 0];
const red: u8 = rgb[0];
```

### Slices

`[T]` — an unsized borrowed view of a contiguous sequence. Used as function
parameters for arrays.

### `Array[T]`

Owned growable collection from the prelude. Supports `push`, `get`, `set`,
`len`, `free`, and checked index assignment. Implements `Index` for `[]`
syntax.

```quazi
var items: Array[i32] = Array[i32].new();
items.push(42);
const first: i32 = items[0];
```

## Reference and pointer types

### Safe references

`&T` — a shared safe reference. Currently restricted to a conservative lexical
model:

- `&value` accepts only a local variable or parameter.
- Reference bindings cannot be rebound, returned, stored in aggregates, or
  captured by closures.
- The referenced owner cannot be mutated, moved, or passed to a method while
  the borrow exists.
- Fields, indexes, dereferences, and temporaries are not valid address-of
  operands.
- Pointee types are invariant: `&i32` is not `&u64`.
- Dereferencing a shared reference works for scalar pointees. Aggregate
  pointees require immutable receiver semantics.
- `str`/`&str` are the representation-identical string-view exception.

These restrictions keep references sound before lifetime parameters and
mutable-reference syntax exist.

### Raw pointers

`*T` — a raw native pointer. May be null, dangling, misaligned, or point to
invalid storage. Dereferencing requires an unsafe context. Integer `0` is the
null-pointer constant for any `*T`. All raw pointer types are mutually
compatible: `*T` converts to `*U`. Raw pointers serve FFI boundaries; typed
wrappers are preferred for application code.

## Named types

### Structs

User-defined product types with named fields:

```quazi
struct Point { x: f64, y: f64, }
```

Fields may be `const` (immutable after construction). `pub` makes a struct
visible outside its module. Layout is compiler-defined; use `@repr(C)` for C
compatibility.

### Enums

Sum types with optional payloads:

```quazi
enum Option[T] { Some(T), None, }
enum Shape { Circle(f64), Rectangle(f64, f64), }
```

Matching must be exhaustive. Variants are constructed by name; qualified
construction uses `EnumName.Variant(...)`.

### Type aliases

```quazi
type Rune = u32;
```

Aliases are transparent to the type system. `@repr(C)` function-pointer
aliases define raw C callback types.

## Generic types

Types, traits, functions, and methods may declare type parameters in `[T, U]`:

```quazi
fn first[T](items: Array[T]) T { ret items[0]; }
struct Pair[A, B] { left: A, right: B, }
```

Generic calls normally infer arguments. Explicit arguments use
`function[Type](...)`. The compiler monomorphizes each concrete instantiation.

## Function types

`fn(T, U) V` — a function value type. Quazi `fn` values own their closure
environment. Assigning, passing, or returning one transfers ownership.
Calling only borrows the environment. Use-after-move is rejected.

Currently, closure captures are limited to immutable plain scalars, and
function values cannot be placed inside arrays, structs, enums, or generic
type arguments.

`@repr(C) type Callback = fn(T, U) V` — a non-owning raw C function pointer.
Only `@export` functions coerce to callback values; invocation is unsafe.

## Trait objects

`dyn Trait` — dynamic dispatch through a data pointer and vtable.

## Prelude types

The prelude automatically provides `String`, `Box[T]`, `Array[T]`,
`Option[T]`, `Result[T, E]`, common traits, `Range`, `fmt`, `PanicInfo`, and
parse errors. No import is required for these types.

## The `any` keyword

`any` is reserved syntax, not a runtime value type. It is rejected in
variables, fields, parameters, returns, casts, and generic arguments. The only
supported use is the final `...args: any` pseudo-parameter of an `@format`
function; the compiler converts each argument at the call site.

## Conversions

`as` performs explicit type conversion:

```quazi
const wide: i64 = 42 as i64;
const ratio: f64 = 3 as f64 / 2.0;
const byte: u8 = 256 as u8; // truncation
```

Conversions that may change representation are always explicit. Integer
overflow checking is not a substitute for input validation; parsing returns
`Result` and rejects malformed or out-of-range values.
