# Numeric literals, float printing and IEEE edge cases

Audience: language users.

- **Literals.** The lexer now accepts the documented `0xFF` hexadecimal
  integers and `6.022e23` / `1.5e-10` / `2E+3` exponent floats; they previously
  split into a number and an identifier (`E02`).
- **Printing.** `f64` values with magnitude `>= 1e18` printed as
  `9223372036854775809` because the integer part overflowed an i64 conversion.
  They now use scientific notation (`1e+300`, `6.02e+23` with `{:.2}`).
- **Prelude.** `f32`/`f64` `min`/`max` follow IEEE minNum/maxNum (previously
  `max(1, NaN)` was NaN but `min(NaN, 1)` was 1); `abs(-0.0)` is `+0.0`.
- **std.math.** `asin(NaN)` was 0, `atan2(-0, -1)` was `+pi`,
  `atan2(inf, inf)` and `hypot(inf, NaN)` were NaN, `pow(1, NaN)` was NaN,
  `atan(-0)` and `sqrt(-0)` lost the sign; all now follow C99 Annex F.
  Angle reduction uses a three-part 2*pi, keeping `sin(1e6)` within `1e-9`.

Verification: lexer unit test; `scripts/check_float_format.py` (native and
QZI); std `math` tests for every special value and large-argument accuracy.
