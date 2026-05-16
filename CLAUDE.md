<<<<<<< Updated upstream
# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build             # debug build
cargo build --release   # release build
cargo test              # all tests
cargo test <name>       # single test, e.g. cargo test parses_enum_and_match_expression
cargo clippy            # lint
cargo fmt               # format
```

CLI (one dependency: `clap 4.6`):

```bash
void build <file> [files...]        # compile files to native binary (default)
void build <file> -b                # emit .vbc bytecode
void build <file> -c                # emit relocatable .o object file (no link)
void build <file> [-b|-c] -o out    # specify output filename
void build <file> -r                # compile and run
void build                          # build project from void.toml (no files given)
void build [-b|-c] [-o out]         # build project with options
void build -r                       # build + run project from void.toml
void build --linker /path/to/ld     # override linker binary
void run                            # build + run project from void.toml
void run --linker /path/to/ld       # build + run with explicit linker
void check                          # analyze project without emitting
void debug [-b]                     # compile hardcoded demo source
void new <name>                     # create a new project
void fmt                            # trim trailing whitespace in .void files
void clean                          # remove build artifacts
```

Default output names: `<stem>.vbc` (bytecode), `<stem>.o` (object), `<stem>` / `<stem>.exe` (binary).

**Linker selection**: binary emit requires an external linker. Search order: `VOID_LINKER` env var → `ld.lld` → `mold` → `ld`. Override with `--linker /path/to/linker` or `export VOID_LINKER=/path/to/linker`.

Rust edition 2024.

## Architecture

Compiler frontend pipeline:
```
source files → Loader → merged source → Lexer → Vec<Token> → Parser → Program
             → Analyzer → SemanticReport → Codegen → Vec<Chunk>
             → ElfBackend (object + iced-x86) → .o bytes
             → LinkerInvocation (ld/lld/mold) → binary
```

VBC (`-b`) output: Codegen → `Vec<Chunk>` serialized directly, no backend involved.  
Object (`-c`) output: same as binary but stop after `.o`; no linker step.  
The GCC/GAS toolchain is no longer required or used.

### Loader (`src/loader.rs`)
- `load_programs(entries: &[PathBuf]) -> Result<LoadResult>` — resolves local imports recursively, merges sources in dependency-first order, parses the merged string as one `Program`.
- `load_programs_with_resolver(entries, resolver)` — like `load_programs`, but resolves module imports via `ModuleResolver` when a dependency name matches the first import segment.
- **Local import detection**: `import foo.bar` resolves to a module when `foo` exists in `ModuleResolver`; otherwise local if `foo.void` exists next to the importing file. Otherwise treated as stdlib/external (no file loaded).
- Deduplicates via canonical-path `HashSet` — circular or repeated imports are safe.
- Merged source byte offsets are contiguous, so `render_diagnostic` works correctly across file boundaries.
- Declare pass in semantic: when an import name collides with an already-declared function (from a loaded local file), the import binding is silently skipped — no duplicate-declaration error.

### Project Config (`src/project.rs`)
- `ProjectContext::load(start)` finds the nearest `void.toml`, loads project metadata, and builds a `ModuleResolver`.
- `ProjectContext::discover(start)` is used by `void compile` to apply module resolution when a project config exists.
- `void.toml` supports `[package]`, `[build]`, and `[dependencies]` (path + optional version).
- `void.lock` is created on build/run when dependencies exist, and validated if present.

### Lexer (`src/lexer/`)
- `token.rs` — `Token`, `TokenKind`, `Span` (line, col, byte start/end).
- `mod.rs` — `Lexer`: character-by-character, emits `TokenKind::Error` on unknown chars rather than panicking.
- `&&` and `||` are **not** dedicated tokens — the lexer only has `Ampersand` and `Pipe`. The parser synthesizes `&&`/`||` via `match_and_and()` / `match_or_or()` in `common.rs`.
- Generic type arguments use `[T]` **square brackets**, not angle brackets.
- `pub` is a recognized keyword (`TokenKind::Pub`) but has no effect — silently consumed as an optional visibility modifier before any item.

### Parser (`src/parser/`)
- `ast.rs` — all node types. Every node is `Spanned<T>` (a struct with `node: T` and `span: Span`). Two `Span` types exist: `lexer::token::Span` and `parser::ast::Span` (same fields, different type). `to_ast_span` in `common.rs` converts between them.
- `mod.rs` — `Parser` struct + statement and expression parsing. Expressions use precedence climbing: assignment → logical-or → logical-and → equality → comparison → term → factor → unary → postfix (call/field/method) → primary.
- `items.rs` — top-level item parsers: `fn`, `struct`, `trait`, `enum`, `impl`, `import`.
- `common.rs` — parser utilities (`expect`, `advance`, checkpoint/restore for backtracking), `render_diagnostic` (formats error with source snippet + caret underline), synchronize methods for error recovery.

**Error codes**: E00 (generic), E01 (expected identifier), E02 (expected token), E03 (unexpected item position), E04 (unexpected EOF in block), E05 (expected type).

**Import syntax**: `import std.io.stdout;` / `import a.b.{x, y};` / `import a.b as c;` / `import a.b.*;`

**Trait impl syntax**: `impl TraitName[T] for StructName[T] { ... }` — the `for` keyword is consumed by `parse_impl`, not a reserved `TokenKind`.

### Semantic Analysis (`src/semantic/`)
Split across files. `Analyzer` runs five sequential passes over the `Program` AST:

1. **Declare** (`declare.rs`) — register top-level functions, structs, traits, enums, imports into global scope.
2. **Type-check** (`typecheck.rs`) — scope tracking, type inference, type compatibility, initialization checks, expression annotations.
3. **Unused** (`unused.rs`) — warn on unused variables (W01), parameters (W02), functions (W03), imports (W03). Supports `@ignore` attribute.
4. **Dead code** (`unused.rs`) — reachability analysis, warn on statements after guaranteed returns (W04).
5. **Optimization** (`optimize.rs`) — inline candidates (small non-branching bodies, non-recursive, hot-path calls or `@inline`), match exhaustiveness, removable imports, **constant folding** (both-sides-known → `ConstValue`), **math identity/absorber reduction** (`x*0=0`, `x+0=x`, `x&&false=false`, etc. — result stored in `ExprAnnotation.const_value` in annotated tree), **lazy import hints** (field-chain tracking: `import std;` + `std.io.stdout.println(...)` → suggests `import std.io.stdout;`).

**Warning suppression**: `@ignore` attribute silences warnings for items/statements. Forms:
- `@ignore` — silence all warnings for that item.
- `@ignore(unused_vars)` — silence W01/W02 (unused variable/parameter).
- `@ignore(dead_code)` — silence W03/W07 (unused/dead function).

`types_compatible` treats `Any` as compatible with everything and `Named` types as compatible with everything (generics are not yet resolved).

`main` is exempt from unused-function and inline-candidate checks.

Public types live in `types.rs`, re-exported from `mod.rs`.

### `SemanticReport`
Structured output with: `errors`, `warnings` (each warning includes a `suggestions: Vec<String>` field that merges related suggestions), `suggestions` (standalone list, deprecated in favor of per-warning suggestions), `symbol_table`, `dependency_graph`, `optimization_hints` (includes `math_optimizations`, `lazy_import_hints`), `annotated_exprs`, `constant_evaluations`, `used_imports_map`, `non_exhaustive_matches`, `lazy_import_hints`, **`inline_candidates`** (consumed by `Codegen` for the inline expansion pass).

### Bytecode (`src/bytecode/`)
VBC (Void Bytecode) — platform-independent, AOT-only. **6 bytes per instruction.**

```
[byte 0]     opcode (u8, up to 256 opcodes)
[bytes 1–4]  operands (32 bits, layout varies by opcode group)
[byte 5]     flags / reserved
```

Operand layouts (32-bit field):
- **RRR** — `ops[0]`=dst, `ops[1]`=src1, `ops[2]`=src2
- **RI16** — `ops[0]`=dst, `ops[1..2]`=imm16 (LE)
- **MEM** — `ops[0]`=value\_reg, `ops[1]`=base\_reg, `ops[2..3]`=offset16 (LE, signed)

Opcode groups:
- `0x00–0x0F` — data movement (`Nop`, `Mov`, `MovI`, `MovConst`)
- `0x10–0x1F` — arithmetic/logic (`Add`–`Sar`)
- `0x20–0x2F` — memory & ownership (`Load`, `Store`, `Lea`, `Move`, `Drop`, `Dup`)
- `0x30–0x3F` — control flow (`Cmp`, `Jmp`, `Je`–`Jnz`, `CallIdx`, `CallReg`, `Ret`)
- `0x40–0x4F` — structs/objects (`New`, `NewObj`, `FieldLoad`, `FieldStore`, `VtblLoad`)
- `0x50–0x5F` — atomics, threading & foreign calls (`AtomicAdd`, `AtomicCas`, `MemFence`, `Spawn`, `CallExt=0x5D`, `Syscall=0x5E`)
- `0x60–0x6F` — string ops (`StrLen=0x60`, `StrConcat=0x61`, `StrToInt=0x62`, `StrToFloat=0x63`, `PrimToStr=0x64`, `StrAsStr=0x65`)
- `0x70–0x7F` — reserved

Key design decisions:
- `Move` (ownership transfer, src invalidated) vs `Dup` (copy, Copy types only) — compiler chooses at codegen, never both for same value.
- `Drop` = RAII destructor call emitted by compiler at scope exit, no runtime GC.
- No null — `Option[T]` is discriminant + value, `match` compiles to `Cmp` + conditional jump.
- `Chunk` = one function's code (`Vec<Instruction>`) + constant pool (`Vec<ConstPoolEntry>`) + `name` + `param_count` + `reg_count`. `patch_jump` backpatches forward jumps after target is known.
- Flat serialization: `Chunk::to_bytes()` → `Vec<u8>` at 6 bytes/instruction.
- Register allocation and platform lowering happen in AOT backend, not in VBC.

**VBC file format** (`serialize_vbc`):
```
magic:       \x00VBC  (4 bytes)
version:     0x01     (u8)
chunk_count: u32 LE
per chunk:
  name_len:  u16 LE + name bytes
  const_count: u16 LE
  per constant: tag(u8) + value (8 bytes Int/Float, u16 len + bytes for Str)
  instr_count: u32 LE
  instructions: instr_count * 6 bytes
```

### Codegen (`src/bytecode/codegen.rs`)
`Codegen` takes a `&SemanticReport` and compiles a `Program` to `Vec<Chunk>`.

- **Pass 1**: assign each `fn` item a function-table index (order of appearance).
- **Pass 2**: compile each function body via `FnCompiler` (virtual register allocator).
- **Post-pass**: inline expansion — replaces `CallArg* + CallIdx` sequences for functions in `inline_candidates` with a register-remapped copy of the callee body (Ret stripped, args copied to callee param regs via base offset). Vtable calls (`CallReg`) are not inlined.
- `@syscall` functions: emit single `Syscall` (num from Linux x86-64 table or raw number) + `Ret`, skip body.
- `@api` functions: emit single `CallExt` (symbol from attribute string in the constant pool) + `Ret`, flags store arg count.
- Const-fold: expressions with a `ConstValue` in `const_map` (from semantic) emit `MovI`/`MovConst` directly.
- `&&` / `||`: short-circuit via `Jz`/`Jnz` — right side only evaluated if needed.
- Return value convention: always in virtual register 0 (`r0`).
- Known method call builtins: `len()→StrLen`, `to_string()→PrimToStr`, `as_str()/as_string()→StrAsStr`, `parse[T]()→StrToInt` or `StrToFloat` depending on type arg.
- `FnCompiler.next_reg` monotonically allocates virtual registers; stored in `Chunk.reg_count` for use by inline expansion and the backend.

### Backend (`src/backend/`)

Replaces the old `src/aot/` (deleted). Emits native ELF `.o` object files directly using the `object` crate (0.36) for ELF construction and `iced-x86` (1.x, `code_asm` feature) for x86-64 instruction encoding. No GCC/GAS involved.

```
src/backend/
├── mod.rs             # Backend trait, ObjectOutput, select_backend()
├── target.rs          # TargetSpec { arch, os, abi, emit_start }
├── linker.rs          # LinkerInvocation: find ld/lld/mold, exec
└── x86_64/
    ├── mod.rs         # ElfBackend: impl Backend, orchestrates sub-modules
    ├── encoder.rs     # FnEncoder: one Chunk → (machine-code bytes, PendingReloc[])
    ├── sections.rs    # SectionAccumulator: .text/.rodata/.data management
    ├── symbols.rs     # SymbolTable: defined + UNDEF symbols
    ├── relocations.rs # PendingReloc, RelocKind, write_reloc()
    └── start.rs       # _start stub (Linux only, raw bytes)
```

**`Backend` trait** (`src/backend/mod.rs`):
```rust
pub trait Backend {
    fn compile(&self, chunks: &[Chunk], target: &TargetSpec) -> Result<ObjectOutput, BackendError>;
}
pub fn select_backend(target: &TargetSpec) -> Box<dyn Backend>
// Linux/MacOs → ElfBackend; Windows → panic (CoffBackend: future work)
```

**`TargetSpec`** (`src/backend/target.rs`):
```rust
pub enum Arch  { X86_64 }
pub enum Os    { Linux, Windows, MacOs }
pub enum Abi   { SysV, Win64 }
pub struct TargetSpec { pub arch, pub os, pub abi, pub emit_start: bool }
impl TargetSpec {
    pub fn host() -> Self              // derive from cfg!(target_os/target_arch)
    pub fn binary_format() -> BinaryFormat   // Elf / Coff / MachO
    pub fn object_architecture() -> Architecture
    pub fn dynamic_linker() -> Option<&'static str>  // "/lib64/ld-linux-x86-64.so.2" on Linux
    pub fn without_start(self) -> Self // for -c (object-only) builds
}
```

**Calling conventions**:
- **SysV AMD64** (Linux/macOS): args in `rdi, rsi, rdx, rcx, r8, r9`; return in `rax`.
- **Win64** (Windows): args 1-4 in `rcx, rdx, r8, r9`; args 5-6 at `[rsp+32]`/`[rsp+40]`; return in `rax`. Float args in `xmm0`-`xmm3` by position.

**VBC register N** maps to stack slot `[rbp - (N+1)*8]` (`slot(N)` helper in encoder).  
**Frame**: `round_to_16(reg_count * 8)` bytes (SysV) or `round_to_16(reg_count * 8 + 48)` bytes (Win64 — 32 shadow + 16 stack-arg slots).  
**Prologue**: `push rbp; mov rbp,rsp; sub rsp,frame`; then load ABI arg regs into param slots. Win64 params 5-6 loaded from `[rbp+48]`/`[rbp+56]`.  
**`CallArg` + `CallIdx`**: pending arg list accumulated; on `CallIdx`, args moved to ABI regs (+ stack for Win64 args 5-6), `call target` with PLT32/REL32 reloc; result in `rax` moved to dst slot.  
**`Syscall`**: Linux x86-64 only — `rax`=syscall number, args in `rdi, rsi, rdx, r10, r8, r9`. On Win64, emits `xor rax,rax` (no-op) since raw syscalls are unsafe on Windows.  
**`CallExt`**: uses the target ABI (SysV on Linux/macOS, Win64 on Windows).  
**Unimplemented**: `VtblLoad`, `CallReg`, `New`, `NewObj`, `FieldLoad`, `FieldStore` emit `xor rax,rax` placeholder.

### VBC → Object Lowering Design

**`FnEncoder`** (`src/backend/x86_64/encoder.rs`) encodes one `Chunk` to `(Vec<u8>, Vec<PendingReloc>)`:

1. Create a `fn_start` label at byte 0 as a dummy target for all external references.
2. For every call or RIP-relative data load, emit `call fn_start` / `lea rax,[fn_start]` — the displacement will be wrong (relative to self) but the instruction shape and size are correct.
3. Record `(asm_instr_idx, disp_byte_offset_within_instr, kind, target_symbol, addend=-4)`.
4. `asm.assemble(fn_offset)` → raw bytes.
5. Decode bytes with `iced_x86::Decoder` to map `asm_instr_idx → byte_offset_in_text`.
6. Zero out the 4 displacement bytes at each recorded position.
7. Emit `PendingReloc { offset_in_text, kind, symbol, addend }` for each.

The linker fills in the real displacements at link time. No partial encodings or manual byte patching of instruction opcodes.

**Relocation offsets within instruction**:
- `call rel32` (E8 xx xx xx xx, 5 bytes) → disp at `instr_offset + 1`
- `lea rax,[rip+rel32]` (48 8D 05 xx xx xx xx, 7 bytes) → disp at `instr_offset + 3`

### Symbol and Relocation Model

**Relocation kinds** (`src/backend/x86_64/relocations.rs`):

| Kind | ELF type | Use |
|------|----------|-----|
| `Plt32` | `R_X86_64_PLT32` | Function calls (`CallIdx`, `CallExt`, `_start→main`) |
| `Pc32` | `R_X86_64_PC32` | RIP-relative data (`lea rax,[rip+str]`, `lea rax,[rip+buf]`) |

Addend is always `-4` (PC-relative displacement accounts for end-of-instruction IP).

**Symbol naming**:
- Functions: chunk name as-is (e.g. `main`, `add`).
- String constants: `__void_str_{chunk_name}_{const_pool_idx}` in `.rodata`.
- PrimToStr format string: `__void_fmt_ld` in `.rodata` (`"%ld\0"`), one per object.
- PrimToStr buffers: `__void_itoa_{chunk_name}_{instr_idx}` in `.data` (32 bytes each).
- Entry point: `_start` (emitted when `target.emit_start = true`; Linux binary builds only).

External symbols referenced but not defined (libc, etc.) become UNDEF entries and are resolved by the linker.

### Entry-Point Stubs (`src/backend/x86_64/start.rs`)

Emitted as raw bytes — no CRT object files needed. Selected by `target.os`; `-c` builds omit the stub entirely (`emit_start = false`).

**Linux `_start`** (`StartStub::generate`):
```
48 31 ED              xor rbp, rbp
E8 00 00 00 00        call main   ← PLT32 reloc → "main", addend=-4
48 89 C7              mov rdi, rax
48 C7 C0 3C 00 00 00  mov rax, 60
0F 05                 syscall
```
Symbol: `_start`, `SymbolScope::Linkage`.

**Windows `mainCRTStartup`** (`StartStub::generate_windows`):
```
48 83 EC 28           sub rsp, 40  (32-byte shadow + 8-byte alignment)
E8 00 00 00 00        call main   ← REL32 reloc → "main", addend=-4
48 89 C1              mov rcx, rax
E8 00 00 00 00        call ExitProcess ← REL32 reloc → "ExitProcess", addend=-4
```
Symbol: `mainCRTStartup`, `SymbolScope::Linkage`. Linked with `kernel32.lib`. macOS uses the system dyld entry; `-c` builds omit this stub.

### Linker (`src/backend/linker.rs`)

**Detection order**:
- Linux/macOS: `VOID_LINKER` env var → `ld.lld` → `mold` → `ld`
- Windows: `VOID_LINKER` env var → `lld-link` → `link`

**Override**: `--linker /path/to/linker` CLI flag or `export VOID_LINKER=/path/to/linker`.

Linux invocation:
```
<linker> -o <output> <obj.o> -lc -lm --dynamic-linker /lib64/ld-linux-x86-64.so.2
```
macOS invocation:
```
<linker> -o <output> <obj.o> -lc -lm
```
Windows invocation (requires `LIB` env var pointing to Windows SDK + MSVC lib dirs):
```
<linker> /out:<output.exe> <obj.obj> /subsystem:console /entry:mainCRTStartup kernel32.lib ucrt.lib
```
Extra flags from `void.toml [build].flags` are appended after.

`write_temp_object(bytes, stem)` writes object bytes to `$TMPDIR/void_{stem}_{pid}.o`; `remove_temp(path)` cleans up after linking.

### Target Support

| OS | Object format | Calling convention | Entry stub | Status |
|----|--------------|-------------------|------------|--------|
| Linux x86-64 | ELF64 | SysV AMD64 | `_start` | Supported |
| macOS x86-64 | Mach-O | SysV AMD64 | system dyld | Partial |
| Windows x86-64 | PE/COFF | Win64 | `mainCRTStartup` | Supported |

All targets share the same `ElfBackend` (the `object` crate writes the correct binary format based on `TargetSpec.binary_format()`). Future architectures implement `trait Backend` and are selected in `select_backend()`. VBC stays the same canonical IR regardless of target.

**Windows notes**: linking requires `lld-link` or `link.exe` with `LIB` pointing to Windows SDK + MSVC runtime directories. `@syscall` functions emit a no-op on Windows (raw syscalls are kernel-internal only). `@api("WinFn")` functions use Win64 ABI automatically.

## Language Syntax (current)

```
import std.io.stdout;
import mymodule.myFunc;  // auto-loads mymodule.void from same directory

pub fn name[T](param: Type) ReturnType {  // pub is optional, ignored
    const x: i32 = 1 + 2;
    var y: &str;
    y = "hello";           // string literals are &str
    x += 1; x -= 1; x *= 2; x /= 2; x %= 2;  // compound assign
    x++; x--; ++x; --x;                        // inc/dec (prefix and postfix)
    if (cond) { ... } else { ... }
    while (cond) { ... }
    for i : 0..10 { ... }          // range loop (i in [0, 10))
    for i : collection { ... }     // iterator loop
    for i, v : collection { ... }  // iterator loop with index+value binding
    var arr = [1, 2, 3];           // array literal
    arr[0];                        // index
    ret expr;
}

struct Foo[T] { field: T, const flag: bool, }
trait Bar[T] { fn method(x: T) T; }
impl Bar[i32] for Foo[i32] { fn method(x: i32) i32 { ret x; } }
enum Option[T] { Some(T), None, }

match value {
    Some(v) => v,
    Option.None => 0,
    _ => default,
}
```

Primitive types: `i8/i16/i32/i64`, `u8/u16/u32/u64`, `isize`, `usize`, `f16/f32/f64`, `bool`, `str`, `void`, `any`.

## String Model

- `&str` — canonical string view type. Immutable fat pointer (`ptr: *u8, len: usize`), valid UTF-8. **String literals have type `&str`**. Preferred form in user code.
- `str` — primitive keyword, alias for `&str`. Fully compatible in all type positions — `str` and `&str` are interchangeable. Kept for ergonomics (`fn foo(s: str)` and `fn foo(s: &str)` are equivalent).
- `String` — stdlib struct. Owned heap string: `ptr + len + cap`, UTF-8 valid. Allocated/freed by RAII (`Drop` at scope exit).
- `&[u8; N]` — byte string (future). Fixed-size byte array reference for non-UTF-8 data.
- `Rune = u32` — Unicode codepoint. Defined as a type alias in stdlib.
- `RuneIterator` — stdlib struct that iterates UTF-8 codepoints over a `&str`.

**Compatibility rule** (`types_compatible`): `TypeKind::Str` ↔ `TypeKind::Ref { inner: Str }` always compatible. Both lower to identical fat-pointer representation in codegen/backend.

Key API surface (implemented as stdlib methods via vtable):
```
(&str).len()           -> usize   (bytecode: StrLen 0x60 — calls strlen)
(&str).to_string()     -> String  (bytecode: PrimToStr 0x64 — heap alloc)
(&str).as_str()        -> &str    (bytecode: StrAsStr 0x65 — String→&str view)
(&str).as_string()     -> &str    (alias for as_str)
(&str).parse[i32]()    -> i64     (bytecode: StrToInt 0x62)
(&str).parse[f64]()    -> f64     (bytecode: StrToFloat 0x63)
String.bytes()         -> &str    (view of heap buffer, no copy)
String.runes()         -> RuneIterator
RuneIterator.next()    -> Option[Rune]
RuneIterator.at(i)     -> Rune    // O(n)
```

Mutability is a property of the binding (`var`/`const`), not the type. A `var s: &str` can be rebound; bytes viewed through `&str` are immutable.

## Attribute System

Attributes annotate items and statements. Syntax: `@name` or `@name(args)`.

```
@syscall("write")
fn write(fd: i32, buf: str, len: usize) isize { }

@api("WriteFile")
fn write_file(handle: usize, buf: str, len: usize, out: usize, overlapped: usize) usize { }

@cfg(target_os = "linux")
fn linux_only() void { ... }

// conditional block inside a function:
@cfg(target_os = "linux") {
    var x: i32 = 1;
}
```

**Built-in attributes:**

| Attribute | Target | Effect |
|-----------|--------|--------|
| `@syscall("name")` / `@syscall(number)` | `fn` | Body replaced by single `Syscall` instruction. Name is mapped via Linux x86-64 table or the number is used directly. Skips guaranteed-return check. |
| `@api("FunctionName")` | `fn` | Body replaced by single `CallExt` instruction that calls an external symbol. Win64 ABI on Windows, SysV ABI on other targets. Skips guaranteed-return check. |
| `@cfg(key = "value")` | `fn`, `struct`, `enum`, `trait`, block | Conditional compilation. Currently compiled unconditionally; cfg evaluation deferred to AOT backend. |
| `@inline` | `fn` | Forces inlining eligibility even when the call count is low (still excluded if recursive). |
| `@ignore` / `@ignore(unused_vars)` / `@ignore(dead_code)` | `fn`, `var`, `const`, parameter | Suppresses specific warnings. `@ignore` silences all; `unused_vars` silences W01/W02; `dead_code` silences W03/W07. Implemented in `unused.rs`. |

**Parsing:** `parse_attributes()` in `parser/mod.rs` consumes zero or more `@ident(args)` before items or statements. Item parsers (`parse_fn`, `parse_struct`, etc.) take `Vec<Attribute>` as a parameter. Statement-level `@cfg` becomes `StmtKind::CfgBlock { condition, body }`. Parameters and var/const statements can also have attributes (`Param.attributes`, `StmtKind::Var/Const.attributes`).

**Codegen:** `@syscall` functions emit `Syscall` (0x5E) + `Ret` instead of the normal body. `StmtKind::CfgBlock` body is compiled unconditionally (AOT handles stripping).

## Syscall Example (Linux)

```void
@syscall("write")
fn write(fd: i32, buf: str, len: usize) isize { }

fn main() void {
    const msg: str = "hello from syscall\n";
    write(1, msg, msg.len());
    ret;
}
```
=======
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

### Codegen (`src/bytecode/codegen.rs`)
- Pass 1: assign fn-table indices. Pass 2: compile via `FnCompiler` (virtual reg allocator). Post-pass: inline expansion with jump-target fixup.
- `@syscall` → `Syscall+Ret`. `@api` → `CallExt+Ret`.
- Const-fold: `ConstValue` in `const_map` → `MovI`/`MovConst` directly.
- `&&`/`||`: short-circuit via `Jz`/`Jnz`.
- Variadics: call-site packs coerced args into consecutive slots, emits `Lea` (ptr) + `MovI` (len), passes as two registers to callee.
- Enum constructors: `New` + discriminant `FieldStore` at `ENUM_DISCRIM_OFFSET`, payloads at `ENUM_PAYLOAD_OFFSET+i*8`.
- `?` operator: reads discriminant, uses `.expect()` for tag lookup (no silent fallback).
- Struct: `New(size)` + `FieldStore` per field in declaration order. Field access → `FieldLoad(dst, ptr, offset)`.
- Fn-name as value: `MovConst(FnAddr(name))`. Variable callee: `CallArg*+CallReg`.
- Closure: `__void_closure_N` chunk; captures detected via `capture_ident_names`; env struct heap-allocated; fn ptr at `ENUM_DISCRIM_OFFSET`, captures at `ENUM_PAYLOAD_OFFSET+i*8`; hidden env ptr in r0 on call.
- Monomorphization: `id[T]` call → `id<i32>` mangled chunk. Struct mono not yet implemented.
- Intrinsic dispatch: `INTRINSIC_MAP` HashMap; array ops via `INTRINSIC_OPCODE_MAP` HashMap.

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
| `io` | Done | println, print, eprintln, eprint, read_line — @format aware |
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

**`@format`**: marks fn as format-aware; compiler coerces all args to `str` before passing.

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
| `@format` | Fn is format-aware variadic; compiler pre-coerces args to str. |
| `@intrinsic("void.X")` | Safe stdlib wrapper; dispatched by encoder case number. |

`StmtKind::CfgBlock { condition, body }` — statement-level `@cfg`. Condition evaluated; body compiled only if matching.

## Closures / First-Class Functions

- Type: `TypeKind::Fn { params, return_ty }`. Syntax: `fn(T, U) V`.
- Closure: `|params| expr` → `ExprKind::Closure`. No-capture: `MovConst+FnAddr`. With captures: env struct (`fn_ptr + captured_vals`), hidden r0 env ptr on call.
- Fn-name as value: `MovConst(ConstPoolEntry::FnAddr(name))` → `lea rax,[rip+fn_sym]` + `Pc32` reloc.
- Variable callee: `CallArg*+CallReg`. No reloc needed.
- `ConstPoolEntry::FnAddr` tag = `3`, serialized as u16 len + name bytes.

## Roadmap

### Language
| Feature | Status |
|---------|--------|
| Primitives, arithmetic, control flow | Done |
| Structs, enums, match | Done |
| Functions, closures, fn pointers | Done |
| Generics (fn monomorphization) | Partial — struct mono not yet |
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
>>>>>>> Stashed changes
