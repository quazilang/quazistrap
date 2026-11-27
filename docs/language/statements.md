# Statements and control flow

Audience: language users and implementers.

## Statements

All statements end with `;`. A block `{ ... }` groups statements.

## Variable bindings

### Constants

`const` declares an immutable binding. An initializer is required:

```quazi
const answer: i32 = 42;
const name: str = "Quazi";
const inferred = 3.14;
```

Constants cannot be reassigned after declaration.

### Mutable variables

`var` declares a mutable binding. It may be initialized immediately or
declared and assigned later:

```quazi
var count: i32 = 0;
var label: str;
label = "ready";
```

Type annotations may be omitted when the type can be inferred from the
initializer.

A read requires initialization on every normal path reaching it. Branches and
match arms are checked independently and joined; returning branches do not
contribute fallthrough. A possibly empty loop, a skipped logical operand or a
deferred closure body cannot initialize a later read. Loop updates follow the
body. See [initialization migration](../migrations/source-initialization.md).

## Assignment

`=` assigns a value to a mutable binding:

```quazi
count = count + 1;
```

Compound assignments: `+=`, `-=`, `*=`, `/=`, `%=`.

Increment and decrement: `count++`, `++count`, `count--`, `--count`.

Assignment to a previously initialized owned value destroys the previous
value before storing the new one.

## Return

`ret expression;` returns a value from a function. `ret;` returns from a
`void` function. Every non-void function path must reach a `ret` or a tail
value:

```quazi
fn double(x: i32) i32 {
    ret x * 2;
}
```

### Tail values

The final element of a function body may be an expression without `;`. In a
function that returns a value it is the result, exactly as if written with
`ret`; in a `void` or `!` function it is evaluated and discarded:

```quazi
fn double(x: i32) i32 {
    x * 2
}

fn clamp(x: i32, low: i32, high: i32) i32 {
    if (x < low) {
        ret low;
    }
    if (x > high) {
        ret high;
    }
    x
}
```

Tail position extends into every branch of a final `if` / `else if` / `else`
statement and into a final `unsafe` block, so each branch may end with its own
tail value:

```quazi
fn max(a: i32, b: i32) i32 {
    if (a > b) {
        a
    } else {
        b
    }
}
```

Rules:

- Only these tails may omit `;`. A value expression without `;` anywhere else,
  for example at the end of a loop body or of an `if` that is followed by more
  statements, is error `E02`.
- A final `if` without `else` cannot supply a function's value (`E02`); use an
  `else` branch or `ret`.
- `ret` remains the only way to leave a function early.
- `{ ... }` blocks are not expressions; see
  [D-017](../decisions/expression-oriented-blocks.md).

## Conditional statements

### `if` / `else if` / `else`

```quazi
if temperature < 0 {
    println("freezing");
} else if temperature < 20 {
    println("cool");
} else {
    println("warm");
}
```

The condition needs no parentheses, as in `for`; parentheses are an ordinary
grouping, so `if (a) || b {` is the condition `(a) || b`. Braces are required
for the body.
A condition may be `bool` or an integer, which is true when nonzero.

`if` can also produce a value; see
[`if` expressions](expressions.md#if-expressions).

## Loops

Quazi uses `for` for all loop forms.

### Range loops

```quazi
for i : 0..10 {
    println(format("{}", i));
}
```

`0..10` iterates from 0 through 9 (exclusive upper bound).
`0..=10` iterates from 0 through 10 (inclusive upper bound).

### Iterator loops

```quazi
for value : values {
    println(format("{}", value));
}
```

`for` accepts fixed arrays, slices, the prelude `Array[T]` and iterators.
Iterating an `Array[T]` by value consumes it: elements that own resources
(such as `String`) are moved into the loop variable one at a time, so the body
may keep them (`kept.push(name)`); elements left after `break` are released
with the array. Iterating a borrowed array (`for value : &values`, or a
`values: &Array[T]` parameter) reads copyable elements in place; a borrowed
array of owning elements is rejected (S10), iterate it by index instead. A
fixed array (`[T; N]`) of owning elements cannot be iterated by value either
(S10); iterate it by index.

Besides these, `for` accepts any value whose type has
`has_next(self: &I!) bool` and `next(self: &I!) Option[T]` methods (`I` is
the iterator type); the loop variable
has type `T` and the loop ends when `has_next` is false or `next` returns
`None`.

### Index-value loops

The first variable is the zero-based `usize` index, the second the value. This
works for every iterable, including iterators.

```quazi
for index, value : values {
    println(format("{}: {}", index, value));
}
```

### C-style loops

```quazi
for var i = 0; i < 10; i++ {
    println(format("{}", i));
}
```

The initializer, condition, and update are separated by semicolons.

### Condition loops (while-like)

```quazi
for (running) {
    process();
}
```

### Infinite loops

```quazi
for {
    if (should_stop()) { break; }
}
```

### `break` and `continue`

`break;` exits the nearest enclosing loop. `continue;` skips to the next
iteration, running the C-style update or range/index increment when present.
Both destroy supported owners in the body scopes they exit before transferring
control. Loop initializer owners survive `continue` and are destroyed at loop
exit. See [scope cleanup](safety.md#scope-cleanup-raii).

For example:

```quazi
for i : 0..100 {
    if (i % 2 == 0) { continue; }
    if (i > 50) { break; }
    println(format("{}", i));
}
```

## Unsafe blocks

Operations requiring unsafe context — raw pointer dereferences, calls to
`unsafe fn`, `@api`, or `@syscall` — must appear inside an `unsafe` block:

```quazi
unsafe {
    const byte: u8 = *pointer;
    puts(message);
}
```

An `unsafe` block does not change the safety of surrounding code. It only
permits specific unsafe operations within its scope.

## Expression statements

Any expression followed by `;` is a statement. The result is discarded. The
semicolon may be omitted only for a function's [tail value](#tail-values):

```quazi
compute(42);
items.push(value);
```
