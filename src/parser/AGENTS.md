# Parser (`src/parser/`)

## AST

- All nodes are `Spanned<T>`.
- Two `Span` types exist; `to_ast_span` converts between them.

## Expression Precedence

```
assign → logical-or → logical-and → equality → comparison → term → factor → unary → postfix → primary
```

## Error Codes

| Code | Meaning |
|------|---------|
| E00 | Generic |
| E01 | Expected identifier |
| E02 | Expected token |
| E03 | Bad item position |
| E04 | EOF in block |
| E05 | Expected type |

## Language Constructs

- **Import**: `import std.io.stdout;` / `import a.b.{x,y};` / `import a.b as c;` / `import a.b.*;`
- **Closure**: `|params| expr` — `Pipe` in primary position. Params are bare idents (no types).
- **Fn pointer type**: `fn(T, U) V` — greedy return type via `peek_is_type_start()`.
- **Variadics**: `...args: T` in param list; inside fn body `args` is `Slice[T]` with `.len()`.
- **Pattern matching**: wildcard, bind, literal, variant, and **guards** (`pat if expr =>`).
- **Named arguments**: `foo(x=1, y=2)` — all positional args must precede named args. Resolved to param position at compile time; unknown name or position conflict = S09 error.

## Missing / Planned

- **`break` / `continue`**: No lexer tokens or AST nodes exist yet. Need `TokenKind::Break` / `TokenKind::Continue` and new `StmtKind` variants.
- **`else if` chains**: Currently parsed as `else { if cond { ... } }` (nested `If` inside `else_block`). A dedicated `else_if: Vec<(Expr, Block)>` field on `If` would flatten the AST and improve codegen. See P1 roadmap in `DOCS.md`.
