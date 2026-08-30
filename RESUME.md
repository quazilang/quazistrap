# Compiler checkpoint resume

Audience: the next assistant continuing the current `feat/test` worktree.

## Read first

- `/home/amapekibert/quazilang/RESUME.md`
- `/home/amapekibert/quazilang/AGENTS.md`
- `/home/amapekibert/quazilang/quazistrap/AGENTS.md`
- `/home/amapekibert/quazilang/quazistrap/src/semantic/AGENTS.md`
- the full active objective linked from the workspace resume

Do not commit this checkpoint unless the user explicitly authorizes the exact
commit scope. Preserve unrelated/prior dirty changes.

Note: the rustup proxies were unreliable during this checkpoint. Use
`$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin` directly for
cargo/rustc (1.98.0).

## Implemented in this checkpoint (2026-08-30, third entry)

Phase 2 of the generic-storage design is implemented and verified:

- Codegen consumes canonical layout records through a derived mangled-name
  lookup. Multi-slot parameters reserve contiguous registers; multi-slot
  results use a hidden sret pointer and fixed `r1..rN` result slots.
- `ArrayLoad`/`ArrayStore` encode element slot count for stride and block copy.
  Array and Box wrappers lower concrete multi-slot storage directly rather
  than invoking the one-slot generic template.
- The allocator preserves multi-slot store sources, sret load effects, and
  return slots through DCE and register remapping. Missing specializations are
  code-generation errors, not template fallbacks.
- The contained LSP gained semantic identifier completion and UTF-16 position
  conversion; `docs/tooling/lsp.md` documents capabilities and limits.

Verification: `cargo test --offline` passes 465 tests, the native
`examples/35-multi-slot-arrays` project prints all expected triples, and a
`Box[[i32; 3]]` round trip exits zero. Targeted rustfmt checks and
`git diff --check` pass.

The still-open Phase 2 representation issue is values wider than the 255-slot
register-block limit. It requires an explicit indirect-value design; do not
relax the current compile error without that representation and tests.

## Prior checkpoint (2026-08-30, second entry)

Phase 1 of the generic-storage design (`docs/internals/generic-storage-layout.md`):

- `src/runtime_layout.rs`: `MoveKind` (Plain/Owned), `LayoutInfo`,
  `FnValueLayout`, `byte_size`/`align` queries, with unit tests.
- `SemanticReport::fn_value_layouts`: every concrete function and generic
  specialization records resolved parameter/variadic-element/result layouts
  during analysis (`record_fn_value_layout` in `typecheck.rs`). Keys are the
  internal name plus canonical resolved type arguments (`identity<i32>`),
  never the lossy mangled name. The `Array.set` index-assignment site now
  resolves the real symbol signature instead of a synthetic element-only one;
  concrete functions are recorded after `fn_name` resolution, excluding the
  erased `@format` pseudo-parameter.
- New `S14` gates (all guarded to skip the `Error` recovery type): enum
  payload declarations, struct field declarations (exempting `@repr(C)`,
  which has a real layout solver), fixed-array literals with multi-slot
  elements, generic struct instantiations via `StructInit`, contextual sum
  constructors, and direct `Some`/`Ok`/`Err` calls.
- Qualified enum constructor calls (`Option.Some(x)`, `Shape.Circle(5.0)`)
  are now validated in the `MethodCall` arm: unknown variant/associated
  function → `S04`, wrong arity → `S08`, unstorable payload → `S14`.
  Previously they fell through every resolution path into an untyped value
  and could reach internal codegen errors (`Option.Nope(5)`). Valid
  constructors keep the historical untyped result; the intercept preserves
  `reject_nested_owned_function_expression` and associated-function
  resolution on enum types.
- Change record `docs/changes/2026-08-30-value-layout-recording.md`; audit,
  design doc status, and AGENTS.md notes updated.

## Current verified state

- `cargo test --offline`: 451/451 (439 baseline + layout unit tests + gate
  and recorder tests).
- All 34 example projects build; example 03 runs; 32-testing 6/6.
- End-to-end probes: `Option[[i32; 3]]` rejected on bare/contextual/
  qualified paths; `Option.Nope(5)` fails in analysis; declaration gates and
  the `@repr(C)` exemption verified; nested array literals rejected.
- `git diff --check` passes; owned files are rustfmt-clean (whole-repo fmt
  remains red only on pre-existing unrelated sections).

## Review status

The golden-fixture/v2-reader review and the design-doc adversarial review
were reconciled earlier today. The phase-1 implementation diff is assigned
to a fresh read-only review; reconcile before phase 2.

## Next up

1. Phase 2: consume `fn_value_layouts` — register-block parameters/results
   in call sites and callee bodies, indirect fallback beyond the per-value
   block cap, mangled-only dispatch with hard errors (incl. the two
   Index-read fallthroughs), `size_of`/`align_of` intrinsics, stride-correct
   prelude storage. QZI v8 and QZC v6 land here.
2. Documented follow-up: qualified enum constructors are validated but
   untyped; semantic constructor resolution would type them properly.

## Current verified state

- `cargo test --offline`: 439/439.
- `git diff --check` passes.
- `cargo fmt -- --check` remains red on pre-existing unrelated formatting
  differences (`backend/target.rs`, `lexer/mod.rs`, `loader.rs`,
  `parser/format.rs`, and prior dirty sections). Do not reformat unrelated
  files; format only owned lines.

## Review status

Both checkpoint reviews completed and were reconciled:

- Reader/fixture review: verified the v2 fix byte-exact against the v2 writer,
  hand-checked every fixture assertion, and confirmed no downstream safety
  hole from the v2 flags=0 default. Fixed its three findings: a duplicated v4
  assertion (now asserts the real export-adapter chunk name), vacuous
  v6-lib relocation wording (v6 executables now assert non-empty relocations;
  docs corrected), and a missing cross-era link-conflict caveat in the README.
- Design review: found five HIGH and five MEDIUM issues in the first design
  draft (no size mechanism for prelude allocation; borrowed-`get` incoherent
  with function-long borrows and reallocation; place-level move double-destroy;
  QZI/QZC bumps misplaced at phase 4; unexpressible Clone-conditional APIs;
  plus the enum-payload storage subsystem, `take` holes, template-elimination
  breadth, the fake `Array.set` recorder signature, and doc-drift nits). The
  design doc was revised to address all of them: `size_of`/`align_of`
  intrinsics, an itemized six-piece borrowed-access machinery list with a
  container-freeze rule, place-level move-suppression rules, QZI v8/QZC v6 at
  phase 2, borrow-only `get`, shift-down `remove`, enum payload layout, and an
  expanded std migration inventory.

## Next up

1. Phase 1 of the generic-storage design: extend `src/runtime_layout.rs` into
   a size/alignment/move/drop model; turn `validate_specialized_internal_abi`
   (`src/semantic/typecheck.rs`) into a per-specialization layout recorder
   keyed by canonical substituted types (concrete declarations included; the
   `Array.set` call site must resolve the real signature; use the `_variadic`
   hook); gate multi-slot enum payloads, struct fields, and nested fixed-array
   literals with `S14`. No ABI change in phase 1.
2. Phase 2 lands QZI v8 and QZC v6 together with register-block
   parameters/results, mangled-only dispatch with hard errors (including the
   two Index-read fallthroughs), and `size_of`/`align_of` intrinsics.
3. Known design hazards are mapped in the design doc's Risks section; the
   dormant `for x : array` path (type checker rejects named iterables) is now
   documented in `src/bytecode/AGENTS.md`.
