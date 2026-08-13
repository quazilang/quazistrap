# Example: 03-enum-pattern-matching

Demonstrates enums with payloads, pattern matching, and the `Option[T]` type.

## Running

```bash
qz build -o enums
./enums
```

## Features shown

- `divide` returns `Option[i32]` for safe division.
- `Shape` enum has `Circle(f64)`, `Rect(f64, f64)`, and `Triangle(f64, f64)` variants.
- `print_shape` uses one exhaustive `match` with variant patterns.
