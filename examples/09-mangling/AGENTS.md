# Example: 09-mangling

Demonstrates module-level function namespacing / mangling.

## Running

```bash
qz build -o mangling
./mangling
```

## Features shown

- `core.write()` — qualified call into `std.core`
- `unix.write()` — qualified call into `std.unix`
- `@cfg(target_os = "linux")` for platform-specific branches
- Multiple modules can define the same bare function name without collision

## Files

| File | Purpose |
|------|---------|
| `src/main.qz` | Entry point showing cross-module qualified calls |
