# Example: 08-dynamic-arrays

Demonstrates the `Array[T]` collection type.

## Running

```bash
qz build -o array
./array
```

## Features shown

- `Array.new()` with an explicit `Array[i32]` binding, `push`, `set`, `len`
- Index reads through the `Index` impl: `prices[index]`
- Range-loop iteration over the length: `for index : 0..prices.len() { ... }`.
  Direct `for x : arr` over a named `Array[T]` is not currently accepted by
  the type checker; iterate a range or a slice instead.
- Variadic formatting with `io.println`
