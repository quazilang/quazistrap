# D-013: formatting result ownership

Status: maintainer decision required.

## Evidence

`Display.to_string()` is declared as returning `str`, but primitive formatting
lowers through allocation-backed runtime formatting. A borrowed `str` has no
owner or destruction contract for that allocation. The current implementation
can therefore either leak the formatting buffer or make a returned view dangle.
The `Display` comments currently acknowledge that its ownership API is not
stabilized; this is not a safe public contract.

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

## Decision requested

Choose whether Quazi accepts the explicit breaking correction in option 1, or
whether the formatting surface should be temporarily narrowed under option 2.
Do not implement an ownerless allocation-backed `str` workaround.

## Verification required after a decision

Cover primitive numeric, boolean, static text, formatting interpolation,
return/move/destruction paths, repeated formatting under a memory checker or
allocation counter, QZI round trips, and Linux/Windows native compilation.
