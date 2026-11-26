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

## Parameter effects

Each callable with a source graph also carries `parameter_effects`, collected
from its graph and deferred closure graphs: parameter slots read, written
(and written through a dereference, which the caller observes), moved into
`Move` parameters, borrowed (shared or exclusive), returned by value or as a
borrow, and captured by closures, plus whether it writes globals. `unmodeled`
counts opaque regions (except effect-free regions of names, literals,
closure literals and field paths rooted at a name, whose reads are recorded as
place accesses) and indirect (except closed calls of immutable local closures),
foreign and format calls whose effects the sets omit; zero means every source
effect of the body is modeled.

A method call through a `dyn Trait` receiver (also `&dyn Trait` /
`&dyn Trait!`) has a closed target set when the program is closed: it has
`main`, every callable has a source body (no QZI dependency can add
implementations) and no implementing type is generic. Its site then records
`OwnershipCallTarget::Closed` with every implementation of the method, its
flow event is `DynamicInvoke` (modeled), and the receiver and positional
arguments map to slots 0, 1, ... Composition takes the union of the
implementations' effects. Otherwise the call stays an unmodeled
`IndirectInvoke`.

Callee effects compose to a fixed point over resolved calls. Each caller
instance composes the concrete specializations its own call sites resolved
to, with argument places typed under its substitution; a call whose target or
arguments are unresolved makes the caller `callee_unmodeled`. An argument
rooted at a caller parameter inherits the callee's write through a reference
for that slot when the argument itself refers into the caller's memory (a
reference-typed root or a dereference projection); borrowing a by-value
parameter only changes the callee's own copy. A callee that writes globals
makes its callers write globals.

A declared intrinsic whose call only reads its arguments and returns a fresh
scalar (`quazi.int.wrapping_add/sub/mul`, `quazi.f64.to_bits/from_bits/sqrt`,
`quazi.str.byte_at`, `quazi.strlen`; identified by the declared intrinsic, not
the wrapper's name, and only for a non-generic signature of scalars plus the
`str` operand of the two text reads, since `f64.to_bits` emits no code and
would otherwise pass a reference through) is lowered as an ordinary call of
its chunk. Its calls are
scheduled like direct source calls and its summary reads every parameter and
nothing else. Other intrinsics (allocation, raw memory, I/O, runtime state)
stay without a summary, so their callers remain `callee_unmodeled`. `bytecode::effects` checks this
differentially, including on the tutorial programs with std, and a callee with unmodeled effects or without a source
summary marks its callers `callee_unmodeled`. A call whose result a `ret`
hands out directly (`ret callee(p)`) also passes on what the callee returns:
a callee returning slot k by value or as a borrow makes the caller return the
parameter its argument k is rooted at. Captures are not composed. This is source evidence toward the D-014 export summary, not
an artifact certificate.

A bodiless callable implemented by a QZI dependency takes its effects from the
dependency's verified [ownership certificate](ownership-certificates.md)
instead (`Analyzer::certified_parameter_effects`): register slots map to
source parameters through its value layout, reads and writes through
references carry over, returned slots become `returned_borrow` for a borrowed
result and `returned` otherwise, and stored (escaping) or unknown-reaching
slots count as `unmodeled`. Records say nothing about globals, so such a
callable `writes_globals`; moves, borrows and captures stay with its signature
capabilities. A dependency without a certificate yields an unverified record
(every slot unknown, so `unmodeled`); a layout that disagrees with the record
and variadic or generic callables stay without a summary. Either way their
callers remain `callee_unmodeled`.

## Support boundary

The graph does not drive the lexical temporary-loan shortcut. Its signatures
leave `direct_call_eligible`, `source_effects_verified`, and
`transitive_effects_verified` false. The pre-existing declaration summaries
and source-body call facts remain the separate conservative shortcut input.
The QZI ownership envelope remains unverified.

The separate [source result-retention pass](source-result-retention.md) now
composes conservative parameter dependencies for lexical aggregate roots.
These facts do not make this graph an effect proof.

The graph now retains [argument storage snapshots](ownership-call-arguments.md).
It also retains [source statement control flow](source-control-flow.md), shared
by concrete instances of a declaration. Selected operand controls are shared
with codegen; unclassified expression regions remain opaque. No typed
reads/writes, alias provenance, escape paths or destruction effects are proved.
Closure calls remain conservatively attributed
to their enclosing source callable, with deferred closure spans on argument
snapshots, while their statement graphs have separate deferred entries;
compiler-injected
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
