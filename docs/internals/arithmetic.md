# Arithmetic operand and reference contracts

Audience: compiler developers.

`semantic/arithmetic.rs` shares declaration-slot obligations across ++/--,
compound assignments, eager + - * / % and unary negation. Numeric operands use
existing directional compatibility. Negation requires a numeric value; references
need an explicit dereference. Pointer offsets require a raw pointer on the left,
an integer on the right, and + or -. Offsets are bytes, without element scaling.
Pointer differences and pointer RHS arithmetic have no supported result contract.

Only Add admits str-view pairs. Owned String operands are rejected: the old
concatenation intrinsic returned a thin pointer while inference claimed an owned
String. Generic actuals may be numeric primitives, permitted raw pointers or
representation-identical shared string views. Generic exclusive string references
cannot borrow the template's absent implicit-load annotation. Symbolic &T
operands require explicit dereference; distinct T/U inference is unchanged.

Deferred facts retain declaration parameter slots and are checked for every
proven concrete specialization after closure, using checked alias expansion.
Generic integer-capable division/remainder templates conservatively retain the
hidden panic dependency; concrete floating operations need no integer guard.
The [mutation contract](numeric-mutations.md) retains the bounds, reset and
cancellation rules. S06 rejects invalid operand categories, S01 incompatible
numeric actuals, and S14 missing types/resolution/budget evidence. Successful
symbolic emission never discharges a concrete obligation.

Reference annotations describe physical VM-slot loads. Resolve aliases before
collapsing types, and keep marking idempotent. Numeric/pointer references load;
shared string wrappers preserve their view representation. One exclusive string
wrapper adds a slot and loads once; deeper implicit exclusive chains are rejected.
Explicit dereference uses the same shared-wrapper representation rule. These
loads remain qword VM accesses, including f32; native *f32 uses its own conversion.

Floating Mod now honors FLOAT_FLAG. The encoder classifies absolute operand bits
before any x87 load. Nonfinite dividend, zero divisor and NaN divisor produce a
quiet NaN; finite dividend modulo infinity copies its original bits. Finite,
nonzero operands load divisor then dividend and repeat FPREM until C2 clears.
Store the result and pop the divisor, preserving the caller's control word.
FPREM uses truncation toward zero and preserves the dividend's zero sign; see
the [Intel instruction reference](https://cdrdv2-public.intel.com/782156/325383-sdm-vol-2abcd.pdf).
Constprop uses the corresponding floating remainder operation. NaN payload
identity and custom unmasked floating-environment behavior are not promised.
No implicit libc/libm dependency is added to ordinary programs.

QZC26 rebuilds previous source caches. QZI13 remains unchanged and Unverified;
rebuild old QZI bodies from source for corrected typing/reference instructions.
New native Mod emission interprets existing FLOAT_FLAG correctly. Bitwise/Pow
constraints, generic ordering proof, mixed float/integer coercion, narrow-result
normalization and complete ownership/effect/artifact proof remain incomplete.
See [verification](../changes/2026-10-22-arithmetic.md) and
[migration guidance](../migrations/arithmetic.md).
