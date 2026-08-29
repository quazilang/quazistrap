# Value-Layout Recording and the Remaining One-Slot Gates

Date: 2026-08-30

## Motivation

The generic value-shape checkpoint closed silent truncation only for generic
function parameters and results. The same one-word assumption still silently
truncated multi-register values in enum payloads (`Option[[i32; 3]]`), struct
fields, and nested fixed-array literals. Qualified enum constructor calls
(`Option.Some(x)`, `Shape.Circle(5.0)`) additionally bypassed semantic
validation entirely: unknown variants such as `Option.Nope(5)` reached code
generation and failed with an internal error instead of a diagnostic, and no
payload checking existed on that path.

This is phase 1 of the
[generic storage layout design](../internals/generic-storage-layout.md):
record layouts everywhere and gate the remaining holes, without changing the
ABI.

## Behavior

- The compiler now records the resolved internal-ABI layout of every ordinary
  function and every generic specialization (parameters, variadic element,
  result, and whether a value is a plain copy or an owner) in
  `SemanticReport::fn_value_layouts`, keyed by the canonical resolved type
  arguments rather than the lossy mangled name. Code generation does not
  consume the records yet; they are the phase-2 input.
- New `S14` gates reject multi-register shapes where storage is still one
  slot per value: enum payload declarations, struct field declarations
  (`@repr(C)` aggregates keep their real C layout), fixed-array literals with
  multi-slot elements, generic struct instantiations that produce such
  fields, and `Option`/`Result` constructor payloads on the bare, contextual,
  and qualified paths.
- Qualified enum constructor calls are now validated in analysis: unknown
  variants produce `S04` and wrong argument counts produce `S08` instead of
  an internal code-generation failure. Valid qualified constructors keep
  their historical behavior, including the untyped analysis result; typing
  them properly is a documented follow-up.

## Compatibility

Programs that stored fixed arrays or slices in enum payloads, struct fields,
or nested array literals now fail during analysis. Those programs were
silently miscompiled before; these are safety corrections, not behavior
changes. `@repr(C)` aggregates are unaffected. No QZI/QZC version changes:
artifact formats are unchanged in this phase.

## Verification

- `cargo test --offline` passes 451/451, including new unit tests for every
  gate, for the layout records (concrete, variadic, specialized, and the real
  `Array.set` signature), and for qualified-constructor validation.
- End-to-end probes confirm `Option[[i32; 3]]` is rejected on the bare,
  contextual, and qualified constructor paths and that `Option.Nope(5)`
  fails in analysis.
- All 34 example projects build with the new gates; `examples/03` runs and
  `examples/32-testing` passes 6/6.
