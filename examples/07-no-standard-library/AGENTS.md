# Example: 07-no-standard-library

A minimal "Hello, World!" using only an intrinsic syscall.

## Running

```bash
qz build -o minimal-hw
./minimal-hw
```

## Features shown

- `@intrinsic("quazi.write")` wrapper around the raw write syscall
- No `std` imports — smallest possible binary
- Direct syscall via `write(1, "Hello, World!\n", 14)`
