# D-016: typed power operator

Audience: language users and compiler developers.

Accepted by the maintainer on 2026-10-22. This is the approved target contract;
implementation is pending. The final clarification supersedes an earlier UI
selection requesting truncation of fractional results.

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

The current Opcode::Pow backend does not implement this contract: it always
performs signed-integer-to-double conversion, calls external pow, then truncates
the result to an integer. It loses precision and mishandles unsigned/floating
operands. General bytecode/artifact compatibility and runtime panic dependencies
must be covered when replacing it. Do not claim this decision implemented until
source, native, source-removed QZI and target-object regressions establish it.
