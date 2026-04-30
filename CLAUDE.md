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
void compile <file> [files...]   # compile; auto-resolves local imports
void compile <file> -b           # emit .vbc bytecode
void compile <file> -s           # emit assembly (unimplemented)
void debug [-b|-s]               # compile hardcoded demo source
void build | run | check | new | fmt | clean   # unimplemented
```

Rust edition 2024.

## Architecture

Compiler frontend pipeline: source files → `Loader` → merged source string → `Lexer` → `Vec<Token>` → `Parser` → `Program` → `Analyzer` → `SemanticReport`.

### Loader (`src/loader.rs`)
- `load_programs(entries: &[PathBuf]) -> Result<LoadResult>` — resolves local imports recursively, merges sources in dependency-first order, parses the merged string as one `Program`.
- **Local import detection**: `import foo.bar` is local if `foo.void` exists next to the importing file. Otherwise treated as stdlib/external (no file loaded).
- Deduplicates via canonical-path `HashSet` — circular or repeated imports are safe.
- Merged source byte offsets are contiguous, so `render_diagnostic` works correctly across file boundaries.
- Declare pass in semantic: when an import name collides with an already-declared function (from a loaded local file), the import binding is silently skipped — no duplicate-declaration error.

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
3. **Unused** (`unused.rs`) — warn on unused variables, parameters, functions, imports.
4. **Dead code** (`unused.rs`) — reachability analysis, warn on statements after guaranteed returns.
5. **Optimization** (`optimize.rs`) — inline candidates (≤2-statement non-branching functions), match exhaustiveness, removable imports, **constant folding** (both-sides-known → `ConstValue`), **math identity/absorber reduction** (`x*0=0`, `x+0=x`, `x&&false=false`, etc. — result stored in `ExprAnnotation.const_value` in annotated tree), **lazy import hints** (field-chain tracking: `import std;` + `std.io.stdout.println(...)` → suggests `import std.io.stdout;`).

`types_compatible` treats `Any` as compatible with everything and `Named` types as compatible with everything (generics are not yet resolved).

`main` is exempt from unused-function and inline-candidate checks.

Public types live in `types.rs`, re-exported from `mod.rs`.

### `SemanticReport`
Structured output with: `errors`, `warnings`, `suggestions`, `symbol_table`, `dependency_graph`, `optimization_hints` (includes `math_optimizations`, `lazy_import_hints`), `annotated_exprs`, `constant_evaluations`, `used_imports_map`, `non_exhaustive_matches`, `lazy_import_hints`.

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
- `0x50–0x5F` — atomics, threading & syscalls (`AtomicAdd`, `AtomicCas`, `MemFence`, `Spawn`, `Syscall=0x5E`)
- `0x60–0x6F` — string ops (`StrLen=0x60`, `StrConcat=0x61`)
- `0x70–0x7F` — reserved

Key design decisions:
- `Move` (ownership transfer, src invalidated) vs `Dup` (copy, Copy types only) — compiler chooses at codegen, never both for same value.
- `Drop` = RAII destructor call emitted by compiler at scope exit, no runtime GC.
- No null — `Option[T]` is discriminant + value, `match` compiles to `Cmp` + conditional jump.
- `Chunk` = one function's code (`Vec<Instruction>`) + constant pool (`Vec<ConstPoolEntry>`). `patch_jump` backpatches forward jumps after target is known.
- Flat serialization: `Chunk::to_bytes()` → `Vec<u8>` at 6 bytes/instruction.
- Register allocation and platform lowering happen in AOT backend (QBE or custom), not in VBC.

## Language Syntax (current)

```
import std.io.stdout;
import mymodule.myFunc;  // auto-loads mymodule.void from same directory

pub fn name[T](param: Type) ReturnType {  // pub is optional, ignored
    const x: int32 = 1 + 2;
    var y: str;
    y = "hello";
    if (cond) { ... } else { ... }
    while (cond) { ... }
    ret expr;
}

struct Foo[T] { field: T, const flag: bool, }
trait Bar[T] { fn method(x: T) T; }
impl Bar[int32] for Foo[int32] { fn method(x: int32) int32 { ret x; } }
enum Option[T] { Some(T), None, }

match value {
    Some(v) => v,
    Option.None => 0,
    _ => default,
}
```

Primitive types: `int8/16/32/64`, `uint8/16/32/64`, `isize`, `usize`, `float16/32/64`, `bool`, `str`, `void`, `any`.

## String Model

- `str` — primitive type (keyword). Immutable byte-slice view: fat pointer (`ptr: *u8, len: usize`). No ownership, no UTF-8 guarantee. String literals have type `str`. Fat-pointer layout is resolved by the AOT backend.
- `String` — stdlib struct. Owned heap string: `ptr + len + cap`, UTF-8 valid. Allocated/freed by RAII (`Drop` at scope exit).
- `Rune = u32` — Unicode codepoint. Defined as a type alias in stdlib.
- `RuneIterator` — stdlib struct that iterates UTF-8 codepoints over a `str`.

Key API surface (implemented as stdlib methods via vtable):
```
str.len()              -> usize   (bytecode: StrLen 0x60 — reads the len field)
str.to_string()        -> String
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
fn write(fd: int32, buf: str, len: usize) isize { }

@cfg(target_os = "linux")
fn linux_only() void { ... }

// conditional block inside a function:
@cfg(target_os = "linux") {
    var x: int32 = 1;
}
```

**Built-in attributes:**

| Attribute | Target | Effect |
|-----------|--------|--------|
| `@syscall("name")` | `fn` | Body replaced by single `Syscall` instruction. Syscall number looked up by name (Linux x86-64 table). Skips guaranteed-return check. |
| `@cfg(key = "value")` | `fn`, `struct`, `enum`, `trait`, block | Conditional compilation. Currently compiled unconditionally; cfg evaluation deferred to AOT backend. |
| `@inline` | `fn` | Hint for inlining (connects to semantic inline-candidate pass). |

**Parsing:** `parse_attributes()` in `parser/mod.rs` consumes zero or more `@ident(args)` before items or statements. Item parsers (`parse_fn`, `parse_struct`, etc.) take `Vec<Attribute>` as a parameter. Statement-level `@cfg` becomes `StmtKind::CfgBlock { condition, body }`.

**Codegen:** `@syscall` functions emit `Syscall` (0x5E) + `Ret` instead of the normal body. `StmtKind::CfgBlock` body is compiled unconditionally (AOT handles stripping).
