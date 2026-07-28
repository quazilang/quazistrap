# Example: 13-args

Demonstrates `fn main(args: Array[str])` — receiving command-line arguments as an `Array[str]`.

The startup stub builds the array from the process `argc`/`argv` block. Each argument is stored as a pointer to its null-terminated C string; `args.len()` and `args[i].len()` work as expected.

## Running

```bash
qz build -o args
./args hello world
```

## Features shown

- `fn main(args: Array[str])` — receiving command-line arguments as an `Array[str]`
- `args.len()` and `args[i].len()` work as expected.
- `str` fat pointer
