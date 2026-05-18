# CLAUDE.md

## Commands

```bash
cargo build / cargo build --release / cargo test / cargo clippy / cargo fmt
```

CLI (dep: `clap 4.6`):
```bash
void build <file> [-b|-c] [-o out] [-r] [-s] [--linker path]
void run / void check / void fmt / void clean
void new <name> [--lib] / void init [--lib]
void debug [-b]
```

Output: `<stem>.vbc` (bytecode), `<stem>.o` (object), `<stem>`/`<stem>.exe` (binary).  
`.vbc` as input: skips frontend, goes straight to backend.  
Linker: `VOID_LINKER` env → `ld.lld` → `mold` → `ld` (Linux/macOS); `lld-link` → `link` (Windows).  
Rust edition 2024.

## Architecture

```
source → Loader → Lexer → Parser → Analyzer → Codegen → Backend (iced-x86) → .o → Linker → binary
```

VBC (`-b`): Codegen → serialized chunks, no backend. Object (`-c`): backend only, no linker.

### Loader (`src/loader.rs`)
- `load_programs` — resolves imports recursively, merges dependency-first, parses as one `Program`.
- Std resolution: `VOID_STD_ROOT` → `~/.void/std` / `%USERPROFILE%/.void/std` → `CARGO_MANIFEST_DIR/std` → `cwd/std`.
- `foo/mod.void` = opaque module directory; `pub import` controls what's exported.
- Deduplicates via canonical-path `HashSet`. Circular imports safe.

### Project (`src/project.rs`)
- `void.toml`: `[package]`, `[build]`, `[dependencies]` (path + optional version). `void.lock` validated on build.
- `type = "lib"` → lib project; default entry `src/lib.void`; default output `.vbc`.

### Lexer (`src/lexer/`)
- `&&`/`||` not dedicated tokens — synthesized by parser via `match_and_and()`/`match_or_or()`.
- Generics use `[T]` square brackets. `pub` silently consumed (no effect).
- `TokenKind::While` exists but is **not** handled in `parse_stmt` — use `for (cond) {}` instead.

### Parser (`src/parser/`)
- `ast.rs`: all nodes are `Spanned<T>`. Two `Span` types; `to_ast_span` converts.
- Expression precedence: assign → logical-or → logical-and → equality → comparison → term → factor → unary → postfix → primary.
- Error codes: E00 generic, E01 expected ident, E02 expected token, E03 bad item position, E04 EOF in block, E05 expected type.
- Import: `import std.io.stdout;` / `import a.b.{x,y};` / `import a.b as c;` / `import a.b.*;`
- Closure: `|params| expr` — `Pipe` in primary position. Params are bare idents (no type).
- Fn pointer type: `fn(T, U) V` — greedy return type via `peek_is_type_start()`.
- Variadics: `...args: T` in param list; inside fn body `args` is `Slice[T]` with `.len()`.

### Semantic (`src/semantic/`)
Five sequential passes:
1. **Declare** — register fns, structs, traits, enums, imports. `@cfg`-disabled items skipped.
2. **Typecheck** — scope, inference, compatibility, init checks, expression annotations.
3. **Unused** — W01 unused var, W02 unused param, W03 unused fn/import.
4. **Dead code** — reachability, W04 unreachable after return.
5. **Optimize** — inline candidates, const folding, math identity reduction, lazy import hints.

**`types_compatible` rules:**
- `Any` ↔ everything.
- `Named` ↔ everything (generic monomorphization).
- Integer → float (implicit widening for literals).
- `*T` ↔ `*U` (all raw pointers mutually compatible — C void* semantics).
- Integer ↔ `*T` (null pointer constant support: `0` is valid `*T`).
- `Str` ↔ `Ref { inner: Str }` always compatible.
- `Named { name }` ↔ `Dyn { trait_name }` — compatible if `trait_impls[name]` contains `trait_name`.
- `Dyn { a }` ↔ `Dyn { b }` — compatible if `a == b`.

**Warning suppression**: `@ignore` / `@ignore(unused_vars)` / `@ignore(dead_code)`.  
**`@cfg` evaluation**: `target_os`, `target_arch`, `target_abi` evaluated against `std::env::consts`. Applied in: declare pass (fn/impl methods), typecheck CfgBlock, unused CfgBlock.

**Borrow checker** (`borrow.rs`): move semantics for Named types (structs/enums). S10 = use-after-move / move-in-loop. `reassign_targets` set suppresses move-in-loop for `x = x.method()` patterns (value immediately re-owned).

### Bytecode (`src/bytecode/`)
VBC — platform-independent, AOT-only. **6 bytes/instruction**: `[opcode u8][operands 4B][flags u8]`.

Operand layouts: **RRR** (dst/src1/src2), **RI16** (dst/imm16 LE), **MEM** (val/base/offset16 LE signed).

Opcode groups:
- `0x00–0x0F` data movement: `Nop`, `Mov`, `MovI`, `MovConst`
- `0x10–0x1F` arithmetic/logic: `Add`–`Sar`
- `0x20–0x2F` memory: `Load`, `Store`, `Lea`, `Move`, `Drop`, `Dup`
- `0x30–0x3F` control: `Cmp`, `Jmp`, `Je`–`Jnz`, `CallIdx`, `CallReg`, `Ret`
- `0x40–0x4F` structs: `New`, `NewObj`, `FieldLoad`, `FieldStore`, `VtblLoad`
- `0x50–0x5F` foreign: `AtomicAdd`, `AtomicCas`, `MemFence`, `Spawn`, `CallExt=0x5D`, `Syscall=0x5E`
- `0x60–0x6F` strings: `StrLen=0x60`, `StrConcat=0x61`, `StrToInt=0x62`, `StrToFloat=0x63`, `PrimToStr=0x64`, `StrAsStr=0x65`

Key: `ENUM_DISCRIM_OFFSET=0`, `ENUM_PAYLOAD_OFFSET=8`, `enum_variant_alloc_size(n)=((n+1)*8).max(16)`.  
`Chunk` = fn code + const pool + name + param_count + reg_count. Return value always in `r0`.

**VBC file**: magic `\x00VBC`, version `0x02`, chunk_count u32 LE; per chunk: name, param_count, reg_count, consts, instrs.

**Const pool tags**: `0`=Int(i64), `1`=Float(f64), `2`=Str(u16_len+bytes), `3`=FnAddr(u16_len+bytes), `4`=VtableAddr(type u16+bytes, trait u16+bytes).

### Codegen (`src/bytecode/codegen.rs`)
- Pass 1: assign fn-table indices. Pass 2: compile via `FnCompiler` (virtual reg allocator). Post-pass: inline expansion with jump-target fixup. Then: `elim_dead_regs` + `linear_scan_alloc` per chunk.
- `@syscall` → `Syscall+Ret`. `@api` → `CallExt+Ret`.
- Const-fold: `ConstValue` in `const_map` → `MovI`/`MovConst` directly.
- `&&`/`||`: short-circuit via `Jz`/`Jnz`.
- Variadics: call-site packs coerced args into consecutive slots, emits `Lea` (ptr) + `MovI` (len), passes as two registers to callee.
- Enum constructors: `New` + discriminant `FieldStore` at `ENUM_DISCRIM_OFFSET`, payloads at `ENUM_PAYLOAD_OFFSET+i*8`.
- `?` operator: reads discriminant, uses `.expect()` for tag lookup (no silent fallback).
- Struct: `New(size)` + `FieldStore` per field in declaration order. Field access → `FieldLoad(dst, ptr, offset)`.
- Fn-name as value: `MovConst(FnAddr(name))`. Variable callee: `CallArg*+CallReg`.
- Closure: `__void_closure_N` chunk; captures detected via `capture_ident_names`; env struct heap-allocated; fn ptr at `ENUM_DISCRIM_OFFSET`, captures at `ENUM_PAYLOAD_OFFSET+i*8`; hidden env ptr in r0 on call.
- **Monomorphization**: top-level fn `id[T]` call → `id<i32>` mangled chunk. Impl method `Box[i32].get` → mangled `Box.get<i32>` chunk. Pre-pass searches both `ItemKind::Fn` and `ItemKind::Impl`. `struct_generic_params` in `SemanticReport` maps struct name → param names for subst. Mangled format: `name<type1,type2>`.
- **Struct mono MethodCall**: receiver type `Named { type_args }` → mangle call target; falls back to unmangled if mangled not in fn_index.
- Intrinsic dispatch: `INTRINSIC_MAP` HashMap; array ops via `INTRINSIC_OPCODE_MAP` HashMap.
- **Pattern matching**: `PatternKind` = `Wildcard | Bind(String) | Literal(LiteralValue) | Variant { enum_name, variant, sub_patterns }`. `compile_pattern_match` recurses into sub_patterns. String literal match: StrLen fast-path + byte loop via intrinsic ID 23 (consecutive regs required: r_a, r_a+1).
- **Format specs**: `extract_format_specs(template)` parses `{:spec}` at compile time → `Vec<String>`. `strip_format_specs` rewrites `{:spec}` → `{}` for runtime `format()`. `coerce_with_spec(reg, spec, span)` applies int/float specs → extended `PrimToStr` tags. str_variadic fns use spec-aware coercion per arg slot.
- **dyn Trait**: `coerce_to_dyn(obj_reg, type_name, trait_name)` allocates 16-byte fat pointer — `fat[0]=concrete_ptr`, `fat[8]=vtable_ptr` (via `MovConst(VtableAddr)`). MethodCall on `Dyn` receiver: `FieldLoad fat[8]→vtbl_ptr`, `VtblLoad vtbl_ptr[slot]→fn_ptr`, `FieldLoad fat[0]→data_ptr`, `CallArg*+CallReg`.

### Regalloc (`src/bytecode/regalloc.rs`)
Two passes run on each chunk after inline expansion:
1. **`elim_dead_regs`**: iterative dead-def removal → Nop → `strip_nops_fix_jumps` → `compact_regs` (params pinned to identity; rest sequential).
2. **`linear_scan_alloc`**: builds live intervals, finds **pinned** regs (params; Intrinsic/Syscall consecutive arg groups when `flags>1`; Lea(offset=0) adjacent MovI base groups), then linear scans non-pinned with slot reuse. Back-edge extension: for each backward jump at `j` targeting `t`, any reg with `start < t && end >= t` gets `end = j` — prevents slot recycling across loop iterations.

Key: `Ret` has `ops[0]` = return-value register (remapped by `remap_instr_regs`; encoder uses `slot(ops[0])` not hardcoded `slot(0)`).

### Backend (`src/backend/`)
```
src/backend/
├── mod.rs         # Backend trait, select_backend()
├── target.rs      # TargetSpec { arch, os, abi, emit_start }
├── linker.rs      # find + exec linker
└── x86_64/
    ├── encoder.rs # FnEncoder: Chunk → (bytes, PendingReloc[])
    ├── sections.rs / symbols.rs / relocations.rs / start.rs
```

**Calling conventions**: SysV (Linux/macOS): `rdi,rsi,rdx,rcx,r8,r9`→`rax`. Win64: `rcx,rdx,r8,r9`→`rax`; args 5-6 at `[rsp+32/40]`.  
**VBC reg N** → `[rbp-(N+1)*8]`. Frame: `round_to_16(regs*8)` SysV, `round_to_16(regs*8+48)` Win64.  
**Relocs**: `Plt32` (calls), `Pc32` (RIP-relative data). Addend always `-4`.  
**Encoder**: emits dummy `call fn_start` / `lea rax,[fn_start]`, records pending relocs, zeros displacement bytes after assembly.  
**Implemented**: `New` (calloc), `FieldLoad/Store`, `VtblLoad`, `CallReg`. `NewObj` = placeholder.

**Entry stubs** (no CRT needed):
- Linux `_start`: `xor rbp,rbp` → `call main` → `mov rdi,rax; mov rax,60; syscall`
- Windows `mainCRTStartup`: `sub rsp,40` → `call main` → `mov rcx,rax; call ExitProcess`

**Target support**:
| OS | Format | ABI | Status |
|----|--------|-----|--------|
| Linux x86-64 | ELF64 | SysV | Full |
| Windows x86-64 | PE/COFF | Win64 | Full (needs `lld-link` + `LIB`) |
| macOS x86-64 | Mach-O | SysV | Partial |

## Unsafe System

- `*T` in fn signature → must be `unsafe fn` (S12). Exception: `@syscall`/`@api` implicitly unsafe.
- Calling `unsafe fn` or dereferencing `*T` outside unsafe context → S11.
- `@intrinsic` = safe (unsafety handled internally).
- `*T` ↔ `*U`: all raw pointers mutually compatible. Integer `0` valid as any `*T` (null pointer constant).

## Language Syntax

```void
import std.io.stdout;
pub fn name[T](param: Type, ...rest: str) ReturnType {
    const x: i32 = 1 + 2;
    var y: &str = "hello";
    x += 1; x -= 1; x++; x--;
    if (cond) { ... } else { ... }
    for (cond) { ... }                  // while-loop: ForLoop::Cond
    for i : 0..10 { ... }              // range loop
    for i : collection { ... }         // iterator loop
    for i, v : collection { ... }      // index+value
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

var f: fn(i32, i32) i32 = |x, y| x + y;   // closure
var g: fn() i32 = my_func;                  // fn-name as value
fn takes_cb(cb: fn(i32) i32, v: i32) i32 { ret cb(v); }
```

**Named arguments**: `foo(x=1, y=2)` — `name=value` pairs at call site. All positional args must precede named args. Named args resolved to param position at compile time; unknown name or position conflict = S09 error.

Primitives: `i8/i16/i32/i64`, `u8/u16/u32/u64`, `isize`, `usize`, `f16/f32/f64`, `bool`, `str`, `void`, `any`.  
**`while` keyword exists in lexer but is NOT parsed** — use `for (cond) {}` (`ForLoop::Cond`).

## String Model

- `str` / `&str` — interchangeable. Immutable, valid UTF-8, fat pointer internally.
- `String` — owned heap string (`ptr+len+cap`). RAII.
- `Rune = u32` — Unicode codepoint. `RuneIterator` iterates UTF-8 codepoints.

Key API: `s.len()→StrLen`, `s.to_string()→PrimToStr`, `s.as_str()→StrAsStr`, `s.parse[i32]()→StrToInt`, `s.parse[f64]()→StrToFloat`.

## Standard Library (`std/src/`)

Source-based `.void` files, merged at compile time.

| Module | Status | Notes |
|--------|--------|-------|
| `core` | Done | write, read, exit, malloc/free/realloc, memcpy/set/move/cmp, strlen, str_concat, int_to_str, float_to_str, str_byte_at, str_from_byte |
| `io` | Done | println, print, eprintln, eprint, read_line — str_variadic (auto-coerces args) |
| `fmt` | Done | `format(template, ...args: str) str` — pure void, `{}` placeholders, byte-by-byte parsing |
| `string` | Done | String: new, push, push_str, len, as_str, bytes |
| `panic` | Done | PanicInfo, __void_panic_handler, panic |
| `result` | Done | ok/is_ok/is_err/unwrap/unwrap_err/unwrap_or; `?` operator |
| `option` | Done | is_some/is_none/unwrap/unwrap_or; `?` operator |
| `box` | Done | Box[T]: new, get, set |
| `traits` | Partial | Display, Debug, Clone, Copy, Drop, Iterator defined |
| `prelude/mod.void` | Done | re-exports String, Box, option, result, traits, fmt, panic |
| `collections/vec` | WIP | push, get, len, iteration |
| `collections/map` | WIP | insert, get, contains |
| `unix` | Done | raw syscall wrappers |
| `windows` | Done | Win32 API wrappers |
| `fs` | Done | File open/create/read/write/close/seek/sync/truncate/exists/remove/mkdir/rmdir/rename/chmod |
| `net` | Done | TcpListener, TcpStream, UdpSocket. 2 intrinsics: bind_tcp (20), connect_tcp (21). accept/send/recv/close via unix.* |
| `os` | Done | exit, sleep, yield_cpu, getpid/ppid, getenv, cwd (via unix.getcwd), kill, umask |
| `thread` | Done | spawn/join. 2 intrinsics: thread_spawn (18), thread_join (19). No-capture only. |

**Active `@intrinsic` dispatch** (encoder case IDs):
| Intrinsic | ID | Effect |
|-----------|-----|--------|
| `void.thread.spawn` | 18 | pthread_create / CreateThread |
| `void.thread.join` | 19 | pthread_join / WaitForSingleObject+CloseHandle |
| `void.net.bind_tcp` | 20 | bind sockfd to 0.0.0.0:port |
| `void.net.connect_tcp` | 21 | inet_pton + connect |
| `void.str.byte_at` | 23 | `movzx rax, byte[rax+rcx]` — byte at index |
| `void.str.from_byte` | 24 | malloc 2 bytes, write byte+null, return ptr |
| `void.array.store` | — | ArrayStore opcode (HashMap dispatch) |
| `void.array.load` | — | ArrayLoad opcode (HashMap dispatch) |

## Attribute System

| Attribute | Effect |
|-----------|--------|
| `@syscall("name"/"num")` | Body → `Syscall+Ret`. Implicitly unsafe. |
| `@api("Symbol")` | Body → `CallExt+Ret`. Win64 on Windows, SysV elsewhere. Implicitly unsafe. |
| `@cfg(key="val")` | Conditional compile. Keys: `target_os`, `target_arch`, `target_abi`. Evaluated at declare/typecheck/unused passes. |
| `@inline` | Force inline eligibility (still excluded if recursive). |
| `@ignore` / `@ignore(unused_vars)` / `@ignore(dead_code)` | Suppress W01/W02/W03/W07. |
| `@intrinsic("void.X")` | Safe stdlib wrapper; dispatched by encoder case number. |

`StmtKind::CfgBlock { condition, body }` — statement-level `@cfg`. Condition evaluated; body compiled only if matching.

## Closures / First-Class Functions

- Type: `TypeKind::Fn { params, return_ty }`. Syntax: `fn(T, U) V`.
- Closure: `|params| expr` → `ExprKind::Closure`. No-capture: `MovConst+FnAddr`. With captures: env struct (`fn_ptr + captured_vals`), hidden r0 env ptr on call.
- Fn-name as value: `MovConst(ConstPoolEntry::FnAddr(name))` → `lea rax,[rip+fn_sym]` + `Pc32` reloc.
- Variable callee: `CallArg*+CallReg`. No reloc needed.
- `ConstPoolEntry::FnAddr` tag = `3`, serialized as u16 len + name bytes.

## Roadmap

### Main Goal: Optimisation
Fast binaries, small output, zero runtime waste. Every language and toolchain decision should serve this goal.

| Area | Target |
|------|--------|
| Codegen quality | Dead reg elimination done; linear scan done; better const folding, strength reduction pending |
| Inline expansion | Threshold-based auto-inline (currently `@inline` only) |
| AOT `@cfg` stripping | Remove disabled branches before codegen, not just semantic |
| Struct monomorphization | Done — static namespace calls infer type args via `infer_type_subst`; instance method mono via `MonomorphizationInfo` |
| `void link` built-in linker | Eliminate external linker dep; goal ELF <500B, PE <700B |
| Register allocation | Done — DRE + linear scan slot sharing in `src/bytecode/regalloc.rs` |

### Language
| Feature | Status |
|---------|--------|
| Primitives, arithmetic, control flow | Done |
| Structs, enums, match | Done |
| Functions, closures, fn pointers | Done |
| Generics (fn + struct monomorphization) | Partial — static namespace constructor calls now infer concrete type args; instance method mono done; struct-level layout mono not needed (uniform 8B fields) |
| Traits + impl | Partial — static dispatch done; vtable construction + fat ptr not yet |
| `?` operator | Done |
| Unsafe + raw pointers (`*T↔*U`, int↔`*T`) | Done |
| `@cfg` conditional compilation | Partial — evaluated at semantic; AOT stripping not yet |
| Type aliases | Done |
| Borrow checker (move semantics) | Partial — use-after-move, move-in-loop, reassign-suppression done; lifetimes not yet |
| Format specs (`{:.2}`, `{:#x}`, etc.) | Not started |
| Pattern matching (nested, tuple, guard) | Not started |
| `async`/`await` | Not started |

### Toolchain
| Component | Status |
|-----------|--------|
| `void build/run/check/fmt/new/init` | Done |
| `void lsp` | Partial |
| `void link` (built-in linker) | Not started — goal: ELF <500B, PE <700B |
| JIT VM | Deferred |
| `void test` / `void doc` | Not started |

### Philosophy
No LLVM, no GCC, no libc. `@intrinsic` → raw syscalls (Linux) or Win32 (Windows). VBC = stable portable IR. `void link` + JIT are planned first-class ecosystem components.
