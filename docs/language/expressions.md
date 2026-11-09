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

Eager `+ - * / %` and unary minus require numeric values. Concrete generic
instantiations satisfy the same requirements. Unary minus on references requires
explicit dereference. Known numeric reference binary operands load their values.
Only `+` supports str-view concatenation; owned String operands use explicit
String operations. Raw pointer `+`/`-` integer uses byte offsets with the pointer
on the left; pointer differences are unsupported.

Floating `%` uses a quotient truncated toward zero, preserves the dividend's
zero sign, yields NaN for infinite dividends or zero divisors, and returns a
finite dividend unchanged for infinite divisors. Mixed integer/float coercion,
narrow-result normalization and `**` lowering remain incomplete;
these checks do not establish general numeric correctness. See
[upgrade guidance](../migrations/arithmetic.md).

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
comparisons. Ordering accepts integer pairs, float pairs or recognized strings,
including through generic helpers; mixed integer/float ordering is rejected.
Use explicit comparison methods for other structured values. Struct and enum
values (including `Option` and `Result`) are heap handles, so `==` and `!=` on
them are rejected with `S06`; test a variant with `match`
(`match kind { Kind.A => true, _ => false }`). Earlier compilers accepted the
operators and compared addresses, so even `Kind.A == Kind.A` was false. Runtime String
identity validation remains incomplete; see [the implementation boundary](../internals/operator-requirements.md).

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
`&`, `|` and `^` also accept two Boolean operands and return Bool. Shifts require
integer operands. These constraints apply to concrete generic instantiations;
floats and arbitrary owning values are rejected. This does not define normalization
of noncanonical Bool values created through casts or foreign code.
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

## Qoroutines (`qo`)

`qo f(args)` runs the call in a new qoroutine and evaluates to a
`Qoroutine[T]`, where `T` is the call's result type. `handle.join()` waits for
the qoroutine and returns `Result[T, QoroutineError]`:

```quazi
fn square(n: i64) i64 { n * n }

fn main() i32 {
    const handle: Qoroutine[i64] = qo square(7 as i64);
    const value = handle.join().unwrap(); // 49
    0
}
```

Qoroutines are scheduled M:N on a pool of OS worker threads that starts on the
first `qo` ([D-019](../decisions/qoroutines.md)). The operand must be a call.
When `main` returns, the program ends and running qoroutines stop.

A qoroutine that calls `join` gives its worker to other qoroutines while it
waits, so joins may nest arbitrarily deep; `join` outside a qoroutine yields
its OS thread.

Arguments that are variables move into the qoroutine: owned values such as
`String` and structs are consumed by `qo` (using them afterwards is a
use-after-move error), scalars are copied. Argument expressions other than
variables are evaluated inside the qoroutine.

Current limits: references and `str` views cannot be passed, and the result
must be a plain scalar. The `Qoroutine` handle itself is not released yet
(struct storage without a `free` hook is not reclaimed in general). There is no preemption: a qoroutine runs
until it finishes or waits in `join`. At most 1024 qoroutines (256 on Windows)
are alive at once. Preemption is planned.

A panic inside a qoroutine prints `qoroutine panicked at '...'`, ends only that
qoroutine and makes its `join()` return `Err(QoroutineError.Panicked)`; values
still owned by the abandoned qoroutine are not cleaned up. A panic outside
qoroutines ends the program with status 101 as before.

### Channels

`Channel[T]` is a bounded FIFO for qoroutines (built in, no import):

```quazi
fn produce(ch: Channel[i64], count: i64) i64 {
    for i : 0..count { ch.send(i as i64); }
    ch.close();
    count
}

fn main() i32 {
    const ch: Channel[i64] = Channel.new(16);
    const writer = ch.clone();
    const producer = qo produce(writer, 100 as i64);
    var total: i64 = 0;
    for (true) {
        const value = ch.receive();          // Option[i64]
        if (value.is_none()) { break; }      // closed and drained
        total = total + value.unwrap();
    }
    producer.join().unwrap();
    0
}
```

`send` waits while the buffer is full and panics on a closed channel;
`receive` waits while it is empty and returns `None` once the channel is
closed and drained; `close` stops further sends. The handle is reference
counted: `clone()` creates another owner (pass the clone to `qo`), and the
buffer is released with the last owner. A waiting qoroutine yields its
worker. Values occupy one slot: send plain scalars for now.

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

## `if` expressions

`if` produces a value when it appears where an expression is expected:

```quazi
fn abs(x: i32) i32 {
    const result: i32 = if (x < 0) { -x } else { x };
    result
}

fn sign(x: i32) i32 {
    ret if (x < 0) { -1 } else if (x == 0) { 0 } else { 1 };
}
```

- `else` is required; `else if` chains are allowed.
- Each branch contains exactly one expression with no `;`. Use an `if`
  statement, a function tail, or a helper function when a branch needs
  statements.
- The condition follows `if` statement rules: `bool`, or an integer that is
  true when nonzero.
- Branch types must be compatible like `match` arms. A branch of type `!`, such
  as `panic(...)`, is compatible with any type.
- Only the selected branch is evaluated, and its value (including an owned
  value) is moved out of it exactly once.

An `if` expression is equivalent to `match condition { true => then, _ => else }`,
so its diagnostics report match arm mismatches. Design and alternatives:
[D-017](../decisions/expression-oriented-blocks.md).

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
