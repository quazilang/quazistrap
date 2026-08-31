# Quazi Language Guide

This guide documents the implemented language surface. Quazi source files use
`.qz`; statements end with `;`; names and module paths use `.`, never `::`.

## Program entry points

```quazi
fn main() i32 { ret 0; }
fn main(args: Array[str]) i32 { ret args.len() as i32; }
```

`main` may omit its return type, return `i32`, or return `!`. Argument-aware
programs receive the executable path at `args[0]`. `ret expression;` returns a
value; `ret;` returns from a `void` function.

## Bindings and literals

```quazi
const answer: i32 = 42;
var count: i32 = 0;
var inferred = 3.5;
var declared: str;
declared = "ready";
```

`const` requires an initializer and cannot be assigned later. `var` is mutable.
Type annotations may be inferred from initializers. Literals include decimal
integers, floating-point values, `true`, `false`, strings, and byte strings.

```quazi
const escaped: str = "line one\nline two";
const raw: str = `C:\data\file.txt`;
const data: bytes = b"PNG\x0D\x0A";
const raw_data: bytes = br"\x00 is text here";
```

Quoted strings decode common C escapes, hexadecimal/octal escapes, Unicode
escapes, and escaped newlines. Backtick strings preserve contents exactly.
`bytes` is immutable and length-carrying; `.as_ptr()` is its explicit FFI escape.

## Types

- Signed integers: `i8`, `i16`, `i32`, `i64`, `isize`.
- Unsigned integers: `u8`, `u16`, `u32`, `u64`, `usize`.
- Floating point: `f16`, `f32`, `f64`.
- Other primitives: `bool`, `str`, `bytes`, `void`, `!`.
- Containers/references: `[T; N]`, `[T]`, `&T`, `*T`.
- Named/generic types: `Name`, `Name[T, U]`.
- Functions: `fn(T, U) V`; trait objects: `dyn Trait`.

`as` performs explicit conversion:

```quazi
const wide: i64 = 42 as i64;
const ratio: f64 = 3 as f64 / 2.0;
```

`str` is immutable UTF-8. Indexes count Unicode scalars, negative indexes count
from the end, and slices follow Python spelling: `text[start:end:step]`.
`String` is the owned growable string from the prelude.

`any` is reserved syntax, not a runtime value type. Quazi currently has no
tagged dynamic-value representation, so `any` is rejected in variables,
fields, parameters, returns, casts, and generic arguments. The sole supported
use is the final `...args: any` pseudo-parameter of an `@format` function; the
compiler converts each argument at the call site and the function body cannot
access that pseudo-parameter. Use a generic parameter for static polymorphism,
`dyn Trait` for trait-object dispatch, or an exact `@repr(C)` callback at a
foreign boundary.

## Operators

From lower to higher intent: assignment `=`, logical `||`/`&&`, bitwise
`|`/`^`/`&`, equality `==`/`!=`, ordering `< <= > >=`, shifts `<< >>`,
addition `+ -`, multiplication `* / %`, power `**`, unary `- ! & *`, casts
with `as`, calls/indexing/fields. Parentheses remove ambiguity.

Mutable lvalues support `+=`, `-=`, `*=`, `/=`, `%=`, prefix/postfix `++` and
`--`. Logical operations short-circuit. Integer bitwise operations preserve
their integer type.

## Functions and closures

Functions may call themselves directly or through other functions. Recursion
uses normal call semantics; a terminating base case remains the programmer's
responsibility. Example 28 uses a recursive factorial library through a Git
dependency.

```quazi
fn clamp(value: i32, min: i32, max: i32) i32 {
    if (value < min) { ret min; }
    if (value > max) { ret max; }
    ret value;
}

fn greet(name: str, punctuation: str) void {}
greet(punctuation="!", name="Quazi");

var double: fn(i32) i32 = |value| value * 2;
```

Closure parameters are inferred from an expected `fn(...) Return` type. That
context may come from a typed binding, assignment, return, or an argument to a
function value, module function, inherent method, or dynamic trait method. A
standalone closure binding therefore needs an explicit function type, as in
`double` above.

Quazi `fn` values are affine owners of their closure environment. Assigning,
passing, or returning one transfers ownership; using the previous binding after
the move is an error. Replacing a binding destroys its previous environment,
and the last owner is destroyed at scope exit. Calling a function value only
borrows it, so it may be called repeatedly.

The current safe closure checkpoint permits immutable captures and closure
parameters/results only when their runtime value is a plain scalar (`bool`, a
number, raw pointer, or C function pointer). Owned values, strings, references,
mutable captures, and `fn` values nested inside arrays or named aggregates are
rejected until recursive environment and aggregate destruction is available.
Callable parameter and return types must match exactly; numeric conversions do
not adapt a function signature. See [Migrating function values and
closures](migrations/closures.md).

Until cleanup state becomes path-sensitive, an outer `fn` owner cannot be moved
from only one branch, loop iteration, short-circuit operand, or match arm. Move
it before control flow or create and consume the owner entirely inside that
path. A function-valued assignment is not itself transferable: assign first,
then move the binding. Match expressions cannot currently produce `fn` values.

Positional arguments precede named arguments. Quazi variadics declare a typed
final parameter as `...values: T`; bare `...` is reserved for C variadic
`@api` declarations.

## Branches and loops

```quazi
if (temperature < 0) { ... }
else if (temperature < 20) { ... }
else { ... }

for i : 0..10 { ... }              // exclusive upper bound
for value : values { ... }
for index, value : values { ... }
for (var i = 0; i < 10; i++) { ... }
for (condition) { ... }
for { ... }
```

`break;` exits the nearest loop; `continue;` begins its next iteration. Quazi
uses `for` for range, iterator, C-style, while-like, and infinite loops.
Outside loop headers, `start..end` and `start..=end` produce the prelude `Range`
value with an exclusive or inclusive upper bound. APIs such as
`std.random.range(0..=5)` consume that value.

## Structs, enums, and matching

```quazi
struct Point { x: f64, y: f64, }
const origin = Point { x: 0.0, y: 0.0 };

enum Message[T] { Data(T), Closed, }
const label = match message {
    Data(value) if value > 0 => "positive",
    Data(_) => "other data",
    Message.Closed => "closed",
};
```

Matches support literals, bindings, `_`, qualified/unqualified enum variants,
nested payload patterns, and `if` guards. Enum matches must be exhaustive.

## Methods, traits, and generics

```quazi
trait Area { fn area(self: Rectangle) f64; }

struct Rectangle { width: f64, height: f64, }

impl Rectangle {
    fn square(size: f64) Rectangle {
        ret Rectangle { width: size, height: size };
    }
}

impl Area for Rectangle {
    fn area(self: Rectangle) f64 { ret self.width * self.height; }
}

fn first[T](items: Array[T]) T { ret items[0]; }
```

Types, traits, aliases, functions, and methods may declare `[T, U]`. Generic
calls normally infer arguments; explicit arguments use `function[Type](...)`.
`@derive(Trait, ...)` registers supported derived traits. `dyn Trait` represents
dynamic dispatch through a data pointer and vtable.

## `Option`, `Result`, and `?`

`Option[T]` and `Result[T, E]` are prelude types. Match them directly or use
their methods. `expression?` unwraps `Some`/`Ok`; `None`/`Err` returns early
from a compatible function.

```quazi
fn port(text: str) Result[u16, ParseError] {
    const value = text.parse[u16]()?;
    ret Ok(value);
}
```

## Modules and visibility

```quazi
import std.io;
import store.Product;
import store.{find, save};
import store.find as lookup;
import ./local_helper.value;
pub import codec.Decoder;
```

Each file is a module. A directory module uses `mod.qz`. Imports are lazy and
dotted. `./` forces local resolution. `pub` on functions/types exposes them;
`pub import` intentionally re-exports them. Private declarations stay inside
their file. See [MODULES_AND_PACKAGES.md](MODULES_AND_PACKAGES.md).

## Safety and ownership

Safe references use `&T`; raw pointers use `*T`. Raw dereference and calls to
`unsafe fn`, `@api`, or `@syscall` require an unsafe context:

```quazi
unsafe fn read_raw(pointer: *u8) u8 { ret *pointer; }
unsafe { const byte = read_raw(pointer); }
```

Shared references currently use a conservative lexical model:

- `&value` accepts only a local variable or parameter (parentheses are fine).
- A value never converts into a reference, and reference pointee types are
  invariant: `&i32` is not `&u64`.
- A shared-reference binding cannot be rebound, returned, stored in an owned
  aggregate, or captured by a closure. The referenced owner cannot be mutated,
  moved, or passed to a method while that borrow remains in the function.
- Fields, indexes, dereferences, calls, and temporary expressions are not yet
  valid address-of operands. These need real place-address lowering.
- Dereferencing a shared reference materializes only scalar/value-like
  pointees. Aggregate pointees require immutable receiver/view semantics; a
  shallow aggregate load would otherwise create a mutable alias.
- `str`/`&str` retain their existing representation-identical string-view rule.

These restrictions keep references sound before lifetime parameters and
mutable-reference syntax exist. A direct `&local` may be stored or passed as an
exact raw pointer, but dereferencing that pointer or calling an unsafe function
still requires an unsafe context. Raw pointers never convert back into safe
references.

Owned values such as `String`, `Array[T]`, `Box[T]`, and OS handles are cleaned
at lexical scope exit, including early returns. Returning or moving an owned
value transfers ownership; use-after-move is rejected. Borrowed method receivers
do not consume their owner. See [TYPES_AND_MEMORY.md](TYPES_AND_MEMORY.md).

## Attributes

Attributes have a universal syntactic form: `@name(arguments...)`. Their
arguments are string or integer literals, identifiers, or named values such as
`name="value"`. The parser preserves attributes it does not recognize; it never
maintains a whitelist for third-party metadata.

Struct and union fields accept postfix attributes after their type (and, for C
bitfields, before or after the width):

```quazi
struct User {
    name: String @ini("username") @json(name="user_name"),
    age: u32 @ini("age"),
}
```

These field attributes are opaque language metadata. Quazi itself does not
make `@ini`, `@json`, or any other field attribute imply serialization,
validation, or layout behavior. A library, derive implementation, or external
tool chooses its meaning. Metadata is retained in the parsed AST and public QZI
interfaces so tooling can handle future community attributes without a parser
upgrade. Runtime reflection is not implied by this mechanism.

### Custom attribute API

There is no attribute-registration declaration: an attribute author creates a
custom field attribute by choosing an identifier and documenting its arguments
and meaning. For example, a serialization package may define
`@my_serializer(name="wire_name")`; users can apply it immediately to an
appropriate field. This deliberately prevents a new community attribute from
requiring a compiler or parser update. A consuming library or tool is
responsible for reporting an unknown, misplaced, or invalid attribute in its
own domain.

- `@cfg(target_os="linux")`, `target_arch`, `target_abi`: conditional compile.
- `@inline`: request inlining; recursive functions remain excluded.
- `@derive(...)`: register derived traits.
- `@ignore`, `@ignore(unused_vars)`, `@ignore(dead_code)`: warning control.
- `@test`: declare a zero-argument `void` test run by `qz test`.
- `@panic_handler`: declare the validated panic handler.
- `@repr(C)`, `@opaque`, `@api`, `@export`: C interoperability.
- `@intrinsic`, `@syscall`: compiler/standard-library implementation tools.

Standard-library inclusion, crash registration, and native symbol mangling are
package settings in `quazi.toml`, not source attributes. See
[PROJECTS.md](PROJECTS.md) and [TESTING.md](TESTING.md).

## Platform code

```quazi
@cfg(target_os="windows") fn separator() str { ret "\\"; }
@cfg(target_os="linux") fn separator() str { ret "/"; }

@cfg(target_os="windows") {
    const platform_name: str = "Windows";
}
```

Supported target keys are `target_os`, `target_arch`, and `target_abi`.
Current production triples are `x86_64-linux`/`sysv` and
`x86_64-windows`/`win64`.
