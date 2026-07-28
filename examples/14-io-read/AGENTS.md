# Example: 14-io-read

Demonstrates standard input reading and string formatting using std.io in Quazilang.

## Running

```bash
qz build -o io-read
./io-read
```

## Features shown

- io.readln() — reads a full line of text from standard input
- io.readkey() — reads a single character/keypress from stdin
- io.read(delimiter) — reads input until a specific byte delimiter (e.g., . / byte 46)
- Formatted output — printing values with {} placeholders using io.println