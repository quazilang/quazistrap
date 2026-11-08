# Integer literals above `i64::MAX` fold and divide as unsigned

Audience: language users and compiler developers.

Every integer literal was typed `i32` before contextual coercion, so a literal
that only fits `u64` (above 9223372036854775807) entered constant folding and
code generation as a negative signed value:

| Expression in a `u64` context | Before | Now |
| --- | --- | --- |
| `18446744073709551615 / 60` | `0` | `307445734561825860` |
| `18446744073709551614 / 2` | `18446744073709551615` | `9223372036854775807` |
| `9223372036854775808 / 2` | `13835058055282163712` | `4611686018427387904` |

Such literals are now typed `u64`, so folding and division, remainder,
comparison and right shift use unsigned semantics. Constant `+`, `-` and `*`
folding wraps like the runtime instructions instead of overflowing inside the
compiler. Smaller literals keep their previous typing; general numeric literal
inference remains open.

QZC33 invalidates cached chunks compiled with the old folding.

Verification: a semantic regression checks the folded values; all compiler
e2e scripts (including arithmetic and power), every example and the `std` test
suite pass. `std.time` relies on `u64::MAX / 60` overflow guards.
