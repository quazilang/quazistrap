# D-022: Checked integer arithmetic

Status: accepted 2026-11-15 (maintainer request: "`i32::MAX + 1` silently
wraps to -2147483648").

## Decision

Integer `+`, `-`, `*`, unary `-`, `/` and `%` are checked, like `**`
([D-016](power-operator.md)) and like Rust's debug builds:

- A result outside the operand type panics: `attempt to add with overflow`,
  `attempt to subtract with overflow`, `attempt to multiply with overflow`,
  `attempt to negate with overflow`, `attempt to divide with overflow`,
  `attempt to calculate the remainder with overflow` (`MIN / -1`,
  `MIN % -1`). Compound assignment and `++`/`--` follow the same rules.
- The same operation on constants is a compile error (S06, "this arithmetic
  operation will overflow"), and an integer literal must fit the type it
  initializes (`var y: u8 = 300;` is S06).
- Expressions made only of literals are computed exactly (`1 << 40` is
  2^40); a literal operand takes the type of the other operand (`1 + x`).
- Wrapping is explicit: every integer type has `wrapping_add`,
  `wrapping_sub`, `wrapping_mul` and `wrapping_neg`. Shifts keep the low bits
  of their type and `as` truncates (D-021), neither panics.

## Implementation

Narrow types compute the exact result in the 64-bit slot and compare it with
the value re-extended from the type width. 64-bit types use sign tests (signed
`+`/`-`), unsigned comparisons, and the hardware overflow flag of `imul`/`mul`
(compiler-private intrinsics 64/65). `wrapping_*` use intrinsics 66-68.

## Consequences

Code that relied on silent wrapping (hashes, generators, carry detection
by comparison) must use `wrapping_*`; std was migrated
(`Duration.checked_add`, `random.range`, Map/Set hashing). Each checked
operation costs a compare and branch; the scratch registers of the check are
released immediately.
