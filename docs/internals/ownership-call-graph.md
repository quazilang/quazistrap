# Concrete source ownership call graph

Audience: compiler and tooling developers.

`src/semantic/ownership.rs` builds `SemanticReport::ownership_call_graph`
from resolved source call sites. This is an input to the
[D-014 ownership analysis](../decisions/whole-program-ownership.md), not its
completed effect solver or an ownership certificate.

## Identities and specialization

A callable identity contains its qualified declaration name and tagged,
alias-resolved type arguments. The encoding discards source spans and
distinguishes nested reference constructors even when normal type display
prints them identically. Bytecode mangling is an emission name, not identity.

All non-generic declarations seed the graph. Generic specializations are
discovered by substituting actual source targets under each concrete caller's
environment. Symbolic monomorphization records never seed this graph: a bound
parameter can have the same spelling as an unrelated global type. Enclosing
method generics come from actual impl declarations, not a matching module-name
prefix. The declaration and type-check passes share the impl target's actual
binder names/order; codegen consumes the same metadata for concrete method
substitution, including renamed and reordered binders.
Declared target types and aliases are excluded from the implicit binder list
after all declarations are known; receiver matching checks the actual impl
target before allowing an inherent call. Concrete receiver,
fixed parameter, variadic, and result capabilities
are recomputed from substituted types using the existing semantic ownership
rules; a template's symbolic `Copy` classification is not reused.

Calls retain their source span. Repeated visits during named-argument checking
share one site, while two syntactically distinct calls remain separate. Records
are ordered by callable identity and then source position. Resolution preserves
qualified import targets, associated functions, and inherent method targets;
unknown dispatch remains opaque.

## Closure and bounds

Reverse rejection propagation computes `source_calls_closed`. Unsafe,
intrinsic, variadic, unavailable, opaque, and unresolved boundaries reject every
recorded caller that reaches them. Finite recursive components survive only
when all their recorded calls remain within the safe source graph.

The graph has a separate 65,535-callable operational budget. Exceeding it
produces `S14`; rejected outgoing targets remain `Unresolved` and therefore
cannot appear closed. Specialization expansion shares an iterative limit of
64 type-constructor levels and 4,096 type nodes across the type arguments,
checked before and after alias resolution. The emission identity registry is
also bounded by the bytecode's 16-bit function-index space. These are compiler
resource limits, not permissions to truncate a type or use a generic template
as a concrete fallback.

The emission registry rejects unequal structural identities that map to one
bytecode symbol. This includes symbolic helper specializations still emitted
by current codegen. Removing all symbolic helper emission and changing to an
injective artifact symbol format require their own coordinated implementation;
until then, an ambiguous emitted name is a compiler error.

## Support boundary

The graph does not drive the lexical temporary-loan shortcut. Its signatures
leave `direct_call_eligible`, `source_effects_verified`, and
`transitive_effects_verified` false. The pre-existing declaration summaries
and source-body call facts remain the separate conservative shortcut input.
QZI v10's ownership envelope remains unverified.

This graph does not yet record argument places, reads/writes, provenance,
escape paths, control flow, or destruction effects. Closure bodies are currently
attributed conservatively to their enclosing source callable; compiler-injected
cleanup, formatting, and panic calls are not complete source-site evidence.
`source_calls_closed` must never be used as a runtime-effect or artifact proof.
Cycle-safe, resource-bounded alias expansion is also a separate prerequisite:
the specialization checks do not replace the compiler's current alias resolver.

The next ownership work must add typed place/effect facts and control-flow
edges, compose those facts through concrete recursion, and verify retained
artifact bodies before permitting QZI-only safe borrowed calls or
[D-015 element loans](../decisions/borrowed-elements.md).

## Verification

The `semantic::ownership::tests` module covers transitive concrete calls,
owned versus plain capabilities, aliases, ambiguous type display, mangled
collisions, distinct sites, named arguments, imported targets, inherent
receivers, recursive rejection, unknown calls, cancellation, generic shadowing,
module/struct name collisions, and depth/width/count exhaustion. Existing
semantic, codegen, documentation-fixture, and native collection regressions
remain required when this graph changes.

## Concrete names and lexical binders

A concrete source argument may name a global type using the same spelling as a
callee's generic parameter. Graph concreteness follows the source substitution
context and resolved declaration, rather than rejecting the spelling alone.
The graph is constructed before emission dependency closure; its concrete
identities combine with binding context retained at specialization registration.
This includes injected cleanup/intrinsic roots; symbolic registrations cannot
be reinterpreted as concrete aliases after their caller scopes end.

Proven concrete mono arguments are alias-normalized before building lowering
substitutions, retaining their registered emitted names. An actual alias `T`
resolving to `[i32; 3]` must not become the symbolic identity substitution
`T -> T`, which would select an incorrect physical representation. Unproven
symbolic helper arguments are retained. Transitive specializations use resolved
concrete arguments. None of this certifies ownership effects or relaxes loans.
