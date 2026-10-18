# Source result retention

Audience: compiler and tooling developers.

`src/semantic/retention.rs` derives conservative parameter-slot dependencies for
source function results. `SemanticReport::source_result_retention` exposes them
in canonical callable-name order. The lexical borrow checker uses these facts
to preserve borrowed roots inside helper-returned aggregates.

For example, if `wrap(text: str) View` returns `View { text: text }`, a local
`wrap(owner.as_str())` retains the caller's `owner` root. Moving or releasing
that owner while the result's lexical scope remains live produces S10. A helper
that retains only its first parameter does not freeze an unused second owner.
Returning an aggregate derived from a callee-local owner, or assigning it into
an outer scope, is rejected. Static-only results and synchronous observation of
parameter-derived aggregates remain supported.

## Derivation

Source declarations seed a greatest fixed point with every parameter slot.
Body analysis removes dependencies proved unnecessary; unresolved recursive
relationships remain conservative. Method receivers occupy their declared
slot, and named arguments map to declaration order. Calls compose result slots
rather than collecting every call made by the function. Unknown calls retain
all supplied provenance. Missing source facts never prove that an aggregate
result is static.

The evaluator tracks lexical bindings, assignments, aggregate construction,
pattern bindings, branch unions, and loop fixed points. Closure creation retains
captures conservatively without executing the closure body. Parameter scopes
protect generic binders during checked alias resolution. Structural type queries
retain uncertain generic, recursive, or unresolved shapes. Proven scalar results
carry no view provenance. The compiler's `quazi.str_concat` intrinsic creates a
fresh allocation; its input views are not dependencies of that allocation.
This rule uses the intrinsic's declared identity and runtime semantics, rather
than a standard-library function or type name.

Expression and type traversal have conservative resource bounds. Derivation
polls typed cooperative cancellation during declaration, iteration, statement,
and expression work. Analysis reuse clears all facts.

## Lexical enforcement

`borrow.rs` propagates retained roots through named aggregates, fixed arrays,
enum payloads, nested helpers, fields, and matches. Qualified enum constructors
retain payload roots even when they lack a result annotation. Aggregate
bindings extend root loans to their lexical scope, with existing declaration
identities preventing shadowed names from sharing move or loan state.

Borrowed fields and an aggregate's own destructible storage are distinct:
projections from by-value parameters containing consuming destruction hooks
also retain that parameter's local storage root. They cannot turn a view into
an externally rooted return merely because the aggregate also contains `str`.
These checks are conservative across fields; they do not derive exact
projection-specific regions or shorten a loan after its last use.

## Support boundary

These source facts are not the [D-014](../decisions/whole-program-ownership.md)
whole-program effect solver. They do not certify writes, global/heap escapes,
destruction paths, indirect targets, or artifact bodies. They do not relax
reference escape restrictions or enable [D-015 borrowed collection
elements](../decisions/borrowed-elements.md). The separate concrete ownership
call graph now retains [argument storage facts](ownership-call-arguments.md),
but still lacks control-flow edges and effect proofs.

QZI remains v13, and its ownership envelope remains Unverified. Source result
facts are not serialized as ownership certificates. QZC v14 invalidates older
exact hits and reusable units so previously accepted source must pass these
checks again. Historical QZI artifacts have not acquired this source proof.

## Verification

`semantic::retention::tests` checks dependency composition, recursion, named
arguments, methods, mutation, lexical identities, loops, closures, reset, and
cancellation. Borrow regressions exercise retained aggregate roots and existing
owned-storage projection restrictions. Run
`python3 scripts/check_result_retention.py target/debug/qz` for unsafe-source
rejections during checking/building/QZI emission and valid source/QZI native
execution plus Windows COFF emission. COFF emission is not Windows execution.
