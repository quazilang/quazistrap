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
- **Fn pointer type**: `fn(T, U) V` — greedy return type via `peek_is_type_start()`. An exact `@repr(C)` on a non-generic type alias turns this syntax into a raw C callback type; the unannotated form remains a Quazi closure/function value.
- **Byte strings**: `b"..."` decodes byte escapes while `br"..."` is raw. Both have the immutable `bytes` type; indexing yields `u8` and `.as_ptr()` points at the first payload byte.
- **C aggregates**: `@repr(C) union U { ... }`, `@repr(C, packed, align=8) struct S`, named fields such as `flags: u32:5`, and final `[u8; ..]` flexible array members. Struct/union fields may carry opaque postfix metadata (`name: str @ini("user_name")`); preserve every attribute rather than validating its name.
- **Foreign globals**: top-level `@api("symbol") var name: Type;` has no initializer and is validated as a mutable C data import by semantic analysis.
- **Variadics**: `...args: T` in param list; inside fn body `args` is `Slice[T]` with `.len()`.
- **Pattern matching**: wildcard, bind, literal, variant, and **guards** (`pat if expr =>`).
- **Named arguments**: `foo(x=1, y=2)` — all positional args must precede named args. Resolved to param position at compile time; unknown name or position conflict = S09 error.
- **String slicing**: `text[start:end:step]` lowers during parsing to the hidden
  `__slice(start, end, step, start_omitted, end_omitted)` method. Bounds and step
  remain ordinary expressions; omitted-bound flags preserve dynamic-step semantics.

## Tail values and `if` expressions (D-017)

- `parse_expr_stmt` accepts an expression closed by `}` without `;` and records
  it in `unterminated_tails` with its would-be `E02` error.
  `parse_function_body` consumes only the function tail, recursing into the
  branches of a final `if` statement and a final `unsafe` block: it becomes
  `StmtKind::Return(Some(expr))` unless the declared return type is `void` or
  `!`. Any other recorded tail is reported. A tail `if` without `else` cannot
  supply a value.
- `if` in primary position (`parse_if_expr`) requires `else`, allows `else if`,
  takes exactly one expression per branch, and is produced as
  `match cond { true => a, _ => b }` (boolean literal patterns share `if`
  truthiness). Statement-position `if` stays `StmtKind::If`.
- These are the only producers; every later pass sees ordinary `Return`/`Match`
  nodes. Do not add a separate AST node without a design for value-carrying
  block scopes (general block expressions are deferred in D-017).

`break`/`continue` and flat `else if` (`StmtKind::If { else_if }`) are
implemented.
