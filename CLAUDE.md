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

### Loader (`src/loader.rs`)
- `load_programs` — resolves imports recursively, merges dependency-first, parses as one `Program`.
- Std resolution: `VOID_STD_ROOT` → `~/.void/std` / `%USERPROFILE%/.void/std` → `CARGO_MANIFEST_DIR/std` → `cwd/std`.
- `foo/mod.void` = opaque module directory; `pub import` controls what's exported.
- Deduplicates via canonical-path `HashSet`. Circular imports safe.

### Project (`src/project.rs`)
- `void.toml`: `[package]`, `[build]`, `[dependencies]` (path + optional version). `void.lock` validated on build.
- `type = "lib"` → lib project; default entry `src/lib.void`; default output `.vbc`.

### Lexer (`src/lexer/`)
- `&&`/`||` synthesized by parser via `match_and_and()`/`match_or_or()`.
- Generics use `[T]` square brackets.
- `TokenKind::While` exists but is **not handled** — `for (cond) {}` is the only while-like loop.

### Parser (`src/parser/`)
- `ast.rs`: all nodes are `Spanned<T>`. Two `Span` types; `to_ast_span` converts.
- Expression precedence: assign → logical-or → logical-and → equality → comparison → term → factor → unary → postfix → primary.
- Error codes: E00 generic, E01 expected ident, E02 expected token, E03 bad item position, E04 EOF in block, E05 expected type.
- Import: `import std.io.stdout;` / `import a.b.{x,y};` / `import a.b as c;` / `import a.b.*;`
- Closure: `|params| expr` — `Pipe` in primary position. Params are bare idents (no type).
- Fn pointer type: `fn(T, U) V` — greedy return type via `peek_is_type_start()`.
- Variadics: `...args: T` in param list; inside fn body `args` is `Slice[T]` with `.len()`.
- Pattern matching: wildcard, bind, literal, variant, **guards** (`pat if expr =>`).

### Semantic (`src/semantic/`)
Five sequential passes:
1. **Declare** — register fns, structs, traits, enums, imports. `@cfg`-disabled items skipped.
2. **Typecheck** — scope, inference, compatibility, init checks, expression annotations.
3. **Unused** — W01 unused var, W02 unused param, W03 unused fn/import.
4. **Dead code** — reachability, W04 unreachable after return. (Merged into `unused.rs`.)
5. **Optimize** — inline candidates, const folding, math identity reduction, lazy import hints, exhaustiveness checking.

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
**`@cfg` evaluation**: `target_os`, `target_arch`, `target_abi` evaluated against `std::env::consts`. Applied in: declare pass, typecheck CfgBlock, unused CfgBlock.

**Borrow checker** (`borrow.rs`): move semantics for all non-primitive, non-reference types (structs, enums, arrays, slices, dyn Trait, etc.). S10 = use-after-move / move-in-loop. `reassign_targets` set suppresses move-in-loop for `x = x.method()` patterns (value immediately re-owned). `for x : iterable` **moves** the iterable (like Rust's `for x in collection`); borrow with `for x : &collection`. The iterable is consumed before the loop body so it does not trigger move-in-loop. Method receivers are borrowed (non-consuming). No explicit reference lifetimes yet.
**Generic receiver methods**: for `Named` receivers with concrete type args, substitute receiver generics into method params before checking args (`Array[i32].push("x")` must error because `T = i32`).

**`pub` visibility:**
- **Functions**: enforced. Private fn imported cross-module emits S04 error.
- **Structs, traits, enums, type aliases**: parsed but hardcoded `public: false`; not enforced.
- **Re-exports**: `pub_import` stored but not read.

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
- **Struct mono MethodCall**: receiver type `Named { type_args }` → mangle call target; falls back to unmangled if mangled not in `fn_index`.
- **Intrinsic dedicated-opcode params**: `compile_intrinsic_fn` for `ArrayStore`/`ArrayLoad` (and future dedicated-opcode intrinsics) must emit `rrr(op, param_reg, ...)` matching the callee param registers, not hardcoded `rrr(op, 0, 0, 0)`. The inline pass remaps registers via `base + r`, so hardcoded zeros become `base+0` for all operands — wrong.
- **str_variadic module call**: `ExprKind::Field` path (e.g. `io.println("{}", x)`) must pack coerced args into `(ptr, len)` before calling `format`, just like the `ExprKind::Ident` path does. Passing raw coerced args causes `format` to interpret the first coerced string pointer as the variadic slice pointer, leading to an invalid dereference.
- Intrinsic dispatch: `INTRINSIC_MAP` HashMap; array ops via `INTRINSIC_OPCODE_MAP` HashMap.
- **Pattern matching**: `PatternKind` = `Wildcard | Bind(String) | Literal(LiteralValue) | Variant { enum_name, variant, sub_patterns }`. `compile_pattern_match` recurses into sub_patterns. String literal match: `StrLen` fast-path + byte loop via intrinsic ID 23 (consecutive regs required: r_a, r_a+1).
- **Format specs**: `extract_format_specs(template)` parses `{:spec}` at compile time → `Vec<String>`. `strip_format_specs` rewrites `{:spec}` → `{}` for runtime `format()`. `coerce_with_spec(reg, spec, span)` applies int/float specs → extended `PrimToStr` tags. str_variadic fns use spec-aware coercion per arg slot.
- **`dyn Trait`**: `coerce_to_dyn(obj_reg, type_name, trait_name)` allocates 16-byte fat pointer — `fat[0]=concrete_ptr`, `fat[8]=vtable_ptr` (via `MovConst(VtableAddr)`). MethodCall on `Dyn` receiver: `FieldLoad fat[8]→vtbl_ptr`, `VtblLoad vtbl_ptr[slot]→fn_ptr`, `FieldLoad fat[0]→data_ptr`, `CallArg*+CallReg`.
- **Non-slice iterator** (`ForLoop::Each` on non-slice): `Array[T]` is special-cased to compile as an index loop (`len()` + `get()`). The typechecker records `Array.len`/`Array.get` monomorphizations so the specialized chunks exist in `fn_index`; codegen falls back to the unmangled name if the mangled one is missing. `for i : &arr` strips the `Ref` and iterates the inner value directly. Other `Named` types use `has_next()` / `next()` protocol with `Option[T]` unwrapping. `Dyn` trait objects use vtable dispatch.
- **For-loop move semantics**: `for x : collection` moves/consumes the collection. Codegen calls `mark_consumed_expr` on the iterable and registers a hidden `__for_iter` drop-local so the value is cleaned up when the enclosing scope exits (early returns included). `for x : &collection` borrows and leaves the original variable untouched.
- **Index assignment**: `arr[i] = val` is supported for `Array[T]` (dispatches to `Array.set`), slices (direct stack store), and fixed-size arrays (register move for literal index, computed address store for dynamic index).
- **Local RAII cleanup**: codegen tracks local variables whose resolved `Named` type has a `free(self)` method and emits destructor calls at block exits and before returns. Destructor roots are forced reachable so tree-shaking keeps `Type.free` and its dependencies. Values moved into returns/call args are deactivated to avoid double-free; method receivers remain non-consuming to match existing `self: Array[T]` APIs. `Array[T]` and `String` locals are auto-cleaned at scope exit. This is source-level cleanup, not `Drop` bytecode yet.

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
├── linker.rs      # find + exec external linker
└── x86_64/
    ├── encoder.rs # FnEncoder: Chunk → (bytes, PendingReloc[])
    ├── sections.rs / symbols.rs / relocations.rs / start.rs
```

**Calling conventions**: SysV (Linux/macOS): `rdi,rsi,rdx,rcx,r8,r9`→`rax`. Win64: `rcx,rdx,r8,r9`→`rax`; args 5-6 at `[rsp+32/40]`.  
**VBC reg N** → `[rbp-(N+1)*8]`. Frame: `round_to_16(regs*8)` SysV, `round_to_16(regs*8+48)` Win64.  
**Relocs**: `Plt32` (calls), `Pc32` (RIP-relative data). Addend always `-4`.  
**Encoder**: emits dummy `call fn_start` / `lea rax,[fn_start]`, records pending relocs, zeros displacement bytes after assembly.

**Entry stubs** (no CRT needed):
- Linux `_start`: `xor rbp,rbp` → zero sigaction struct → register handlers for `SIGSEGV`, `SIGABRT`, `SIGFPE`, `SIGBUS` → `call main` → `mov rdi,rax; mov rax,60; syscall`
- Windows `mainCRTStartup`: `sub rsp,40` → `AddVectoredExceptionHandler` → `call main` → `ExitProcess`

**Crash handler** (Linux & Windows): identical output format. Prints `== CRASHED ==\n`, then `fatal: signal 0x<hex>` / `fatal: exception 0x<hex>`, `fault: 0x<addr>`, `rip: 0x<rip>`. Checks `__void_trace_enabled`; if set, calls `__void_print_backtrace` (RBP chain walk, up to 16 frames). If not set, prints `use VOID_TRACE=1 to see full stack trace\n`. Both full and minimal Linux stubs, as well as both Windows stubs, are generated via `iced-x86` `CodeAssembler` — raw-byte stubs with hand-computed relocations were removed to eliminate relocation-offset bugs.

**`@no_crash`**: File-level directive (like `@no_std`). When present, the entry stub omits crash-handler registration (Linux `sigaction`, Windows `AddVectoredExceptionHandler`). `__void_print_backtrace` is still emitted so panic backtraces work. Produces a smaller binary (~1 KB smaller).

**Panic handler**: `PanicInfo` carries `message`, `file`, `line`. `__void_panic_handler` prints all three, then calls `__void_print_backtrace()` (intrinsic ID 25) before exiting 101. `panic("msg")` call sites get `file`/`line` injected by codegen. `prelude/array.void` uses `panic("index out of bounds")` for `Array.get` / `Array.set` / `Index` OOB checks.

**`VOID_TRACE=1` detection**: Windows `mainCRTStartup` uses `GetEnvironmentVariableA`; Linux `_start` calls `getenv` (libc is already linked, the dynamic linker initialises `environ` before entry). Both set the `__void_trace_enabled` byte in `.data`.

**Encoder silent fallback** (`encoder.rs:1607–1612`): unimplemented opcodes (`NewObj`, `Move`, `Drop`, `Dup`, `Spawn`, `AtomicAdd`, `AtomicCas`, `StrConcat`) emit `xor rax,rax; mov slot(0), rax` — produces wrong code instead of crashing.

**Target support**:
| OS | Format | ABI | Status |
|----|--------|-----|--------|
| Linux x86-64 | ELF64 | SysV | Full |
| Windows x86-64 | PE/COFF | Win64 | Full (needs `lld-link` + `LIB`) |
| macOS x86-64 | ~~Mach-O~~ ELF | SysV | Broken — `select_backend()` maps macOS to `ElfBackend`, `emit_start: false`, no Mach-O relocations |

## Unsafe System

- `*T` in fn signature → must be `unsafe fn` (S12). Exception: `@syscall`/`@api` implicitly unsafe.
- Calling `unsafe fn` or dereferencing `*T` outside unsafe context → S11.
- `@intrinsic` = safe (unsafety handled internally).
- `*T` ↔ `*U`: all raw pointers mutually compatible. Integer `0` valid as any `*T` (null pointer constant).

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

## String Model

- `str` / `&str` — interchangeable. Immutable, valid UTF-8, fat pointer internally.
- `String` — owned heap string (`ptr+len+cap`). Local variables auto-clean via `String.free`; empty strings (`cap == 0`) do not free static storage.
- `Rune = u32` — Unicode codepoint. `RuneIterator` iterates UTF-8 codepoints.

Key API: `s.len()→StrLen`, `s.to_string()→PrimToStr`, `s.as_str()→StrAsStr`, `s.parse[i32]()→StrToInt`, `s.parse[f64]()→StrToFloat`.

## Standard Library (`std/src/`)

Source-based `.void` files, merged at compile time.

| Module | Status | Notes |
|--------|--------|-------|
| `core` | Done | write, read, exit, malloc/free/realloc, memcpy/set/move/cmp, strlen, str_concat, int_to_str, float_to_str, str_byte_at, str_from_byte |
| `io` | Done | println, print, eprintln, eprint, read_line — str_variadic (auto-coerces args) |
| `fmt` | Done | `format(template, ...args: str) str` — pure void, `{}` placeholders, byte-by-byte parsing; spec-aware coercion (`{:x}`, `{:o}`, `{:.Nf}`) |
| `string` | Done | `String`: new, push, push_str, len, as_str, free |
| `panic` | Done | PanicInfo {message,file,line}, __void_panic_handler, panic. Codegen injects hidden `file`/`line` args at call sites. Typechecker special-cases `panic` to accept 1+ user args. |
| `result` | Done | ok/is_ok/is_err/unwrap/unwrap_err/unwrap_or; `?` operator |
| `option` | Done | is_some/is_none/unwrap/unwrap_or; `?` operator |
| `box` | Done | `Box[T]`: new, get, set, free |
| `traits` | Done | Display, Debug, Clone, Copy, Drop, Iterator, Eq, Ord, Hash, Default, Into, From, Index, Write, Add/Sub/Mul/Div/Rem/Neg/BitAnd/BitOr/BitXor/Shl/Shr |
| `prelude/mod.void` | Done | re-exports String, Box, Array, option, result, traits, fmt, panic. Prelude is **always** auto-injected; `@no_std` only disables `std`. |
| `collections/array` | Done | `Array[T]`: push, get, set, len, free, from, Index impl. Index assignment `arr[i] = val` supported. Locals auto-clean at scope exit. |
| `collections/map` | Done | open-addressing hash map with tombstones |
| `collections/set` | Done | open-addressing hash set |
| `unix` | Done | raw syscall wrappers |
| `windows` | Done | Win32 API wrappers |
| `fs` | Done | File open/create/read/write/close/seek/sync/truncate/exists/remove/mkdir/rmdir/rename/chmod |
| `net` | Done | TcpListener, TcpStream, UdpSocket. 2 intrinsics: bind_tcp (20), connect_tcp (21) |
| `os` | Done | exit, sleep, yield_cpu, getpid/ppid, getenv, cwd, kill, umask |
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
| `void.print_backtrace` | 25 | `call __void_print_backtrace` — walks RBP chain (max 16 frames) when `__void_trace_enabled` is set |
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
| `@derive(Trait, ...)` | Register derived traits for struct. |
| `@panic_handler` | Validate signature; mark as panic handler. |
| `@no_crash` | File-level: disable crash handler in entry stub (smaller binary, no signal/exception catching). |

`StmtKind::CfgBlock { condition, body }` — statement-level `@cfg`. Condition evaluated; body compiled only if matching.

## Closures / First-Class Functions

- Type: `TypeKind::Fn { params, return_ty }`. Syntax: `fn(T, U) V`.
- Closure: `|params| expr` → `ExprKind::Closure`. No-capture: `MovConst+FnAddr`. With captures: env struct (`fn_ptr + captured_vals`), hidden r0 env ptr on call.
- Fn-name as value: `MovConst(ConstPoolEntry::FnAddr(name))` → `lea rax,[rip+fn_sym]` + `Pc32` reloc.
- Variable callee: `CallArg*+CallReg`. No reloc needed.
- `ConstPoolEntry::FnAddr` tag = `3`, serialized as u16 len + name bytes.

## LSP Status

Basic server is running. Capabilities:
- ✅ Diagnostics (publish on open/change/save)
- ✅ Hover (type + const value, fallback to symbol table)
- ✅ Goto Definition (naïve: searches symbol table by name, no scoping)
- ✅ Completion (trigger on `.` — **only** for `std.*` chains via filesystem scanning)
- ✅ Document formatting

Missing:
- ❌ General identifier completion (non-std)
- ❌ Scoped/resolving goto-definition
- ❌ Find references
- ❌ Rename symbol
- ❌ Code actions / quick fixes
- ❌ Inlay hints
- ❌ Workspace symbols

## Roadmap

### Philosophy
Fast binaries, small output, zero runtime waste. No LLVM, no GCC, no libc. `@intrinsic` → raw syscalls (Linux) or Win32 (Windows). VBC = stable portable IR.

---

### P0 — Critical Bugs / Safety

| Item | Problem |
|------|---------|
| ~~Enhanced Crash/Panic handler~~ | ✅ Done. Linux: signal info + hex formatting + RBP backtrace when `VOID_TRACE=1`. Panic: file/line injected at call sites + backtrace via intrinsic 25. |
| ~~Fix encoder silent fallback~~ | ✅ Done. Unimplemented opcodes (`NewObj`, `Move`, `Drop`, `Dup`, `Spawn`, `AtomicAdd`, `AtomicCas`, `StrConcat`) now `panic!("encoder: unimplemented opcode {:?}", ...)` instead of emitting bogus `xor rax,rax; mov slot(0),rax`. |
| ~~Fix non-slice iterator codegen~~ | ✅ Done. `ForLoop::Each` on non-slice collections now emits proper `has_next()` → `next()` → `Option` unwrap loop. Supports both `Named` static dispatch and `Dyn` vtable dispatch. |
| ~~For-loop move semantics~~ | ✅ Done. `for x : collection` now moves the iterable (Rust semantics). Borrow with `for x : &collection`. Borrow checker + codegen + drop cleanup all wired. |
| ~~Index assignment~~ | ✅ Done. `arr[i] = val` works for `Array[T]`, slices, and fixed-size arrays. |
| ~~Human-readable move errors~~ | ✅ Done. S10 use-after-move errors show `file:line:col` for the move site instead of raw merged-source coordinates. |

### P1 — High Impact

| Item | Target |
|------|--------|
| **Bitwise operators** | `&` `|` `^` `<<` `>>` are not parsed as binary ops. Encoder has `And`/`Or`/`Xor`/`Shl`/`Shr`/`Sar`; traits `BitAnd`/`BitOr`/`BitXor`/`Shl`/`Shr` exist in stdlib; codegen silently falls back to `Add` for unknown ops. Need: lexer tokens (`Caret`, `Shl`, `Shr`), parser `BinOpKind` variants + precedence, semantic typecheck (integers), codegen opcode mapping. Keep `&` prefix = ref, `|` primary = closure. |
| **AOT `@cfg` stripping** | Prune dead `CfgBlock` AST nodes before codegen, not just skip them in semantic. Reduces binary bloat. |
| **`void link` built-in linker** | Triggered when `void build` receives `.o` input (e.g. `void build myprog.o`). Eliminates external linker dependency. Goal: ELF <500B, PE <700B. |
| **`void test` runner** | `@test` attribute + `void test` CLI command. Essential for ecosystem maturity. |
| **`pub` on types** | Enforce `public` field for structs, traits, enums, type aliases during import resolution (same S04 pattern as functions). |

### P2 — Codegen Quality

| Item | Target |
|------|--------|
| **Threshold-based auto-inline** | Heuristic inline (size < N instrs, no recursion) beyond current `@inline` only. |
| **Cross-basic-block const folding** | Currently local expression only. |
| **Strength reduction** | `x * 2` → `x << 1`, `x / pow2` → shift, etc. |

### P3 — Platform / Runtime

| Item | Target |
|------|--------|
| **macOS Mach-O backend** | Actually implement Mach-O object output, `__start` stub, Mach-O relocations. Currently broken (emits ELF). |
| **Ownership / RAII bytecode** | Partially done via source-level codegen cleanup for locals with `free(self)`. Still need real `Move`/`Drop`/`Dup` opcodes, `Drop` trait dispatch, parameter/field/element recursive drops, and reference lifetimes. |
| **Atomic opcodes in encoder** | Implement `AtomicAdd`, `AtomicCas` lowering. |

### P4 — Language Features

| Feature | Status |
|---------|--------|
| Format string literals | Not started — `"value = {x}"` compile-time interpolation |
| Nested struct patterns | Not started — `Point { x, y }` in match arms |
| `async`/`await` | Not started — defer until lifetimes done |

### P5 — Tooling

| Component | Status |
|-----------|--------|
| LSP improvements | Partial — see LSP Status section above |
| `void doc` | Not started |
| JIT VM | Deferred |

---

**Test coverage**: 135 inline `#[cfg(test)]` unit tests. No integration test suite or `tests/` directory yet.
