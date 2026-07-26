# quazilang

**A fast, strict, and expressive systems programming language with no runtime, no libc, and no LLVM.**

Quazilang (`qz`) compiles directly to native x86-64 binaries via its own backend powered by `iced-x86`. It features a clean C-like syntax, strong generics, trait-based polymorphism, move semantics, and a growing standard library — all without depending on LLVM, GCC, or libc.

---

## Features

- **Zero-dependency native codegen** — compiles to ELF/PE via raw x86-64 assembly, no LLVM or GCC
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

Requires **Rust 2024 edition** (latest stable recommended).

```bash
git clone https://github.com/quazilang/quazistrap
cd quazistrap
cargo build --release
# binary is at: target/release/qz
```

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

```void
import std.io;

fn main() i32 {
    io.println("Hello, Quazilang!");
    ret 0;
}
```

```void
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

---

## CLI Reference

```
qz build <file|dir> [-o out] [-i] [-c] [-r] [-s] [--linker path]
qz run                    # build and run (reads quazi.toml)
qz check                  # type-check without compiling
qz fmt                    # trim trailing whitespace in .qz files
qz clean                  # remove build artifacts
qz new <name> [--lib]     # scaffold a new project
qz init [--lib]           # init in current directory
qz lsp                    # start language server
```

**Flags:**
- `-i` — emit `.qzi` bytecode only (no backend)
- `-c` — emit `.o` object file only (no linker)
- `-r` — release mode
- `-s` — strip symbols

**Environment variables:**
- `QUAZI_LINKER=/path/to/linker` — override linker detection
- `QUAZI_STD_ROOT=/path/to/std` — override standard library location
- `QUAZI_TRACE=1` — enable crash backtraces

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

---

## Architecture

```
source → Loader → Lexer → Parser → Analyzer → Codegen → Backend (iced-x86) → .o → Linker → binary
```

For full technical details, see [DOCS.md](DOCS.md).

---

## Contributors

- **namnam1105** — lexer, tokenizer, AST
- **amapekibert** — spans, full generic syntax, readable diagnostics

---

## License

This project is licensed under the **BSD Zero Clause License**.  
See [LICENSE](LICENSE) for details.
