# D-013: formatting result ownership

Status: resolved 2026-10-09 by the maintainer. The primitive/text implementation
and QZI v11 migration have a verified local checkpoint on 2026-10-10; whole
stdlib stabilization remains incomplete. See the
[implementation record](../changes/2026-10-10-owned-formatting.md).

## Selected contract

`Display.to_string()` returns owned `String`. The maintainer explicitly
accepted the source and ABI break described in option 1 below. Dynamic numeric,
boolean, and text conversion results must use that same owned contract;
borrowed `to_str()` views remain separate and cannot own hidden allocations.

The migration must update prelude trait implementations, primitive semantic
return types and lowering, standard-library callers, examples, documentation,
and QZI compatibility checks together. Returned owners transfer rather than
being freed locally. Temporary results need exact-once destruction after their
last use; copying a borrowed view out of a temporary owner is not a workaround.

The old `to_string() -> str` behavior is replaced by the selected owned
contract; see the [migration guide](../migrations/owned-formatting.md).

## Evidence

Before this migration, `Display.to_string()` returned `str`, while primitive formatting
lowered through allocation-backed runtime formatting. A borrowed `str` has no
owner or destruction contract for that allocation. That implementation
could therefore either leak the formatting buffer or make a returned view dangle.
This was not a safe public ownership contract.

## Required invariant

A dynamically produced formatting result must keep an owned allocation alive
for exactly the lifetime of the result. It must not be represented as borrowed
`str`. Formatting text which is truly static may remain a borrowed view.

## Alternatives

1. **Change `Display.to_string()` to return `String` (recommended).** This
   makes ownership explicit, matches the existing `String` type, and lets
   normal move/destruction rules own the buffer. It is source- and ABI-breaking
   for trait implementations and callers that expect `str`.
2. **Keep `to_string() -> str` and add a separate owned method.** For example,
   `to_owned_string() -> String`. This preserves the old spelling but cannot
   make the existing allocation-backed `to_string()` safe; it would require
   changing it to a static/borrowed-only operation or rejecting primitive
   implementations.
3. **Introduce an implicit owner-bearing string view.** This would change the
   representation and lifetime semantics of `str` across the language, FFI,
   QZI, and references. It is substantially larger than formatting and should
   not be introduced as a local workaround.

## Maintainer choice

The maintainer selected option 1 on 2026-10-09. Options 2 and 3 remain recorded
for the compatibility tradeoff; they are not implementation alternatives to
silently substitute for the accepted owned result.

## Verification required after a decision

Cover primitive numeric, boolean, static text, formatting interpolation,
return/move/destruction paths, repeated formatting under a memory checker or
allocation counter, QZI round trips, and Linux/Windows native compilation.
