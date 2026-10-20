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

An exposed `?` evaluates its operand once and branches on `TrySuccess`, a wrapper
success-discriminant condition distinct from ordinary pointer truthiness.
Success records payload extraction; failure returns from the current callable
and retains its active scope exits. Typed wrapper compatibility and emitted
cleanup effects remain unverified by this graph.

Refinement is iterative and charges expansion to the existing aggregate budget.
Completed wrappers are not scanned again as opaque regions, so an exposed `?`
does not acquire a duplicate possible-return edge.

## Opaque expression regions

An evaluation event references one expression, or the input spans belonging to
one region. Span-vector order is not runtime operand order. The normal edge
means normal completion is possible; it does not prove termination or account
for panics, traps, callbacks or implicit cleanup.

An iterative scan detects `?` within the region and adds a conservative possible
failure-return edge. It stops at closure creation; a closure's `?` exits only
its deferred graph. The edge does not identify the exact failing subexpression
or prove which preceding effects occurred. A `?` on a conditionally unexecuted
path can therefore introduce a conservative extra edge.

Calls, assignments, increment/decrement, guarded matches, fields/indexes and
aggregate initializers remain whole opaque regions. Controls nested inside
these regions are not individually expanded. Multi-input iteration regions also
remain opaque. Their internal branches and operand order require further shared
normalization before any path-sensitive ownership proof consumes them.
In particular, current lowering reorders named arguments and struct fields,
and uses different operand orders for indexed and field assignments. The
lexical [argument snapshot walk](ownership-call-arguments.md) is not that plan.

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
