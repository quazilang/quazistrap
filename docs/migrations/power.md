# Power and QZI14 migration

Audience: language users maintaining existing code and compiled libraries.

Integer power now computes an exact integer and checks overflow in the base's
integer type. `2 ** -2` is a compile error. If a mutable variable or parameter
supplies a negative integer exponent, execution panics. Runtime overflow also
panics. Use `2.0 ** -2` for floating 0.25. Integer `2 ** 0` is 1.

Either floating operand selects an f64 result. Fractional powers use a
self-contained approximation, with no implicit libm dependency. Do not expect
correctly rounded libm results. Nonnumeric operands are rejected, including
through generic specializations. Templates whose provisional integer result
would become floating after substitution require an explicitly floating operand.

Rebuild pre-QZI14 artifacts containing Pow from source. Earlier Pow carried no
reliable operand representations and cannot safely be upgraded by changing its
version byte. Compatible older artifacts without Pow remain readable. QZC28
invalidates older source caches automatically. QZI ownership remains Unverified.
See [the implementation contract](../internals/power.md).
