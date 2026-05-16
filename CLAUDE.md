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
void build -s / --strip             # strip debug symbols from output binary
void run                            # build + run project from void.toml
void run --linker /path/to/ld       # build + run with explicit linker
void run -s / --strip               # strip debug symbols before running
void check                          # analyze project without emitting
void debug [-b]                     # compile hardcoded demo source
void new <name>                     # create a new binary project
void new <name> --lib               # create a library project
void init                           # initialize project in current directory
void init --lib                     # initialize library project in current directory
void fmt                            # trim trailing whitespace in .void files
void clean                          # remove build artifacts
```

Default output names: `<stem>.vbc` (bytecode), `<stem>.o` (object), `<stem>` / `<stem>.exe` (binary).

**.vbc as input**: `void build foo.vbc -o out` compiles pre-compiled VBC bytecode to native (skips frontend). Supports `-c`, `-b`, `-r`, `--linker`.

**Linker selection**: binary emit requires an external linker. Search order: `VOID_LINKER` env var → `ld.lld` → `mold` → `ld`. Override with `--linker /path/to/linker` or `export VOID_LINKER=/path/to/linker`.

Rust edition 2024.

## Architecture

Compiler frontend pipeline:
```
source files → Loader → merged source → Lexer → Vec<Token> → Parser → Program
             → Analyzer → SemanticReport → Codegen → Vec<Chunk>
             → Backend (object + iced-x86) → .o bytes
             → LinkerInvocation (ld/lld/mold) → binary
```

VBC (`-b`) output: Codegen → `Vec<Chunk>` serialized directly, no backend involved.  
Object (`-c`) output: same as binary but stop after `.o`; no linker step.  
The GCC/GAS toolchain is no longer required or used.

### Loader (`src/loader.rs`)
- `load_programs(entries: &[PathBuf]) -> Result<LoadResult>` — resolves local imports recursively, merges sources in dependency-first order, parses the merged string as one `Program`.
- `load_programs_with_resolver(entries, resolver)` — like `load_programs`, but resolves module imports via `ModuleResolver` when a dependency name matches the first import segment.
- **Local import detection**: `import foo.bar` resolves to a module when `foo` exists in `ModuleResolver`; otherwise local if `foo.void` exists next to the importing file. Otherwise treated as stdlib/external (no file loaded).
- **Built-in std resolution order**: `VOID_STD_ROOT` env var → `~/.void/std` / `%USERPROFILE%/.void/std` → `CARGO_MANIFEST_DIR/std` → `cwd/std`.
- Deduplicates via canonical-path `HashSet` — circular or repeated imports are safe.
- Merged source byte offsets are contiguous, so `render_diagnostic` works correctly across file boundaries.
- Declare pass in semantic: when an import name collides with an already-declared function (from a loaded local file), the import binding is silently skipped — no duplicate-declaration error.

### Project Config (`src/project.rs`)
- `ProjectContext::load(start)` finds the nearest `void.toml`, loads project metadata, and builds a `ModuleResolver`.
- `ProjectContext::discover(start)` is used by `void compile` to apply module resolution when a project config exists.
- `void.toml` supports `[package]`, `[build]`, and `[dependencies]` (path + optional version).
- `void.lock` is created on build/run when dependencies exist, and validated if present.
- `ProjectKind::Bin` (default) vs `ProjectKind::Lib` — set via `type = "lib"` in `[package]`.
- Lib projects default entry: `src/lib.void`. Lib builds default to `.vbc` output (bytecode), not a native binary.
- `void new --lib` / `void init --lib` scaffold a library project with `type = "lib"` and `src/lib.void`.

**Library `void.toml` template**:
```toml
[package]
name = "mylib"
version = "0.1.0"
type = "lib"

[build]
entry = "src/lib.void"
src = "src"
```

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
version:     0x02     (u8)
chunk_count: u32 LE
per chunk:
  name_len:  u16 LE + name bytes
  param_count: u16 LE   (v2+)
  reg_count:  u8        (v2+)
  const_count: u16 LE
  per constant: tag(u8) + value (8 bytes Int/Float, u16 len + bytes for Str)
  instr_count: u32 LE
  instructions: instr_count * 6 bytes
```
`deserialize_vbc` handles both v1 (lacks param_count/reg_count, defaults to 0)
and v2. `void build foo.vbc` reads any supported version.

### Codegen (`src/bytecode/codegen.rs`)
`Codegen` takes a `&SemanticReport` and compiles a `Program` to `Vec<Chunk>`.

- **Pass 1**: assign each `fn` item a function-table index (order of appearance).
- **Pass 2**: compile each function body via `FnCompiler` (virtual register allocator).
- **Post-pass**: inline expansion — replaces `CallArg* + CallIdx` sequences for functions in `inline_candidates` with a register-remapped copy of the callee body (Ret stripped, args copied to callee param regs via base offset). Vtable calls (`CallReg`) are not inlined. **Critical**: after each `splice`, all absolute jump targets ≥ old splice end are adjusted by `delta = new_len - old_len` to prevent jump drift (fixed bug: jumps were looping back into inlined body).
- `@syscall` functions: emit single `Syscall` (num from Linux x86-64 table or raw number) + `Ret`, skip body.
- `@api` functions: emit single `CallExt` (symbol from attribute string in the constant pool) + `Ret`, flags store arg count.
- Const-fold: expressions with a `ConstValue` in `const_map` (from semantic) emit `MovI`/`MovConst` directly.
- `&&` / `||`: short-circuit via `Jz`/`Jnz` — right side only evaluated if needed.
- Return value convention: always in virtual register 0 (`r0`).
- Known method call builtins: `len()→StrLen`, `to_string()→PrimToStr`, `as_str()/as_string()→StrAsStr`, `parse[T]()→StrToInt` or `StrToFloat` depending on type arg.
- `FnCompiler.next_reg` monotonically allocates virtual registers; stored in `Chunk.reg_count` for use by inline expansion and the backend.
- **Zero-arg enum variant construction** in `ExprKind::Ident`: if name not in regs but matches `enum_ctor_tag`, emits `New(16) + MovI(tag) + FieldStore(tag,ptr,0) + Mov(dst,ptr)`. Fixes `None` / `Err(...)` used as bare identifiers.
- **`coerce_to_display_str`**: converts format args to C string pointers for the format engine. `TypeKind::Any` and `None`/unresolved → treated as int (tag=0 → PrimToStr). `Str`/`Ref`/`Named`/`RawPtr`/`Slice` passed unchanged (already a pointer).
- **Enum constructors**: `ExprKind::Call` with a name matching `enum_ctor_tag` → heap-allocates `New(size)`, stores discriminant at offset 0, payloads at offsets 8, 16, …
- **Struct field access**: `ExprKind::Field { object, name }` → looks up field byte offset from `struct_defs`, emits `FieldLoad(dst, obj_ptr, offset)`.
- **Struct construction**: `ExprKind::StructLit { name, fields }` → emits `New(size)` then `FieldStore` for each field in declaration order.
- **Array intrinsics**: `void.array.store` (case 18), `void.array.load` (case 19) — args from consecutive registers starting at `slot(dst)`. Emitted by `@intrinsic("void.array.store/load")` stdlib wrappers.
- **Static method dispatch**: for `Named` types, mangled to `TypeName.method`, looked up in `fn_index`. Works without vtables for concrete types. Falls back to VtblLoad+CallReg for truly dynamic dispatch.

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
// Linux/MacOs → ElfBackend; Windows → PeBackend
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
**Implemented opcodes** (encoder.rs): `New` (calloc heap alloc), `FieldLoad`/`FieldStore` (struct member read/write at byte offset), `VtblLoad` (reads vtable ptr from object[0] then slot from vtable[slot*8]), `CallReg` (indirect call through function pointer in register). `NewObj` still emits `xor rax,rax` placeholder.

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

## Unsafe System

Raw pointers and syscalls require explicit unsafe opt-in.

**Rules:**
1. A function with `*T` in any param or return type **must** be declared `unsafe fn` (error S12 otherwise). Exception: `@syscall` / `@api` functions are implicitly unsafe without the keyword.
2. `@syscall` and `@api` functions are implicitly `unsafe fn` — the `Symbol.unsafe_fn` flag is set in the declare pass regardless of whether the source says `unsafe fn`.
3. Calling an `unsafe fn` (including `@syscall`/`@api`) outside an `unsafe` context → error S11.
4. Dereferencing `*T` or storing through `*T` outside an `unsafe` context → error S11.
5. `@intrinsic` functions are **safe** — they are stdlib wrappers that handle unsafety internally.

**Unsafe contexts:** `unsafe fn` body or `unsafe { }` block. Both increment `unsafe_depth`; checks fire when `unsafe_depth == 0`.

```
// safe wrapper — @intrinsic, no unsafe needed at call site
import std.core.write;
fn main() void { write(1, "hello\n", 6); }

// raw syscall — unsafe at call site
@syscall("write")
fn raw_write(fd: i32, buf: str, len: usize) isize { }

fn safe_wrapper(msg: str) void {
    unsafe { raw_write(1, msg, 4); }   // OK — inside unsafe block
}

// raw pointer in signature — must be unsafe fn
unsafe fn alloc_ptr(n: usize) *u8 { ... }

fn use_ptr() void {
    unsafe {
        var p: *u8 = alloc_ptr(64);    // OK
        var b: u8  = *p;               // OK — deref inside unsafe
    }
}
```

**Symbol tracking:** `Symbol.unsafe_fn: bool` — set in declare pass. `@syscall`/`@api` set it `true`; explicit `unsafe fn` sets it `true`; all others `false`.

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
unsafe fn ptr_fn(p: *u8) *u8 { ret p; }   // *T in sig → must be unsafe fn
unsafe { var x = ptr_fn(p); *x = 1; }     // unsafe block for calls + deref

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

## Standard Library (`std/src/`)

Source-based — `.void` files merged at compile time, no precompiled objects. Resolved via `VOID_STD_ROOT` env var or `~/.void/std` / `%USERPROFILE%/.void/std` or `CARGO_MANIFEST_DIR/std`.

| File | Description |
|------|-------------|
| `core.void` | `write`, `read`, `exit`, `malloc`, `free`, `calloc`, `realloc`, `memcpy`, `memset`, `strlen`, `strcmp` — raw syscall/api wrappers (`@intrinsic`) |
| `io.void` | `println`, `print`, `eprintln`, `eprint`, `read_line` — format-aware I/O |
| `fmt.void` | `format` — variadic format intrinsic (`@intrinsic("void.format")`), processes `{}` placeholders |
| `string.void` | `String` struct — owned heap string (`ptr+len+cap`), `new`, `push`, `push_str`, `len`, `bytes`, `as_str`, `from` |
| `panic.void` | `PanicInfo`, `__void_panic_handler`, `panic` — unrecoverable error with message |
| `result.void` | `Result[T,E]` enum — `ok()`, `is_ok()`, `is_err()`, `unwrap()`, `unwrap_err()`, `unwrap_or()`, `unwrap_err_or()` |
| `option.void` | `Option[T]` enum — `ok()`, `is_some()`, `is_none()`, `unwrap()`, `unwrap_or()` |
| `box.void` | `Box[T]` — heap-allocated owned pointer, `new`, `get`, `set` |
| `traits.void` | Common trait definitions (`Display`, `Debug`, `Clone`, `Copy`, `Drop`, `Iterator`) |
| `prelude.void` | Re-exports: `Option`, `Result`, `String`, `Box`, `panic`, `format`, `println` |
| `collections/` | `vec.void` (`Vec[T]`), `map.void` (`HashMap[K,V]`) — WIP |
| `unix.void` | Unix-specific syscall wrappers |
| `windows.void` | Windows-specific Win32 API wrappers |

**`@format` attribute**: marks a function as a format-aware variadic. At call sites, the compiler pre-formats all args (via `format` intrinsic) into a single string and passes that. Format template uses `{}` placeholders.

**`@intrinsic("void.X")` dispatch** (encoder case numbers):
| Intrinsic | Case | Effect |
|-----------|------|--------|
| `void.format` | 17 | format engine — all args must be C string pointers; `{}` replaced sequentially |
| `void.array.store` | 18 | `arr[idx] = val` — writes u64 into heap array slot |
| `void.array.load` | 19 | `val = arr[idx]` — reads u64 from heap array slot |

**`?` operator**: desugars to match on Result/Option, short-circuit return Err/None on failure, unwrap value on success. End-to-end verified working.

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
| `@syscall("name")` / `@syscall(number)` | `fn` | Body replaced by single `Syscall` instruction. Name is mapped via Linux x86-64 table or the number is used directly. Skips guaranteed-return check. **Implicitly unsafe** — calling site requires `unsafe {}` or `unsafe fn`. |
| `@api("FunctionName")` | `fn` | Body replaced by single `CallExt` instruction that calls an external symbol. Win64 ABI on Windows, SysV ABI on other targets. Skips guaranteed-return check. **Implicitly unsafe** — calling site requires `unsafe {}` or `unsafe fn`. |
| `@cfg(key = "value")` | `fn`, `struct`, `enum`, `trait`, block | Conditional compilation. Currently compiled unconditionally; cfg evaluation deferred to AOT backend. |
| `@inline` | `fn` | Forces inlining eligibility even when the call count is low (still excluded if recursive). |
| `@ignore` / `@ignore(unused_vars)` / `@ignore(dead_code)` | `fn`, `var`, `const`, parameter | Suppresses specific warnings. `@ignore` silences all; `unused_vars` silences W01/W02; `dead_code` silences W03/W07. Implemented in `unused.rs`. |

**Parsing:** `parse_attributes()` in `parser/mod.rs` consumes zero or more `@ident(args)` before items or statements. Item parsers (`parse_fn`, `parse_struct`, etc.) take `Vec<Attribute>` as a parameter. Statement-level `@cfg` becomes `StmtKind::CfgBlock { condition, body }`. Parameters and var/const statements can also have attributes (`Param.attributes`, `StmtKind::Var/Const.attributes`).

**Codegen:** `@syscall` functions emit `Syscall` (0x5E) + `Ret` instead of the normal body. `StmtKind::CfgBlock` body is compiled unconditionally (AOT handles stripping).

## Syscall Example (Linux)

`@syscall` functions are implicitly unsafe — call site must be inside `unsafe {}` or `unsafe fn`.

```void
@syscall("write")
fn write(fd: i32, buf: str, len: usize) isize { }

fn main() void {
    const msg: str = "hello from syscall\n";
    unsafe {
        write(1, msg, msg.len());
    }
    ret;
}
```

## Ecosystem Roadmap

The goal is a fully independent void toolchain — no LLVM, no GCC, no libc dependency.

```
void source
    ↓
VBC (platform-independent IR)  ←── serialize/deserialize (.vbc files)
    ↓
AOT backend (iced-x86)
    ↓
void link  (built-in linker — replaces lld-link / ld.lld)
    ↓
native binary  (ELF / PE / Mach-O)

JIT VM: deferred to AOT-complete or self-host rewrite phase
```

### `void link` — Built-in Linker

Subcommand (`void link`) replacing external linkers (lld-link, ld.lld, mold). Owned linker unlocks:

- **ELF**: `p_align=1`, no `.note.gnu.build-id`, single PT_LOAD segment — target: hello world < 500 bytes
- **PE**: import table packed inside `.text`, `FileAlignment=16`, no DOS stub — target: hello world < 700 bytes
- No `LIB` env var requirement on Windows
- Deterministic output (no timestamps, no build IDs)
- Direct `.vbc` → binary path (no intermediate `.o` file)
- Planned location: `src/linker/` inside this repo, invoked automatically by `void build`

Until `void link` exists, external linker selection order: `VOID_LINKER` env var → `lld-link`/`ld.lld` → `mold` → `ld`.

### JIT VM

Deferred — planned for when AOT backend is feature-complete or during the self-hosted rewrite (void compiler written in void). VBC chunks are the natural JIT unit (platform-independent, already serializable). No separate JIT IR needed when the time comes.

### Binary Size Status

| Target | Current stripped hello world | Goal with `vld` |
|--------|------------------------------|-----------------|
| Linux x86-64 ELF | ~1.0 KB | < 500 bytes |
| Windows x86-64 PE | ~1.5 KB | < 700 bytes |

Current floor is set by external linker padding (FileAlignment=512 on PE, 2MB segment alignment on ELF with `-z max-page-size=0x1000` applied → 4KB). `vld` removes this ceiling entirely.

### Philosophy

- No LLVM — optimizations built into VBC passes (`optimize.rs`) and the backend directly
- No libc — `@intrinsic` lowers to raw syscalls (Linux) or Win32 (Windows)
- VBC is the stable serialization format — compile once, run on any void runtime
- Linker (`vld`) and JIT are first-class parts of the void ecosystem, not external dependencies

---

## Feature Roadmap

### Language

| Feature | Status | Notes |
|---------|--------|-------|
| Primitive types + arithmetic | Done | i8–i64, u8–u64, f32/f64, bool, str |
| Control flow (if/while/for/match) | Done | Range loop, iterator loop, exhaustiveness check |
| Functions + closures | Partial | Named functions done; closures/lambdas not yet |
| Structs + field access | Done | `New` + `FieldLoad`/`FieldStore` in backend |
| Enums + match | Done | Heap-allocated discriminant+payload; zero-arg variants in Ident path |
| Traits + impl | Partial | Static dispatch for concrete Named types; vtable (trait objects / fat pointers) partial |
| Generics | Partial | Parsed + type-checked with `Any` placeholder; not monomorphized — specialization TBD |
| `?` operator | Done | Desugars to match/short-circuit; end-to-end verified |
| `unsafe` blocks + raw pointers | Done | S11/S12 errors, unsafe depth tracking |
| `@cfg` conditional compilation | Partial | Parsed; AOT stripping not yet evaluated |
| Type aliases | Not started | `type Rune = u32` style |
| Closures / first-class functions | Not started | Capture environments, `fn(T) -> U` type |
| Pattern matching improvements | Not started | Nested patterns, tuple destructuring, guard clauses |
| Lifetimes / borrow checker | Not started | Currently `@borrow` pass is stub; no enforcement |
| `async`/`await` | Not started | — |
| Remove hardcodes | Not started | Array intrinsic cases 18/19, method name strings (`len`/`to_string`/etc.), enum tag values (Ok=1/Err=0), struct sizes — replace with proper type-driven codegen |

### Standard Library

| Module | Status | Notes |
|--------|--------|-------|
| `core` (syscalls/api) | Done | write, read, exit, malloc, free, memcpy, strlen, etc. |
| `io` (println/print/read_line) | Done | format-aware, cross-platform |
| `fmt` (format intrinsic) | Done | `{}` placeholders, PrimToStr coercion |
| `string` (String struct) | Done | heap string, push, as_str, len |
| `result` / `option` | Done | full method surface, `?` operator |
| `panic` | Done | panic handler, PanicInfo |
| `box` (Box[T]) | Done | heap allocation wrapper |
| `traits` | Partial | trait definitions, not all impls |
| `prelude` | Done | re-exports common types |
| `collections/vec` (Vec[T]) | WIP | push, get, len, iteration |
| `collections/map` (HashMap[K,V]) | WIP | insert, get, contains |
| `collections/set` (HashSet[T]) | Not started | — |
| `fs` (file I/O) | Not started | open, read_to_string, write_all |
| `net` (networking) | Not started | — |
| `thread` / `sync` | Not started | Spawn opcode exists (0x5B), no high-level API |

### Toolchain

| Component | Status | Notes |
|-----------|--------|-------|
| `void build` (AOT binary) | Done | Linux ELF, Windows PE, macOS Mach-O partial |
| `void run` | Done | build + exec |
| `void check` | Done | analyze without emit |
| `void fmt` | Done | trailing whitespace trim |
| `void new` / `void init` | Done | bin + lib variants |
| `void lsp` | Partial | stdio mode; hover/completions basic |
| `void link` (built-in linker) | Not started | replaces lld-link/ld.lld; ELF < 500 B, PE < 700 B |
| JIT VM | Deferred | planned for AOT-complete phase or self-host rewrite |
| Package registry | Not started | path deps work; version registry TBD |
| `void doc` | Not started | doc comment extraction |
| `void test` | Not started | built-in test runner (`@test` attribute) |

### Backend

| Target | Status | Notes |
|--------|--------|-------|
| Linux x86-64 (ELF) | Supported | Full |
| Windows x86-64 (PE) | Supported | Requires `lld-link` + `LIB` env |
| macOS x86-64 (Mach-O) | Partial | Object format works; dyld entry needs work |
| aarch64 (Linux/macOS) | Not started | New `trait Backend` impl needed |
| WASM | Not started | — |
