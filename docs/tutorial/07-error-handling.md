# Chapter 7: Error handling

This chapter covers Quazi's error handling model using `Result`, `Option`,
and the `?` operator.

## The philosophy

Quazi separates expected failures from bugs:

- **Expected failures** use `Result[T, E]` — file not found, invalid input,
  network errors.
- **Bugs and unrecoverable problems** use `panic` — index out of bounds,
  assertion failures.

## `Option[T]`

`Option` represents a value that may or may not exist:

```quazi
enum Option[T] { Some(T), None, }
```

### Creating Options

```quazi
const found: Option[i32] = Some(42);
const missing: Option[i32] = None;
```

### Using Options

```quazi
fn safe_divide(a: i32, b: i32) Option[i32] {
    if (b == 0) { ret None; }
    ret Some(a / b);
}

fn main() i32 {
    const result = safe_divide(10, 3);

    // Check and unwrap
    if (result.is_some()) {
        io.println("Result: {}", result.unwrap());
    }

    // With a default
    const value = safe_divide(10, 0).unwrap_or(0);
    io.println("Default: {}", value);

    // Pattern matching
    match safe_divide(10, 2) {
        Some(v) => io.println("Got: {}", v),
        None => io.println("Division failed"),
    };

    ret 0;
}
```

## `Result[T, E]`

`Result` carries either a success value or a typed error:

```quazi
enum Result[T, E] { Ok(T), Err(E), }
```

### Using Results

```quazi
fn parse_port(text: str) Result[u16, ParseError] {
    ret text.parse[u16]();
}

fn main() i32 {
    const result = parse_port("8080");

    if (result.is_ok()) {
        io.println("Port: {}", result.unwrap());
    }

    match parse_port("not_a_number") {
        Ok(port) => io.println("Port: {}", port),
        Err(e) => io.println("Error: {}", e.message()),
    };

    ret 0;
}
```

## The `?` operator

`?` propagates errors automatically. On `Ok`/`Some`, it unwraps the value.
On `Err`/`None`, it returns early:

```quazi
fn load_config(path: str) Result[i32, ParseError] {
    const text = "42"; // would come from a file
    const value = text.parse[i32]()?;  // returns Err early if parsing fails
    ret Ok(value);
}
```

The enclosing function must have a compatible `Result` or `Option` return
type.

### Chaining with `?`

```quazi
fn process(input: str) Result[i32, ParseError] {
    const a = input.parse[i32]()?;
    const b = "10".parse[i32]()?;
    ret Ok(a + b);
}
```

Each `?` either succeeds and continues, or returns the error immediately.

## Standard library error types

The standard library uses domain-specific error enums:

| Error type | Module | Example causes |
| --- | --- | --- |
| `ParseError` | prelude | `Empty`, `InvalidDigit`, `Overflow` |
| `FsError` | `std.fs` | `NotFound`, `PermissionDenied`, `InvalidData` |
| `NetError` | `std.net` | `ConnectionRefused`, `BrokenPipe`, `InvalidUtf8` |
| `ReadError` | `std.io` | Input failures |

All error types provide a `message()` method for user-facing text.
`Native(code)` variants preserve platform-specific error codes for
diagnostics.

## Panics

`panic` terminates the process for unrecoverable situations:

```quazi
fn checked_access(items: Array[i32], index: usize) i32 {
    if (index >= items.len()) {
        panic("index out of bounds");
    }
    ret items[index];
}
```

There is no recovery from a panic. Use `Result` for expected failures.

### Custom panic handlers

A program may install one custom panic handler:

```quazi
import std.core;

@panic_handler
fn my_handler(info: PanicInfo) ! {
    // Custom terminal diagnostics
    core.exit(101);
}
```

See [panic handling](../language/panic.md) for details.

## Next steps

Continue to [Chapter 8: Collections](08-collections.md) to learn about
arrays, maps, and sets.
