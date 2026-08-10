# Example: 14-io-read

Demonstrates fallible, owned UTF-8 input with `std.io`.

## Running

```bash
qz build -o io-read
./io-read
```

## Features shown

- `io.readln()` returns `Result[String, ReadError]` for a line without its newline.
- `io.readkey()` returns one complete UTF-8 scalar immediately, without Enter.
- `io.read(delimiter)` returns as soon as an interactive delimiter key is
  pressed and validates UTF-8; redirected input retains buffered behavior.
- Returned strings own their storage; `.as_str()` creates a borrowed view.
- `io.println` supports `{}` formatting placeholders.
