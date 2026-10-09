# Compiler checkpoint resume

Audience: the next assistant continuing the current `dev` worktree.

## Checkpoint (2026-10-09, checked transparent aliases)

Verified current branch is `dev`; historical checkpoints below used `feat/test`.
Local commits: `2bce4e5` (compiler graph/aliases) and `caa5c0f` (Graphify setup).
User now explicitly authorized committing completed local changes. Preserve
unrelated changes and keep the full workspace production-readiness goal active.
The no-website clarification in `../RESUME.md` remains authoritative.

Added `src/type_resolution.rs`, a shared bounded resolver used by semantic
analysis, codegen, native ABI type lowering, and raw C-header validation.
It performs substitution lazily with lexical environments, normalizes actual
arguments before activating an alias, and rejects cycles/arity/growth before
unchecked tree copying. Protected binders cannot be renamed by nominal
canonicalization. Alias bodies do not capture caller substitutions.

Analysis preflights target-active declaration types before initial ownership and
serialization consumers. Generic binder scopes now cover function signatures,
struct/enum declarations, trait method generics, and alias definitions. Deferred
resolver errors are mandatory S14 diagnostics and reset between analyses.
Codegen shares an error sink with anonymous function compilers and rejects
malformed aliases in raw reports. Header spelling preserves callback typedefs
and target-dependent C names. This does not complete D-014 or verify QZI effects.

Verification: the pinned/bfd `cargo test --offline -q` passes 690/690; a final
trait-binder regression rerun passes after adding a global reference alias.
`cargo build --offline -q` and `cargo clippy --offline -q` succeed, with existing
warnings. Earlier full runs encountered one transient `Text file busy` test
runner failure; its isolated rerun and subsequent full suite both passed.
Resolver tests cover 14 cases; standalone header tests pass 9/9.
Native probes in `/tmp/quazi-alias-native-wwc5opt6` pass Linux execution, portable
QZI emission, Windows COFF emission, and Linux/Windows C-header generation.
Unused pointer-cycle aliases fail with S14 before codegen.

Independent agents reviewed resolver/header and semantic binder integration;
their findings were reconciled, including trait-method binder protection.
Their usage limit interrupted final review of codegen; root checked its changes
and regression coverage. Do not claim a final cross-workspace readiness review.
See `docs/changes/2026-10-09-type-alias-resolution.md` and
`docs/internals/type-alias-resolution.md` for the exact operational boundary.

## Checkpoint (2026-10-09, ownership specialization evidence)

The current workspace is `/DISK/QUAZI-PROJECTS/quazilang`, not the historical
home-directory paths below. Read `../RESUME.md`, `../goal.md`, and
`../AGENTS.md`; this is still the full production-readiness objective, with
the later clarification excluding a documentation website. Do not mark that
objective complete on the basis of this compiler checkpoint.

Resumed the prior dirty `OwnershipCallEffect` / `CallableOwnershipBodyEffects`
work without discarding it. Added a separate concrete source call graph in
`src/semantic/ownership.rs`: qualified structural identities, substituted
capabilities, source spans, explicit opaque/unavailable/unresolved boundaries,
recursive source closure, cancellation, and independent graph bounds. Source
declarations seed it; symbolic mono roots do not. Actual impl declarations now
record enclosing generics instead of guessing from dotted module prefixes.

Central emission checks reject mangled-name collisions and specialization
growth beyond 64 levels / 4,096 type nodes with S14. The graph and emission
registry have separate 16-bit callable budgets. Explicit wrong generic arity
is S08. Recorded source target structure supplements transitive generic
dependency expansion. Implicit dense-container release records its layout and
uses the same identity checks.

Current codegen still relies on some symbolic helper chunks; their names are
covered by the collision diagnostic too. Removing all template/helper emission
requires coordinated changes to generic intrinsic/mutation lowering. The source
graph neither relaxes lexical loans nor verifies a QZI envelope. Place/effect
IR, CFG/provenance, structural destruction, D-015 element loans, and artifact
verification remain required. Alias resolution also still needs independent
cycle/resource hardening; these specialization limits are not that resolver.

Verification and command details are recorded in
`docs/changes/2026-10-09-ownership-specialization-graph.md` and
`docs/internals/ownership-call-graph.md`. Twenty focused graph regressions
pass. Linux native scalar/multi-slot, Windows COFF, QZI, and negative CLI probes
pass. The complete compiler suite passes 666/666, including 20 graph tests.
Impl binder renaming/reordering is now shared with codegen, and a multi-slot
renamed receiver regression checks its hidden-sret ABI. Use the pinned Rust toolchain with
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`: its bundled lld wrapper currently points
to a missing Nix path. Give repeated full test runs a fresh TMPDIR because the
existing LSP workspace fixture can otherwise collide with a stale PID directory.

Preserve the prior Graphify setup changes and untracked example 35 build
artifacts. Canonical std has unrelated CLI work; editor and Tree-sitter
repositories remain separate. Nothing was committed or published.

## Read first

- `/home/amapekibert/quazilang/RESUME.md`
- `/home/amapekibert/quazilang/AGENTS.md`
- `/home/amapekibert/quazilang/quazistrap/AGENTS.md`
- `/home/amapekibert/quazilang/quazistrap/src/semantic/AGENTS.md`
- the full active objective linked from the workspace resume

Commit only the completed task's owned files when the user explicitly requests
it. Preserve unrelated/prior dirty changes.

Note: the rustup proxies were unreliable during this checkpoint. Use
`$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin` directly for
cargo/rustc (1.98.0).

## Implemented in this checkpoint (2026-08-30, fourth entry)

Example-build regressions are fixed and every example now compiles:

- Semantic analysis closes generic template call dependencies for each concrete
  caller specialization and records the resulting callee layout. This prevents
  valid calls such as `Array.get[str]` from being absent during bytecode
  emission.
- Codegen resolves type aliases before matching monomorphizations, so aliases
  such as `Rune` and their concrete `u32` representation select the same
  generic chunk.
- The built-in ELF linker skips allocated GNU property notes and `.eh_frame`
  metadata from native C objects while continuing to reject unsupported loaded
  sections such as TLS.
- The standard library compares `String` values through `.as_str()` in HTTP
  parsing paths, keeping comparison operand types compatible.

Verification: `cargo test --offline -q` passes 481 tests. Examples `01` through
`35` all build using the canonical `~/.quazi/std`, including the Git-backed
example `28` after its dependency fetch.

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
