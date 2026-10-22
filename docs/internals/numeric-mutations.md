# Numeric mutation obligations

Audience: compiler developers.

`semantic/arithmetic.rs` checks ++/-- and arithmetic compound assignments.
Increment requires an integer or float. Compound operations require numeric
operands with existing directional compatibility; raw pointer +=/-= accepts an
integer offset. Other pointer mutations, handle arithmetic and string += are
rejected with S06. Missing operand types fail with S14. String concatenation
requires its separate lowering and is not an arithmetic mutation.

Only a declaration generic parameter may defer a requirement. Facts retain its
declared parameter slot, canonical callable and source span. After specialization
closure, every proven concrete identity is checked using checked alias resolution.
Non-numeric actuals produce S06, incompatible numeric actuals S01, and resolution
or missing-slot failures S14. Symbolic helper emission is not concrete proof.
Facts reset between analyses and cancellation is polled. Bounds are 65,536 deferred source
sites, 16 MiB retained declaration text and 1,000,000 specialization checks;
exhaustion fails with S14. Diagnostic deduplication never skips later identities.

Known numeric reference RHS operands are explicitly loaded even when the target
is a generic parameter. Implicit loading of symbolic &T is unsupported; use an
explicit dereference. General distinct T/U arithmetic inference is unchanged.
Builtin indexing resolves the referenced array/slice/bytes value before checking
arity, bounds and element type. Codegen loads a complete borrowed slice descriptor
before indexing; byte descriptors load their handle. Bounds guards remain active
and byte strings remain immutable through shared and exclusive references.

This page describes the initial mutation checkpoint. The subsequent
[arithmetic checkpoint](arithmetic.md) extends its facts to eager arithmetic
and unary negation. Bitwise operations, floating power, mixed float/integer
coercion and narrow-result normalization remain incomplete. It establishes no complete typed-effect,
escape, recursive-destruction or artifact proof. QZC25 invalidates earlier source
caches. QZI13 remains unchanged and Unverified; rebuild old bodies from source.
See the [verification record](../changes/2026-10-22-numeric-mutations.md).
