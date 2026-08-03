# Semantic Analysis (`src/semantic/`)

Five sequential passes:

1. **Declare** — register fns, structs, traits, enums, imports. `@cfg`-disabled items skipped.
2. **Typecheck** — scope, inference, compatibility, init checks, expression annotations.
3. **Unused** — W01 unused var, W02 unused param, W03 unused fn/import.
4. **Dead code** — reachability, W04 unreachable after return. (Merged into `unused.rs`.)
5. **Optimize** — inline candidates, const folding, math identity reduction, lazy import hints, exhaustiveness checking.

## `types_compatible` Rules

- `Any` ↔ everything.
- `Named` ↔ everything (generic monomorphization).
- Integer → float (implicit widening for literals).
- `*T` ↔ `*U` (all raw pointers mutually compatible — C void* semantics).
- Integer ↔ `*T` (null pointer constant support: `0` is valid `*T`).
- `Str` ↔ `Ref { inner: Str }` always compatible.
- `Named { name }` ↔ `Dyn { trait_name }` — compatible if `trait_impls[name]` contains `trait_name`.
- `Dyn { a }` ↔ `Dyn { b }` — compatible if `a == b`.

## Warning Suppression

`@ignore` / `@ignore(unused_vars)` / `@ignore(dead_code)` suppress W01/W02/W03/W07.

## `@cfg` Evaluation

Keys `target_os`, `target_arch`, `target_abi` evaluated against `std::env::consts`. Applied in: declare pass, typecheck CfgBlock, unused CfgBlock.

## Borrow Checker (`borrow.rs`)

- Move semantics for all non-primitive, non-reference types (structs, enums, arrays, slices, dyn Trait, etc.).
- S10 = use-after-move / move-in-loop.
- `reassign_targets` set suppresses move-in-loop for `x = x.method()` patterns (value immediately re-owned).
- `for x : iterable` **moves** the iterable (like Rust's `for x in collection`); borrow with `for x : &collection`.
- The iterable is consumed before the loop body so it does not trigger move-in-loop.
- Method receivers are borrowed (non-consuming).
- No explicit reference lifetimes yet.

## Generic Receiver Methods

For `Named` receivers with concrete type args, substitute receiver generics into method params before checking args (`Array[i32].push("x")` must error because `T = i32`).

## `pub` Visibility

- **Functions**: enforced. Private fn imported cross-module emits S04 error.
- **Structs, traits, enums, type aliases**: parsed but hardcoded `public: false`; not enforced. (P1 roadmap item.)
- **Re-exports**: `pub_import` stored but not read.

## C ABI validation

- `@api` declarations are bodyless and always unsafe to call. Bare `@api` uses
  the local function name; `@api("symbol")` is the recommended explicit form.
- `@export` requires an explicitly `pub`, non-generic Quazi body and is retained
  as a native root even without Quazi callers.
- Phase-one signatures accept integer/bool scalars, pointer-sized integers, raw
  pointers, and `void`. Floats, variadics, function pointers, and aggregates by
  value emit `S14` until their real SysV ABI lowering exists.
- `@repr(C)` is limited to non-generic scalar/pointer fields. Layout uses target
  size/alignment and tail padding; field offsets are recorded in SemanticReport.
- `@opaque` requires an empty, non-generic struct and rejects Quazi construction.
- A panic in exported code follows the existing terminating panic path. It never
  unwinds across a C frame; recoverable ABI errors must be explicit return codes
  or out parameters.
