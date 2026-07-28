# Example: 02-structs

Demonstrates structs, methods, and impl blocks.

## Running

```bash
qz build -o structs
./structs
```

## Features shown

- `Vec2` — a 2D vector with `new`, `add`, `scale`, and `dot` methods.
- `Color` — an RGB struct with `new`, `blend`, and `invert` methods.

Shows struct literal syntax (`Vec2 { x: x, y: y }`), method receiver syntax (`self: Vec2`), and field access.
