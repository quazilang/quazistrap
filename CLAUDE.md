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
void build <file> [files...]        # compile files to native binary via gcc (default)
void build <file> -b                # emit .vbc bytecode
void build <file> -s                # emit .s AT&T x86-64 assembly
void build <file> [-b|-s] -o out    # specify output filename
void build <file> -r                # compile and run
void build                          # build project from void.toml (no files given)
void build [-b|-s] [-o out]         # build project with options
void build -r                       # build + run project from void.toml
void run                            # build + run project from void.toml
void check                          # analyze project without emitting
void debug [-b|-s]                  # compile hardcoded demo source
void new <name>                     # create a new project
void fmt                            # trim trailing whitespace in .void files
void clean                          # remove build artifacts
```

Default output names: `<stem>.vbc` (bytecode), `<stem>.s` (assembly), `<stem>` / `<stem>.exe` (binary).

Rust edition 2024.

## Architecture

Compiler frontend pipeline: source files → `Loader` → merged source string → `Lexer` → `Vec<Token>` → `Parser` → `Program` → `Analyzer` → `SemanticReport` → `Codegen` → `Vec<Chunk>` → `X86Emitter` → AT&T asm → `gcc` → binary.

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
- `FnCompiler.next_reg` monotonically allocates virtual registers; stored in `Chunk.reg_count` for use by inline expansion and AOT.

### AOT Backend (`src/aot/mod.rs`)
`X86Emitter` lowers `Vec<Chunk>` to AT&T x86-64 assembly (`.s` file). Binary emit writes `.s` then shells out to `gcc`, deletes the `.s` after.

- **Calling convention**: SysV AMD64 — args in `rdi, rsi, rdx, rcx, r8, r9`; return in `rax`.
- **VBC register N** maps to stack slot `[rbp - (N+1)*8]`.
- **Frame**: `round_to_16(max_reg_used * 8)` bytes, allocated with `subq`.
- **Prologue**: `pushq %rbp; movq %rsp, %rbp; subq $frame, %rsp`; then load SysV arg regs into param slots.
- **Jump labels**: collected first (`jump_targets`), emitted as `.{fn_label}_L{instr_idx}:`.
- **`CallArg` + `CallIdx`**: pending arg list accumulated; on `CallIdx`, args moved to SysV regs, then `callq`; result in `%rax` moved to dst slot.
- **`Syscall`**: Linux x86-64 ABI — `rax` = syscall number, args in `rdi, rsi, rdx, r10, r8, r9`, return in `rax`.
- **`CallExt`**: external symbol call — Windows uses Win64 ABI (`rcx, rdx, r8, r9` + 32-byte shadow + stack args), non-Windows uses SysV ABI (`rdi, rsi, rdx, rcx, r8, r9`).
- **Unimplemented**: `VtblLoad`, `CallReg` emit placeholder `xorq %rax, %rax` + comment.
- String constants go to `.rodata` as `.{fn_label}_str{idx}:`, loaded via `leaq ... (%rip), %rax`.

## Language Syntax (current)

```
import std.io.stdout;
import mymodule.myFunc;  // auto-loads mymodule.void from same directory

pub fn name[T](param: Type) ReturnType {  // pub is optional, ignored
    const x: i32 = 1 + 2;
    var y: str;
    y = "hello";
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

- `str` — primitive type (keyword). Immutable byte-slice view: fat pointer (`ptr: *u8, len: usize`). No ownership, no UTF-8 guarantee. String literals have type `str`. Fat-pointer layout is resolved by the AOT backend.
- `String` — stdlib struct. Owned heap string: `ptr + len + cap`, UTF-8 valid. Allocated/freed by RAII (`Drop` at scope exit).
- `Rune = u32` — Unicode codepoint. Defined as a type alias in stdlib.
- `RuneIterator` — stdlib struct that iterates UTF-8 codepoints over a `str`.

Key API surface (implemented as stdlib methods via vtable):
```
str.len()              -> usize   (bytecode: StrLen 0x60 — reads the len field)
str.to_string()        -> String  (bytecode: PrimToStr 0x64 — heap alloc)
str.as_str()           -> str     (bytecode: StrAsStr 0x65 — String→str view)
str.as_string()        -> str     (alias for as_str)
str.parse[i32]()        -> i64   (bytecode: StrToInt 0x62)
str.parse[f64]()        -> f64   (bytecode: StrToFloat 0x63)
String.bytes()         -> str     (view of heap buffer, no copy)
String.runes()         -> RuneIterator
RuneIterator.next()    -> Option[Rune]
RuneIterator.at(i)     -> Rune    // O(n)
```

Mutability is a property of the binding (`var`/`const`), not the type. A `var s: str` can be rebound; bytes viewed through `str` are immutable.

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
