# Declarations

Audience: language users and implementers.

## Functions

```quazi
fn name(param: Type) ReturnType {
    ret value;
}
```

Functions may return `void` (explicitly or by omission), a value type, or `!`
(never returns). Parameters are typed. Recursion uses normal call semantics;
a terminating base case is the programmer's responsibility.

### Generic functions

```quazi
fn first[T](items: Array[T]) T {
    ret items[0];
}
```

Type parameters are declared in `[T, U]` after the function name. Generic
calls normally infer arguments; explicit arguments use `function[Type](...)`.

### Variadic functions

```quazi
pub fn sum(first: i32, ...rest: i32) i32 { ... }
```

A typed final parameter `...name: Type` collects additional arguments. Bare
`...` in `@api` declarations marks C variadic calling convention.

### Unsafe functions

```quazi
unsafe fn read_raw(pointer: *u8) u8 {
    ret *pointer;
}
```

Functions with raw pointer parameters (`*T`) must be declared `unsafe`. Calling
them requires an unsafe context. `@syscall` and `@api` functions are
implicitly unsafe.

### Entry points

Explicit generic function calls must supply the declaration's exact number of
type arguments; a wrong explicit count is `S08`. Specialization types currently
have compiler limits of 64 constructor levels and 4,096 type nodes across their
arguments. Expanding recursion or colliding emitted specialization names is
rejected with `S14`, rather than silently selecting another representation.
See [generic-call diagnostics](../changes/2026-10-09-ownership-specialization-graph.md).

```quazi
fn main() i32 { ret 0; }
fn main(args: Array[str]) i32 { ret args.len() as i32; }
```

`main` may take no arguments or a single `Array[str]`. It may omit its return
type, return `i32`, or return `!`. Argument-aware programs receive the
executable path at `args[0]`.

## Structs

```quazi
pub struct Point { x: f64, y: f64, }

struct Config {
    name: str,
    const version: i32,
}
```

Fields may be `const` (immutable after construction). `pub` makes a struct
visible outside its module. Structs are constructed with field initialization
syntax:

```quazi
const origin = Point { x: 0.0, y: 0.0 };
```

### Field attributes

Struct and union fields accept postfix attributes after their type:

```quazi
struct User {
    name: String @ini("username") @json(name="user_name"),
    age: u32 @ini("age"),
}
```

Field attributes are opaque language metadata. Quazi itself assigns no
built-in meaning; libraries, derive implementations, or tools choose their
interpretation. Metadata is retained in the parsed AST and public QZI
interfaces.

### Generic structs

```quazi
pub struct Pair[A, B] { left: A, right: B, }
```

### Layout

Ordinary structs use compiler-defined layout. `@repr(C)` gives target C
layout for supported scalar fields and aggregates. See
[attributes](attributes.md) for `@repr(C)` details.

## Enums

```quazi
pub enum Option[T] { Some(T), None, }
enum Shape { Circle(f64), Rectangle(f64, f64), }
```

Variants may carry zero or more payload values. Construction:

```quazi
const some: Option[i32] = Some(42);
const none: Option[i32] = None;
const circle: Shape = Shape.Circle(5.0);
```

Qualified construction (`EnumName.Variant(...)`) is validated for variant name,
arity, and payload representation. Matching must be exhaustive.

## Traits

```quazi
pub trait Area {
    fn area(self: Rectangle) f64;
}
```

Traits declare method signatures. The `self` parameter names the receiver
type explicitly. Trait methods may have default implementations.

### Generic traits

```quazi
trait Convert[T] {
    fn convert(self: Self) T;
}
```

## Impl blocks

### Inherent implementations

```quazi
impl Rectangle {
    fn square(size: f64) Rectangle {
        ret Rectangle { width: size, height: size };
    }
}
```

Static methods (without `self`) are called as `Type.method(...)`.

Generic impl targets introduce implicit local binders. A bare name that repeats
a formal parameter of the target declaration is a binder, even if a global
alias or type has that name. For `struct Holder[T]`, `impl Holder[T]` is generic;
use `impl Holder[i32]` or a differently named concrete alias for a fixed target.
Undeclared bare target-argument names may rename binders, such as `impl Holder[U]`.
A declared concrete name that does not repeat a target formal parameter stays
concrete, and qualified names do not introduce binders.

### Trait implementations

```quazi
impl Area for Rectangle {
    fn area(self: Rectangle) f64 {
        ret self.width * self.height;
    }
}
```

## Type aliases

```quazi
type Rune = u32;
```

Aliases are transparent to the type system. The compiler resolves them
before type checking. `@repr(C)` function-pointer aliases define raw C
callback types:

```quazi
@repr(C) type CCallback = fn(i32, i32) i32;
```

Generic aliases require exactly their declared number of type arguments.
Alias parameters are lexical: `type Id[T] = T` uses its parameter even if a
global alias named `T` exists. Nested uses such as `Id[Id[i32]]` are valid.
Recursive aliases, including cycles through pointers, are rejected with `S14`,
even when unused. Use a named struct or enum for recursive data.

Expansion is bounded by 64 type-constructor levels and 4,096 expanded nodes,
with separate limits for alias-chain depth, resolution work, and type-name
storage. Exceeding a bound produces a diagnostic; the compiler never silently
truncates the type. See [alias-resolution limits](../internals/type-alias-resolution.md).

## Derive

```quazi
@derive(Clone, Display)
struct Point { x: f64, y: f64, }
```

`@derive(Trait, ...)` registers derived traits for a struct. Currently
supported:

- `Serialize` — compiler-generated JSON serialization for non-generic structs
  with `bool`, `i64`, and owned `String` fields. Respects `@json(name="...")`
  field attributes for wire names. Unsupported field types produce an `S14`
  diagnostic.

Other derive names are recorded as metadata but do not generate
implementations.

## Visibility

`pub` on functions, types, constants, and imports makes them visible outside
their module:

```quazi
pub fn api_function() void { ... }
pub struct PublicType { ... }
pub import codec.Decoder;
```

Private declarations (without `pub`) are accessible only within their file.
`pub import` re-exports an imported name from the current module.
