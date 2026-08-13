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
- Other primitives: `bool`, `str`, `bytes`, `void`, `any`, `!`.
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

Owned values such as `String`, `Array[T]`, `Box[T]`, and OS handles are cleaned
at lexical scope exit, including early returns. Returning or moving an owned
value transfers ownership; use-after-move is rejected. Borrowed method receivers
do not consume their owner. See [TYPES_AND_MEMORY.md](TYPES_AND_MEMORY.md).

## Attributes

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
