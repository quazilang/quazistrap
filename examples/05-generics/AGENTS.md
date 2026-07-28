# Example: 05-generics

Demonstrates generic functions.

## Running

```bash
qz build -o generics
./generics
```

## Features shown

- `max[T]`, `min[T]`, `clamp[T]` — generic comparison functions
- `swap[T]`, `identity[T]` — generic utility functions
- Explicit generic instantiation: `max[i32](3, 7)`, `max[f64](1.5, 2.7)`
