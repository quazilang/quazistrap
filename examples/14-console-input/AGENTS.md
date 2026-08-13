# Example: 14-console-input

Demonstrates fallible, owned UTF-8 input with `std.io`.

## Running

```bash
qz build -o io-read
./io-read
```

## Features shown

- `io.readln()` returns `Result[String, ReadError]` for a line without its newline.
- `io.readkey()` returns one complete UTF-8 scalar immediately, without Enter.
- `io.read(delimiter: str)` accepts UTF-8 text, returns as soon as an
  interactive delimiter is complete, and validates UTF-8. Enter is normalized
  to `\n`; redirected input retains buffered behavior.
- Returned strings own their storage; `.as_str()` creates a borrowed view.
- `io.println` supports `{}` formatting placeholders.
