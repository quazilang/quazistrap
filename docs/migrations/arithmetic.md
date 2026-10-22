# Arithmetic operand validation

Audience: language users upgrading existing source.

The compiler now rejects arithmetic on owners, enums, arrays, functions and
booleans, including when hidden by a concrete generic instantiation. Missing
operand types require an explicit annotation. An annotation cannot make an
unsupported operand category numeric. Generic numeric helpers remain supported.

Use explicit field/element access or a method implementing the intended operation
for structured values. Unary minus on references needs an explicit dereference.
For symbolic &T in generic arithmetic, dereference the argument before operating.

Only str views support + concatenation. Owned String + String previously produced
a raw view under an owning result type and is now rejected. Use an explicit String
operation such as `left.push_str(right.as_str())` to construct an owned result;
use view concatenation only where a str result is intended. This checkpoint does
not change allocation/lifetime policy for concatenated views.

Raw pointer + or - integer remains a byte offset. Pointer multiplication,
pointer RHS arithmetic and pointer differences are rejected; no element-scaling
or pointer-difference result contract is inferred.

Rebuild source caches and previous QZI bodies. QZC26 invalidates older source
chunks; the unchanged QZI13 envelope remains Unverified. General numeric
coercion, overflow/rounding normalization, bitwise and power constraints remain
open. See the [change record](../changes/2026-10-22-arithmetic.md).
