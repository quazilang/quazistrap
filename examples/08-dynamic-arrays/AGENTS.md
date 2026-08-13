# Example: 08-dynamic-arrays

Demonstrates the `Array[T]` collection type.

## Running

```bash
qz build -o array
./array
```

## Features shown

- `Array.new()`, `push`, `get`, `set`, `len`, `is_empty`
- Index assignment: `arr[2] = 88`
- Iteration: `for i : arr { ... }`
- Borrowed iteration: `for word : &words { ... }`
- `Array.from([...])` for array literals
- `choose_value` shows `if/else if/else` with `ret` in each branch
