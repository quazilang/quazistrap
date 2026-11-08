# D-017: tail expressions and `if` expressions

Audience: language users and compiler developers.

Status: assessed 2026-11-08 from the proposal in the workspace `idea.txt`;
the maintainer then asked for the recommendation to be implemented.
Recommendation: **ADOPT WITH MODIFICATIONS** — implicit tail values (option B)
and value-producing `if` (option C) now; general block expressions (option D)
deferred. Implementation: [change record](../changes/2026-11-08-tail-and-if-expressions.md).

## 1. Current compiler findings

- Blocks are statement lists only: `Block { stmts, span }` in
  `src/parser/ast.rs`; `parse_block` (`src/parser/mod.rs`) parses statements
  until `}`, and `parse_expr_stmt` always requires `;`. A final expression
  without `;` was a parse error, so giving it a meaning breaks no valid program.
- `if` exists only as `StmtKind::If` (`parse_if_stmt`). Its condition follows
  `if` statement truthiness: `bool`, or an integer tested against zero.
- `match` is already an expression (`ExprKind::Match`, `parse_match_expr`)
  whose arms are single expressions. Type checking unifies arm types, treats
  `!` arms as compatible with any type, and checks exhaustiveness. Boolean
  literal patterns use the same zero/nonzero truthiness as `if`. Codegen,
  initialization, control-flow, retention and borrow passes all handle match
  results, including owned values (`match flag { true => String.from("a"), _ => ... }`
  returns an owned `String` correctly today).
- `ret` is `StmtKind::Return`; every analysis pass and the backend understand
  it, including scope-exit cleanup of owners on early returns.
- `qz fmt` is text based (`analysis::format_quazi_source`), so no pass prints
  the AST back to source.

## 2. Benefits

- Removes ceremony at the end of functions (`a + b` instead of `ret a + b;`)
  while keeping `ret` for early exits.
- Removes the `match`/`if` asymmetry: `const r = if (x < 0) { -x } else { x };`
  no longer needs a mutable temporary or a two-arm boolean `match`.
- Leaves the meaning of every existing program unchanged.

## 3. Drawbacks and edge cases

- **Semicolon sensitivity.** `a + b` and `a + b;` differ at the end of a
  value-returning function. Mitigation: only the final expression of a function
  body (recursively through its tail `if`/`else` and `unsafe` blocks) may omit
  `;`. Anywhere else a missing `;` stays an error, so a stray omission cannot
  silently change behavior.
- **`if` without `else`.** It cannot produce a value; a value-position `if`
  without `else`, or a tail `if` without `else` whose branch has a tail value,
  is rejected with a direct message.
- **Branch typing.** Branches of an `if` expression must be compatible exactly
  like `match` arms; a branch of type `!` (`panic(...)`) is compatible with any
  type. A `void` branch cannot be the value of a non-void expression.
- **Ownership and destruction.** A tail value is returned exactly like
  `ret value;`; an `if` expression moves its selected branch value exactly like
  a `match` arm. Neither introduces a new move, borrow, escape or destruction
  path, so the current lexical checks and the planned D-014 analysis apply
  unchanged. General block expressions would introduce new ones (locals that
  end inside an expression while a value escapes it), which is why they are
  deferred.
- **Diagnostics** for a value `if` mention its branches through the existing
  match machinery; this is acceptable for the first version.

## 4. Implementation feasibility

Options B and C are syntax over constructs every pass already supports, so the
parser can produce the existing AST directly:

- A function-tail expression becomes `StmtKind::Return(Some(expr))` in a
  function with a value return type, or an expression statement in a `void` or
  `!` function. Tail `if`/`else` branches and tail `unsafe` blocks are
  processed recursively, so their branch tails become returns.
- A value-position `if (c) { a } else { b }` becomes
  `match c { true => a, _ => b }`; `else if` nests another match. The condition
  keeps `if` truthiness because boolean literal patterns share it.

No AST, semantic, QZI or codegen change is required, and QZI/QZC formats are
unchanged. Option D needs a new expression kind, block-scoped locals inside
expressions, value-carrying scope exits in initialization/control-flow/retention/
borrow passes and codegen, and cleanup of locals declared in the block while its
result escapes. That is a separate, larger change.

## 5. Recommended semantics (implemented)

1. The final element of a function body may be an expression without `;`.
   In a function whose declared return type is not `void` or `!`, it is the
   return value, equivalent to `ret expr;`. In a `void` or `!` function it is
   evaluated as an expression statement.
2. Tail position extends recursively into the branches of a final
   `if`/`else if`/`else` statement and the body of a final `unsafe` block.
   A tail `if` without `else` cannot supply a value in a value-returning function.
3. Everywhere else an expression statement still requires `;`.
4. `if (cond) { a } else { b }` is an expression wherever an expression is
   allowed (`const`, `var`, arguments, `ret`, match arms, operands). Each branch
   contains exactly one expression with no `;`; `else` is mandatory; `else if`
   chains are allowed. The condition follows `if` statement truthiness.
5. Branch values unify like `match` arms; `!` branches diverge.
6. `ret` is unchanged and remains the only way to return early. Blocks other
   than the ones in rule 2 do not produce values; `{ ... }` is not an
   expression (option D deferred).

## 6. Implementation phases and regression strategy

1. Parser tail handling and `if` expressions (this change) with parser unit
   tests for every rule above, including the error cases.
2. End-to-end execution tests: tail values, nested tail `if`, owned `String`
   results, `!` branches, `void` tails, early `ret` plus tail (the `clamp` case),
   in native and source-removed QZI builds.
3. Tree-sitter grammar and corpus update, plus documentation and tutorial.
4. Later, separately: general block expressions after D-014 value-scope work,
   then possibly statement-bearing `if`-expression branches.

## 7. Final recommendation

**ADOPT WITH MODIFICATIONS.** Option A keeps an avoidable inconsistency between
`if` and `match`. Option B alone helps only straight-line tails. Option C (B
plus value `if`) gives most of the benefit with no new semantic machinery,
because it is defined as `ret` and `match`, constructs whose ownership,
cleanup and typing are already implemented and tested. Option D does not fit
yet: it needs value-carrying block scopes and ownership work that is still open
in D-014. `ret` remains mandatory for early exits; keeping exactly one early-exit
form preserves readable control flow and the existing cleanup model.
