# Source call argument storage facts

Audience: compiler and tooling developers.

`semantic/places.rs` records `OwnershipCallSite::arguments` in the concrete
[ownership call graph](ownership-call-graph.md). This is a D-014 input, not an
effect solver. Codegen uses declared slots to order named call arguments,
independently of short method names or emitted specialization symbols. These
facts do not change lexical borrow eligibility, source result-retention semantics,
or QZI ownership verification.

## Operands and slots

Each actual retains its source span, expression type when available, syntactic
address formation, automatic-read annotation, and lexical role: receiver,
positional index, or named spelling. Actuals retain source spelling order,
including named arguments. Declared parameter slots are separate metadata;
multiple variadic actuals share one slot. Legacy implicit receiver names and
explicit receivers use the same slot mapping as result retention. Associated
functions and module namespaces do not invent receiver operands.

Unknown/indirect targets can still retain operand facts, but their parameter
slots stay unknown. A missing snapshot is distinct from a known empty argument
list. Synthetic unavailable-body edges have no snapshot. Calls in closure
bodies carry enclosing closure spans; they are deferred source evidence, not
proof of synchronous execution. The vector order is not a CFG or a guarantee
about lowering's evaluation order.

## Storage and projections

A place root retains the resolved binding's name, declaration span and kind,
its declaration storage type, and its storage category: parameter slot, local,
global, or closure parameter. Full binding identity distinguishes shadowed
locals and same-span pattern/closure parameters with different names.

Supported projections are fields, explicit/implicit dereferences with their
reference kind, and builtin fixed-array/slice indexing. An index retains its
span, type and optional typed constant; it never proves disjointness. Overloaded
Index operations, casts, assignment results, try expressions and projections
of temporary values remain unknown. Literals and call results are rvalues;
that classification does not assert ownership or lifetime. Qualified foreign
global expressions without a resolved-global annotation remain unknown, while
resolved imported aliases use their canonical declaration.

Snapshot types are alias-resolved in their declaration context and instantiated
under concrete caller substitutions. Global types have no caller binders, so a
caller's generic `T` cannot capture a globally declared alias `T`. Storage type
is retained separately from automatic value reads.

## Resource and cancellation contract

The source walk is iterative and polls cancellation for each visit. Each body
has a 100,000-node budget including retained snapshot/type structure. Place
projection and deferred closure depth are limited to 64. Concrete graph
snapshots share a 1,000,000-node structure/type budget across specializations;
types are resolved and charged before retaining each instantiated operand.
Exhaustion produces S14, rather than successful truncated facts. Existing
checked alias/substitution limits also apply. Analyzer reset clears call-site
snapshots along with the source-site registry. Cancelled analyzers must be
discarded under the ordinary semantic cancellation contract.

## Verification and remaining work

`cargo test --offline semantic::places` covers declaration identity, module and
import aliases, receivers, named/variadic slots, fields, builtin and overloaded
indexing, address formation, automatic reads, closure attribution, generics,
global binder isolation, temporary projections, budget exhaustion, cancellation
and analyzer reuse.

The separate [statement graph](source-control-flow.md) records structured
control-flow edges and selected expression controls shared with codegen;
unclassified regions remain opaque. Typed reads/writes, alias
flow, complete expression control flow, place moves, destruction effects,
recursive effect composition and retained artifact-body verification remain
required by [D-014](../decisions/whole-program-ownership.md). These snapshots do
not enable [D-015 element loans](../decisions/borrowed-elements.md). QZI remains
v13 with unverified envelopes. QZC v18 reruns refined flow checks and emits
canonical logical bools; v16 introduced the
[named-call emission correction](../changes/2026-10-20-named-call-slots.md).
