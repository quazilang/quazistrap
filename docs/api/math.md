# `std.math`

Audience: Quazi application developers needing dependency-free scalar math.

`std.math` is implemented in Quazi and does not import libc or libm. Its
floating-point functions operate on `f64`; trigonometric arguments and results
use radians. The implementation uses finite iteration approximations and is a
portable convenience layer, not a correctly rounded numerical library.

## Integer combinatorics

- `gcd(a, b)` returns a non-negative `i64` greatest common divisor. It panics
  when the mathematical result cannot fit in `i64` (notably the magnitude of
  `i64::MIN`).
- `lcm(a, b)` returns zero if either operand is zero. Its arithmetic can
  overflow; callers requiring checked combinatorics must bound their inputs.
- `factorial(n)`, `permutations(n, r)`, and `combinations(n, r)` return `u64`.
  `permutations` and `combinations` return zero when `r > n`; none of these
  functions report overflow.

## Basic floating-point operations

`abs`, `min`, `max`, `clamp`, `trunc`, `floor`, `ceil`, `round`, `fract`, and
`sign` operate on `f64`. `round` rounds half values away from zero. `lerp` does
not constrain its interpolation amount; `map_range` divides by
`from_high - from_low`, so a zero-width source range follows IEEE division
behavior. `radians` and `degrees` convert angular units.

## Roots, trigonometry, and logarithms

`sqrt` returns NaN for negative inputs; `cbrt` preserves a negative sign;
`hypot` scales its inputs before squaring to reduce avoidable overflow.

`sin`, `cos`, `tan`, `atan`/`arctan`, `atan2`/`arctan2`, `asin`, and `acos`
use finite polynomial or series approximations after range reduction. `asin`
returns NaN outside `[-1, 1]`; `acos` follows from it. Very large finite angle
inputs can lose precision during reduction, and `tan` grows without a special
pole error.

`exp`, `ln`, `log2`, and `log10` provide IEEE-style NaN/infinite boundary
handling where implemented: `exp` saturates to infinity/zero beyond its
documented reduction thresholds, and `ln(0)` is negative infinity while
negative inputs produce NaN. `sinh`, `cosh`, and `tanh` are derived from
`exp`; `tanh` clamps beyond ±20. `pow` supports integral exponents for negative
bases and returns NaN for a negative base with a non-integral exponent.

```quazi
import std.math;

fn main() i32 {
    if (math.gcd(84, 30) != 6) { ret 1; }
    if (math.combinations(5, 2) != 10) { ret 2; }
    if (math.sqrt(81.0) != 9.0) { ret 3; }
    ret 0;
}
```

For financial, safety-critical, or strict cross-platform reproducibility work,
validate the approximation and overflow behavior against the application’s
required error bounds before relying on this module.
