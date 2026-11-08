# Tail values and `if` expressions

Audience: language users and compiler developers.

Implements [D-017](../decisions/expression-oriented-blocks.md):

- A function body's final expression may omit `;`. In a function returning a
  value it is the result (`fn add(a: i32, b: i32) i32 { a + b }`); in a `void`
  or `!` function it is evaluated and discarded. Tail position extends into the
  branches of a final `if`/`else if`/`else` statement and a final `unsafe` block.
- `if (c) { a } else { b }` is an expression wherever expressions are allowed.
  `else` is required, `else if` chains work, and each branch holds one
  expression.
- `ret` is unchanged and remains the only early exit.

See [statements](../language/statements.md#tail-values) and
[expressions](../language/expressions.md#if-expressions).

## Implementation

The parser produces existing AST forms: a value tail becomes `ret expr;` and a
value `if` becomes `match cond { true => a, _ => b }`. No semantic, QZI or
codegen change is needed, so ownership, cleanup, typing and lowering are those
already implemented for `ret` and `match`. QZI14 and QZC32 are unchanged; the
parse result of every previously valid program is unchanged, because a missing
`;` was always a parse error.

Misplaced tails report `E02: expected ;, found } (only the final expression of
a function body may omit ;)`. A value `if` without `else`, a tail `if` without
`else` in a value-returning function, and a branch containing statements also
report `E02` with a specific message. Mismatched branch types report `S01`
(`match arm type mismatch`).

## Compatibility

Source compatible: no previously accepted program changes meaning. Tools that
parse Quazi independently (Tree-sitter grammar) need the new forms.

## Verification

- `cargo test --offline -q` passes 999 tests; parser tests cover tail conversion by return type,
  tail `if`/`unsafe` branches, the `match` shape of `if` expressions and every
  rejected form.
- `python3 scripts/check_tail_expressions.py --qz target/debug/qz`: 7 invalid
  programs rejected by check/native/QZI/COFF; 11 value checks (tails, nested
  tail `if`, `else if`, `clamp`-style early `ret`, integer conditions, `void`
  tail through `&i32!`) pass natively, from source-removed QZI, and emit
  Windows COFF; owned `String` branch results and a `!` (`panic`) branch run
  natively and from QZI.
- Runnable tutorial fences in chapters 3 and 4 analyze and lower.
- Every example binary still builds; all compiler e2e scripts and the `std`
  test suite pass. Tree-sitter commit `47b51ce` parses the new forms.
