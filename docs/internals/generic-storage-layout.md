# Generic storage layout and recursive destruction — implementation design

Audience: language and tooling developers.

Status: approved direction, reviewed against the implementation; **phase 1 is
implemented** (layout model, per-function layout recording into
`SemanticReport::fn_value_layouts`, and the extended `S14` gates for enum
payloads, struct fields, nested fixed-array literals, and constructor
payloads, including qualified-constructor validation). This page turns maintainer decisions
[D-001](../decisions/README.md#d-001-generic-value-layout) (full
runtime-layout implementation), [D-002](../decisions/README.md#d-002-receiver-ownership)
(explicit `&`/`&mut`/consuming receivers), and [D-003](../decisions/README.md#d-003-destruction-and-explicit-close)
(structural destruction with a Drop hook) into a concrete compiler and
standard-library plan. It was revised after an independent adversarial review;
the phasing and the reference-model work items below reflect that review.

## Problem statement

Generic storage today is one machine word per element:

- `prelude/src/array.qz` hardcodes an 8-byte stride (`malloc 64`, `cap * 8`,
  one-register `__ptr_load`/`__ptr_store` intrinsics); `prelude/src/box.qz`
  hardcodes one 8-byte slot with `usize`-typed intrinsics. The language has no
  `size_of`/`align_of` mechanism, so prelude source cannot learn an element's
  layout even once the compiler knows it.
- The encoder multiplies the element index by a literal `8` in `ArrayLoad` /
  `ArrayStore` (`src/backend/x86_64/encoder.rs`); QZI carries no stride or
  layout metadata (`src/bytecode/chunk.rs`).
- Since the generic value-shape checkpoint, the semantic gate
  `validate_specialized_internal_abi` (`src/semantic/typecheck.rs`) rejects
  multi-register shapes in generic specializations with `S14`. That closed
  silent truncation **for generic function parameters and results only**.
  Multi-slot shapes remain ungated and miscompiled in enum payloads
  (`Option[[i32; 3]]` truncates; enum storage is discriminant + 8 bytes per
  payload via `enum_variant_alloc_size`), in struct fields (uniform
  `fields.len() * 8` model computed in `src/semantic/mod.rs`), and in nested
  fixed-array literals (one slot reserved per element regardless of element
  width). Closing those holes is part of this design, not a separate problem.
- Named aggregates are one-word heap handles. `Array.get` shallow-aliases the
  handle (a second apparent owner), `Array.set` overwrites without destroying
  the replaced element, and `Array.free` releases only the backing buffer.
  `std.net.Headers` (`Array[Header]`, owned `String` fields) and
  `std.random.choose`/`shuffle` depend on today's shallow behavior and leak or
  alias by design. Several `std.net` aggregates (`UdpDatagram`, `Url`,
  `HttpRequest`, `HttpResponse`) contain owned fields with no destructor at
  all, and `headers()` accessors move owned fields out of borrowed receivers.

## Decided semantics

1. Every type has a compiler-known layout: slot count, byte size, alignment,
   and a move/drop kind (`Plain`, `Owned`, or aggregate of owned parts).
2. The internal generic ABI passes values according to their layout. Scalars
   and handles keep one slot. Fixed arrays occupy contiguous register blocks
   (flat one-slot-element locals already work this way); blocks larger than
   the per-value register-block cap fall back to indirect storage. Slices keep
   pointer+length.
3. Destruction is structural: the compiler destroys owned fields and elements
   recursively in reverse declaration order, after running the type's explicit
   Drop hook (today's `free(self)` convention, formalized). Moves, returns,
   and reassignment keep exactly-once semantics; explicit `free()` is a
   move-out that suppresses automatic destruction. Panic terminates without
   unwinding; destructor guarantees cover normal control flow only.
4. Method receivers are explicit: `self: &T` shared borrow, `self: &mut T`
   exclusive, `self: T` consuming. Container element APIs become
   ownership-correct: `get` borrows, `set` destroys the replaced element
   (legal only under an exclusive receiver), and `remove` moves an element
   out.

## Design

### 1. Layout model (`src/runtime_layout.rs`)

Extend `RuntimeValueLayout` (or add a sibling query) so that, given an
alias-resolved, generic-substituted `TypeKind`, the compiler can compute:

- `slots: usize` — virtual-register slots when passed by value.
- `size: usize`, `align: usize` — heap footprint for container storage.
- `move_kind` — `Plain` (bit-copyable) versus `Owned` (shallow copy creates a
  second owner; move must deactivate the source).
- drop requirements — whether the value, its fields, or its elements need
  destruction.

The query must serve every value-storage subsystem, not only generics: enum
payloads, struct fields, and fixed-array literal elements are in scope (see
the problem statement). The C layout solver (`ffi_type_size_align`,
`ffi_aggregate_layout`, `align_up` in `src/semantic/mod.rs`) already computes
size/alignment for `@repr(C)` aggregates and is the direct precedent. Quazi
aggregates keep their existing uniform field layout in this milestone; only
element/payload handling and drop metadata change.

Two new compiler intrinsics, tentatively `quazi.size_of[T]()` and
`quazi.align_of[T]()`, expose the recorded element layout to prelude source so
`Array`/`Box` allocation arithmetic (`__array_malloc(64)`, `cap * 8`,
`__box_malloc(8)`) stops hardcoding 8. New intrinsic IDs must be allocated
(IDs 17 and 22 are currently unallocated) and added to the QZI whitelist in
`src/bytecode/chunk.rs`.

### 2. Recording layouts per specialization and declaration

`validate_specialized_internal_abi` (`src/semantic/typecheck.rs`) stops being
a pure gate and becomes a layout recorder: for every monomorphized
function/method it computes substituted parameter, variadic element, and
result layouts and stores them in `SemanticReport`. Details that matter:

- Layout records are keyed by the **canonical substituted type structure**,
  not by mangled name: mangling maps non-alphanumeric characters to `_`, so
  distinct types can collide. Making mangling injective is deferred because
  mangled names cross QZI named relocations and changing them is itself an
  artifact-compatibility break that must ride the QZI v8 bump.
- The recorder also covers concrete (non-generic) declarations, replacing the
  parallel concrete gate, so phase 2 has layouts for ordinary multi-slot
  functions too.
- The `Array.set` call site currently synthesizes a fake `[T]`-only parameter
  list; as a recorder it must resolve the real `Array.set` signature with the
  receiver substituted, like the other call sites.
- The currently unused `_variadic` parameter is the hook for variadic-element
  layout recording and must be used.
- Only `Unsized` and `Unrepresentable` shapes remain `S14` errors at recording
  sites.

### 3. Multi-slot internal ABI (phase 2, with the format bumps)

- Parameter binding in `compile_fn_with_subst` (`src/bytecode/codegen.rs`)
  reserves each parameter's recorded slot count. Recorded layouts drive both
  callee-body binding and call-site expansion, while register allocation pins
  every adjacency-sensitive block.
- Results wider than one slot use a hidden sret pointer in `r0`; the callee
  writes its value block from fixed ABI registers `r1..rN` into that buffer.
  The caller reserves and pins the buffer before the call.
  Two distinct limits apply: the per-value register-block cap
  (`fixed_array_block_length`) — beyond which the value is passed by heap
  handle with ownership transferred, exactly how named aggregates already
  work — and whole-frame register pressure (`alloc_reg` hard-errors at 255),
  which the fallback cannot fix and which must be reported as a compile error
  rather than overflow.
- Multi-slot variadic elements remain `S14` in this milestone: stride-aware
  variadic packing and the meaning of the length register (elements versus
  slots) are deferred deliberately.
- The unmangled generic *template* chunks compile with an empty substitution
  and therefore assume one slot per value. They are never a valid fallback for
  a concrete call: codegen resolves required specializations to emitted
  mangled chunks and reports a code-generation error when one is absent.
  This includes direct calls, module-qualified calls, inherent-method
  dispatch, and `Index` reads. Silently compiling against a one-slot template
  is prohibited because it truncates concrete multi-slot values.

### 4. Container storage and element access (phase 4, after D-002 receivers)

- `Array[T]`/`Box[T]` use `size_of` for allocation, and multi-slot
  `ArrayLoad`/`ArrayStore` carry their concrete element-slot count as stride
  metadata. The encoder copies the full element block. Typed copy/drop helpers
  are still required for owned element semantics.
- `get` returns a borrow `&T`. This is the largest semantic work item in the
  design and is itemized honestly — each piece is new machinery:
  1. Lift the ban on returning non-string references for references derived
     from a borrowed receiver or parameter (`typecheck.rs` currently rejects
     all of them).
  2. At call sites whose result is such a derived reference, mark the source
     container borrowed (today only literal `&x` expressions mark borrows).
  3. Replace today's never-expiring function-long `borrowed_at` with
     scope-granular borrow expiry, or ordinary loops of `arr.get(i)` calls
     reject themselves on the second iteration.
  4. Distinguish shared from mutating methods in the borrowed-owner rules
     using the D-002 markers, so `arr.len()` stays legal while a shared
     borrow is live.
  5. Add `&mut T` to the type system (`TypeKind::Ref` has no mutability) and
     to generic parameter lists, which are bare identifiers today.
  6. Element deref loads: aggregate dereference is rejected today, so reading
     a `&[i32; 3]` element needs the typed-load helpers.
- Reallocation invalidation: an outstanding shared borrow **freezes the
  container** — `push`, `set`, and `remove` take `&mut self` and are rejected
  while any borrow derived from the container is live. This is the only sound
  rule within the lexical model and it is stated as user-visible semantics,
  not an implementation detail.
- `remove(i) T` moves an element out with order-preserving shift-down
  semantics: the vacated slot is overwritten by shifting the tail, `len`
  decreases, and no liveness bitmap or hole state exists. (`take`-with-holes
  was rejected: every hole representation — null sentinel, bitmap,
  swap-remove — either fails for inline multi-slot elements or changes
  observable order.)
- `set(i, v)` destroys the replaced element before installing the new one; it
  is sound only because its `&mut self` receiver guarantees no outstanding
  element borrows.
- `get` is borrow-only in this milestone. A `cloned` convenience for `Clone`
  types is deferred: the language has no trait bounds (generic parameters are
  bare identifiers) and no type implements `Clone` anywhere today, so
  clone-conditional APIs are unexpressible.
- `free` (the Drop hook) destroys every live element, then releases the
  backing buffer.

### 5. Structural drop glue (phase 3)

- The existing source-level RAII pass (drop locals, move deactivation,
  replacement-drop, scope-exit cleanup in `codegen.rs`) stays the mechanism;
  `Move`/`Drop`/`Dup` opcodes remain reserved future work (emitting them
  hard-errors the backend today).
- `drop_action_for_type` generalizes from "exact named local type has `free`"
  to a compiler-synthesized destructor per owned type: run the type's own
  Drop hook if declared, then destroy owned fields in reverse declaration
  order. The unwired `Drop` trait in `prelude/src/traits.qz` is removed in
  the coordinated prelude change; `free(self)` remains the recognized hook
  and never runs through trait dispatch.
- Enums destroy the active payload by discriminant, and enum payload layout
  becomes layout-driven like container elements (phase 1 gates multi-slot
  payloads with `S14` until then).
- Place-level move suppression is a required design piece, not a detail.
  Today a `match` arm or `Option.unwrap` moves a payload out of an enum
  shallowly while the enum itself remains owned, and `std.net` moves owned
  fields out of structs (`headers()` accessors) with partial moves explicitly
  untracked. Under structural destruction those patterns double-destroy. The
  rules:
  - An owned place cannot move out of an aggregate by value; field and
    payload access borrows (phase 4) or consumes the whole aggregate.
  - A `match` that moves payloads out requires a consumed scrutinee; the
    remaining allocation (the husk) is destroyed after the arm completes,
    without re-destroying moved payloads.
  - Matching a borrowed enum cannot move payloads at all.
- Generic drop glue: when a container specialization `Array[T]` has an owned
  element type, the compiler emits the element-destruction loop into the
  specialized `Array.free<T>` chunk (a compiler-recognized destructor
  template, not prelude source, because the prelude cannot dispatch on
  element ownership). Semantic must record destructor monomorphizations for
  owned element/field types; today only explicit source calls to `.free` are
  recorded.
- `dyn Trait` destruction is explicitly deferred: vtables have no destructor
  slot, so owned concretes behind `dyn` keep leaking. Adding a drop slot is a
  vtable-layout change that would ride a future QZI bump; the limitation is
  documented rather than hidden.

### 6. QZI/QZC boundary — lands at phase 2, not later

- Phase-2 output already breaks the old boundary: new stride/size intrinsics
  are rejected as unknown intrinsic IDs by the current reader, and
  mangled-only dispatch changes artifact contents. QZI therefore bumps to
  **v8 at phase 2** (element stride/layout metadata, new intrinsic IDs,
  mangled-only generic dispatch assumptions, any mangling-injectivity change
  adopted from §2), and drop-glue chunk marking joins in phase 3 under the
  same major version policy.
- QZC bumps to **v6 at phase 2** as well. `semantic_context_hash` fingerprints
  declarations, but phases 2-3 change how the *same declarations* lower, and
  `compiler_identity` does not change during development; a phase-1 cache
  would otherwise be admitted into a phase-2 pipeline mixing one-slot and
  multi-slot chunks. (Optionally also extend the semantic-context domain
  separator with a layout-ABI version.)
- Pre-v8 artifacts that rely on one-slot generic element handling require a
  source rebuild; the reader rejects them explicitly, matching the existing
  strict-gate pattern (Lea metadata, unsigned flags, affine closures).

### 7. Standard-library migration (`std` repository)

- `std.net.Headers`: `append`/`set` keep by-value `Header` (moved in);
  `get`/`get_at`/`encode` switch to borrowed access; `Headers` gains an
  explicit Drop hook and `Header` becomes structurally destroyed. Behavior
  change: previously leaked strings are now freed.
- `std.net` aggregates: `HttpRequest`/`HttpResponse.headers()` currently move
  an owned field out of a borrowed receiver and `HttpRequest.encode` mutates
  an aliased copy (observably appending `Host`/`Content-Length` into the
  alias); these become borrowed accessors or consuming methods under the
  place-move rules. `UdpDatagram`, `Url`, `HttpRequest`, and `HttpResponse`
  gain structural destruction.
- `std.random.choose` returns a borrowed element (this requires references
  derived from *parameters*, not only receivers — piece 1 of the §4
  machinery); `shuffle` uses borrowed swaps instead of alias-then-overwrite.
- `Map`/`Set` insertion moves from returned shallow copies to `self: &mut`
  in-place mutation returning `Result[void, E]` (D-002).
- Every signature change gets a migration note in `docs/migrations/`. The
  phases also falsify statements in existing docs, which must be updated in
  the same change: `docs/TYPES_AND_MEMORY.md` (free-idempotence wording) and
  `docs/migrations/references.md` (the no-returned-references rule gains the
  derived-reference exception).

## Phasing

Each phase is independently testable and lands behind the existing strict
gates:

1. **Layout recording + extended gates** — extend `runtime_layout`; turn the
   S14 gate into a recorder keyed by canonical types (concrete declarations
   and the real `Array.set` signature included); gate multi-slot enum
   payloads, struct fields, and nested fixed-array literals with `S14` so the
   remaining silent-truncation holes close without an ABI change. No ABI
   change, no QZI/QZC bump.
2. **Multi-slot generic ABI + size intrinsics** — register-block
   parameters/results driving call sites *and* callee bodies, indirect
   fallback beyond the per-value block cap, compile error at the frame cap,
   mangled-only dispatch with hard errors for every missing specialization
   (including the two Index-read fallthroughs), `size_of`/`align_of`
   intrinsics, stride-correct prelude storage. **QZI v8 and QZC v6 land
   here.** Exit criterion: `Array[[i32; 3]]` round-trips natively.
3. **Structural destruction** — synthesized destructors, layout-driven enum
   payloads with husk rules, place-move restrictions, destructor
   monomorphization recording, drop glue in container specializations,
   `Drop` trait removal.
4. **Ownership-correct element APIs + std migration** — D-002 receivers
   across the prelude and `std`, the §4 borrowed-access machinery, `remove`,
   dropping `set`, std migration with migration notes and dependent doc-page
   updates.

## Alternatives considered

- Temporary plain-copy restriction: safe and small, but source-breaking for
  `std.net.Headers` and permanently defers the owned-element soundness hole;
  rejected by D-001.
- Boxed generic elements (every element heap-allocated): avoids ABI work but
  taxes every container with allocation, still needs recursive destruction,
  and does not fix fixed-array values; rejected by D-001 in favor of the full
  layout route.
- `take`-with-holes instead of shift-down `remove`: every hole representation
  either fails for inline multi-slot elements or changes observable order;
  rejected (§4).

## Testing strategy

- Native round-trips: `Array[[i32; 3]]`, `Box[[i32; 3]]`, nested fixed
  arrays, slices as generic arguments, `Option[[i32; 3]]` after phase 3.
- Destructor exactness: counted Drop hooks through nested structs, enums
  (including consuming matches and husks), `Array[String]`, replacement
  (`set`), `remove`, early return, and branch paths; allocator
  instrumentation or poisoning where feasible.
- Borrow discipline: `get` results cannot escape, rebind, or mutate through a
  shared borrow; container freeze rejects `push`/`set`/`remove` while a
  derived borrow is live; consuming APIs reject use-after-move; place-move
  restrictions reject field/payload move-outs.
- Compatibility: QZI v8 gate tests against the historical golden fixtures
  (they remain valid v2-v7 inputs), QZC v6 invalidation tests.
- Coordinated std verification: `std.net` header round-trips and
  `std.random` choose/shuffle under the new ownership rules, each repository
  tested independently, then together.

## Risks

- The Array element-access surface is larger than four paths: `emit_lvalue_load`
  indexing, `compile_expr_inner` Index reads (never mangles today),
  `emit_lvalue_store`, `compute_lvalue_addr`, assignment-statement indexing,
  for-loop lowering (currently dormant — the type checker rejects named
  `Array[T]` iterables), and the iterator protocol. Every one must honor the
  new element semantics, and every missing specialization must be a hard
  error, not a fallback.
- Shallow aliasing is load-bearing in current `std`; phase 4 deliberately
  changes live behavior (including `HttpRequest.encode`'s observable alias
  mutation) and must land with the std migration in the same coordinated
  change.
- Register-frame pressure from multi-slot parameters can push ordinary
  functions over the 255-register frame cap; that must surface as a compile
  error, and the indirect fallback only covers per-value block size.
- Emitting the reserved `Move`/`Drop`/`Dup` opcodes hard-errors the backend
  today; this design avoids them, and any future lowering must add encoder
  arms first.
