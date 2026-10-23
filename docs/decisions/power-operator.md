# D-016: typed power operator

Audience: language users and compiler developers.

Accepted by the maintainer on 2026-10-22. The approved target contract is
implemented by the [typed-power checkpoint](../changes/2026-10-23-typed-power.md).
The final clarification supersedes an earlier UI selection requesting truncation
of fractional results.

| Expression | Required behavior |
| --- | --- |
| Integer 2 ** 3 | Integer 8 |
| Integer 2 ** -2 with a compile-time negative exponent | Compile-time error |
| Integer 2 ** n with a runtime negative n | Language panic |
| Float 2.0 ** -2 | Floating 0.25 |
| Integer 2 ** 0 | Integer 1 |

Integer power retains the corresponding integer result type. Use exact integer
exponentiation by squaring and check multiplication overflow against that type;
do not convert to C pow and truncate. Results must remain exact while they fit.
Runtime overflow must not silently wrap. Known invalid constant expressions
should be diagnosed before executable emission.

An expression's type must not depend on the runtime sign of its exponent.
A floating exponent may use a separate f64-result overload. Floating-base power
supports negative exponents. Floating conversion/lowering must preserve the
operand representation rather than reinterpret integer/floating slot bits.

The previous Opcode::Pow backend did not implement this contract: it always
performed signed-integer-to-double conversion, called external pow, then truncated
the result to an integer. It lost precision and mishandled unsigned/floating
operands. General bytecode/artifact compatibility and runtime panic dependencies
are covered by source, native, source-removed QZI and target-object regressions.
See [the implemented contract](../internals/power.md) for approximation and generic
inference limits; ownership/effect certification remains separate.
