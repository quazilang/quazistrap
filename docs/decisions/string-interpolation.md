# D-020: string interpolation in every literal

Audience: language users and compiler developers.

Status: **decided** by the maintainers on 2026-11-09 (asks.txt round 3,
answers В7Б and В8А).

## Decision

- **В7Б.** Every ordinary string literal is a format string, as in Rust's
  format macros but without a prefix: `"value = {x}"` interpolates `x`.
  Literal braces are written `{{` and `}}` ("мы как в расте {{}} чтобы оно было
  как {}"). No `f"..."` prefix.
- **В8А.** `async`/`await` is removed from the plan; `qo` qoroutines and
  `Channel[T]` ([D-019](qoroutines.md)) replace it.

## Consequences

- A literal containing `{expr}` placeholders produces an owned `String`
  (like `format`). A literal without placeholders stays a `str`; `{{`/`}}` in
  it denote single braces.
- An unmatched `{` or `}` in a literal is an error.
- Raw backtick strings and byte strings do not interpolate.
- Format calls (`println`, `format`, ...) keep their template rules: `{}` and
  `{:spec}` consume explicit arguments.
