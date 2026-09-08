# Numeric cast boundary

Date: 2026-09-09

Audience: Quazi users and language implementers.

The documentation now records the implemented `as` boundary accurately.
Integer-to-float and float-to-integer casts are rejected with S06. The bytecode
lowering currently has no cross-family conversion instruction, so accepting
those casts would silently preserve the source bit pattern rather than perform
the documented numeric conversion.

Use correctly typed literals, parsing, or an API that performs the desired
conversion until dedicated cross-family lowering is implemented.

Compatibility: documentation clarification only; the compiler has always
rejected these casts.

Verification:

```bash
cargo test --offline numeric_casts_do_not_cross_integer_and_float_families
```
