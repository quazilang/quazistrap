# Generic binding context across semantic passes

Audience: language users and compiler developers.

A global type using a callee's generic parameter spelling previously caused the
source graph to discard valid concrete calls. With `type T = [i32; 3]`, a concrete
`identity[T]` call could also reach lowering as the identity substitution
`T -> T`: the binary built successfully but failed a three-element round trip.
Global aliases could capture implicit prelude impl binders and borrow-checker
annotations after their original source scopes ended.

## Selected binder rule

The maintainer accepted this rule on 2026-10-09: a bare impl target argument
that repeats a formal parameter of the target declaration introduces a local
binder, even with a same-named global type or alias. `impl Holder[T]` for
`struct Holder[T]` is generic. A fixed target uses its concrete type or a
differently named alias. Other declared concrete names and qualified arguments
remain concrete; undeclared bare names may rename binders.

## Implementation

Nominal parameter metadata, including enum declarations, now supplies both impl
binder inference and concrete graph arity. Binder scopes cover borrow checking
and begin before trait-impl conformance. Generic template move classification
respects the current binders rather than looking up same-named global owners.

Specialization registration retains whether arguments were symbolic in their
lexical context and records proven concrete identities, including implicit
cleanup. Source graph identities supplement that evidence before emission
closure. Symbolic helpers remain available for emission but are not revalidated
through unrelated global aliases. Concrete arguments are normalized before
lowering substitutions; registered emitted names remain stable, while newly
derived transitive specializations use resolved concrete arguments.

This adds binding-context precision, not an ownership certificate or a broader
borrow permission. Symbolic helper removal and D-014 effects remain incomplete.

## Verification

Eight added regressions cover same-named concrete source types, alias-backed
multi-slot arguments, impl formal binders, symbolic helpers, enum signatures,
generic template copies, implicit transitive cleanup, and analyzer reuse. The full pinned/bfd
compiler suite passes 698/698. The originally failing alias fixture now exits
zero with both std disabled and default prelude enabled. QZI and Windows COFF
emission pass for the default-prelude fixture.
