# Never-returning calls end a path; carets stay on one line

Audience: language users.

- A function whose last statement calls a `!` function was rejected:
  `fn f() i32 { panic("x"); }` reported `S03 ... does not always return a
  value`. An expression statement of type `!` now ends its path like `ret`.
- Diagnostics whose span covers several lines (S03 on a whole function, or
  any multi-line expression) printed one `^` per character of the span, far
  past the end of the source line. The underline now stops at the end of the
  first line.

Verification: semantic regressions for both; all Rust tests, e2e scripts,
examples and the std suite pass.
