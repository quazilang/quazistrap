# Layout Intrinsics and QZI v8

Date: 2026-08-30

## Motivation

`Array[T]` and `Box[T]` allocation code used literal eight-byte element sizes.
That matched the historical one-slot ABI but gave prelude source no way to use
the compiler's resolved layout model. Phase 2 of the generic storage design
needs a source-visible layout query before container storage can become
stride-correct.

## Behavior

- The prelude now exposes `size_of[T]()` and `align_of[T]()` through
  `prelude.layout`.
- Code generation resolves `quazi.size_of` and `quazi.align_of` during
  monomorphization and emits constants for the concrete type argument.
- `Array[T]` and `Box[T]` allocation paths use `size_of[T]()` instead of
  hardcoded allocation sizes. Existing `Array` load/store opcodes still use the
  current one-slot element access path; multi-register generic function
  boundaries remain rejected until the rest of phase 2 lands.
- QZI output is now v8 and QZC caches are now v6.

## Compatibility

Older compilers must reject QZI v8 artifacts because they do not know the new
layout intrinsic IDs. Current compilers keep the existing compatible legacy QZI
reader behavior. QZC v6 ignores older incremental caches automatically.

No source migration is required for ordinary scalar or handle-backed container
code. Programs that try to pass or return multi-register generic values remain
diagnosed with `S14`.

## Verification

- `cargo test --offline layout_intrinsics_compile_to_specialized_constants`
  verifies that `size_of[[i32; 3]]()` lowers to `24` and `align_of[i32]()` lowers
  to `8` in monomorphized intrinsic chunks.
