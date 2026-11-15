# Module constants; no implicit numeric conversions

Date: 2026-11-15

Audience: Quazi users.

## Module-level constants (В20А)

`[pub] const NAME: T = constant expression;` is now valid at the top of a
module (numbers, `bool`, `str`). Uses are replaced by the value; other modules
use `module.NAME` or `import module.NAME`; `qz doc` lists them under
Constants; QZI libraries export literal-valued `pub const`.

## Implicit numeric conversions removed

Assignments, arguments and returns accepted any integer for any integer type,
`bool` for integers and back, and integers for floats, without converting
the value: `var y: u8 = some_i64` kept 64-bit contents in a `u8`, and
`var f: f64 = some_i32` reinterpreted integer bits as a float. Only lossless
widening (`i32` to `i64`, `u8` to `u32`, `f32` to `f64`) stays implicit; the
rest needs `as` (S01 names the fix). Inference gaps that made literal code
look mistyped were fixed with it: `0..n` counts in the type of `n`,
`0.0 - x` has the float type of `x`, slice bounds accept any integer, and
`if`/`match` branches of literals take the expected type.

Migration: add `as T` where the compiler asks for it.
