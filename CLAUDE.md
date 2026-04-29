# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build             # debug build
cargo build --release   # release build
cargo run               # run bootstrap demo (hardcoded source in main.rs)
cargo test              # all tests
cargo test <name>       # single test, e.g. cargo test parses_enum_and_match_expression
cargo clippy            # lint
cargo fmt               # format
```

No external dependencies. Rust edition 2024.

## Architecture

Compiler frontend pipeline: source string → `Lexer` → `Vec<Token>` → `Parser` → `Program` → `Analyzer` → `SemanticReport`.

### Lexer (`src/lexer/`)
- `token.rs` — `Token`, `TokenKind`, `Span` (line, col, byte start/end).
- `mod.rs` — `Lexer`: character-by-character, emits `TokenKind::Error` on unknown chars rather than panicking.
- `&&` and `||` are **not** dedicated tokens — the lexer only has `Ampersand` and `Pipe`. The parser synthesizes `&&`/`||` via `match_and_and()` / `match_or_or()` in `common.rs`.
- Generic type arguments use `[T]` **square brackets**, not angle brackets.

### Parser (`src/parser/`)
- `ast.rs` — all node types. Every node is `Spanned<T>` (a struct with `node: T` and `span: Span`). Two `Span` types exist: `lexer::token::Span` and `parser::ast::Span` (same fields, different type). `to_ast_span` in `common.rs` converts between them.
- `mod.rs` — `Parser` struct + statement and expression parsing. Expressions use precedence climbing: assignment → logical-or → logical-and → equality → comparison → term → factor → unary → postfix (call/field/method) → primary.
- `items.rs` — top-level item parsers: `fn`, `struct`, `trait`, `enum`, `impl`, `import`.
- `common.rs` — parser utilities (`expect`, `advance`, checkpoint/restore for backtracking), `render_diagnostic` (formats error with source snippet + caret underline), synchronize methods for error recovery.

**Error codes**: E00 (generic), E01 (expected identifier), E02 (expected token), E03 (unexpected item position), E04 (unexpected EOF in block), E05 (expected type).

**Import syntax**: `import std.io.stdout;` / `import a.b.{x, y};` / `import a.b as c;` / `import a.b.*;`

**Trait impl syntax**: `impl TraitName[T] for StructName[T] { ... }` — the `for` keyword is consumed by `parse_impl`, not a reserved `TokenKind`.

### Semantic Analysis (`src/semantic/mod.rs`)
`Analyzer` runs five sequential passes over the `Program` AST:

1. **Declare** — register top-level functions, structs, traits, enums, imports into global scope.
2. **Type-check** — scope tracking, type inference, type compatibility, initialization checks, expression annotations.
3. **Unused** — warn on unused variables, parameters, functions, imports.
4. **Dead code** — reachability analysis, warn on statements after guaranteed returns.
5. **Optimization hints** — inline candidates (≤2-statement non-branching functions), match exhaustiveness, removable imports.

`types_compatible` treats `Any` as compatible with everything and `Named` types as compatible with everything (generics are not yet resolved).

`main` is exempt from unused-function and inline-candidate checks.

### `SemanticReport`
Structured output with: `errors`, `warnings`, `suggestions`, `symbol_table`, `dependency_graph`, `optimization_hints`, `annotated_exprs`, `constant_evaluations`, `used_imports_map`, `non_exhaustive_matches`.

## Language Syntax (current)

```
import std.io.stdout;

fn name[T](param: Type) ReturnType {
    const x: int32 = 1 + 2;
    var y: str;
    y = "hello";
    if (cond) { ... } else { ... }
    while (cond) { ... }
    return expr;
}

struct Foo[T] { field: T, const flag: bool, }
trait Bar[T] { fn method(x: T) T; }
impl Bar[int32] for Foo[int32] { fn method(x: int32) int32 { return x; } }
enum Option[T] { Some(T), None, }

match value {
    Some(v) => v,
    Option.None => 0,
    _ => default,
}
```

Primitive types: `int8/16/32/64`, `uint8/16/32/64`, `float16/32/64`, `bool`, `str`, `void`, `any`.
