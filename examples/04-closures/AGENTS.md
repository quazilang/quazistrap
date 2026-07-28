# Example: 04-closures

Demonstrates first-class functions and closures.

## Running

```bash
qz build -o closures
./closures
```

## Features shown

- Closures: `|x| x * 2`, `|x| x * x`, `|x| x * -1`
- Storing closures in a variable
- Arrays of function pointers: `[fn(i32) i32; 3]`
- Calling closures via variable