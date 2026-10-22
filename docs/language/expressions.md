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

`&&` (logical AND) and `||` (logical OR) short-circuit. `!` is logical NOT.

`&&` and `||` accept booleans or integers. Zero is false and nonzero is true;
the result is always a `bool`, with canonical `false`/`true` values, rather than
the integer value of an evaluated operand. The right operand is evaluated only
when needed to determine the result.

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

`as` performs explicit conversion within the integer family and between
supported pointer/function-pointer representations:

```quazi
const wide: i64 = 42 as i64;
const index: usize = length as usize;
```

Integer-to-float and float-to-integer casts are not implemented yet.

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

- Literal patterns: integers, floats, strings, booleans.
- Binding patterns: `name` captures the matched value.
- Wildcard: `_` matches anything without binding.
- Enum variant patterns: `Variant(payload)`, `EnumName.Variant(payload)`.
- Nested payload patterns.
- Guards: `if condition` after a pattern.

Enum matches must be exhaustive: every variant must be covered, or a `_`
wildcard must be present.

Known scalar value matches also require complete coverage or a catch-all arm.
If a value match reaches no arm at runtime, it traps; Void matches may fall
through. A nested refutable payload pattern requires its actual payload type;
annotate an untyped qualified-constructor result before matching it.

A match snapshots its scrutinee once before evaluating patterns and guards.
Boolean literals test truthiness: true matches any nonzero integer or Boolean
operand, false matches zero. Integer literals retain exact equality. Float
patterns use IEEE equality: signed zeros compare equal and NaN does not match
a numeric literal. String patterns use the runtime first-NUL view boundary.
These rules do not establish canonical Boolean storage or change equality and
bitwise operations. See [the migration guide](../migrations/source-initialization.md).

A borrowed enum may be matched to inspect its variant, for example
`match view { Some(_) => true, None => false }` where `view: &Option[T]`.
Payload bindings such as `Some(value)` remain unavailable through a borrowed
enum because they would require aggregate projection semantics.

## Range expressions

`start..end` creates an exclusive-upper-bound `Range`. `start..=end` creates
an inclusive-upper-bound range:

```quazi
for i : 0..10 { ... }      // 0 through 9
for i : 0..=10 { ... }     // 0 through 10
```

Outside loop headers, range expressions produce the prelude `Range` value,
consumed by APIs such as `std.random.range(0..=5)`.

Transparent aliases to enum types are resolved before checking match patterns,
payload bindings and exhaustiveness. An alias to `&Option[T]` keeps the same
borrowed-enum restrictions as the underlying reference.

## Error propagation

`expression?` unwraps `Some`/`Ok` values. `None` or `Err` returns early from
the nearest function or closure. `Option[T]?` requires an `Option[U]` return
type; `Result[T, E]?` requires `Result[U, E]`. The success types `T` and `U`
may differ. After transparent alias resolution, error types must match without
conversion: `?` never widens numbers, changes signedness or converts error
pointers. Handle and convert errors explicitly with `match` when needed.

A closure has its own return context and cannot propagate to the enclosing
function. Its return type must be known and compatible; wrapper-returning
closures currently also fail the separate closure-result ownership gate.
A qualified constructor such as `Option.Some(1)` has no resolved result type
yet and cannot be used directly as a `?` operand. Use a typed local built with
`Some`/`Ok`/`Err`/`None`, or a function with a declared wrapper return type.
Incompatible or unknown propagation emits `S14`.

For example (fragment, requiring a `ParseError` type and `parse` method):

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
