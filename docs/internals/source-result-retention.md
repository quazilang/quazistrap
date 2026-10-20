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

## Raw storage and copying

The evaluator separates a pointer's identity from the contents reachable
through it. Lexical bindings, allocation sites and unmodeled expression sites
have structurally distinct identities. Branch/loop merges preserve reachable
allocation cells after branch-local bindings leave scope. Explicit loads and
implicit field/index/iteration reads through references recover stored pointer
identities; writes use the addressed storage rather than the pointer binding.
Field and element paths remain coarse and writes are weak unions.

Declared `quazi.malloc`, `quazi.memcpy`, `quazi.memmove`, `quazi.memset`,
`quazi.str.as_ptr` and `quazi.str.from_ptr` intrinsics have bounded storage
semantics. Recognition uses resolved declaration/attribute identity, not API
names. Named actuals use declared slots. Copying a stored `str` handle retains
its owner dependencies; copying ordinary UTF-8 character bytes into fresh
storage does not copy that handle. String-pointer conversions preserve any
already tracked storage and uncertainty. Fill operations never erase an
existing dependency merely because bytes were overwritten.

Unknown pointer writes may affect any later memory read, and unknown reads may
observe any earlier known write. These shared facts participate in branch/loop
convergence. Known direct allocations remain distinct until uncertainty connects
them. A source helper's empty parameter-slot summary does not prove its returned
storage fresh or empty: pointer-bearing results retain conservative abstract
storage even without an input dependency. This includes raw-pointer fields in
aggregates. The model deliberately does not serialize source allocation or
effect certificates, and may retain unrelated inputs at uncertain boundaries.

These facts allow valid prelude range copies to remain independent after the
original owner is released. They do not enable new pointer casts, projected
address-of syntax, element loans, or arbitrary unsafe memory reconstruction.
Raw-pointer operations retain their existing unsafe validity obligations;
complete raw aliasing, external/global state, byte ranges and interprocedural
memory effects are not certified by these parameter-slot facts.

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
and [statement control flow](source-control-flow.md). Opaque expression regions
and unproved effects still prevent a path-sensitive ownership proof.

QZI remains v13, and its ownership envelope remains Unverified. Source result
facts are not serialized as ownership certificates. QZC v15 introduced cache
invalidation for the raw-storage dependency model; the current v18 also reruns
refined flow checks and fixes logical-result emission. Exact hits and reusable units from older
versions cannot bypass the current source checks. Historical QZI
artifacts have not acquired this source proof.

## Verification

`semantic::retention::tests` checks dependency composition, recursion, named
arguments, methods, mutation, lexical identities, loops, closures, reset, and
cancellation. Borrow regressions exercise retained aggregate roots and existing
owned-storage projection restrictions. Run
`python3 scripts/check_result_retention.py target/debug/qz` for unsafe-source
rejections during checking/building/QZI emission and valid source/QZI native
execution plus Windows COFF emission. COFF emission is not Windows execution.

`semantic::retention_memory_tests` covers known/unknown storage, copied handles,
byte copies, source/opaque helper returns, nested pointer loads, implicit
reference projections, branch/loop storage, intrinsic aliases/named slots,
string-pointer round trips, weak fills and analyzer reuse. It also locks the
existing rejection of pointer/integer casts and projected address-of syntax.
