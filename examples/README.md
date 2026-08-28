# Quazi Examples

Examples are ordered as a learning path. Each directory is a runnable project:

```bash
cd examples/01-hello-world
qz run
```

| # | Example | What it teaches |
|---|---------|-----------------|
| 01 | `hello-world` | Smallest normal Quazi application |
| 02 | `struct-methods` | Data modeling and inherent methods |
| 03 | `enum-pattern-matching` | Payload enums, exhaustive match, guards |
| 04 | `closures` | Function values and captured expressions |
| 05 | `generics` | Reusable generic functions |
| 06 | `panic-and-backtrace` | Panic flow and diagnostic backtraces |
| 07 | `no-standard-library` | Direct intrinsic without prelude/runtime convenience |
| 08 | `dynamic-arrays` | Owned `Array[T]`, mutation, iteration |
| 09 | `modules-and-imports` | Multiple source files and selected imports |
| 10 | `bitwise-operations` | Masks, shifts, bitwise logic |
| 11 | `conditional-branches` | `if`, `else if`, `else` |
| 12 | `loop-control` | Range loops, `break`, `continue` |
| 13 | `command-line-arguments` | `main(args: Array[str])` |
| 14 | `console-input` | Fallible line/key/delimiter input |
| 15 | `boolean-logic` | Short-circuit `&&`, `||`, `!` |
| 16 | `module-visibility` | Public module API and private implementation |
| 17 | `constant-expressions` | Compile-time values in application calculations |
| 18 | `string-formatting` | Placeholders, specifications, raw/escaped strings |
| 19 | `c-interop` | Calling C and exporting Quazi functions |
| 20 | `c-variadic-functions` | Calling C varargs safely through `CString` |
| 21 | `c-abi-aggregates` | Aggregates, callbacks, exports, foreign globals |
| 22 | `system-information` | Portable OS, CPU, memory, filesystem data |
| 23 | `standard-library-tour` | Unicode strings, parsing, `Result`, math |
| 24 | `local-library` | Publishable source/QZI library artifact |
| 25 | `local-dependency` | Relative dependency plus incremental QZC reuse |
| 26 | `http-client-server` | TCP-backed HTTP client and local server |
| 27 | `text-and-math` | Unicode text, checked parsing, practical math |
| 28 | `git-library-dependency` | Downloaded Git library and recursive factorial |
| 29 | `guess-the-number` | Small game using secure standard-library randomness |
| 30 | `dynamic-libraries` | Runtime DLL/SO loading and typed C function pointers |
| 31 | `udp-echo` | UDP bind, datagrams, peer addresses, and echo |
| 32 | `testing` | `@test`, automatic discovery, filtering, and isolated runners |
| 33 | `ini-library` | Source-backed INI library with owned text and lookup APIs |
| 34 | `ini-parser` | Local INI dependency, executable checks, and QZI-compatible API |

Examples 19–21 need a C toolchain. Example 26 runs as two processes:

```bash
cd examples/26-http-client-server
qz run --bin http-server
# In another terminal:
qz run --bin http-client
```

Example 28 needs network access and the published `namnam1105/qz-test-lib`
repository. It calls the dependency's recursive factorial function five times.

Example 30 loads `kernel32.dll` on Windows or `libc.so.6` on Linux, resolves a
process-ID function at runtime, casts it to an exact `@repr(C)` signature, and
calls it while the library remains open. Linux uses the external linker because
`dlopen` is a native runtime-loader API.

Example 31 runs as two processes:

```bash
cd examples/31-udp-echo
qz run --bin udp-server
# In another terminal:
qz run
```

Example 32 runs with `qz test`; pass a substring such as
`qz test addition` to select matching tests.

Example 34 consumes example 33 as a local source dependency. Run it with
`qz run` to exercise global properties, sections, duplicates, empty values,
reparsing, and source round-tripping.
