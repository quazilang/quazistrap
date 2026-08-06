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
- An exact `@repr(C) type Callback = fn(...) ...` declares a raw C function
  pointer. Its signature follows the same ABI validation as `@api`/`@export`,
  calls require unsafe context, and only signature-compatible `@export`
  functions may coerce to it. Ordinary Quazi functions and closures retain
  their environment-pointer representation and cannot cross the C boundary.
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
