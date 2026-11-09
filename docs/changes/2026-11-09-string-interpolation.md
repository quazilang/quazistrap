# Every string literal interpolates (D-020)

Audience: language users and compiler developers.

The maintainers decided (D-020, В7Б) that every ordinary string literal is a
format string, Rust-style, without a prefix.

- `"hello {name}, {x:X}"` interpolates; the literal is an owned `String`
  produced through `fmt.format` (analysis types it `String` and checks the
  placeholder expressions; codegen lowers it like a format call).
- `{{` and `}}` are literal braces. A literal without placeholders stays a
  `str`; analysis records its brace-unescaped text, which codegen emits.
- An unmatched `{`/`}` or an argument-less `{}`/`{:spec}` outside a format
  call is `S01`.
- The first literal of a format call (`@format`/`str_variadic`: `println`,
  `format`, `Stdout.write`, ...) is still that call's template: `{}` and
  `{:spec}` consume the explicit arguments. Analysis marks these template
  spans for free, module and method calls.
- Raw backtick strings never interpolate: the lexer stores their braces
  escaped, so they read back verbatim.
- A single non-literal text argument of a format function is printed
  verbatim instead of being parsed for braces at run time.

Migration: the prelude formatter and `std.json` used bare `"{"`, `"}"` and
`"{}"` literals; they now write `"{{"`, `"}}"` and `"{{}}"`. Example outputs
are unchanged.

Not done: interpolated literals are not visible to the borrow and retention
passes as expressions (the same limitation format-call placeholders already
had); Tree-sitter highlights literals as plain strings.

Verification: `scripts/check_string_interpolation.py` (interpolation with
specs, escapes, raw strings, templates, verbatim values, native and
source-removed QZI, three diagnostics); classifier and lexer unit tests; all
Rust tests, e2e scripts, the std suite and every example (outputs compared
with the previous compiler).
