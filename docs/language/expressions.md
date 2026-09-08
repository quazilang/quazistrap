# Expressions

Audience: language users and implementers.

## Operator precedence

From lowest to highest precedence:

| Precedence | Operators | Associativity |
| --- | --- | --- |
| 1 | `=`, `+=`, `-=`, `*=`, `/=`, `%=` | Right |
| 2 | `\|\|` | Left |
| 3 | `&&` | Left |
| 4 | `\|` | Left |
| 5 | `^` | Left |
| 6 | `&` | Left |
| 7 | `==`, `!=` | Left |
| 8 | `<`, `<=`, `>`, `>=` | Left |
| 9 | `<<`, `>>` | Left |
| 10 | `+`, `-` | Left |
| 11 | `*`, `/`, `%` | Left |
| 12 | `**` | Right |
| 13 | Unary `-`, `!`, `&`, `*` | Prefix |
| 14 | `as` | Left |
| 15 | Calls, indexing, field access | Left |

Parentheses override precedence.

## Arithmetic

```quazi
const sum: i32 = 3 + 4;
const product: f64 = 2.5 * 4.0;
const power: i32 = 2 ** 10;
const remainder: i32 = 17 % 5;
```

Integer division by zero panics. Floating-point division by zero produces
signed infinity; `0.0 / 0.0` produces NaN.

## Compound assignment

Mutable lvalues support `+=`, `-=`, `*=`, `/=`, `%=`:

```quazi
var count: i32 = 0;
count += 1;
count *= 2;
```

Prefix and postfix `++` and `--` are supported:

```quazi
count++;
++count;
count--;
```

## Comparison

`==`, `!=`, `<`, `<=`, `>`, `>=` compare values. String comparisons compare
contents lexicographically, not addresses. Signedness affects integer
comparisons.

## Logical operators

`&&` (logical AND) and `||` (logical OR) short-circuit. `!` is logical NOT:

```quazi
var ok: bool = true && false;
ok = true || false;
ok = !ok;
```

## Bitwise operators

`&` (AND), `|` (OR), `^` (XOR), `<<` (left shift), `>>` (right shift).
Right shift is sign-preserving for signed integers and zero-extending for
unsigned integers. Results preserve the integer type:

```quazi
var bits: u32 = 0xFF & 0x0F;
bits = bits | 0x01;
bits = bits ^ 0x0F;
bits = bits << 2;
bits = bits >> 1;
```

## Type casts

`as` performs explicit type conversion between numeric types and between
pointer types:

```quazi
const wide: i64 = 42 as i64;
const ratio: f64 = 3 as f64 / 2.0;
const index: usize = length as usize;
```

## Function calls

```quazi
const result: i32 = compute(10, 20);
```

### Named arguments

Positional arguments must precede named arguments:

```quazi
greet(punctuation="!", name="Quazi");
```

### Quazi variadics

A typed final parameter `...values: T` collects additional arguments:

```quazi
fn sum(first: i32, ...rest: i32) i32 { ... }
sum(1, 2, 3, 4);
```

Bare `...` is reserved for C variadic `@api` declarations.

## Indexing

`collection[index]` accesses elements. Safe indexing is bounds-checked:

```quazi
const first: i32 = items[0];
items[0] = 42;
```

Index assignment is supported through the `Index` trait.

## Field access

```quazi
const x: f64 = point.x;
const name: str = user.name;
```

## Method calls

Methods are called with dot syntax:

```quazi
const length: usize = text.len();
const upper: String = text.to_uppercase();
```

## Closures

Closure expressions create function values. Parameters are inferred from an
expected `fn(...) Return` type:

```quazi
var double: fn(i32) i32 = |value| value * 2;
var add: fn(i32, i32) i32 = |a, b| a + b;
```

Context for parameter inference may come from a typed binding, assignment,
return, or a function argument. A standalone closure binding needs an explicit
function type.

Current closure limitations:

- Captures are limited to immutable plain scalars (`bool`, numbers, raw
  pointers, C function pointers).
- Owned values, strings, references, mutable captures, and `fn` values inside
  arrays or aggregates are rejected.
- Parameter and return types must match exactly; numeric conversions do not
  adapt a function signature.
- An outer `fn` owner cannot be moved from only one branch of control flow.

See [closures migration](../migrations/closures.md).

## Match expressions

`match` evaluates an expression against a series of patterns:

```quazi
const label: str = match message {
    Data(value) if value > 0 => "positive",
    Data(_) => "other data",
    Message.Closed => "closed",
};
```

Match supports:

- Literal patterns: integers, strings, booleans.
- Binding patterns: `name` captures the matched value.
- Wildcard: `_` matches anything without binding.
- Enum variant patterns: `Variant(payload)`, `EnumName.Variant(payload)`.
- Nested payload patterns.
- Guards: `if condition` after a pattern.

Enum matches must be exhaustive: every variant must be covered, or a `_`
wildcard must be present.

## Range expressions

`start..end` creates an exclusive-upper-bound `Range`. `start..=end` creates
an inclusive-upper-bound range:

```quazi
for i : 0..10 { ... }      // 0 through 9
for i : 0..=10 { ... }     // 0 through 10
```

Outside loop headers, range expressions produce the prelude `Range` value,
consumed by APIs such as `std.random.range(0..=5)`.

## Error propagation

`expression?` unwraps `Some`/`Ok` values. `None` or `Err` returns early from
a function with a compatible return type:

```quazi
fn port(text: str) Result[u16, ParseError] {
    const value = text.parse[u16]()?;
    ret Ok(value);
}
```

## String operations

String expressions support content comparison, slicing, and search:

```quazi
const equal: bool = name == "Quazi";
const slice: String = text[1:5];
const reversed: String = text[::-1];
const found: bool = text.contains("hello");
```

See [primitive APIs](../PRIMITIVE_APIS.md) for the complete string method
reference.
