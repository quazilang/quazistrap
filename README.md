# quazilang

[![AI Slop Inside](https://sladge.net/badge.svg)](https://sladge.net)

**A fast, strict, and expressive systems independent programming language**

Quazilang (`qz`) compiles directly to native x86-64 binaries via its own backend powered by `iced-x86`. It features a clean C-like syntax, strong generics, trait-based polymorphism, move semantics, and a growing standard library. Plain Linux programs require no LLVM, GCC, system linker, or libc; explicitly requested native dependencies remain available through an external toolchain.

---

## Features

- **Self-contained Linux binaries** — emits and links static x86-64 ELF executables in-process, with no LLVM, system linker, libc, or CRT by default
- **Generics** — `fn max[T](a: T, b: T) T` with compile-time monomorphization
- **Traits** — `trait Foo { fn method(...) ...; }` with `impl Foo for Bar`
- **Enums with payloads** — `enum Option[T] { Some(T), None }` + `match`
- **Move semantics** — non-primitive types are moved by default, borrow with `&`
- **First-class functions & closures** — `|x, y| x + y` stored as `fn(i32, i32) i32`
- **Unsafe system** — `unsafe fn` / `unsafe { ... }` for raw pointer work
- **Modules and libraries** — dotted imports, `pub import`, downloadable dependencies, and compiled QZI libraries
- **LSP support** — hover, diagnostics, go-to-definition, completion, formatting
- **Cross-platform** — Linux (ELF) and Windows (PE) targets

---

## Install & Build

Building the compiler requires **Rust 2024 edition** (latest stable recommended).
Plain x86-64 Linux and Windows Quazi builds need no system linker or C
toolchain. Native C sources, archives, shared libraries, and explicit `-l`
dependencies may use corresponding external tools.

```bash
git clone https://github.com/quazilang/quazistrap
cd quazistrap
cargo build --release
```

The binary is at: `target/release/qz`

---

## Quick Start

Create a new project:

```bash
qz new my_app
cd my_app
qz run
```

Or compile a single file:

```bash
qz build src/main.qz -o hello
./hello
```

---

## Language Syntax

```quazi
import std.io;

fn main() i32 {
    io.println("Hello, Quazilang!");
    ret 0;
}
```

```quazi
// Generics
fn max[T](a: T, b: T) T {
    if a > b { ret a; }
    ret b;
}

// Structs with methods
struct Vec2 { x: f64, y: f64, }
impl Vec2 {
    fn dot(self: Vec2, other: Vec2) f64 {
        ret self.x * other.x + self.y * other.y;
    }
}

// Enums + pattern matching
enum Shape { Circle(f64), Rect(f64, f64), }
fn area(s: Shape) f64 {
    match s {
        Shape.Circle(r) => 3.14159 * r * r,
        Shape.Rect(w, h) => w * h,
    }
}

// Closures
var double: fn(i32) i32 = |x| x * 2;

// Unsafe raw pointer work
unsafe fn memzero(p: *u8, len: usize) void {
    var i: usize = 0;
    for (i < len) { *p = 0; p++; i++; }
}
```

### String escapes

Ordinary double-quoted strings support C- and Rust-style escapes:

| Escape | Value |
|--------|-------|
| `\0` | Null (`U+0000`) |
| `\a`, `\b`, `\e`, `\f`, `\v` | Bell, backspace, escape, form feed, vertical tab |
| `\n`, `\r`, `\t` | Newline, carriage return, tab |
| `\\`, `\"`, `\'`, `\?` | Literal punctuation |
| `\xNN` | Exactly two hexadecimal digits; ASCII range `00`–`7F` |
| `\ooo` | One to three octal digits |
| `\uNNNN`, `\UNNNNNNNN` | C-style 4- or 8-digit Unicode scalar |
| `\u{H...}` | Rust-style Unicode scalar with one to six hex digits; `_` separators allowed |
| Backslash + newline | Continue the string, ignoring following whitespace |

Malformed and unknown escapes are compile errors. Backtick raw strings preserve
all characters exactly, never decode escapes, and may span lines:

```quazi
const escaped = `\n\e\x41\u{41}`;
const message = `first line
second line`;
```

An unterminated raw string is a compile error.

### String types

- `str` is an immutable borrowed UTF-8 view. String literals have this type.
  It does not own or free its bytes.
- `&str` currently has the same representation and accepted operations as
  `str`; it makes borrowing explicit for readers and future lifetime checking.
- `String` owns a UTF-8 allocation and stores data, byte length, and capacity.
  Local `String` values are automatically freed; use `.as_str()` to borrow one
  without transferring ownership.

---

## CLI Reference

```
qz build [source.qz|program.qzi|native.o ...] [-o out] [-i] [-c] [-r] [-s]
         [--bin name|--lib] [--target x86_64-linux|x86_64-windows]
         [--linker builtin|path] [-L dir] [-l name]
         [--silent|--no-progress] [--no-color] [--no-unicode]
qz run [source.qz|program.qzi|native.o ...]  # build and run; project if omitted
qz header [file ...] [-o quazi.h] [--target x86_64-linux|x86_64-windows]
qz check                  # type-check without compiling
qz fetch                  # download, verify, and lock dependencies
qz deps                   # show resolved dependency sources
qz add <path-or-url> [--alias name]  # infer package name and add dependency
qz remove <name>          # remove dependency
qz fmt                    # trim trailing whitespace in .qz files
qz clean                  # remove build artifacts
qz new <name> [--lib]     # scaffold a new project
qz init [--lib]           # init in current directory
qz lsp                    # start language server
```

`qz new` and `qz init` initialize a Git repository when Git is available and
create a project `.gitignore` for `build/` and native/QZI artifacts.
`quazi.lock` stays tracked for reproducible dependency builds.

`qz add ../math` reads the local package name from `quazi.toml`.
`qz add https://host/math.git --type git` derives an identifier from the URL,
and `qz add ../math --alias numbers` selects a different import name while
retaining the discovered package identity in `quazi.lock`; no separate
`package` manifest field is required.
then validates it against downloaded package metadata. Git `--version` accepts
a tag, commit hash, or `latest`.

Project builds use QZC v2 at `build/quazi/<target>/<artifact>/incremental.qzc`.
Exact hits reuse linked QZI; partial hits restore unchanged pre-WPO functions,
compile changed files, and rerun full WPO. Progress reports hit/partial/miss,
restored/compiled function counts, and cache writes.
Pass `--no-incremental` to bypass both reads and writes. See
[Libraries, QZI, and incremental builds](docs/LIBRARIES.md) for dependency TOML,
QZI v6 library rules, lockfile behavior, and cache guarantees.

`qz header` reads the current project when no files are supplied and emits the
public C surface formed by `@export` functions and their C-compatible type
dependencies. Select `x86_64-linux` or `x86_64-windows` explicitly so `@cfg`,
`c_long`, packing, and alignment match the intended consumer. The result can be
included from either C or C++ and does not require compiling or linking the
Quazi project.

**Flags:**
- `-i` — emit `.qzi` bytecode only (no backend)
- `-c` — emit `.o` object file only (no linker)
- `-r` — run the emitted binary after `qz build`
- `-s` — strip symbols
- `--linker builtin` — require the in-process ELF/PE linker
- `--linker <path>` — explicitly use an external linker
- `-L <dir>` / `-l <name>` — opt into a native library search path/library

- `-q`, `--silent` â€” emit nothing for successful builds; errors still print
- `--no-progress` â€” hide stages and print only `built <name>` on success
- `--no-color` â€” remove ANSI color from progress and diagnostics
- `--no-unicode` â€” use ASCII headers, trees, and `[ok]`/`[fail]` markers

**Environment variables:**
- `QUAZI_LINKER=builtin` — require the in-process ELF/PE linker
- `QUAZI_LINKER=/path/to/linker` — select an external linker
- `QUAZI_TRACE=1` — enable crash backtraces

---

## Built-in Linker and Runtime

On x86-64 Linux and Windows, ordinary builds use Quazi's in-process linker.
Linux emits static ELF; Windows emits PE32+ with direct Win32/Winsock imports.
Compiler objects and supported native objects may be combined:

```bash
qz build src/main.qz native/helper.o -o app
qz build program.qzi native/helper.o -o app
qz run program.o helper.o
```

No library is implicit. In particular, libc and libm are not linked merely
because they exist on the host. Passing `-l`, an archive/shared-library input,
or an explicit external linker path selects the external
linking path. `--linker builtin` rejects unsupported native flags instead of
silently falling back. To use libc intentionally:

```bash
qz build src/main.qz -l c -o app
# or choose the exact linker as well:
qz build src/main.qz --linker ld.lld -l c -o app
```

The compiler embeds only the pure-assembly Linux runtime routines referenced
by the program. This currently covers allocation, memory/string primitives,
and numeric formatting; allocation calls `mmap`/`munmap` directly.

The linkers target x86-64 ELF and PE/COFF. Mach-O, archives, shared objects,
TLS, linker scripts, and general-purpose native compatibility remain external
linker territory. See [docs/LINKER.md](docs/LINKER.md).

---

## Project Config (`quazi.toml`)

```toml
[package]
name = "hello"
version = "0.1.0"

[[bin]]
name = "hello"
path = "src/main.qz"

[dependencies]
utils = { path = "../utils", version = "0.1.0" }
```

If a `quazi.lock` file exists, it pins dependency versions. It is created automatically on first build when dependencies are present.

See [project and manifest documentation](docs/PROJECTS.md) and
[`std.net` documentation](docs/NETWORK.md).

---

## Standard Library

| Module | Description |
|--------|-------------|
| `std.io` | `println`, `print`, `eprintln`, `read_line` |
| `std.math` | Dependency-free GCD/LCM, combinatorics, floating arithmetic, trig, powers, roots, and logarithms |
| `std.fmt` | `format("{}", ...)` — `{}` placeholder formatting |
| `std.string` | `String`: owned heap string with automatic lexical cleanup |
| `std.collections.array` | `Array[T]`: dynamic array with index, push, iteration |
| `std.collections.map` | Hash map with open-addressing |
| `std.collections.set` | Hash set |
| `std.fs` | Cross-platform owned files, whole-file reads, paths, and metadata |
| `std.net` | TCP, HTTP/1.1 requests, and local server support |
| `std.os` | Cross-platform environment, hostname, memory, process, and OS queries |
| `std.thread` | spawn/join |
| `std.option` | `Option[T]` + `?` operator |
| `std.result` | `Result[T, E]` + `?` operator |
| `std.box` | `Box[T]`: heap-allocated single value |
| `std.panic` | `panic("msg")` with file/line injection |
| `std.ffi` | C types (`c_int`, `c_char`, `va_list`), `CString`, `CStr`, `nullptr` |

---

## Examples

| Example | Description |
|---------|-------------|
| [`01-hello-world`](examples/01-hello-world/) | Minimal "Hello, world!" |
| [`02-struct-methods`](examples/02-struct-methods/) | Structs and inherent methods |
| [`03-enum-pattern-matching`](examples/03-enum-pattern-matching/) | Payload enums and matching |
| [`04-closures`](examples/04-closures/) | First-class functions and closures |
| [`05-generics`](examples/05-generics/) | Generic functions |
| [`06-panic-and-backtrace`](examples/06-panic-and-backtrace/) | Panic diagnostics and backtraces |
| [`07-no-standard-library`](examples/07-no-standard-library/) | Direct intrinsic without std convenience |
| [`08-dynamic-arrays`](examples/08-dynamic-arrays/) | Owned `Array[T]` usage |
| [`09-modules-and-imports`](examples/09-modules-and-imports/) | Multi-file modules and selected imports |
| [`10-bitwise-operations`](examples/10-bitwise-operations/) | Masks, shifts, bitwise operators |
| [`11-conditional-branches`](examples/11-conditional-branches/) | `if`, `else if`, `else` |
| [`12-loop-control`](examples/12-loop-control/) | `break` and `continue` |
| [`13-command-line-arguments`](examples/13-command-line-arguments/) | Arguments via `Array[str]` |
| [`14-console-input`](examples/14-console-input/) | Fallible console input |
| [`15-boolean-logic`](examples/15-boolean-logic/) | Short-circuit boolean operators |
| [`16-module-visibility`](examples/16-module-visibility/) | Public API and private implementation |
| [`17-constant-expressions`](examples/17-constant-expressions/) | Application calculations with constants |
| [`18-string-formatting`](examples/18-string-formatting/) | Formatting, raw strings, ANSI, escapes |
| [`19-c-interop`](examples/19-c-interop/) | Calling C and exporting Quazi functions |
| [`20-c-variadic-functions`](examples/20-c-variadic-functions/) | C-style variadic `@api` with `printf` |
| [`21-c-abi-aggregates`](examples/21-c-abi-aggregates/) | Aggregates, callbacks, globals, exports |
| [`22-system-information`](examples/22-system-information/) | Portable OS/CPU/memory information |
| [`23-standard-library-tour`](examples/23-standard-library-tour/) | Unicode strings, parsing, results, math |
| [`24-local-library`](examples/24-local-library/) | Source/QZI library artifact |
| [`25-local-dependency`](examples/25-local-dependency/) | Relative dependency and QZC cache |
| [`26-http-client-server`](examples/26-http-client-server/) | HTTP client and local server |
| [`27-text-and-math`](examples/27-text-and-math/) | Unicode text, checked parsing, practical math |
| [`28-git-library-dependency`](examples/28-git-library-dependency/) | Git dependency and repeated recursive factorial calls |

String indexing/slicing, checked parsing, numeric methods, math accuracy goals,
automatic cleanup, and Windows UTF-8 console behavior are documented in
[Primitive APIs and portable text output](docs/PRIMITIVE_APIS.md).

Complete reference: [docs/README.md](docs/README.md). Full curriculum:
[examples/README.md](examples/README.md).

---

## Architecture

```
source → Loader → Lexer → Parser → Analyzer → Codegen → Backend (iced-x86) → .o → Linker → binary
```

The loader visits each source dependency once and reuses the merged parse for
dependency symbol discovery. Whole-program reachability maintains a call-edge
adjacency index, so tree shaking and codegen walk the used graph in `O(V + E)`
instead of rescanning every edge for every reachable function. This cache is
per compilation; persistent incremental artifact caching may be added
separately.

For full technical details, see [AGENTS.md](AGENTS.md).

---

## License

This project is licensed under the **BSD Zero Clause License**.  
See [LICENSE](LICENSE) for details.
