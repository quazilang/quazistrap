# Full-width decimal integer literals

Decimal integer literals now preserve every value in the `u64` range. In
particular, the documented `18446744073709551615` literal now reaches a `u64`
value as all one bits instead of being silently replaced with zero during
lexing. Decimal values greater than `u64::MAX` are rejected as lexical errors.

## Why

Integer source tokens previously used a signed parser with a fallback value of
zero. That discarded both large unsigned values and out-of-range input before
semantic analysis could distinguish them. Source integer magnitudes are now
unsigned through parsing and metadata preservation; code generation converts
them to the bytecode slot's raw 64-bit bit pattern only at its explicit ABI
boundary.

## Compatibility and migration

Valid decimal `u64` programs now receive the value written in source. Programs
with a literal above `u64::MAX` now fail instead of compiling as zero. Negative
numbers remain unary negation over the source magnitude, including
`-9223372036854775808`.

## Verification

Compiler regressions cover the full lexer range, rejection above it, exact
fixed-array-length parsing, direct `u64::MAX` code generation, and the signed
minimum boundary. The standard-library time suite exercises the value through
compiled execution.
