# Chapter 2: Variables and types

This chapter covers variables, constants, primitive types, and basic
operations.

## Constants and variables

`const` declares an immutable binding — it cannot be changed after
initialization:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const answer: i32 = 42;
const greeting: str = "Hello";
```

`var` declares a mutable binding:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
var count: i32 = 0;
count = count + 1;
count += 5;
count++;
```

## Type inference

When the type is clear from the initializer, you can omit the annotation:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const pi = 3.14159;     // inferred as f64
const flag = true;       // inferred as bool
var name = "Quazi";      // inferred as str
```

## Numeric types

### Integers

| Type | Size | Range |
| --- | --- | --- |
| `i8` | 8 bits | -128 to 127 |
| `i16` | 16 bits | -32768 to 32767 |
| `i32` | 32 bits | ±2 billion |
| `i64` | 64 bits | ±9.2 quintillion |
| `u8` | 8 bits | 0 to 255 |
| `u16` | 16 bits | 0 to 65535 |
| `u32` | 32 bits | 0 to 4 billion |
| `u64` | 64 bits | 0 to 18.4 quintillion |
| `isize` | pointer-width | signed |
| `usize` | pointer-width | unsigned |

### Floating point

| Type | Size |
| --- | --- |
| `f32` | 32-bit IEEE-754 |
| `f64` | 64-bit IEEE-754 |

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const temperature: f64 = 36.6;
const tiny: f32 = 0.001;
```

### Boolean

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const yes: bool = true;
const no: bool = false;
```

## Type conversions

Use `as` for explicit conversions within the integer family:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const x: i32 = 42;
const y: i64 = x as i64;
const narrow: u8 = 256 as u8; // truncates to 0
```

Integer-to-float and float-to-integer casts are not implemented by the current
compiler. Write a correctly typed literal when practical, or keep the value in
its original numeric family until an explicit conversion API is available.

## Strings

`str` is immutable UTF-8 text. `String` is owned growable text:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const borrowed: str = "hello";
var owned: String = String.from("world");
```

String indexing counts Unicode scalars. Negative indexes count from the end:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const text: str = "Aλ🙂";
const first = text[0];     // 'A' as Rune (u32)
const last = text[-1];     // 🙂 as Rune
const length = text.len(); // 3
```

Python-style slicing:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const slice: String = text[1:];    // "λ🙂"
const rev: String = text[::-1];    // "🙂λA"
```

## Byte strings

`bytes` holds immutable arbitrary data without a UTF-8 guarantee:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const header: bytes = b"PNG\x0D\x0A";
const raw: bytes = br"\x00 as text";
const size = header.len(); // 5
```

## Arithmetic

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const sum = 3 + 4;
const diff = 10 - 3;
const product = 6 * 7;
const quotient = 15 / 4;    // integer: 3
const remainder = 15 % 4;   // 3
const power = 2 ** 10;      // 1024
const ratio = 15.0 / 4.0;   // float: 3.75
```

Integer division by zero panics. Float division by zero produces infinity.

## Comparison and logic

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const equal = 5 == 5;       // true
const less = 3 < 7;         // true
const both = true && false;  // false (short-circuits)
const either = true || false; // true (short-circuits)
const flipped = !true;      // false
```

## Bitwise operations

```quazi
// tutorial: fragment — requires the surrounding chapter context.
var bits: u32 = 0xFF;
bits = bits & 0x0F;  // AND: 0x0F
bits = bits | 0xF0;  // OR: 0xFF
bits = bits ^ 0x0F;  // XOR: 0xF0
bits = bits << 4;    // left shift
bits = bits >> 4;    // right shift
```

## Printing values

Use `{}` placeholders to format values:

```quazi
// tutorial: runnable
import std.io;

fn main() i32 {
    const name: str = "Quazi";
    const version: i32 = 1;
    io.println("Welcome to {} v{}", name, version);
    ret 0;
}
```

## Next steps

Continue to [Chapter 3: Control flow](03-control-flow.md) to learn about
conditionals and loops.
