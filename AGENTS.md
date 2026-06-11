# AGENTS.md — Void Compiler Agent Reference

This is the primary reference for AI coding agents working on the Void compiler and standard library. For high-level project docs, see [CLAUDE.md](CLAUDE.md).

---

## Coding Rules

1. Write clean, maintainable, performant code.
2. Do not hardcode behavior that can be implemented directly in Void language.
3. Do not create useless attributes that contribute nothing — code that works without them is better.
4. Do not create excess intrinsics or attributes. Intrinsics are permitted when they are the cleanest or only viable choice; prefer stdlib code, but do not force awkward workarounds just to avoid an intrinsic.
5. Do not hardcode behavior.
6. Write all architectural changes to this file (AGENTS.md) and to CLAUDE.md.
7. Aim not for the program just working, but for the program to be maintainable and clean.
8. No band-aid fixes. If a fix feels hacky, step back and redesign.
9. Keep the AST immutable after parsing; semantic analysis resolves meaning via annotations, not source mutation.
10. When fixing warnings, fix the root cause, not the symptom.

---

## Build & Test Pipeline

```bash
cargo build              # debug build
cargo build --release    # release build
cargo test               # run all 137+ unit tests
cargo clippy             # lint
cargo fmt                # format
```

- Rust edition 2024.
- The project is a single binary crate (`bin "void"`) with inline `#[cfg(test)]` modules.
- No `tests/` integration directory yet — all tests are inline.

---

## Architecture

```
source → Loader → Lexer → Parser → Analyzer → Codegen → Backend (iced-x86) → .o → Linker → binary
```

VBC (`-b`): Codegen → serialized chunks, no backend.  
Object (`-c`): backend only, no linker.

### Component Guide

| Component | Path | Agent Doc |
|-----------|------|-----------|
| Lexer | `src/lexer/` | [src/lexer/AGENTS.md](src/lexer/AGENTS.md) |
| Parser | `src/parser/` | [src/parser/AGENTS.md](src/parser/AGENTS.md) |
| Semantic | `src/semantic/` | [src/semantic/AGENTS.md](src/semantic/AGENTS.md) |
| Bytecode / Codegen | `src/bytecode/` | [src/bytecode/AGENTS.md](src/bytecode/AGENTS.md) |
| Backend (overview) | `src/backend/` | [src/backend/AGENTS.md](src/backend/AGENTS.md) |
| x86_64 Backend | `src/backend/x86_64/` | [src/backend/x86_64/AGENTS.md](src/backend/x86_64/AGENTS.md) |
| LSP | `src/lsp/` | [src/lsp/AGENTS.md](src/lsp/AGENTS.md) |
| Loader | `src/loader.rs` | (inline docs) |
| Project / manifest | `src/project.rs` | (inline docs) |

### Loader (`src/loader.rs`)

- `load_programs` — resolves imports recursively, merges dependency-first, parses as one `Program`.
- Std resolution: `VOID_STD_ROOT` → `~/.void/std` / `%USERPROFILE%/.void/std` → `CARGO_MANIFEST_DIR/std` → `cwd/std`.
- `foo/mod.void` = opaque module directory; `pub import` controls what's exported.
- Deduplicates via canonical-path `HashSet`. Circular imports safe.
- **Namespacing**: every non-entry file gets module-qualified function names (`bar.foo`). Entry files keep bare names.

### Project (`src/project.rs`)

- `void.toml`: `[package]`, `[build]`, `[dependencies]` (path + optional version). `void.lock` validated on build.
- `type = "lib"` → lib project; default entry `src/lib.void`; default output `.vbc`.

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
| `@intrinsic("void.X")` | Safe stdlib wrapper; dispatched by encoder case number. |
| `@derive(Trait, ...)` | Register derived traits for struct. |
| `@panic_handler` | Validate signature; mark as panic handler. |
| `@no_mangle` | Keep function symbol name bare (no module prefix). Useful for entry points and FFI symbols. |
| `@no_crash` | File-level: disable crash handler in entry stub. |

---

## Standard Library Status (`std/src/`)

| Module | Status | Notes |
|--------|--------|-------|
| `core` | Done | write, read, exit, malloc/free/realloc, memcpy/set/move/cmp, strlen, str_concat, int_to_str, float_to_str, str_byte_at, str_from_byte |
| `io` | Done | println, print, eprintln, eprint, read_line — str_variadic |
| `fmt` | Done | `format(template, ...args: str)` — `{}` placeholders, spec-aware coercion |
| `string` | Done | `String`: new, push, push_str, len, as_str, free |
| `panic` | Done | PanicInfo, __void_panic_handler, panic. Codegen injects file/line at call sites. |
| `result` | Done | ok/is_ok/is_err/unwrap/unwrap_err/unwrap_or; `?` operator |
| `option` | Done | is_some/is_none/unwrap/unwrap_or; `?` operator |
| `box` | Done | `Box[T]`: new, get, set, free |
| `traits` | Done | Display, Debug, Clone, Copy, Drop, Iterator, Eq, Ord, Hash, Default, Into, From, Index, Write, arithmetic traits |
| `prelude/mod.void` | Done | re-exports String, Box, Array, option, result, traits, fmt, panic. Auto-injected always. |
| `collections/array` | Done | `Array[T]`: push, get, set, len, free, from, Index impl. Index assignment supported. |
| `collections/map` | Done | open-addressing hash map with tombstones ([details](std/src/collections/AGENTS.md)) |
| `collections/set` | Done | open-addressing hash set ([details](std/src/collections/AGENTS.md)) |
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
| **`void link` built-in linker** | ELF <500B, PE <700B. Triggered on `.o` input. |
| **`void test` runner** | `@test` attribute + `void test` CLI. |
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
| LSP improvements | Partial — see [src/lsp/AGENTS.md](src/lsp/AGENTS.md) |
| `void doc` | Not started |
| JIT VM | Deferred |

---

## Active Work Log

When you complete a feature or fix, append a dated entry here.

| Date | Change |
|------|--------|
| 2026-06-07 | Module function namespacing/mangling implemented. Non-entry files prefix top-level functions with module name (`bar.foo`). Entry files keep bare names. `import bar.foo` errors on collision with local fn. `import bar.foo as b_foo` aliases cleanly. All 137 tests pass. |
| 2026-06-11 | Fixed canonical path mismatch in loader that could cause entry files to be namespaced. Added `@no_mangle` attribute: keeps function symbol name bare (no module prefix). All 146 tests pass. |

---

## Examples

| Example | Description | Details |
|---------|-------------|---------|
| `01-hello` | Minimal "Hello, world!" | [examples/01-hello/AGENTS.md](examples/01-hello/AGENTS.md) |
| `02-structs` | Structs, methods, impl blocks | [examples/02-structs/AGENTS.md](examples/02-structs/AGENTS.md) |
| `03-enums` | Enums with payloads, pattern matching, `Option` | [examples/03-enums/AGENTS.md](examples/03-enums/AGENTS.md) |
| `04-closures` | First-class functions and closures | [examples/04-closures/AGENTS.md](examples/04-closures/AGENTS.md) |
| `05-generics` | Generic functions | [examples/05-generics/AGENTS.md](examples/05-generics/AGENTS.md) |
| `06-crash` | Crash handler demonstration | [examples/06-crash/AGENTS.md](examples/06-crash/AGENTS.md) |
| `07-minimal-hw` | Smallest possible binary via intrinsic | [examples/07-minimal-hw/AGENTS.md](examples/07-minimal-hw/AGENTS.md) |
| `08-array` | `Array[T]` usage | [examples/08-array/AGENTS.md](examples/08-array/AGENTS.md) |
| `09-mangling` | Module namespacing demo | [examples/09-mangling/AGENTS.md](examples/09-mangling/AGENTS.md) |
