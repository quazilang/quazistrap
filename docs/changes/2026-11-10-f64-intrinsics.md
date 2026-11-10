# f64 square-root and bit-pattern intrinsics

Audience: standard-library and compiler developers.

`std.math` was written without access to the hardware square root or to the
IEEE-754 bits of a value. Its Newton iteration started from the input
itself, so `sqrt(4e-300)` and `sqrt(5e-324)` were wrong by many orders of
magnitude, and the other routines could not reduce their arguments exactly.

Three intrinsics (QZI intrinsic ids 48-50) are now available:

- `quazi.f64.sqrt(x: f64) f64` emits SSE2 `sqrtsd` (correctly rounded, NaN
  below zero, `-0.0` preserved);
- `quazi.f64.to_bits(x: f64) u64` and `quazi.f64.from_bits(bits: u64) f64`
  reinterpret the register, which already holds the IEEE-754 bits.

QZI validation accepts ids up to 50. The standard library uses them for
`sqrt`, `cbrt`, `trunc`/`floor`/`ceil`/`round`, the Cody-Waite reductions of
`sin`/`cos`/`tan` and the `2^k` scaling of `exp`; see std
`docs/MATH_ACCURACY.md` for the measured bounds (for example `sin` and `cos`
improved from about 5e8 ULP worst case to 1 ULP).
