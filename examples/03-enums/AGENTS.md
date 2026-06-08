# Example: 03-enums

Demonstrates enums with payloads, pattern matching, and the `Option[T]` type.

- `divide` returns `Option[i32]` for safe division.
- `Shape` enum has `Circle(f64)`, `Rect(f64, f64)`, and `Triangle(f64, f64)` variants.
- `area` and `describe` use `match` with variant patterns.
- Also shows fixed-size arrays and iteration.
