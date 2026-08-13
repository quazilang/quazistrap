# Randomness

`std.random` uses the operating system's cryptographically secure generator:
`BCryptGenRandom` on Windows and `getrandom` on Linux. It requires no libc and
returns `RandomError.Unavailable` instead of substituting a weak seed.

The API includes `next_u64`, `next_u32`, `next_u8`, `next_i64`, `next_i32`,
`boolean`, `float`, `chance`, `range`, `choose`, `shuffle`, and `random_bytes`.
Every operation is fallible. `float` returns a
uniform value in `[0.0, 1.0)`. `chance` accepts probabilities from `0.0` through
`1.0`. `range` accepts exclusive and inclusive prelude ranges:

```quazi
const die: i64 = random.range(1..=6)?;
const index: i64 = random.range(0..players.len())?;
```

Range selection uses rejection sampling to avoid modulo bias and supports the
complete signed 64-bit domain. `choose` rejects an empty `Array[T]`; `shuffle`
uses Fisher-Yates and mutates shared array storage. `random_bytes` returns
`Array[u8]`.

System output is nondeterministic. A separately seeded deterministic game
generator may be added later; it must not be described as secure randomness.
