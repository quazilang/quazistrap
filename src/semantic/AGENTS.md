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
generic payload types from the scrutinee.

## Warning Suppression

`@ignore` / `@ignore(unused_vars)` / `@ignore(dead_code)` suppress W01/W02/W03/W07.

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
- Method receivers are borrowed (non-consuming).
- Shared references use a conservative function-local lifetime checkpoint:
  address-of accepts only locals/parameters; reference bindings cannot be
  rebound or escape through returns, owned aggregates, or closures; and an
  address-taken owner cannot be mutated, moved, or used as a method receiver.
  `str`/`&str` remains the representation-identical string-view exception.
- Quazi `fn` values are affine owners. Passing, returning, and assignment move
  them; calls borrow them; self-assignment is rejected. Until recursive cleanup
  and capture ownership are implemented, do not permit `fn` inside aggregates
  or generic arguments, and restrict closure captures, parameters, and results
  to immutable plain-copy scalar shapes. `fn`/`cfn` signature compatibility is
  exact at runtime boundaries rather than numeric-coercion compatible.
  Reject moves of outer `fn` owners from conditional paths until drop state is
  path-sensitive, consuming assignment expressions, and function-valued match
  results. Same-signature casts remain transparent ownership wrappers.

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
