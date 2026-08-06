# Quazilang Examples

Each folder is a standalone project. Run any with:

```
cd examples/<name>
qz run
```

| Example | What it shows |
|---------|---------------|
| `01-hello` | I/O, string formatting with `{}`, range loops |
| `02-structs` | Structs, impl methods, multiple types |
| `03-enums` | Enums, pattern matching, `Option[T]` |
| `04-closures` | Closures, higher-order functions, fn pointers, captures |
| `05-generics` | Generic functions, monomorphization over `i32`/`f64`/`bool` |
| `06-crash` | Crash handler demonstration (segfault) |
| `07-minimal-hw` | Minimal program with raw `@intrinsic` syscalls, no stdlib |
| `08-array` | `Array[T]`: create, push, index, set, get, len, iteration, cleanup |
| `09-mangling` | Module namespacing demo |
| `10-bitwise` | Bitwise operators |
| `11-elseif` | `else if` chains |
| `12-loop-control` | `break` and `continue` |
| `13-args` | Command-line arguments via `Array[str]` |
| `14-io-read` | Example showing I/O reads. |
| `15-logical` | Logical operators: `!`, `&&`, `\|\|` |
| `16-pub-types` | `pub` visibility on types; S04 on private import |
| `17-constfold` | Constant folding demonstration |
| `18-formatting` | Format specifications, raw strings, ANSI escapes |
| `19-cvariadics` | C-style variadic `@api` via bare `...` — calls libc `printf` |
| `20-ffi-abi` | Cross-platform C ABI Phase 2 round trip and self-test |
