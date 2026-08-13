# Example 20 — C Variadic Functions (`20-c-variadic-functions`)

Demonstrates the C-style variadic FFI bridge: calling `printf` from libc using the bare `...`
parameter syntax that is now supported in `@api` declarations.

## Source

```quazi
@api("printf") unsafe fn printf(fmt: *c_char, ...);
```

- Bare `...` (no name or type) marks an `@api` function as C-variadic.
- The caller may pass any number of additional arguments after the fixed parameters.
- On SysV ABI (Linux), the encoder emits `xor rax, rax` before the `call` so `AL` is cleared per ABI rules.
- Quazi-style variadics (`...name: Type`) are still rejected in `@api`/`@syscall` bodies (S14).

## Running

```bash
qz run
# Hello from Quazilang! C variadics work: 40 + 2 = 42
```
