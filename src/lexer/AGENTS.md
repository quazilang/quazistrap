# Lexer (`src/lexer/`)

## Key Behaviours

- `&&` / `||` are **synthesized by the parser** via `match_and_and()` / `match_or_or()`, not emitted as distinct lexer tokens.
- Generics use `[T]` square brackets (not `<T>`).
- `TokenKind::While` exists in the lexer but is **not handled** by the parser — `for (cond) {}` is the only while-like loop syntax.

## Token Conventions

- `Ampersand` (`&`) is emitted for both prefix (reference) and potential infix (bitwise AND) positions.
- `Pipe` (`|`) is emitted for both primary-position (closure start) and potential infix (bitwise OR) positions.
- `Caret` (`^`) and shift tokens (`<<`, `>>`) are **not yet emitted** — bitwise operators are a P1 roadmap item.
