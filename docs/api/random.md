# `std.random`

Audience: Quazi application developers who need operating-system-backed random
values.

`std.random` obtains entropy from target-native CSPRNG facilities through
compiler intrinsics. It does not seed or expose a deterministic pseudo-random
generator. Every entropy-dependent API returns `RandomError.Unavailable` if
secure system entropy cannot be obtained.

## Scalar values

`next_u64`, `next_u32`, `next_u8`, `next_i64`, `next_i32`, and `boolean`
return uniformly sampled values of their stated scalar types. `float()` returns
an `f64` in `[0.0, 1.0)` using 53 random mantissa bits. `chance(p)` compares
that distribution to `p`; it rejects values below 0 or above 1 with
`InvalidProbability`, and returns deterministically for exactly 0 and 1.

## Ranges

`range(bounds)` accepts the prelude’s exclusive `a..b` and inclusive `a..=b`
integer ranges and returns a uniform `i64` in the requested interval. Empty or
reversed ranges produce `InvalidRange`. It uses rejection sampling rather than
`value % width` alone, so non-power-of-two widths are not modulo-biased. The
full inclusive `i64` domain is supported by its wrapping-width case.

## Bytes and collections

`random_bytes(count)` returns a newly allocated `Array[u8]`, filling all
requested bytes from secure random words.

`choose[T](items)` consumes its `Array[T]` and returns an element or
`EmptyCollection`. `shuffle[T](items)` consumes and returns an `Array[T]` after
an in-place Fisher–Yates permutation. These generic collection APIs are
currently suitable only for plain-copy element types: recursive element
destruction and borrowed element access are not implemented yet. Do not use
them with owned aggregate elements, and do not retain aliases to an array after
passing it to either function.

`RandomError.message()` supplies a display string for `Unavailable`,
`InvalidRange`, `EmptyCollection`, and `InvalidProbability`; branch on the
enum rather than parsing that message.

```quazi
import std.random;

fn main() i32 {
    if (random.chance(0.0).unwrap()) { ret 1; }
    var data: Array[u8] = random.random_bytes(16).unwrap();
    if (data.len() != 16) { ret 2; }
    ret 0;
}
```

This module is for random sampling, not an application-wide reproducible test
RNG. Tests needing repeatability should inject fixed values rather than relying
on system entropy.
