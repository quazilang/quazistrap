# quazilang

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
- **Module system** — `import std.io;` / `quazi.toml` project manifest
- **LSP support** — hover, diagnostics, go-to-definition, completion, formatting
- **Cross-platform** — Linux (ELF) and Windows (PE) targets

---

## Install & Build

Building the compiler requires **Rust 2024 edition** (latest stable recommended).
Plain x86-64 Linux Quazi builds need no system linker or C toolchain. Windows
executables currently require `lld-link`/`link.exe` and Windows SDK libraries;
native C sources, archives, shared libraries, and explicit `-l` dependencies
also use their corresponding external tools.

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
all characters exactly and never decode escapes: `` `\n\e\x41\u{41}` ``.

---

## CLI Reference

```
qz build [source.qz|program.qzi|native.o ...] [-o out] [-i] [-c] [-r] [-s]
         [--linker builtin|path] [-L dir] [-l name]
qz run [source.qz|program.qzi|native.o ...]  # build and run; project if omitted
qz header [file ...] [-o quazi.h] [--target x86_64-linux|x86_64-windows]
qz check                  # type-check without compiling
qz fmt                    # trim trailing whitespace in .qz files
qz clean                  # remove build artifacts
qz new <name> [--lib]     # scaffold a new project
qz init [--lib]           # init in current directory
qz lsp                    # start language server
```

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
- `--linker builtin` — require the in-process Linux ELF linker
- `--linker <path>` — explicitly use an external linker
- `-L <dir>` / `-l <name>` — opt into a native library search path/library

**Environment variables:**
- `QUAZI_LINKER=builtin` — require the in-process Linux ELF linker
- `QUAZI_LINKER=/path/to/linker` — select an external linker
- `QUAZI_TRACE=1` — enable crash backtraces

---

## Built-in Linker and Runtime

On x86-64 Linux, ordinary `qz build` and `qz run` commands use Quazi's built-in
static ELF linker automatically. It accepts Quazi source or QZI plus any number
of ELF `.o` inputs, and it also accepts an object-only input list:

```bash
qz build src/main.qz native/helper.o -o app
qz build program.qzi native/helper.o -o app
qz run program.o helper.o
```

No library is implicit. In particular, libc and libm are not linked merely
because they exist on the host. Passing `-l`, an archive/shared-library input,
an explicit external linker path, or a non-Linux target selects the external
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

The linker is experimental and currently limited to static x86-64 ELF
executables and relocatable `.o` inputs. PE/COFF, Mach-O, archives, shared
objects, TLS, linker scripts, and general-purpose ELF compatibility are not yet
implemented. See [docs/LINKER.md](docs/LINKER.md) for selection rules, supported
ELF features, startup behavior, runtime details, diagnostics, and limitations.

---

## Project Config (`quazi.toml`)

```toml
[package]
name = "hello"
version = "0.1.0"

[build]
entry = "src/main.qz"   # optional, defaults to src/main.qz
src = "src"               # optional, defaults to src

[dependencies]
utils = { path = "../utils", version = "0.1.0" }
```

If a `quazi.lock` file exists, it pins dependency versions. It is created automatically on first build when dependencies are present.

---

## Standard Library

| Module | Description |
|--------|-------------|
| `std.io` | `println`, `print`, `eprintln`, `read_line` |
| `std.fmt` | `format("{}", ...)` — `{}` placeholder formatting |
| `std.string` | `String`: owned heap string with push, free |
| `std.collections.array` | `Array[T]`: dynamic array with index, push, iteration |
| `std.collections.map` | Hash map with open-addressing |
| `std.collections.set` | Hash set |
| `std.fs` | File open/read/write/close/seek |
| `std.net` | `TcpListener`, `TcpStream`, `UdpSocket` |
| `std.os` | exit, sleep, getpid, getenv, cwd |
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
| [`01-hello`](examples/01-hello/) | Minimal "Hello, world!" |
| [`02-structs`](examples/02-structs/) | Structs, methods, impl blocks |
| [`03-enums`](examples/03-enums/) | Enums with payloads, pattern matching |
| [`04-closures`](examples/04-closures/) | First-class functions and closures |
| [`05-generics`](examples/05-generics/) | Generic functions |
| [`06-crash`](examples/06-crash/) | Crash handler and stack traces |
| [`07-minimal-hw`](examples/07-minimal-hw/) | Smallest binary via raw intrinsic |
| [`08-array`](examples/08-array/) | `Array[T]` usage |
| [`09-mangling`](examples/09-mangling/) | Module namespacing demo |
| [`10-bitwise`](examples/10-bitwise/) | Bitwise operators |
| [`11-elseif`](examples/11-elseif/) | `else if` chains |
| [`12-loop-control`](examples/12-loop-control/) | `break` and `continue` |
| [`13-args`](examples/13-args/) | Command-line arguments via `Array[str]` |
| [`14-io-read`](examples/14-io-read/) | Example showing I/O reads. |
| [`15-logical`](examples/15-logical/) | Logical operators: `!`, `&&`, `\|\|` |
| [`16-pub-types`](examples/16-pub-types/) | Example showing public types |
| [`17-constfold`](examples/17-constfold/) | Cross-basic-block constant propagation |
| [`18-formatting`](examples/18-formatting/) | Formatting, raw strings, ANSI, and escape sequences |
| [`19-cvariadics`](examples/19-cvariadics/) | C-style variadic `@api` via bare `...` — calls libc `printf` |

---

## Architecture

```
source → Loader → Lexer → Parser → Analyzer → Codegen → Backend (iced-x86) → .o → Linker → binary
```

For full technical details, see [AGENTS.md](AGENTS.md).

---

## License

This project is licensed under the **BSD Zero Clause License**.  
See [LICENSE](LICENSE) for details.
