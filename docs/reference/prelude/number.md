# `prelude.number`

Primitive numeric convenience methods implemented in Quazi.

## Contents

- [`i32`](#i32) (methods)
- [`i64`](#i64) (methods)
- [`isize`](#isize) (methods)
- [`u32`](#u32) (methods)
- [`u64`](#u64) (methods)
- [`usize`](#usize) (methods)
- [`i8`](#i8) (methods)
- [`i16`](#i16) (methods)
- [`u8`](#u8) (methods)
- [`u16`](#u16) (methods)
- [`f32`](#f32) (methods)
- [`f64`](#f64) (methods)

## `i32`

### `i32.wrapping_add`

```quazi
pub fn wrapping_add(self: i32, other: i32) i32
```

Arithmetic that wraps around at the bounds of `i32` instead of
panicking on overflow (hashes, random number generators, checksums).

### `i32.wrapping_sub`

```quazi
pub fn wrapping_sub(self: i32, other: i32) i32
```

`self - other`, wrapping around at the bounds of the type.

### `i32.wrapping_mul`

```quazi
pub fn wrapping_mul(self: i32, other: i32) i32
```

`self * other`, wrapping around at the bounds of the type.

### `i32.wrapping_neg`

```quazi
pub fn wrapping_neg(self: i32) i32
```

`-self`, wrapping around at the bounds of the type (`MIN` stays `MIN`).

### `i32.abs`

```quazi
pub fn abs(self: i32) i32
```

The absolute value; panics for `MIN`, whose absolute value does not fit.

### `i32.pow`

```quazi
pub fn pow(self: i32, exponent: u32) i32
```

`self` raised to `exponent`; panics when the result overflows the type.

### `i32.gcd`

```quazi
pub fn gcd(self: i32, other: i32) i32
```

The greatest common divisor (non-negative); `gcd(0, 0)` is 0. Panics when
the result does not fit the type.

### `i32.lcm`

```quazi
pub fn lcm(self: i32, other: i32) i32
```

The least common multiple; 0 when either operand is 0. Panics when the
result does not fit the type.

### `i32.is_even`

```quazi
pub fn is_even(self: i32) bool
```

Whether the value is divisible by 2.

### `i32.is_odd`

```quazi
pub fn is_odd(self: i32) bool
```

Whether the value is not divisible by 2.

## `i64`

### `i64.wrapping_add`

```quazi
pub fn wrapping_add(self: i64, other: i64) i64
```

Arithmetic that wraps around at the bounds of `i64` instead of
panicking on overflow (hashes, random number generators, checksums).

### `i64.wrapping_sub`

```quazi
pub fn wrapping_sub(self: i64, other: i64) i64
```

`self - other`, wrapping around at the bounds of the type.

### `i64.wrapping_mul`

```quazi
pub fn wrapping_mul(self: i64, other: i64) i64
```

`self * other`, wrapping around at the bounds of the type.

### `i64.wrapping_neg`

```quazi
pub fn wrapping_neg(self: i64) i64
```

`-self`, wrapping around at the bounds of the type (`MIN` stays `MIN`).

### `i64.abs`

```quazi
pub fn abs(self: i64) i64
```

The absolute value; panics for `MIN`, whose absolute value does not fit.

### `i64.pow`

```quazi
pub fn pow(self: i64, exponent: u32) i64
```

`self` raised to `exponent`; panics when the result overflows the type.

### `i64.gcd`

```quazi
pub fn gcd(self: i64, other: i64) i64
```

The greatest common divisor (non-negative); `gcd(0, 0)` is 0. Panics when
the result does not fit the type.

### `i64.lcm`

```quazi
pub fn lcm(self: i64, other: i64) i64
```

The least common multiple; 0 when either operand is 0. Panics when the
result does not fit the type.

### `i64.is_even`

```quazi
pub fn is_even(self: i64) bool
```

Whether the value is divisible by 2.

### `i64.is_odd`

```quazi
pub fn is_odd(self: i64) bool
```

Whether the value is not divisible by 2.

## `isize`

### `isize.wrapping_add`

```quazi
pub fn wrapping_add(self: isize, other: isize) isize
```

Arithmetic that wraps around at the bounds of `isize` instead of
panicking on overflow (hashes, random number generators, checksums).

### `isize.wrapping_sub`

```quazi
pub fn wrapping_sub(self: isize, other: isize) isize
```

`self - other`, wrapping around at the bounds of the type.

### `isize.wrapping_mul`

```quazi
pub fn wrapping_mul(self: isize, other: isize) isize
```

`self * other`, wrapping around at the bounds of the type.

### `isize.wrapping_neg`

```quazi
pub fn wrapping_neg(self: isize) isize
```

`-self`, wrapping around at the bounds of the type (`MIN` stays `MIN`).

### `isize.abs`

```quazi
pub fn abs(self: isize) isize
```

The absolute value; panics for `MIN`, whose absolute value does not fit.

### `isize.pow`

```quazi
pub fn pow(self: isize, exponent: u32) isize
```

`self` raised to `exponent`; panics when the result overflows the type.

### `isize.gcd`

```quazi
pub fn gcd(self: isize, other: isize) isize
```

The greatest common divisor (non-negative); `gcd(0, 0)` is 0. Panics when
the result does not fit the type.

### `isize.lcm`

```quazi
pub fn lcm(self: isize, other: isize) isize
```

The least common multiple; 0 when either operand is 0. Panics when the
result does not fit the type.

### `isize.is_even`

```quazi
pub fn is_even(self: isize) bool
```

Whether the value is divisible by 2.

### `isize.is_odd`

```quazi
pub fn is_odd(self: isize) bool
```

Whether the value is not divisible by 2.

## `u32`

### `u32.wrapping_add`

```quazi
pub fn wrapping_add(self: u32, other: u32) u32
```

Arithmetic that wraps around at the bounds of `u32` instead of
panicking on overflow (hashes, random number generators, checksums).

### `u32.wrapping_sub`

```quazi
pub fn wrapping_sub(self: u32, other: u32) u32
```

`self - other`, wrapping around at the bounds of the type.

### `u32.wrapping_mul`

```quazi
pub fn wrapping_mul(self: u32, other: u32) u32
```

`self * other`, wrapping around at the bounds of the type.

### `u32.wrapping_neg`

```quazi
pub fn wrapping_neg(self: u32) u32
```

`-self`, wrapping around at the bounds of the type (`MIN` stays `MIN`).

### `u32.pow`

```quazi
pub fn pow(self: u32, exponent: u32) u32
```

`self` raised to `exponent`; panics when the result overflows the type.

### `u32.gcd`

```quazi
pub fn gcd(self: u32, other: u32) u32
```

The greatest common divisor (non-negative); `gcd(0, 0)` is 0. Panics when
the result does not fit the type.

### `u32.lcm`

```quazi
pub fn lcm(self: u32, other: u32) u32
```

The least common multiple; 0 when either operand is 0. Panics when the
result does not fit the type.

### `u32.is_even`

```quazi
pub fn is_even(self: u32) bool
```

Whether the value is divisible by 2.

### `u32.is_odd`

```quazi
pub fn is_odd(self: u32) bool
```

Whether the value is not divisible by 2.

## `u64`

### `u64.wrapping_add`

```quazi
pub fn wrapping_add(self: u64, other: u64) u64
```

Arithmetic that wraps around at the bounds of `u64` instead of
panicking on overflow (hashes, random number generators, checksums).

### `u64.wrapping_sub`

```quazi
pub fn wrapping_sub(self: u64, other: u64) u64
```

`self - other`, wrapping around at the bounds of the type.

### `u64.wrapping_mul`

```quazi
pub fn wrapping_mul(self: u64, other: u64) u64
```

`self * other`, wrapping around at the bounds of the type.

### `u64.wrapping_neg`

```quazi
pub fn wrapping_neg(self: u64) u64
```

`-self`, wrapping around at the bounds of the type (`MIN` stays `MIN`).

### `u64.pow`

```quazi
pub fn pow(self: u64, exponent: u32) u64
```

`self` raised to `exponent`; panics when the result overflows the type.

### `u64.gcd`

```quazi
pub fn gcd(self: u64, other: u64) u64
```

The greatest common divisor (non-negative); `gcd(0, 0)` is 0. Panics when
the result does not fit the type.

### `u64.lcm`

```quazi
pub fn lcm(self: u64, other: u64) u64
```

The least common multiple; 0 when either operand is 0. Panics when the
result does not fit the type.

### `u64.is_even`

```quazi
pub fn is_even(self: u64) bool
```

Whether the value is divisible by 2.

### `u64.is_odd`

```quazi
pub fn is_odd(self: u64) bool
```

Whether the value is not divisible by 2.

## `usize`

### `usize.wrapping_add`

```quazi
pub fn wrapping_add(self: usize, other: usize) usize
```

Arithmetic that wraps around at the bounds of `usize` instead of
panicking on overflow (hashes, random number generators, checksums).

### `usize.wrapping_sub`

```quazi
pub fn wrapping_sub(self: usize, other: usize) usize
```

`self - other`, wrapping around at the bounds of the type.

### `usize.wrapping_mul`

```quazi
pub fn wrapping_mul(self: usize, other: usize) usize
```

`self * other`, wrapping around at the bounds of the type.

### `usize.wrapping_neg`

```quazi
pub fn wrapping_neg(self: usize) usize
```

`-self`, wrapping around at the bounds of the type (`MIN` stays `MIN`).

### `usize.pow`

```quazi
pub fn pow(self: usize, exponent: u32) usize
```

`self` raised to `exponent`; panics when the result overflows the type.

### `usize.gcd`

```quazi
pub fn gcd(self: usize, other: usize) usize
```

The greatest common divisor (non-negative); `gcd(0, 0)` is 0. Panics when
the result does not fit the type.

### `usize.lcm`

```quazi
pub fn lcm(self: usize, other: usize) usize
```

The least common multiple; 0 when either operand is 0. Panics when the
result does not fit the type.

### `usize.is_even`

```quazi
pub fn is_even(self: usize) bool
```

Whether the value is divisible by 2.

### `usize.is_odd`

```quazi
pub fn is_odd(self: usize) bool
```

Whether the value is not divisible by 2.

## `i8`

### `i8.wrapping_add`

```quazi
pub fn wrapping_add(self: i8, other: i8) i8
```

Arithmetic that wraps around at the bounds of `i8` instead of
panicking on overflow (hashes, random number generators, checksums).

### `i8.wrapping_sub`

```quazi
pub fn wrapping_sub(self: i8, other: i8) i8
```

`self - other`, wrapping around at the bounds of the type.

### `i8.wrapping_mul`

```quazi
pub fn wrapping_mul(self: i8, other: i8) i8
```

`self * other`, wrapping around at the bounds of the type.

### `i8.wrapping_neg`

```quazi
pub fn wrapping_neg(self: i8) i8
```

`-self`, wrapping around at the bounds of the type (`MIN` stays `MIN`).

## `i16`

### `i16.wrapping_add`

```quazi
pub fn wrapping_add(self: i16, other: i16) i16
```

Arithmetic that wraps around at the bounds of `i16` instead of
panicking on overflow (hashes, random number generators, checksums).

### `i16.wrapping_sub`

```quazi
pub fn wrapping_sub(self: i16, other: i16) i16
```

`self - other`, wrapping around at the bounds of the type.

### `i16.wrapping_mul`

```quazi
pub fn wrapping_mul(self: i16, other: i16) i16
```

`self * other`, wrapping around at the bounds of the type.

### `i16.wrapping_neg`

```quazi
pub fn wrapping_neg(self: i16) i16
```

`-self`, wrapping around at the bounds of the type (`MIN` stays `MIN`).

## `u8`

### `u8.wrapping_add`

```quazi
pub fn wrapping_add(self: u8, other: u8) u8
```

Arithmetic that wraps around at the bounds of `u8` instead of
panicking on overflow (hashes, random number generators, checksums).

### `u8.wrapping_sub`

```quazi
pub fn wrapping_sub(self: u8, other: u8) u8
```

`self - other`, wrapping around at the bounds of the type.

### `u8.wrapping_mul`

```quazi
pub fn wrapping_mul(self: u8, other: u8) u8
```

`self * other`, wrapping around at the bounds of the type.

### `u8.wrapping_neg`

```quazi
pub fn wrapping_neg(self: u8) u8
```

`-self`, wrapping around at the bounds of the type (`MIN` stays `MIN`).

## `u16`

### `u16.wrapping_add`

```quazi
pub fn wrapping_add(self: u16, other: u16) u16
```

Arithmetic that wraps around at the bounds of `u16` instead of
panicking on overflow (hashes, random number generators, checksums).

### `u16.wrapping_sub`

```quazi
pub fn wrapping_sub(self: u16, other: u16) u16
```

`self - other`, wrapping around at the bounds of the type.

### `u16.wrapping_mul`

```quazi
pub fn wrapping_mul(self: u16, other: u16) u16
```

`self * other`, wrapping around at the bounds of the type.

### `u16.wrapping_neg`

```quazi
pub fn wrapping_neg(self: u16) u16
```

`-self`, wrapping around at the bounds of the type (`MIN` stays `MIN`).

## `f32`

### `f32.abs`

```quazi
pub fn abs(self: f32) f32
```

`0.0 - self` also maps -0.0 to +0.0; NaN stays NaN.

### `f32.min`

```quazi
pub fn min(self: f32, other: f32) f32
```

IEEE 754 minNum/maxNum: a NaN operand yields the other operand.

### `f32.max`

```quazi
pub fn max(self: f32, other: f32) f32
```

The larger of the two values (IEEE maxNum: a NaN operand yields the other).

### `f32.clamp`

```quazi
pub fn clamp(self: f32, low: f32, high: f32) f32
```

The value limited to `low..=high`.

## `f64`

### `f64.abs`

```quazi
pub fn abs(self: f64) f64
```

`0.0 - self` also maps -0.0 to +0.0; NaN stays NaN.

### `f64.min`

```quazi
pub fn min(self: f64, other: f64) f64
```

IEEE 754 minNum/maxNum: a NaN operand yields the other operand.

### `f64.max`

```quazi
pub fn max(self: f64, other: f64) f64
```

The larger of the two values (IEEE maxNum: a NaN operand yields the other).

### `f64.clamp`

```quazi
pub fn clamp(self: f64, low: f64, high: f64) f64
```

The value limited to `low..=high`.

### `f64.powi`

```quazi
pub fn powi(self: f64, exponent: i32) f64
```

`self` raised to the integer `exponent` (negative exponents divide).
