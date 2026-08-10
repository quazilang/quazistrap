# Example: 14-io-read

Demonstrates fallible, owned UTF-8 input with `std.io`.

## Running

```bash
qz build -o io-read
./io-read
```

## Features shown

- `io.readln()` returns `Result[String, ReadError]` for a line without its newline.
- `io.readkey()` returns one complete UTF-8 scalar, not merely one byte.
- `io.read(delimiter)` reads until a delimiter byte and validates UTF-8.
- Returned strings own their storage; `.as_str()` creates a borrowed view.
- `io.println` supports `{}` formatting placeholders.
