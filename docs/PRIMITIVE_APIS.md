# Primitive APIs and portable text output

The primitive convenience API lives in the automatically injected prelude, so
no import is required for strings or numeric methods. `std.math` is imported
explicitly.

## UTF-8 strings

`str` is an immutable borrowed UTF-8 string. `String` owns a writable UTF-8
allocation. `len()` counts Unicode scalar values (`Rune`, an alias of `u32`),
while `bytes_len()` reports the encoded UTF-8 byte length without allocating.

```quazi
const text: str = "Aλ🙂";
const count: usize = text.len();        // 3
const encoded: usize = text.bytes_len(); // 7
const lambda: Rune = text[1];           // U+03BB
const smile: Rune = text[-1];           // U+1F642
const tail: String = text[1:];           // "λ🙂"
const backwards: String = text[::-1];    // "🙂λA"
```

- `get(index)` accepts positive or negative scalar indexes and returns
  `Option[Rune]`; indexing returns `Rune` and panics when out of bounds.
- `text[start:end:step]` follows Python's clamped, end-exclusive slice rules.
  Bounds may be omitted or negative, and the step may be negative. A zero step
  panics. Slicing walks UTF-8 boundaries and never splits an encoded scalar.
- `lowercase`/`to_lowercase`, `uppercase`/`to_uppercase`, and `capitalize`
  currently transform ASCII
  letters and preserve non-ASCII UTF-8 unchanged. This limitation is explicit
  until Unicode case tables are shipped.
- `trim` removes ASCII space, tab, carriage return, and line feed.
- `==`, `!=`, `<`, `<=`, `>`, and `>=` compare string contents
  lexicographically. `starts_with`, `ends_with`, `contains`, and `find` perform
  exact matching. Like Rust, `find` returns a UTF-8 byte offset.

Every method is available on both `str` and owned `String`. Methods returning a
`String` allocate a new value. Whole Program Analysis inserts scope cleanup, so
normal application code does not call `.free()` manually.

## Checked parsing

Parsing never silently substitutes zero:

```quazi
const port: Result[i32, ParseError] = "8080".parse[i32]();
const ratio: Result[f64, ParseError] = "6.25e-1".parse[f64]();
```

`parse[T]()` supports `i8`, `i16`, `i32`, `i64`, `isize`, `u8`, `u16`, `u32`,
`u64`, `usize`, `f32`, and `f64`. It always returns `Result[T, ParseError]`.
Failures are `ParseError.Empty`, `InvalidDigit(byte_offset)`, or `Overflow`.
Unsigned parsing covers the full `u64` range. Floating parsing accepts an
optional sign, decimal fraction, and decimal exponent.

## Numeric methods and `std.math`

Signed `i32`, `i64`, and `isize` provide `abs()` and `pow(u32)`. Unsigned
`u32`, `u64`, and `usize` provide `pow(u32)`. Integer powers use exponentiation
by squaring. Signed minimum-value `abs()` panics rather than wrapping; integer
power currently follows ordinary integer overflow behavior. These integer types
also provide `gcd`, `lcm`, `is_even`, and `is_odd`; unsigned GCD handles the full
64-bit range.

`f32` and `f64` provide `abs`, `min`, `max`, and `clamp`; `f64` additionally
provides `powi(i32)`. `std.math` supplies dependency-free `f64` functions:

Integer helpers include `gcd`, `lcm`, `factorial`, `permutations`, and
`combinations`. Floating helpers include `abs`, `min`, `max`, `clamp`, `trunc`,
`floor`, `ceil`, `round`, `fract`, `sign`, `lerp`, `map_range`, `radians`,
`degrees`, `sqrt`, `cbrt`, `hypot`, `sin`, `cos`, `tan`, `atan`/`arctan`,
`atan2`/`arctan2`, `asin`, `acos`, `sinh`, `cosh`, `tanh`, `exp`, `ln`, `log2`,
`log10`, and `pow`.

Angles use radians. The implementations are lightweight Quazi approximations
and do not pull in libc or libm. They prioritize portability and small binaries,
not correctly rounded scientific computation.

## Division behavior

Floating-point division follows IEEE-754. Division by positive or negative zero
therefore produces signed infinity, while `0.0 / 0.0` produces NaN. These values
remain ordinary `f32` or `f64` values and propagate through later calculations.

Integer division by zero panics with `integer division by zero`. Integer
remainder by zero likewise panics with `integer remainder by zero`. The same
rules apply to `/=`, `%=` and all signed and unsigned integer widths. Panic
diagnostics use the source location of the arithmetic expression. A package
with `std = false` omits the panic runtime and therefore falls back to the target's integer
divide trap.

## Windows Unicode output

`std.io` accepts UTF-8 on every platform. On Windows it distinguishes an actual
console from a redirected handle with `GetConsoleMode`:

- consoles receive UTF-16 through `MultiByteToWideChar(CP_UTF8)` and
  `WriteConsoleW`, independent of the active OEM/ANSI code page;
- files and pipes receive portable UTF-8 bytes through `WriteFile`.

This prevents box-drawing text such as `╭────╮` from becoming `Γò¡ΓöÇ...` while
preserving UTF-8 when output is redirected. See
[`examples/23-standard-library-tour`](../examples/23-standard-library-tour/) for a runnable tour.
