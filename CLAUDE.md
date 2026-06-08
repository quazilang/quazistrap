# CLAUDE.md — Void Project Overview

> For agent coding rules, build pipeline, roadmap, and detailed subsystem docs, see **[AGENTS.md](AGENTS.md)**.

## Quick Commands

```bash
cargo build / cargo build --release / cargo test / cargo clippy / cargo fmt
```

CLI (dep: `clap 4.6`):
```bash
void build <file> [-b|-c] [-o out] [-r] [-s] [--linker path]
void run / void check / void fmt / void clean
void new <name> [--lib] / void init [--lib]
void debug [-b]
void lsp
```

Output: `<stem>.vbc` (bytecode), `<stem>.o` (object), `<stem>`/`<stem>.exe` (binary).  
`.vbc` as input: skips frontend, goes straight to backend.  
Linker: `VOID_LINKER` env → `ld.lld` → `mold` → `ld` (Linux/macOS); `lld-link` → `link` (Windows). Linux uses `-dynamic-linker` and links `libc.so.6` / `libm.so.6` by full path to avoid GNU linker scripts that `ld.lld` cannot parse.  
`void build myprog.o` — planned built-in linker path (P1).  
Rust edition 2024.

## Architecture

```
source → Loader → Lexer → Parser → Analyzer → Codegen → Backend (iced-x86) → .o → Linker → binary
```

VBC (`-b`): Codegen → serialized chunks, no backend. Object (`-c`): backend only, no linker.

### Subsystems

| Component | Location | Agent Doc |
|-----------|----------|-----------|
| Lexer | `src/lexer/` | [src/lexer/AGENTS.md](src/lexer/AGENTS.md) |
| Parser | `src/parser/` | [src/parser/AGENTS.md](src/parser/AGENTS.md) |
| Semantic | `src/semantic/` | [src/semantic/AGENTS.md](src/semantic/AGENTS.md) |
| Bytecode / Codegen | `src/bytecode/` | [src/bytecode/AGENTS.md](src/bytecode/AGENTS.md) |
| Backend (overview) | `src/backend/` | [src/backend/AGENTS.md](src/backend/AGENTS.md) |
| x86_64 Backend | `src/backend/x86_64/` | [src/backend/x86_64/AGENTS.md](src/backend/x86_64/AGENTS.md) |
| LSP | `src/lsp/` | [src/lsp/AGENTS.md](src/lsp/AGENTS.md) |

## Language Syntax

```void
import std.io.stdout;
import std.io as io;
pub fn name[T](param: Type, ...rest: str) ReturnType {
    const x: i32 = 1 + 2;
    var y: &str = "hello";
    var n: u64 = 42 as u64;
    x += 1; x -= 1; x++; x--;
    if (cond) { ... } else { ... }
    for (cond) { ... }                  // while-loop: ForLoop::Cond
    for i : 0..10 { ... }              // range loop
    for i : collection { ... }         // iterator loop
    for i, v : collection { ... }      // index+value
    // break; continue;                  // NOT YET IMPLEMENTED — see P1 roadmap
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
match value { Some(v) if v > 0 => v, _ => 0, }   // guards supported

var f: fn(i32, i32) i32 = |x, y| x + y;   // closure
var g: fn() i32 = my_func;                  // fn-name as value
fn takes_cb(cb: fn(i32) i32, v: i32) i32 { ret cb(v); }

fn choose_value(a: i32) i32 {
    if a > 5 { ret 67; }
    else if a < 5 { ret 52; }
    else { ret 42; }
}
```

**Named arguments**: `foo(x=1, y=2)` — `name=value` pairs at call site. All positional args must precede named args. Named args resolved to param position at compile time; unknown name or position conflict = S09 error.

Primitives: `i8/i16/i32/i64`, `u8/u16/u32/u64`, `isize`, `usize`, `f16/f32/f64`, `bool`, `str`, `void`, `any`.

## Roadmap

See **[AGENTS.md](AGENTS.md)** for the full P0–P5 roadmap with priorities, active work log, and coding rules.

### P0 — Critical Bugs / Safety

| Item | Status |
|------|--------|
| Enhanced Crash/Panic handler | ✅ Done |
| Fix encoder silent fallback | ✅ Done |
| Fix non-slice iterator codegen | ✅ Done |
| For-loop move semantics | ✅ Done |
| Index assignment | ✅ Done |
| Human-readable move errors | ✅ Done |
| Module function namespacing/mangling | ✅ Done |

### P1 — High Impact

- Bitwise operators (`&` `|` `^` `<<` `>>`)
- Loop control (`break`, `continue`)
- `else if` chains
- AOT `@cfg` stripping
- `void link` built-in linker
- `void test` runner
- `pub` on types

### P2 — Codegen Quality

- Threshold-based auto-inline
- Cross-basic-block const folding
- Strength reduction

### P3 — Platform / Runtime

- macOS Mach-O backend
- Ownership / RAII bytecode
- Atomic opcodes in encoder

### P4 — Language Features

- Format string literals
- Nested struct patterns
- `async`/`await`

### P5 — Tooling

- LSP improvements (partial)
- `void doc`
- JIT VM (deferred)

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

**Test coverage**: 137 inline `#[cfg(test)]` unit tests. No integration test suite or `tests/` directory yet.
