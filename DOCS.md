# DOCS.md — Quazilang Project Reference

The primary reference for the Quazilang compiler (`qz`) — covers architecture, coding rules, language syntax, standard library, roadmap, and build pipeline.

---

## Quick Commands

```bash
cargo build              # debug build
cargo build --release    # release build
cargo test               # run all 152+ unit tests
cargo clippy             # lint
cargo fmt                # format
```

CLI (dep: `clap 4.6`):
```bash
qz build <file> [-i|-c] [-o out] [-r] [-s] [--linker path]
qz run / qz check / qz fmt / qz clean
qz new <name> [--lib] / qz init [--lib]
qz debug [-i]
qz lsp
```

Output: `<stem>.qzi` (bytecode), `<stem>.o` (object), `<stem>`/`<stem>.exe` (binary).  
`.qzi` as input: skips frontend, goes straight to backend.  
Linker: `QUAZI_LINKER` env → `ld.lld` → `mold` → `ld` (Linux/macOS); `lld-link` → `link` (Windows). Linux uses `-dynamic-linker` and links `libc.so.6` / `libm.so.6` by full path to avoid GNU linker scripts that `ld.lld` cannot parse.  
`qz build myprog.o` — planned built-in linker path (P1).  
Rust edition 2024.

---

## Coding Rules

1. Write clean, maintainable, performant code.
2. Do not hardcode behavior that can be implemented directly in Quazilang.
3. Do not create useless attributes that contribute nothing — code that works without them is better.
4. Do not create excess intrinsics or attributes. Intrinsics are permitted when they are the cleanest or only viable choice; prefer stdlib code, but do not force awkward workarounds just to avoid an intrinsic.
5. Do not hardcode behavior.
6. Write all architectural changes to this file (DOCS.md).
7. Aim not for the program just working, but for the program to be maintainable and clean.
8. No band-aid fixes. If a fix feels hacky, step back and redesign.
9. Keep the AST immutable after parsing; semantic analysis resolves meaning via annotations, not source mutation.
10. When fixing warnings, fix the root cause, not the symptom.

---

## Architecture

```
source → Loader → Lexer → Parser → Analyzer → Codegen → Backend (iced-x86) → .o → Linker → binary
```

VBC (`-b`): Codegen → serialized chunks, no backend.  
Object (`-c`): backend only, no linker.

### Component Guide

| Component | Path | Docs |
|-----------|------|------|
| Lexer | `src/lexer/` | [src/lexer/DOCS.md](src/lexer/DOCS.md) |
| Parser | `src/parser/` | [src/parser/DOCS.md](src/parser/DOCS.md) |
| Semantic | `src/semantic/` | [src/semantic/DOCS.md](src/semantic/DOCS.md) |
| Bytecode / Codegen | `src/bytecode/` | [src/bytecode/DOCS.md](src/bytecode/DOCS.md) |
| Backend (overview) | `src/backend/` | [src/backend/DOCS.md](src/backend/DOCS.md) |
| x86_64 Backend | `src/backend/x86_64/` | [src/backend/x86_64/DOCS.md](src/backend/x86_64/DOCS.md) |
| LSP | `src/lsp/` | [src/lsp/DOCS.md](src/lsp/DOCS.md) |
| Loader | `src/loader.rs` | (inline docs) |
| Project / manifest | `src/project.rs` | (inline docs) |

- The project is a single binary crate (`bin "qz"`) with inline `#[cfg(test)]` modules.
- No `tests/` integration directory yet — all tests are inline.

### Loader (`src/loader.rs`)

- `load_programs` — resolves imports recursively, merges dependency-first, parses as one `Program`.
- Std resolution: `QUAZI_STD_ROOT` → `~/.quazi/std` / `%USERPROFILE%/.quazi/std` → `CARGO_MANIFEST_DIR/std` → `cwd/std`.
- `foo/mod.qz` = opaque module directory; `pub import` controls what's exported.
- Deduplicates via canonical-path `HashSet`. Circular imports safe.
- **Namespacing**: every non-entry file gets module-qualified function names (`bar.foo`). Entry files keep bare names.

### Project (`src/project.rs`)

- `quazi.toml`: `[package]`, `[build]`, `[dependencies]` (path + optional version). `quazi.lock` validated on build.
- `type = "lib"` → lib project; default entry `src/lib.qz`; default output `.qzi`.

---

## Language Quick Reference

```void
import std.io.stdout;
import std.io as io;
pub fn name[T](param: Type, ...rest: str) ReturnType {
    const x: i32 = 1 + 2;
    var y: &str = "hello";
    var n: u64 = 42 as u64;
    x += 1; x -= 1; x++; x--;
    if (cond) { ... } else { ... }
    for (cond) { ... }                  // while-loop
    for i : 0..10 { ... }              // range loop
    for i : collection { ... }         // iterator loop
    for i, v : collection { ... }      // index+value
    // break; continue;                  // NOT YET IMPLEMENTED — see P1
    var arr = [1, 2, 3]; arr[0];
    ret expr;
}
unsafe fn ptr_fn(p: *u8) *u8 { ret p; }
unsafe { var x = ptr_fn(p); *x = 1; }

// Entry point may take no args or a single Array[str].
fn main(args: Array[str]) i32 { ret args.len() as i32; }

struct Foo[T] { field: T, const flag: bool, }
trait Bar[T] { fn method(x: T) T; }
impl Bar[i32] for Foo[i32] { fn method(x: i32) i32 { ret x; } }
enum Option[T] { Some(T), None, }
match value { Some(v) => v, Option.None => 0, _ => default, }
match value { Some(v) if v > 0 => v, _ => 0, }   // guards

var f: fn(i32, i32) i32 = |x, y| x + y;   // closure
var g: fn() i32 = my_func;                  // fn-name as value
```

**Named arguments**: `foo(x=1, y=2)` — `name=value` pairs at call site. All positional args must precede named args.

Primitives: `i8/i16/i32/i64`, `u8/u16/u32/u64`, `isize`, `usize`, `f16/f32/f64`, `bool`, `str`, `void`, `any`.

### Unsafe System

- `*T` in fn signature → must be `unsafe fn` (S12). Exception: `@syscall`/`@api` implicitly unsafe.
- Calling `unsafe fn` or dereferencing `*T` outside unsafe context → S11.
- `@intrinsic` = safe (unsafety handled internally).
- `*T` ↔ `*U`: all raw pointers mutually compatible. Integer `0` valid as any `*T` (null pointer constant).

### String Model

- `str` / `&str` — interchangeable. Immutable, valid UTF-8, fat pointer internally.
- `String` — owned heap string (`ptr+len+cap`). Local variables auto-clean via `String.free`.
- `Rune = u32` — Unicode codepoint.

---

## Attribute System

| Attribute | Effect |
|-----------|--------|
| `@syscall("name"/num)` | Body → `Syscall+Ret`. Implicitly unsafe. |
| `@api("Symbol")` | Body → `CallExt+Ret`. Win64 on Windows, SysV elsewhere. Implicitly unsafe. |
| `@cfg(key="val")` | Conditional compile. Keys: `target_os`, `target_arch`, `target_abi`. |
| `@inline` | Force inline eligibility (excluded if recursive). |
| `@ignore` / `@ignore(unused_vars)` / `@ignore(dead_code)` | Suppress W01/W02/W03/W07. |
| `@intrinsic("quazi.X")` | Safe stdlib wrapper; dispatched by encoder case number. |
| `@derive(Trait, ...)` | Register derived traits for struct. |
| `@panic_handler` | Validate signature; mark as panic handler. |
| `@no_mangle` | Keep function symbol name bare (no module prefix). Useful for entry points and FFI symbols. |
| `@no_crash` | File-level: disable crash handler in entry stub. |

---

## Project Config (`quazi.toml`)

Minimal example:

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

If a `quazi.lock` file exists, it is used to pin dependency versions. When missing and dependencies are present, a lockfile is created on build/run.

---

## Standard Library Status (`std/src/`)

| Module | Status | Notes |
|--------|--------|-------|
| `core` | Done | write, read, exit, malloc/free/realloc, memcpy/set/move/cmp, strlen, str_concat, int_to_str, float_to_str, str_byte_at, str_from_byte |
| `io` | Done | println, print, eprintln, eprint, read_line — str_variadic |
| `fmt` | Done | `format(template, ...args: str)` — `{}` placeholders, spec-aware coercion |
| `string` | Done | `String`: new, push, push_str, len, as_str, free |
| `panic` | Done | PanicInfo, __quazi_panic_handler, panic. Codegen injects file/line at call sites. |
| `result` | Done | ok/is_ok/is_err/unwrap/unwrap_err/unwrap_or; `?` operator |
| `option` | Done | is_some/is_none/unwrap/unwrap_or; `?` operator |
| `box` | Done | `Box[T]`: new, get, set, free |
| `traits` | Done | Display, Debug, Clone, Copy, Drop, Iterator, Eq, Ord, Hash, Default, Into, From, Index, Write, arithmetic traits |
| `prelude/mod.qz` | Done | re-exports String, Box, Array, option, result, traits, fmt, panic. Auto-injected always. |
| `collections/array` | Done | `Array[T]`: push, get, set, len, free, from, Index impl. Index assignment supported. |
| `collections/map` | Done | open-addressing hash map with tombstones |
| `collections/set` | Done | open-addressing hash set |
| `unix` | Done | raw syscall wrappers |
| `windows` | Done | Win32 API wrappers |
| `fs` | Done | File open/read/write/close/seek/sync etc. |
| `net` | Done | TcpListener, TcpStream, UdpSocket |
| `os` | Done | exit, sleep, yield_cpu, getpid, getenv, cwd, etc. |
| `thread` | Done | spawn/join. No-capture only. |

---

## Roadmap

### Philosophy
Fast binaries, small output, zero runtime waste. No LLVM, no GCC, no libc. `@intrinsic` → raw syscalls (Linux) or Win32 (Windows). VBC = stable portable IR.

### P0 — Critical Bugs / Safety

| Item | Status |
|------|--------|
| Enhanced Crash/Panic handler | ✅ Done |
| Fix encoder silent fallback | ✅ Done |
| Fix non-slice iterator codegen | ✅ Done |
| For-loop move semantics | ✅ Done |
| Index assignment | ✅ Done |
| Human-readable move errors | ✅ Done |
| **Module function namespacing/mangling** | ✅ **Done** |

### P1 — High Impact

| Item | Target |
|------|--------|
| **Bitwise operators** | `&` `|` `^` `<<` `>>` — need lexer tokens, parser variants, semantic typecheck, codegen mapping. |
| **Loop control (`break`, `continue`)** | Need lexer tokens, parser `StmtKind`, semantic reachability, codegen loop targets. |
| **`else if` chains** | Parser `else_if: Vec<(Expr, Block)>` on `If` node; codegen chained jumps. |
| **`unsafe` block sugar** | `fn a() void unsafe { ... }` — safe fn with unsafe body (treated as unsafe fn). Also `@cfg(...) unsafe { ... }` parser desugar. Closures too. |
| **AOT `@cfg` stripping** | Prune dead `CfgBlock` AST nodes before codegen. |
| **`qz link` built-in linker** | ELF <500B, PE <700B. Triggered on `.o` input. |
| **`qz test` runner** | `@test` attribute + `qz test` CLI. |
| **`pub` on types** | Enforce `public` for structs, traits, enums, type aliases (same S04 pattern as functions). |

### P2 — Codegen Quality

| Item | Target |
|------|--------|
| Threshold-based auto-inline | Heuristic inline beyond `@inline` only. |
| Cross-basic-block const folding | Currently local expression only. |
| Strength reduction | `x * 2` → `x << 1`, etc. |

### P3 — Platform / Runtime

| Item | Target |
|------|--------|
| macOS Mach-O backend | Actually implement Mach-O output, `__start` stub, relocations. |
| Ownership / RAII bytecode | Real `Move`/`Drop`/`Dup` opcodes, `Drop` trait dispatch, recursive drops. |
| Atomic opcodes in encoder | `AtomicAdd`, `AtomicCas` lowering. |

### P4 — Language Features

| Feature | Status |
|---------|--------|
| Format string literals | Not started — `"value = {x}"` compile-time interpolation |
| Nested struct patterns | Not started — `Point { x, y }` in match arms |
| `async`/`await` | Not started — defer until lifetimes done |

### P5 — Tooling

| Component | Status |
|-----------|--------|
| LSP improvements | Partial — see [src/lsp/DOCS.md](src/lsp/DOCS.md) |
| `qz doc` | Not started |
| JIT VM | Deferred |

---

## Active Work Log

| Date | Change |
|------|--------|
| 2026-06-07 | Module function namespacing/mangling implemented. Non-entry files prefix top-level functions with module name (`bar.foo`). Entry files keep bare names. `import bar.foo` errors on collision with local fn. `import bar.foo as b_foo` aliases cleanly. All 137 tests pass. |
| 2026-06-11 | Fixed canonical path mismatch in loader that could cause entry files to be namespaced. Added `@no_mangle` attribute: keeps function symbol name bare (no module prefix). All 146 tests pass. |
| 2026-06-11 | Implemented `fn main(args: Array[str])` support. Semantic analysis validates the parameter signature and sets `SemanticReport.main_takes_args`. Linux startup stubs build an `Array[str]` from `argc`/`argv` and pass it in `rdi`; Windows stubs use `__getmainargs` to obtain parsed argv and build the same array. Added `examples/13-args`. All 151 tests pass. |
| 2026-06-11 | Hardened slice support: `types_compatible` now rejects fixed-size array ↔ slice coercion, which previously generated invalid code and crashed at runtime. Added a clear `S08` diagnostic for this case. `for item : items` over variadic slices continues to work. Full array-to-slice coercion remains on the roadmap. All 152 tests pass. |
| 2026-07-26 | **Rebranding**: Quazilang → Quazilang. Binary renamed `void` → `qz`. Config files `quazi.toml`/`quazi.lock` → `quazi.toml`/`quazi.lock`. Env vars `QUAZI_LINKER`/`QUAZI_STD_ROOT` → `QUAZI_LINKER`/`QUAZI_STD_ROOT`. Internal ABI symbols `__quazi_*` → `__quazi_*`. Intrinsic namespace `quazi.X` → `quazi.X`. All docs merged from AGENTS.md + CLAUDE.md into DOCS.md. |

---

## Examples

| Example | Description | Details |
|---------|-------------|---------|
| `01-hello` | Minimal "Hello, world!" | [examples/01-hello/DOCS.md](examples/01-hello/DOCS.md) |
| `02-structs` | Structs, methods, impl blocks | [examples/02-structs/DOCS.md](examples/02-structs/DOCS.md) |
| `03-enums` | Enums with payloads, pattern matching, `Option` | [examples/03-enums/DOCS.md](examples/03-enums/DOCS.md) |
| `04-closures` | First-class functions and closures | [examples/04-closures/DOCS.md](examples/04-closures/DOCS.md) |
| `05-generics` | Generic functions | [examples/05-generics/DOCS.md](examples/05-generics/DOCS.md) |
| `06-crash` | Crash handler demonstration | [examples/06-crash/DOCS.md](examples/06-crash/DOCS.md) |
| `07-minimal-hw` | Smallest possible binary via intrinsic | [examples/07-minimal-hw/DOCS.md](examples/07-minimal-hw/DOCS.md) |
| `08-array` | `Array[T]` usage | [examples/08-array/DOCS.md](examples/08-array/DOCS.md) |
| `09-mangling` | Module namespacing demo | [examples/09-mangling/DOCS.md](examples/09-mangling/DOCS.md) |
| `10-bitwise` | Bitwise operators | [examples/10-bitwise/DOCS.md](examples/10-bitwise/DOCS.md) |
| `11-elseif` | `else if` chains | [examples/11-elseif/DOCS.md](examples/11-elseif/DOCS.md) |
| `12-loop-control` | `break` and `continue` | [examples/12-loop-control/DOCS.md](examples/12-loop-control/DOCS.md) |
| `13-args` | `fn main(args: Array[str])` | [examples/13-args/DOCS.md](examples/13-args/DOCS.md) |
