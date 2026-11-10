# `std.math`

Audience: Quazi application developers needing dependency-free scalar math.

`std.math` is implemented in Quazi and does not import libc or libm. Its
floating-point functions operate on `f64`; trigonometric arguments and results
use radians. `sqrt` is correctly rounded (`sqrtsd`) and `floor`/`ceil`/
`round`/`trunc` are exact; the other functions are fdlibm-style
approximations with measured bounds of 1 to 5 ULP over their whole finite
range (std `docs/MATH_ACCURACY.md`, enforced by
`scripts/check_math_accuracy.py`).

## Integer combinatorics

- `gcd(a, b)` returns a non-negative `i64` greatest common divisor. It panics
  when the mathematical result cannot fit in `i64` (notably the magnitude of
  `i64::MIN`).
- `lcm(a, b)` returns zero if either operand is zero and panics when the
  result does not fit `i64`.
- `factorial(n)`, `permutations(n, r)`, and `combinations(n, r)` return `u64`
  and panic when the result does not fit, like integer `**` (D-016).
  `permutations` and `combinations` return zero when `r > n`; `combinations`
  avoids intermediate overflow, so every result that fits is exact.

## Basic floating-point operations

`abs`, `min`, `max`, `clamp`, `trunc`, `floor`, `ceil`, `round`, `fract`, and
`sign` operate on `f64`. `min` and `max` follow IEEE minNum/maxNum: a NaN
operand yields the other operand. `abs(-0.0)` is `+0.0`. `round` rounds half
values away from zero. `lerp` does
not constrain its interpolation amount; `map_range` divides by
`from_high - from_low`, so a zero-width source range follows IEEE division
behavior. `radians` and `degrees` convert angular units.

## Roots, trigonometry, and logarithms

`sqrt` returns NaN for negative inputs and keeps the sign of zero; `cbrt`
preserves a negative sign; `hypot` scales its inputs before squaring to reduce
avoidable overflow and returns infinity if either input is infinite, even when
the other is NaN.

`sin`, `cos`, `tan`, `atan`/`arctan`, `atan2`/`arctan2`, `asin`, and `acos`
use fdlibm kernels; `asin`
returns NaN outside `[-1, 1]` and for NaN; `acos` follows from it. `atan2`
follows C99 Annex F for signed zeros and infinities (`atan2(-0.0, -1.0)` is
`-pi`, `atan2(inf, inf)` is `pi/4`). Arguments are reduced by pi/2 with a
three-part Cody-Waite reduction below about 1.6e6 and Payne-Hanek reduction
above, so `sin`/`cos`/`tan` stay within 1 ULP for every finite input; `tan`
grows without a special pole error.

`exp`, `ln`, `log2`, and `log10` provide IEEE-style NaN/infinite boundary
handling where implemented: `exp` saturates to infinity/zero beyond its
documented reduction thresholds, and `ln(0)` is negative infinity while
negative inputs produce NaN. `sinh`, `cosh`, and `tanh` are derived from
`exp` (series below 1/2, finite up to |x| = 710.47); `tanh` clamps beyond ±20.
`pow` carries `y * ln(x)` in double-double precision (3 ULP), supports
integral exponents for negative bases and returns NaN for a negative base with
a non-integral exponent;
`pow(x, 0)` and `pow(1, y)` are 1 even for NaN operands.

```quazi
import std.math;

fn main() i32 {
    if (math.gcd(84, 30) != 6) { ret 1; }
    if (math.combinations(5, 2) != 10) { ret 2; }
    if (math.sqrt(81.0) != 9.0) { ret 3; }
    ret 0;
}
```

Results are not correctly rounded except `sqrt` and the rounding functions;
for strict reproducibility, check the documented bounds against the
application's requirements.
