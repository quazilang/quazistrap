# Semantic Analysis (`src/semantic/`)

Five sequential passes:

1. **Declare** — register fns, structs, traits, enums, imports. `@cfg`-disabled items skipped.
2. **Typecheck** — scope, inference, compatibility, init checks, expression annotations.
3. **Unused** — W01 unused var, W02 unused param, W03 unused fn/import.
4. **Dead code** — reachability, W04 unreachable after return. (Merged into `unused.rs`.)
5. **Optimize** — inline candidates, const folding, math identity reduction, lazy import hints, exhaustiveness checking.

## `types_compatible` Rules

- Internal `Error` is the recovery wildcard and must not survive successful
  analysis. Source `any` is only compatible with itself and is rejected in
  value-bearing positions; final `@format ...args: any` is compiler-erased.
- Named types must have the same name and invariant generic arguments.
- Integer → float (implicit widening for literals).
- `*T` ↔ `*U` (all raw pointers mutually compatible — C void* semantics).
- Integer ↔ `*T` (null pointer constant support: `0` is valid `*T`).
- `Str` ↔ `Ref { inner: Str }` always compatible.
- Shared-reference compatibility is directional and invariant. An actual `&T`
  may auto-read into an expected representation-identical `T`; a value never
  becomes `&T`, and `&T` only matches the same resolved pointee shape.
- `Named { name }` ↔ `Dyn { trait_name }` — compatible if `trait_impls[name]` contains `trait_name`.
- `Dyn { a }` ↔ `Dyn { b }` — compatible if `a == b`.

`Some`, `None`, `Ok`, and `Err` inherit a surrounding `Option`/`Result` type.
An unconstrained partial `Result` constructor is rejected instead of reaching
code generation with an unknown payload. Pattern bindings recover builtin
generic payload types from the scrutinee. Qualified constructor calls
(`Option.Some(x)`, `Enum.Variant(x)`) are validated for variant existence,
arity, and one-slot payload representation, but their result stays untyped
until semantic constructor resolution exists; codegen lowers them
structurally.

`?` requires a resolved `Option[T]` or `Result[T, E]` operand and the same
wrapper in the nearest callable return context; success types may differ.
Resolve aliases first. Result error payloads are copied unchanged by lowering,
so check their runtime shape in both directions, not ordinary assignment
compatibility. Closures replace the surrounding return context even when their
return type is unknown; an inference `Error` placeholder cannot grant propagation.
Restore the context after body checking and clear it on analyzer reuse. Untyped
qualified constructors cannot be propagated until their result is resolved.
The separate closure-result ownership gate remains enforced.

## Value-Shape Gates

One slot per value is the current internal storage contract outside register
blocks. While the multi-slot ABI is unimplemented, analysis rejects
multi-register shapes with `S14` in every storage position: function
parameters and results (concrete and specialized), enum payload declarations
and constructor payloads (bare, contextual, and qualified paths), struct
fields outside `@repr(C)` (which has a real layout solver), generic struct
instantiations, and fixed-array literals with multi-slot elements. Ordinary
functions and specializations record their resolved parameter, variadic
element, result, and plain-copy/owner kinds into
`SemanticReport::fn_value_layouts` for the phase-2 ABI work, keyed by
canonical resolved type arguments (mangling is not injective).

After all semantic passes, generic template call edges are instantiated for
each concrete caller specialization. The closure must add both the concrete
callee monomorphization and its layout record before bytecode codegen.

## Ownership call evidence

`ownership.rs` records source call sites separately from the existing lexical
call shortcut. The concrete graph starts from non-generic declarations and
substitutes recorded targets under each concrete caller. Never seed it from
symbolic monomorphization names: bound parameters can shadow global types.
Identity is qualified callable plus tagged alias-resolved type structure;
capabilities are recomputed after substitution. Enclosing method generics are
recorded by the declare pass, not inferred from a dotted module name.

Unsafe/intrinsic/variadic, unavailable, opaque, and unresolved boundaries reject
source closure backward through recursion. This is not a CFG/place/provenance
proof, does not alter lexical loan eligibility, and is never a QZI certificate.
Source-site recording uses an ordered span index; distinct calls remain distinct.
The graph has its own callable budget. Shared specialization depth/width/count
checks and emitted-name collisions produce S14 instead of truncation or an
unchecked expansion. Symbolic helper chunks remain in the emission namespace.
All new graph work must poll cooperative cancellation.

`places.rs` snapshots call actuals using resolved declaration identities before
borrow checking. It shares declared-slot mapping with result retention and
specializes snapshot types when constructing the concrete graph. Preserve
lexical argument order independently of slots; namespace objects are not
receivers. Global declaration types resolve outside caller binders. Only
builtin array/slice indexes describe storage projections; overloads and
projections of temporaries stay unknown. Closure-body calls retain their
deferred closure spans. These facts never grant loan or artifact permissions.
Bound source bodies/projection depth and total specialized argument nodes;
report S14 rather than silently publishing truncated success.

`control_flow.rs` separately records source statement transitions before borrow
checking and shares each canonical declaration graph across concrete callers.
Include leaves without calls. Return/break/continue end their path and retain
lexical scope exits; C-style continue targets the update. Closure graphs are
deferred, never synchronous successors of creation. `evaluation.rs` supplies
operand control shared with codegen for unary/eager-binary/short-circuit/try
forms. Result events distinguish group/cast/unary/binary completion, logical
normalization and payload extraction; they prove no typed access or storage alias.
Exposed `?` uses a success-discriminant branch, never enum-handle truthiness.
Do not rescan completed wrappers and duplicate their failure edges. Unclassified
regions and iteration protocols stay opaque; conservative nested-`?` scans stop
at closure boundaries. No effect/loan/artifact flags may be inferred from
this graph. Bound aggregate construction/retained metadata, poll cancellation,
and reject resource exhaustion with S14 instead of publishing partial success.

## Transparent alias resolution

Use the shared `type_resolution` engine. Validate target-active declarations
before serialization and ownership consumers, including unused aliases.
`resolve_type_with_parameters` protects explicit declaration binders; the
current source binder stack must cover signatures as well as bodies. Deferred
resolver failures recover as `Error` and must be flushed into mandatory S14
errors before publishing a report. Clear the sink between analyses. Dependency
ranges suppress warnings, never these errors. Do not eagerly duplicate an alias
body before enforcing expansion bounds.

## Warning Suppression

`@ignore` / `@ignore(unused_vars)` / `@ignore(dead_code)` suppress W01/W02/W03/W07.

## Cooperative cancellation

The cancellable analyzer returns `Cancelled` as an operational result. It
polls at pass, top-level-item, and reachable-statement boundaries; an
individual expression remains atomic. A cancellation may leave scopes and
other semantic state partially populated, so callers must discard that
`Analyzer` rather than publish a partial report.

## `@cfg` Evaluation

Keys `target_os`, `target_arch`, `target_abi` are evaluated against the host for
normal builds. `strip_cfg_for` accepts an explicit target for cross-target tools
such as `qz header`, then removes the selected cfg markers before analysis.
Applied in: declare pass, typecheck CfgBlock, unused CfgBlock.

## Borrow Checker (`borrow.rs`)

- Move semantics for all non-primitive, non-reference types (structs, enums, arrays, slices, dyn Trait, etc.).
- S10 = use-after-move / move-in-loop.
- `reassign_targets` set suppresses move-in-loop for `x = x.method()` patterns (value immediately re-owned).
- `for x : iterable` **moves** the iterable (like Rust's `for x in collection`); borrow with `for x : &collection`.
- The iterable is consumed before the loop body so it does not trigger move-in-loop.
- C-style initialization runs once at ambient loop/control depth inside a lexical loop scope. Condition/body/update run at repeated depth, so initializer owners cannot be consumed repeatedly. Pop the initializer scope and its loans before identity-aware merging into the outer environment. Keep the existing immediate reown-assignment exception.
- Legacy method receivers remain borrowed and non-consuming. An explicit
  `self: &T` receiver is read-only: it may run while shared loans exist and
  creates a shared loan for its call. The implementation validates that an
  explicit receiver's pointee is the enclosing `impl` target. An explicit
  `self: &T!` receiver permits writes and takes an exclusive loan for its call;
  direct inherent calls to consuming `self: T` transfer the receiver, while
  broader interprocedural effects still need D-014 implementation.
- Shared references use a conservative lexical-scope lifetime checkpoint:
  address-of accepts only locals/parameters; reference bindings cannot be
  rebound or escape through returns, owned aggregates, or closures; and an
  address-taken owner cannot be mutated or moved. Explicit `self: &T` methods
  are the read-only receiver exception.
  `str`/`&str` remains the representation-identical string-view exception.
- `&T!` is currently a local exclusive-reference foundation: it requires a
  mutable local/parameter root, permits assignment through its dereference,
  and freezes owner reads, moves, mutation, and overlapping loans while its
  lexical scope is live. A resolved, non-variadic safe unqualified source
  function call derives each direct-call argument capability from its declared
  parameter: `&T` and `&T!` reborrow an existing reference capability rather than consuming it,
  while owned parameters still move. A temporary `&value`/`&value!` passed to
  that narrow source-call boundary ends after the call because current safe
  references cannot escape it. Eligibility now also requires a source-only
  fixed point over explicit direct-call facts: every reachable call must be
  another eligible unqualified, non-generic source function. Function values,
  module-qualified and method calls, unsafe/foreign calls, variadics, and QZI
  interfaces remain opaque. Generic source calls retain their existing
  template-level reference-capability rule; this does not make their QZI
  specializations verified ownership boundaries.
  This is not the D-014 whole-program effect solver; do not relax reference escape, indirect-call,
  or QZI boundaries on the basis of this syntax alone.
- Move-only projections remain rejected. The sole non-destructor exception is
  an exact `ret self.field` from a by-value inherent receiver whose non-generic,
  non-`repr(C)` source struct has no manual `free(self)` hook and whose direct
  field has compiler-generated structural cleanup. Do not extend this to
  aliases, nested projections, wrappers, call arguments, or source destructors
  without place-level move state and path-sensitive destruction.
- Metadata-derived dense-element read checks apply to `[]` and shared methods,
  including alias/reference receivers and observation without consumption.
  Indexed replacement checks only storage/index operands; it does not read the
  prior element. Generic read facts retain the caller binder context and are
  validated after concrete specialization closure through checked substitution.
  Concrete identity provenance propagates through closure, including symbolic
  registrations later proved concrete. Never infer concreteness from names.
- `Array.get` may return Plain elements by value, but an owned element is now
  rejected with S10 instead of materializing a shallow second owner. Do not
  weaken this gate by changing a receiver spelling: safe owned collection
  reads require a provenance-tracked borrowed-element API and element cleanup.
- String-view roots use declaration identity (span plus binding ordinal), name,
  and lexical depth; branch merging cannot transfer loans/moves to a same-name
  declaration. Match result origins use scoped pattern bindings seeded from the
  scrutinee, including untyped qualified enum temporaries. Borrowed parameters
  retain parameter-slot provenance; exclusive capability transfers rebase the
  active capability root, and a derived view freezes that root until scope exit.
  `retention.rs` now composes conservative source parameter-slot dependencies
  through aggregate results before lexical checking. Retained roots extend
  binding loans and reject local-owner/outward escapes; unused arguments and
  static/fresh results remain distinct. Intrinsic allocation facts use declared
  intrinsic identities, never API or type names. Destructible by-value storage
  remains a local projection root. These are conservative source facts, not
  exact projection effects, escape certificates, or verified QZI summaries;
  D-014 and D-015 remain incomplete.
- Retention storage identities are tagged lexical/allocation/unmodeled cells.
  Pointer identity and stored contents are separate; explicit and implicit
  projection loads must recover stored pointer targets. Unknown writes and
  reads use shared weak memory facts, merged through branches/loops. Declared
  memory/string-pointer intrinsics distinguish copied UTF-8 bytes from view
  handles without inferring behavior from API names. Preserve known pointees
  through string-pointer round trips; an empty source result slot summary is
  not a freshness proof. Structural storage queries include nested raw fields
  while the lexical view query retains its separate existing semantics.
- Quazi `fn` values are affine owners. Passing, returning, and assignment move
  them; calls borrow them; self-assignment is rejected. Until recursive cleanup
  and capture ownership are implemented, do not permit `fn` inside aggregates
  or generic arguments, and restrict closure captures, parameters, and results
  to immutable plain-copy scalar shapes. `fn`/`cfn` signature compatibility is
  exact at runtime boundaries rather than numeric-coercion compatible.
  Reject moves of outer `fn` owners from conditional paths until drop state is
  path-sensitive, consuming assignment expressions, and function-valued match
  results. Same-signature casts remain transparent ownership wrappers.

Named Index read/write inference unwraps a safe-reference receiver before
substituting element arguments and registering setter specializations. The
original reference capability remains authoritative for mutation and ownership
checks. Checked `parse[T]()` uses the declared `parse_<suffix>` receiver
capability, preserving explicit custom `parse` methods. Shared `&str` methods
retain the string-view exception. None of these adaptations permit owning
element reads or general reference escape.

Exclusive field receivers require actual value-slot storage. Source fields and
scalar qword C fields qualify; narrow C fields, bitfields, and inline C
aggregates produce S14 instead of borrowing a temporary or overlapping bytes.

## Generic Receiver Methods

For `Named` receivers with concrete type args, substitute receiver generics into method params before checking args (`Array[i32].push("x")` must error because `T = i32`).

`Result[T, E]?` and `Option[T]?` retain `T` in expression annotations. Codegen
also retains explicit local types so fallible construction followed by an
inherent method remains statically dispatched. Imported type identifiers take
priority over module-namespace interpretation for static constructors.

Lazy public re-exports may target either `name.qz` or `name/mod.qz`; this lets
module gateways expose directory-backed APIs such as `std.collections`.
Gateway imports of sibling files use the explicit `./name` form so dependency
or package names cannot shadow prelude and standard-library implementation files.

Generic `str.parse[T]()` and `String.parse[T]()` resolve to checked prelude
parsers during type checking. Supported primitive targets return
`Result[T, ParseError]`; unsupported targets produce S06 instead of silently
lowering to an unchecked numeric conversion.

## `pub` Visibility

- **Functions**: enforced. Private fn imported cross-module emits S04 error.
- **Structs, traits, enums, type aliases**: parsed but hardcoded `public: false`; not enforced. (P1 roadmap item.)
- **Re-exports**: `pub_import` stored but not read.

## C ABI validation

- `@api` declarations are bodyless and always unsafe to call. Bare `@api` uses
  the local function name; `@api("symbol")` is the recommended explicit form.
- `@export` requires an explicitly `pub`, non-generic Quazi body and is retained
  as a native root even without Quazi callers.
- An exact `@repr(C) type Callback = fn(...) ...` declares a raw C function
  pointer. Its signature follows the same ABI validation as `@api`/`@export`,
  calls require unsafe context, and only signature-compatible `@export`
  functions may coerce to it. Ordinary Quazi functions and closures retain
  their environment-pointer representation and cannot cross the C boundary.
- `@api("symbol") var name: Type;` imports a mutable C data symbol. Reading,
  assigning, compound-assigning, or incrementing it requires unsafe context.
  Globals currently accept C scalars, pointers, and C function pointers; C
  aggregate values are rejected because Quazi aggregates use address semantics.
  C macros and thread-local pseudo-globals must be exposed through accessors.
- Signatures accept C integer/bool scalars, pointer-sized integers, raw pointers,
  `f32`/`f64`, `void` returns, and non-generic `@repr(C)` structs by value. Bare
  C variadics validate each actual extra argument and apply default promotions.
  C function pointers are passed as pointer-sized values; other unsupported
  types emit `S14`.
- `@repr(C)` covers non-empty, non-generic structs and unions with scalar
  array fields, optional `packed`/power-of-two `align=N`, named nonzero integer
  bitfields, and final flexible array members. FAM aggregates are pointer-only;
  union field access and FAM indexing require unsafe context. Layout metadata is
  recorded in `SemanticReport`.
- `@opaque` requires an empty, non-generic struct and rejects Quazi construction.
- A panic in exported code follows the existing terminating panic path. It never
  unwinds across a C frame; recoverable ABI errors must be explicit return codes
  or out parameters.

## Source initialization checkpoint (2026-10-22)

`semantic/initialization.rs` snapshots declaration-backed initialization bits and
joins normal paths, with separate truth/falsy states, loop exits and pending
match cases. Deferred closures restore outer state. Match case/fact work is
bounded at 1,000,000 steps; exhaustion is S14. Terminal statement spans align
analysis and emission. Checking follows declared named-argument/field slots;
indirect callees and place operands precede arguments/RHS, and range bounds
precede loop binders. Place validation consumes checked annotations once.

Checked pattern discriminants are published by pattern span and used directly
by codegen. Nested refutable patterns require substituted actual payload types.
Match captures a stable scrutinee snapshot; borrowed scalar slots load, shared
&str preserves its view ABI and &str! loads. Boolean literal patterns use
zero/nonzero truthiness, while integer patterns use exact equality. Float
comparison is IEEE, string comparison uses first-NUL views. Unmatched values
trap; Void matches retain fallthrough. Constant result knowledge cannot erase
executed effects; only elidable expressions publish foldable annotations, and
unknown floating multiplication by zero is not absorbing.

The initialization checkpoint introduced **QZC22**, invalidating previous exact
and reusable
chunks for these checks and corrected execution. QZI13 remains Unverified and
unchanged; rebuild old bodies from source. Stable operand values are covered
by the subsequent QZC23 checkpoint below. Canonical Boolean storage/equality,
partial-place initialization, typed effects,
recursive destruction and verified artifacts remain open. See
[the initialization contract](../../docs/internals/source-initialization.md).

Constant propagation treats every slot in a Lea-addressed block as runtime
memory in both dataflow and rewrite. Aliased stores, calls and intrinsics
cannot leave a stale local constant. This is conservative alias handling;
non-addressable register values can still fold normally.

## Stable operand checkpoint (2026-10-22)

QZC23 rebuilds previous exact/reusable source chunks for eager value captures,
computed places and pending allocation storage. QZI13 remains Unverified.
`compile_value` freezes complete value blocks; `compile_expr` preserves actual
place storage. Adapt borrowed receivers before capture. Pending owner transfers
begin at operand evaluation and commit only at call/store. Slice pointer/length
travel together; match results retain full width. Preserve C typed access flags
when sharing place paths. Borrow checking follows declared named-argument/field
slots and scopes allocation pins separately from shared view loans. Scalar field
writes do not release a pinned aggregate. No normalized alias/effect/artifact
proof follows. See [the contract](../../docs/internals/operand-snapshots.md).

## Scalar memory checkpoint (2026-10-22)

Current source cache is **QZC24**; QZI13 remains unchanged and Unverified.
Raw f32 accesses share the C scalar descriptor and native conversion flag; safe
float references retain qword VM slots. Float ++/-- uses floating 1.0 plus
flagged Add/Sub, with a captured old value for postfix. Integer local fast paths
remain. General narrow-result normalization, f16 memory and numeric type
obligations remain separate work. See [the contract](../../docs/internals/scalar-memory.md).
