# Example: 09-mangling

Demonstrates module-level function namespacing and mangling.

## Running

```bash
qz build -o mangling
./mangling
```

## Features shown

- `core.write()` is a qualified raw call; caller-controlled lengths require an `unsafe` block.
- `unix.write()` is a qualified call into `std.unix`.
- `@cfg(target_os = "linux")` selects platform-specific branches.
- Multiple modules can define the same bare function name without collision.

## Files

| File | Purpose |
|------|---------|
| `src/main.qz` | Entry point showing cross-module qualified calls |
