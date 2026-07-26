# examples/13-args

Demonstrates `fn main(args: Array[str])` — receiving command-line arguments as an `Array[str]`.

The startup stub builds the array from the process `argc`/`argv` block. Each argument is stored as a pointer to its null-terminated C string; `args.len()` and `args[i].len()` work as expected. Directly printing `args[i]` is not yet supported because `Array[str]` currently stores only the pointer half of the `str` fat pointer.

Run with extra arguments:

```bash
qz build examples/13-args/src/main.qz -o 13-args
./13-args hello world
```

This program returns the sum of the lengths of all arguments (including `argv[0]`).
