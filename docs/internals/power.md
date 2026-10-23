# Typed power

Audience: compiler developers. Implements [D-016](../decisions/power-operator.md).

Both operands must be numeric after alias resolution and supported implicit
scalar loads. Integer/integer power preserves the base's integer type. If
either operand is floating, the result is f64. A result's type never depends
on a runtime exponent's sign. Known numeric references load independently of
generic peers; symbolic &T operands require explicit dereference.

`semantic/power.rs` records bounded declaration-slot requirements and validates
every proven concrete specialization after dependency closure. Known negative
integer exponents and known integer overflow produce S06. Mutable locals remain
runtime inputs even when initialized by a literal. A generic substitution that
changes a provisional integer result family to floating fails S14; use an
explicitly floating operand in the template. This is a current generic inference
limit, not permission to reinterpret integer slots as floating values.

Integer-capable bodies retain language panic dependencies. `bytecode/power.rs`
uses exponentiation by squaring with ordinary portable instructions. Signed
negative exponents panic. Base magnitudes remain unsigned, including signed MIN;
the final sign selects the declared type's positive or negative magnitude limit.
Before multiplication, unsigned division/comparison checks the product against
that limit. Zero factors avoid division by zero. Exponent zero returns one;
the last unused square is skipped. Overflow panics rather than wrapping. A
std=false program uses the ordinary deterministic Trap fallback. Symbolic helper
chunks trap; dependency closure must select concrete numeric specializations.
Operands are captured once before entering the loop.

Floating Pow uses explicit operand flags: FLOAT_FLAG=1, base-float=2,
base-unsigned=4, exponent-float=8, exponent-unsigned=16. FLOAT_FLAG and at least
one floating operand are mandatory; unsigned and floating cannot describe the
same operand. Unknown bits are rejected. Integer/integer operations never emit
Pow. The self-contained x86-64 backend imports no pow/libm symbol. Floating bases
with integer exponents use SSE squaring with exact 64-bit exponent magnitude and
parity. Negative exponents invert the base first, preserving cases such as
2.0 ** -1074. Fractional exponents use guarded x87 FYL2X/F2XM1/FSCALE, with
explicit special-value classification and balanced x87 stack. This is approximate
hardware transcendental evaluation, not a correctly rounded libm guarantee.
Floating power is not folded through a separate host pow implementation.

QZC28 rebuilds previous exact/reusable source chunks. QZI14 adds typed Pow;
pre-v14 artifacts containing Pow require a source rebuild. Compatible older
artifacts without Pow retain their ownership ABI origin. All artifacts remain
Unverified: numeric checks provide no ownership/effect certificate. See
[migration](../migrations/power.md) and [verification](../changes/2026-10-23-typed-power.md).
