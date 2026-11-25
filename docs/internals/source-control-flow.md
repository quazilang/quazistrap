# Source statement and expression control flow

Audience: compiler and tooling developers.

`semantic/control_flow.rs` builds `OwnershipCallable::control_flow`, a bounded
source graph for the next stage of [D-014](../decisions/whole-program-ownership.md).
It records statement transitions, lexical scopes, deferred closure bodies and
a lowering-shared subset of expression control. Unclassified regions remain
opaque. This is neither a typed effect IR nor a
provenance, destruction, normal-completion or ownership certificate.

## Identity and representation

The analyzer collects active source function and method bodies under their
canonical declaration names, including leaves without calls. Bodyless,
intrinsic, syscall and foreign declarations have no source graph. Concrete
ownership-call nodes share their declaration's graph through `Arc`; the graph
contains source occurrences rather than substituted value types. Each
specialization retains its own concrete signature and call argument facts.

Block IDs index the graph's block vector. Every block has lexical scope IDs,
source events and one terminator. Source spans identify occurrences; they are
not block IDs, and one occurrence can require several blocks. Scope identities
are local to one graph, with a parent and function/block/loop/closure kind.
Entering a source scope has an explicit event. Transfer edges retain exited
scopes from innermost to outermost. These markers describe source regions;
they do not certify emitted destructor calls or partial-move state.

## Structured transitions

- Conditions in `if` and `else if` lead to separate true/false successors.
  Only surviving paths reach the continuation.
- Conditional loops have a condition, body, back edge and exit. Unconditional
  loops have no invented false condition edge. `break` exits the innermost loop.
- A C-style loop initializes once. Its `continue` target is the update region,
  followed by the condition; normal body completion follows the same route.
- Range bounds and iterable inputs are evaluated once. Separate prepare/next
  protocol events describe iteration with unknown storage and runtime effects.
- `return`, `break` and `continue` terminate their current paths. Later source
  statements may be retained as disconnected blocks; their presence does not
  make them reachable.
- Active `@cfg` and unsafe blocks retain source scope transitions. Disabled
  cfg bodies contribute no operations.
- Closure creation remains in its enclosing opaque region. Each closure body
  has a separate deferred graph and no synchronous edge from the creator.

## Shared expression control

`src/evaluation.rs` classifies operand scheduling for both codegen and this
graph. Groups, casts and unary operators evaluate their child once; ordinary
binary operators evaluate left before right. A separate result event describes
the enclosing operation. In particular, unary `!` does not forward its child's
truth value, and casts are distinct from grouping. These source result identities
do not certify implicit automatic reads, typed conversions, storage aliases,
allocations, traps or ownership effects.

`&&` and `||` evaluate the left operand, branch on its truthiness, and evaluate
the right operand only on the required path. Both paths define the whole
expression's result: canonical boolean truthiness of the RHS, or a shortcut
`false` for `&&` / `true` for `||`. The physical lowering consumes the same
branch-polarity plan and normalizes integer operands' logical results to 0/1.

An assignment whose target is a bare name, possibly grouped (`total = value`,
`(total) = value`), evaluates no target operand. Its value completes first,
then a `NamedAssignment` result names the target and value spans. Lowering
selects the same form through the shared classification. The event does not
certify releasing the previous value, the store, moves, or aliasing.

A compound assignment to a bare name (`total += value`) first records a
`NamedTargetRead` of the target, matching lowering, which loads the previous
value before it evaluates the value. After the value completes, a
`NamedCompoundAssignment` result records the operator. Its arithmetic guards
can panic and are not modeled as edges.

`name++`, `++name`, `name--` and `--name` evaluate no operand. Their entry
block holds a `NamedIncDec` result recording the operator and whether the
result is the new (prefix) or previous value; overflow panics are unmodeled.
A C-style loop update of this form therefore has a result event instead of
an opaque `LoopUpdate` region.

Plain, compound and increment updates of a dereference (`*p`), field
(`object.field`) or single-index (`object[index]`) place evaluate the address
operands first, in lowering's `compute_lvalue_addr` order (pointer; object;
object then index). Compound and increment forms then record a
`PlaceTargetRead`, because lowering loads the place before evaluating the
value. The value, if any, follows, and a `PlaceUpdate` result names the
form. Operands are ordinary expression evaluations: the
graph does not distinguish place from value reads, and bounds, overflow and
store effects are unmodeled. `check_expression_control.py` checks the runtime
order. Multi-index targets stay opaque.

Each read and write of an assignment or increment target also records a
`PlaceAccess` event (`Read` or `Write`) in the block where it happens. Its
place comes from the same classifier as call-argument snapshots (binding
root, storage and field/deref/builtin-index projections), recorded per
callable during argument-place recording; a target it cannot classify is
`Unknown`.

Value reads of places are recorded the same way, for the maximal place only:
`values[i]` reads `values[]`, not `values`; `pair.left` reads `pair.left`.
The objects of recorded projections, borrowed operands (`&x`, `&x!`) and
method receivers are not value reads. A read appears in the block of the
opaque region whose whole expression is that place, or in the `Index`/`Field`
result block. Whether a read copies, moves or borrows, aliasing, and the
destruction of overwritten values remain unverified.

At the `Invoke` of a resolved non-generic call, each argument whose value is
a classified place records a `PlaceAccess` from the callee's parameter (or
receiver) capability: `Move`, or `Borrow { exclusive }`; `Copy` parameters
record nothing beyond the operand's read. Generic templates are skipped
because their capabilities depend on the specialization.

A `ret` whose value is a classified place, or `&place`/`&place!`, records an
`Escape` event in the exit block naming the place and whether it leaves by
value or as a shared/exclusive borrow.

Creating a closure records a `Capture` event for every outer variable or
parameter its body names (declared outside that closure, for each enclosing
closure), in the block that evaluates the creation. `moved` is set for owned
named-type values, which move into the environment (D-023); other captures
copy. The body's uses appear in its deferred graph. Escapes through other
expressions, storage into aggregates or globals and foreign boundaries are
not yet recorded as `Escape` events; result-retention facts still describe
parameter-derived results.

An exposed `?` evaluates its operand once and branches on `TrySuccess`, a wrapper
success-discriminant condition distinct from ordinary pointer truthiness.
Success records payload extraction; failure returns from the current callable
and retains its active scope exits. Typed wrapper compatibility and emitted
cleanup effects remain unverified by this graph.

For a resolved ordinary direct call with a fixed signature and a source body,
the graph uses lowering's declaration-slot schedule for its actual arguments.
The call gets an `Invoke` event keyed by the source call span only after the
actuals complete. A `?` in an earlier argument can bypass later arguments and
the invocation. Named arguments follow parameter order. A method call that
analysis resolves to a source-bodied inherent method with a receiver actual
evaluates the receiver expression first, then the actuals in declaration-slot
order, matching lowering's resolved-target path. A resolved module or
associated call (`module.f(...)`, `Type.f(...)`) without a receiver actual
evaluates no namespace operand and schedules its actuals like a direct call.
A method called through its type name with an explicit receiver
(`Type.method(&value, x)`) records the receiver as its first positional
actual; lowering evaluates all positional arguments in source order, and the
graph schedules them likewise before the `Invoke`. `free` (a separate
destruction path) stays opaque. Generic targets follow the same
schedule (lowering calls the specialization after the same actuals).
Unsafe source functions follow the same schedule (`unsafe` is only a
permission). A call whose callee has the never type `!` ends its path: the
`Invoke` block stops with every active scope exited. A call through a local
or parameter holding a function value or closure (`callback(a, b)`)
evaluates its positional arguments in source order and records an
`IndirectInvoke`: the target and its effects are unknown. A method call on a
`dyn Trait` receiver evaluates the receiver, then its positional arguments,
then records an `IndirectInvoke` for the vtable call.

A direct call of a non-generic format function (`str_variadic`, such as
`println`) with a literal template is lowered by expanding the template: the
explicit arguments and placeholder expressions are evaluated in template
order, then converted for display and passed to the callee. The graph
schedules the expanded arguments and records a `FormatInvoke`; display
conversions (which may run `Display` impls) and the callee stay opaque.
Expansions are computed outside the builder, which only borrows source
trees: a family is rebuilt with the requested expansions, up to
`FORMAT_EXPANSION_ROUNDS` times for format calls nested inside expansions,
restoring its budget counters before each rebuild. Calls without a literal
template stay opaque, as do placeholder calls (analysis records no call site
for them, so their region is evaluated whole).

A call resolved to an `@api` or `@syscall` declaration, directly or through a
module (`posix.write(...)`), evaluates its positional arguments in source
order and records a `ForeignInvoke` with unknown effects.

A call of the prelude-selected runtime panic evaluates a literal message as a
format template (when `fmt.format` exists, using the same expansion rounds)
or its message and extra values in order, then records a `PanicInvoke` whose
block stops with every active scope exited: the panic never returns.
Intrinsics keep
their special lowering and stay opaque, as do other variadic calls, as do the runtime panic and calls
with special lowering such as layout queries.

Refinement is iterative and charges expansion to the existing aggregate budget.
Completed wrappers are not scanned again as opaque regions, so an exposed `?`
does not acquire a duplicate possible-return edge.

## Opaque expression regions

An evaluation event references one expression, or the input spans belonging to
one region. `effect_free` marks regions made only of names, literals and field paths rooted at a name (`p.left.x`); a region that is itself such a place records its read as a `PlaceAccess`. Span-vector order is not runtime operand order. The normal edge
means normal completion is possible; it does not prove termination or account
for panics, traps, callbacks or implicit cleanup.

An iterative scan detects `?` within the region and adds a conservative possible
failure-return edge. It stops at closure creation; a closure's `?` exits only
its deferred graph. The edge does not identify the exact failing subexpression
or prove which preceding effects occurred. A `?` on a conditionally unexecuted
path can therefore introduce a conservative extra edge.

A single-index read `object[index]` evaluates the object, then the index,
then records an `Index` result; bounds checks and the load are unmodeled.
`check_expression_control.py` checks this order at run time.

A built-in method without arguments on a primitive receiver (`len`,
`as_str`, `as_ptr`, classified by the `resolve_primitive_method` that
lowering shares) evaluates only its receiver and records a `BuiltinMethod`
result.

A field read whose object is an expression (`make().field`, `items[i].field`)
evaluates the object and records a `Field` result. Objects that are names or
name chains (`value.field`, `module.constant`, `Enum.Variant`) stay opaque:
they may be namespaces and evaluate nothing with effects.

A variant constructor call (`Some(value)`, `Ok(value)`) evaluates its
positional payloads in source order, as lowering does, and records a
`VariantConstructor` result. It is recognized like the borrow checker does:
analysis types the call as an enum with that variant and the callee resolves
to no function; a callee bound to a local value or parameter stays a call.
Allocation and payload moves are unmodeled.

A struct initializer evaluates its field values in the struct's declaration
order, which lowering uses regardless of the written order, and records a
`StructInit` result. The order comes from the annotated struct type; an
initializer whose declaration is unknown stays opaque.
`check_expression_control.py` checks this order at run time.

An array literal evaluates its elements in source order and records an
`ArrayLiteral` result.

A `match` expression evaluates its scrutinee once, then tests arms in order:
each arm has a `MatchArm` branch (pattern only), a guard is evaluated and
branches as an `Expression` condition, and a failed pattern or guard tries
the next arm. The chosen arm's expression ends in a `Match` result. When no
arm matches, a value-producing match stops (lowering traps) and a `void`
match continues. Arm bindings get no separate lexical scope here; a match
whose type is not annotated stays opaque.

Other calls, multi-index reads and updates and struct initializers of
unknown declarations remain whole opaque regions. Controls nested inside
these regions are not individually expanded. Multi-input iteration regions also
remain opaque. Their internal branches and operand order require further shared
normalization before any path-sensitive ownership proof consumes them.
Lowering evaluates struct fields in declaration order. [Call argument snapshots](ownership-call-arguments.md)
retain lexical order for each site's binding and parameter records.

## Resource and trust boundary

Construction and expression scanning are iterative and poll cooperative
cancellation. Bounds are aggregate across source functions and their deferred
graphs: 1,000,000 charged nodes, 250,000 blocks, 500,000 edges and 64 levels of
lexical scope or deferred-closure nesting. Node charging includes retained scope
and exit vectors, work contexts and deferred references; it is not merely an
AST expression count. Opaque expression scanning has no separate depth limit.
Exceeding a bound emits S14 and discards the collected graph map;
cancellation discards the analyzer under its existing contract. Source graphs
are local report data and are not serialized into QZI/QZC.
These are flow-work and retained-metadata counters, not a hard bound on total
frontend heap usage; existing canonical-name helpers can allocate before their
result is charged.

Before publication the builder checks that every allocated block was terminated.
The structural validator checks the root entry, targets, lexical scope chains,
scope-entry evidence and exited-scope suffixes, including deferred graphs.
This validation says nothing about expression effects or ownership correctness.

The pass leaves all lexical borrow rules, call-closure eligibility and artifact
verification flags unchanged. Typed reads/writes, alias flow, moves, destruction,
recursive effect composition and retained artifact-body verification remain
required. Try wrapper/error compatibility is checked independently by semantic
analysis against the nearest callable return context; graph edges alone do not
certify it. Physical cleanup on abrupt loop edges is separately covered by
[the loop-cleanup regressions](../changes/2026-10-20-loop-cleanup.md); the graph
is not a destruction certificate. QZI13 stays Unverified; QZC20 invalidates
older exact/reusable caches for cleanup and optimizer corrections. QZC19
introduced typed propagation checks and QZC18 refined flow/canonical bools.
Existing QZI bodies retain their emitted behavior; rebuild affected dependencies
from source to obtain those fixes.
