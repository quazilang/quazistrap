# Compiler roadmap checklist — 2026-10-20

Revalidated on 2026-10-23 against the
[typed-power checkpoint](../changes/2026-10-23-typed-power.md).
Previous committed source: `64b2760`.
README statuses are updated before committing this checkpoint, as requested.

**Complete: 73.2%. Remaining: 26.8%.**

Scope: all 33 P0–P5 roadmap items in `AGENTS.md`. The 22 implemented
items retain their existing scope; partial items are expanded below.
The outdated formatting/FFI labels are reconciled against current
implementation checkpoints. D-014 supersedes the old suggestion of
explicit source lifetime syntax. This does not score the separate
workspace-wide editor/documentation production-readiness milestones.

Percentages are calculated from the explicit requirements below, not assigned
from labels such as "partial". A checked requirement has a concrete implementation
or a documented regression fixture. Unimplemented, incomplete, unproven final
validation, and currently failing requirements remain unchecked. Checked source
features do not claim that every runtime/platform combination passes.

Each plan item has equal weight. Within an item, each listed requirement has equal
weight. Item completion is `checked requirements / listed requirements`; overall
completion is the arithmetic mean of item completion fractions, using unrounded
fractions. Remaining is `100% - completion`. Display rounds to one decimal place.
The checklist breakdown is explicit review judgment; the arithmetic is exact for
this checklist. It measures implementation coverage, not remaining effort or a
release-readiness probability. New requirements change the denominator.


This refresh preserves the [2026-10-13 checklist](2026-10-13-readme-plan-checklist.md)
and its 33-item weighting. Four LSP gates previously blocked by failing
current-checkout regressions now pass. No requirement was removed or narrowed;
no credit is added for unfinished ownership-control/effect foundations. Historical
failures in the earlier snapshot remain historical evidence, not current failures.
The initial refresh used `d72a1c5`; after the typed-propagation fix in `7e151aa`,
all 861 tests passed with the same checklist credit. The seven added regressions
strengthened existing correctness requirements without completing a new roadmap
item. The subsequent loop-cleanup/CFG checkpoint passes 872 tests, including
11 new regressions, with the same 33 items and 78 requirement texts. Its fixes
strengthen existing lexical-cleanup and loop-move coverage; they do not complete
a new Ownership requirement. See the [typed-propagation change record](../changes/2026-10-20-typed-try-propagation.md)
and [loop-cleanup verification](../changes/2026-10-20-loop-cleanup.md).

The 2026-10-21 conditional-cleanup checkpoint passes 873 tests and 31 additional
native/QZI cases. Runtime flags fix skipped initialization, untaken movement
and selected match transfer; pending argument failures retain their owners.
These strengthen the existing lexical-cleanup requirement. Ordinary-read
definite initialization, complete structural destruction and interprocedural
ownership proof remain open, so all 33 item weights and 78 requirements retain
the same credit: **73.2% complete, 26.8% remaining; Ownership 9/16, LSP 16/20**.

The 2026-10-22 source-initialization checkpoint passes **897 tests**, rejects
39 invalid sources through check/native/QZI/COFF, and passes 57 native/QZI/COFF
checks plus unmatched-value traps and old-cache rejection/rebuild/exact-hit
execution. Declaration-based initialization joins structured normal paths;
checked operand schedules, stable match snapshots and address-taken constant
facts align analysis and execution. This strengthens existing correctness
coverage, without completing interprocedural loans/effects, partial-place state,
recursive destruction or verified artifacts. All 33 item weights and 78
requirements retain the same credit: **73.2% complete, 26.8% remaining**.

The subsequent stable-operand checkpoint passes **904 tests**, 27 native/QZI/COFF
checks, 6 invalid lifetime cases, and old-cache rebuild/exact-hit execution.
Captured value blocks, stable computed places, scoped allocation pins and pending
receiver/field transfers strengthen existing correctness coverage. General
projection/effect proof and recursive destruction remain incomplete; no new
requirement receives credit. Completion remains **73.2%**, remaining **26.8%**.
README statuses and this audit are refreshed before the commit.

The scalar-memory checkpoint passes **906 tests** and **13 native/QZI/COFF**
checks, with safe previous-compiler wrong-result/cache coverage. Raw f32 access
width/conversion and floating increment/decrement now agree with the scalar
storage model. Generic/alias and VM-reference controls pass. This adds no new
roadmap requirement credit: general numeric result normalization, ownership
place/effect proof and recursive destruction remain incomplete. The unchanged
rubric stays **73.2% complete, 26.8% remaining**. README statuses are refreshed
before this commit too.

The numeric-mutation checkpoint passes **923 tests**, rejecting 24 invalid
programs across check/native/QZI/COFF and passing 7 native/source-removed-QZI/COFF
checks plus 3 borrowed descriptor bounds traps. Concrete generic mutation
obligations, numeric reference loads and referenced builtin element types
strengthen existing correctness coverage. General arithmetic/coercion and
ownership/effect proof remain incomplete. No requirement changes credit:
**73.2% complete, 26.8% remaining; Ownership9/16, LSP16/20**. README statuses
are refreshed before this commit; installed-release and STD memory6 pass.

The arithmetic checkpoint passes **934 tests**, rejects 34 invalid sources
through check/native/QZI/COFF and passes 7 native/source-removed-QZI/COFF groups.
A dynamic C-export remainder passes 1,019 host bit comparisons per native/QZI
path. Declaration-slot arithmetic constraints, reference representation and
floating remainder strengthen correctness. Bitwise/Pow constraints, coercion,
normalization and complete ownership/effect proof remain open. Credit remains
**73.2% complete / 26.8% remaining**, Ownership9/16,LSP16/20. Installed release,
STD memory6 and ordinary quazifetch build/run/cache/COFF pass. README statuses
are updated before this commit.

The operator-requirement checkpoint passes **939 tests**, rejects 37 invalid
programs through check/native/QZI/COFF, and passes 5 value groups plus 2 real
String comparisons through native/source-removed QZI/COFF. Known Boolean
reference loads and declaration-slot category checks strengthen correctness.
Power, String runtime identity and complete effects remain open. No requirement
credit changes: **73.2% complete / 26.8% remaining**, Ownership9/16,LSP16/20.
Installed release, STD memory6 and quazifetch build/run/cache/COFF pass. README
statuses are updated before this commit.

The typed-power checkpoint passes **953 tests**, rejects 12 invalid programs
through check/native/QZI/COFF, and passes 8 value groups plus 13 native/QZI
language panic scenarios. Dynamic signed/unsigned integer exports remain exact;
floating results use the documented approximation. Old source caches rebuild
under QZC28; QZI14 validates floating operand metadata. This strengthens existing
operator correctness without completing another roadmap requirement. String
runtime identity, general coercion/normalization and full ownership/effect proof
remain open. Credit remains **73.2% complete / 26.8% remaining**, Ownership9/16,
LSP16/20. Installed release, STD memory6 and quazifetch build/run/cache/COFF pass.
README statuses are updated before this commit.

## Summary

| Plan item | Implemented requirements | Complete | Remaining |
| --- | --- | --- | --- |
| Enhanced Crash/Panic handler | 1/1 | 100.0% | 0.0% |
| Fix encoder silent fallback | 1/1 | 100.0% | 0.0% |
| Fix non-slice iterator codegen | 1/1 | 100.0% | 0.0% |
| For-loop move semantics | 1/1 | 100.0% | 0.0% |
| Index assignment | 1/1 | 100.0% | 0.0% |
| Human-readable move errors | 1/1 | 100.0% | 0.0% |
| Module function namespacing/mangling | 1/1 | 100.0% | 0.0% |
| Mandatory safe indexing checks | 1/1 | 100.0% | 0.0% |
| Bitwise operators | 1/1 | 100.0% | 0.0% |
| Loop control | 1/1 | 100.0% | 0.0% |
| else-if chains | 1/1 | 100.0% | 0.0% |
| unsafe block sugar | 1/1 | 100.0% | 0.0% |
| AOT cfg stripping | 1/1 | 100.0% | 0.0% |
| Built-in linker | 7/9 | 77.8% | 22.2% |
| qz test runner | 1/1 | 100.0% | 0.0% |
| pub on types | 1/1 | 100.0% | 0.0% |
| Unified formatting | 1/1 | 100.0% | 0.0% |
| Raw backtick string literals | 1/1 | 100.0% | 0.0% |
| C/Rust-style string escapes | 1/1 | 100.0% | 0.0% |
| C ABI FFI phase 1 | 1/1 | 100.0% | 0.0% |
| C ABI FFI phase 2 | 1/1 | 100.0% | 0.0% |
| Threshold-based auto-inline | 1/1 | 100.0% | 0.0% |
| Cross-basic-block const folding | 1/1 | 100.0% | 0.0% |
| Strength reduction | 0/1 | 0.0% | 100.0% |
| macOS Mach-O backend | 0/3 | 0.0% | 100.0% |
| Ownership / RAII bytecode | 9/16 | 56.2% | 43.8% |
| Atomic opcodes | 0/2 | 0.0% | 100.0% |
| Format string literals | 0/1 | 0.0% | 100.0% |
| Nested struct patterns | 0/1 | 0.0% | 100.0% |
| async / await | 0/1 | 0.0% | 100.0% |
| LSP improvements | 16/20 | 80.0% | 20.0% |
| qz doc | 0/1 | 0.0% | 100.0% |
| JIT VM | 0/1 | 0.0% | 100.0% |

## Requirements

### Enhanced Crash/Panic handler

Evidence: [src/backend/x86_64/start.rs](../../src/backend/x86_64/start.rs).

- [x] Generated crash reporting and panic handler path.

### Fix encoder silent fallback

Evidence: [src/backend/x86_64/encoder.rs](../../src/backend/x86_64/encoder.rs).

- [x] Unsupported native opcodes return a backend error.

### Fix non-slice iterator codegen

Evidence: [src/bytecode/codegen.rs](../../src/bytecode/codegen.rs).

- [x] Collection iteration lowers through iterator methods.

### For-loop move semantics

Evidence: [src/semantic/borrow.rs](../../src/semantic/borrow.rs).

- [x] Loop ownership checks reject invalid reuse.

### Index assignment

Evidence: [src/bytecode/codegen.rs](../../src/bytecode/codegen.rs).

- [x] Indexed assignment lowering.

### Human-readable move errors

Evidence: [src/semantic/borrow.rs](../../src/semantic/borrow.rs).

- [x] Ownership violations carry source diagnostics.

### Module function namespacing/mangling

Evidence: [src/loader.rs](../../src/loader.rs).

- [x] Loaded module functions have qualified identities.

### Mandatory safe indexing checks

Evidence: [src/backend/x86_64/encoder.rs](../../src/backend/x86_64/encoder.rs).

- [x] Bounds checks for supported safe fixed-array, slice, and byte indexing.

### Bitwise operators

Evidence: [src/bytecode/codegen.rs](../../src/bytecode/codegen.rs).

- [x] Bitwise and shift operations.

### Loop control

Evidence: [src/bytecode/codegen.rs](../../src/bytecode/codegen.rs).

- [x] Break and continue lowering.

### else-if chains

Evidence: [src/parser/mod.rs](../../src/parser/mod.rs).

- [x] Chained conditional parsing.

### unsafe block sugar

Evidence: [src/parser/mod.rs](../../src/parser/mod.rs).

- [x] Unsafe blocks and semantic unsafe context.

### AOT cfg stripping

Evidence: [src/semantic/mod.rs](../../src/semantic/mod.rs).

- [x] Target configuration strips inactive declarations.

### Built-in linker

Evidence: [docs/LINKER.md](../LINKER.md).

- [x] Static x86-64 ELF executable output.
- [x] PE32+ executable output from COFF.
- [x] Cross-object symbol resolution.
- [x] Checked supported relocations.
- [x] Generated Windows import tables.
- [x] No implicit libc/CRT dependency.
- [x] Unsupported-input diagnostics and focused linker regressions.
- [ ] Archive member extraction in the built-in path.
- [ ] Final target-native Linux/Windows linker validation matrix.

### qz test runner

Evidence: [src/test_runner.rs](../../src/test_runner.rs).

- [x] Discovery, filtering, isolated runners, and infrastructure diagnostics.

### pub on types

Evidence: [src/semantic/declare.rs](../../src/semantic/declare.rs).

- [x] Cross-module type visibility enforcement.

### Unified formatting

Evidence: [docs/changes/2026-10-10-owned-formatting.md](../changes/2026-10-10-owned-formatting.md).

- [x] Shared placeholder/spec/escape behavior and owned output across formatting wrappers.

### Raw backtick string literals

Evidence: [src/lexer/mod.rs](../../src/lexer/mod.rs).

- [x] Raw literals preserve source contents.

### C/Rust-style string escapes

Evidence: [src/lexer/mod.rs](../../src/lexer/mod.rs).

- [x] Control, octal, hex, and Unicode escapes with invalid-escape diagnostics.

### C ABI FFI phase 1

Evidence: [docs/FFI.md](../FFI.md).

- [x] Scalar/pointer imports/exports, C layouts, opaque handles, native inputs, and library output.

### C ABI FFI phase 2

Evidence: [docs/FFI.md](../FFI.md).

- [x] C variadics, floats, aggregates, callbacks, globals, unions/packing/bitfields, bytes/C strings, and headers.

### Threshold-based auto-inline

Evidence: [src/bytecode/mod.rs](../../src/bytecode/mod.rs).

- [x] Threshold-based whole-program inlining.

### Cross-basic-block const folding

Evidence: [src/bytecode/constprop.rs](../../src/bytecode/constprop.rs).

- [x] Constant propagation and folding across basic blocks.

### Strength reduction

Evidence: [src/bytecode/constprop.rs](../../src/bytecode/constprop.rs).

- [ ] Replace expensive operations with equivalent cheaper instructions.

### macOS Mach-O backend

Evidence: [src/backend/AGENTS.md](../../src/backend/AGENTS.md).

- [ ] Mach-O output.
- [ ] macOS executable startup.
- [ ] Mach-O relocations.

### Ownership / RAII bytecode

Evidence: [docs/decisions/whole-program-ownership.md](../decisions/whole-program-ownership.md).

- [x] Affine moves and local use-after-move diagnostics.
- [x] Local shared loans.
- [x] Local exclusive loans.
- [x] Resolved inherent consuming-receiver effects.
- [x] Lexical cleanup on normal exit and early return.
- [x] Concrete generic ownership call graph.
- [x] Dense owned-element replacement/removal lowering.
- [x] Checked dense-element-address bytecode/backend foundation.
- [x] Recursive owned dense-container element destruction.
- [ ] Flow-sensitive interprocedural loan regions.
- [ ] Transitive fixed-point call-effect solver.
- [ ] Closed-target indirect/callback/trait effects.
- [ ] Verified QZI-only ownership certificates.
- [ ] Complete structural destruction including generic/enum/scalar shells.
- [ ] General place-level partial-move state.
- [ ] Safe provenance-tracked dense-element source borrowing.

### Atomic opcodes

Evidence: [src/backend/x86_64/AGENTS.md](../../src/backend/x86_64/AGENTS.md).

- [ ] Native AtomicAdd lowering.
- [ ] Native AtomicCas lowering.

### Format string literals

Evidence: [AGENTS.md](../../AGENTS.md).

- [ ] Compile-time literal interpolation.

### Nested struct patterns

Evidence: [AGENTS.md](../../AGENTS.md).

- [ ] Struct destructuring in match patterns.

### async / await

Evidence: [AGENTS.md](../../AGENTS.md).

- [ ] Lifetime-safe async/await design and implementation.

### LSP improvements

Evidence: [docs/tooling/lsp.md](../tooling/lsp.md).

- [x] Open/change/save/close UTF-16 synchronization.
- [x] Generation-gated diagnostics.
- [x] Semantic hover.
- [x] Identifier and std-path completion.
- [x] Whole-document formatting.
- [x] Document symbols.
- [x] Signature help.
- [x] Semantic tokens.
- [x] Type inlay hints.
- [x] Safe unused-import quick fix.
- [x] Cooperative cancellation.
- [x] Blocking-pool snapshots with stale-result rejection.
- [ ] General semantic features under incomplete source, beyond std-path completion.
- [ ] Caching and measured performance targets.
- [ ] Real-client Unicode/formatting protocol suite.
- [ ] Reproducible Zed/Helix feature-request runtime validation.
- [x] Loader-backed go-to-definition with passing current-checkout regressions.
- [x] Workspace symbols with passing current-checkout regressions.
- [x] Cross-file references with passing current-checkout regressions.
- [x] Workspace-scoped atomic rename with passing current-checkout regressions.

### qz doc

Evidence: [src/main.rs](../../src/main.rs).

- [ ] Documentation generation command.

### JIT VM

Evidence: [AGENTS.md](../../AGENTS.md).

- [ ] Executable JIT VM.

## Current-checkout checks

This README refresh accompanies the verified source checkpoint and is prepared
before its commit, following the latest instruction.
Checks use the existing stable toolchain through `/tmp/quazi-named-rust-bin`
ELF-loader wrappers, with `RUSTC=/tmp/quazi-named-rust-bin/rustc` and
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`:

- `cargo test --offline -q`: **897 passed**, zero failed/ignored. This includes
  the current compiler, backend, semantic and contained-LSP regression suite.
- `cargo test --offline -q lsp::`: **60 passed**, zero failed at the initial
  `d72a1c5` refresh; all 60 also pass inside the latest full 897-test run.
  The three earlier
  failures — `indexes_only_parseable_local_quazi_sources`,
  `loaded_definition_uses_unsaved_importer_and_target_overlays`, and
  `loaded_references_and_rename_follow_an_imported_binding` — now pass.
- Direct inspection of those fixtures confirms valid-source workspace filtering,
  unsaved importer/target overlays, cross-file reference locations, a combined
  multi-file rename edit, and rejection when an affected file is outside the
  permitted workspace. Existing workspace-symbol tests cover deterministic
  aggregation and root exclusion. These are the original four regression gates.
- `cargo build --offline -q` and a direct installed Clippy-driver check pass
  with existing warnings. Source/QZI native, Windows COFF, old-cache upgrade
  and exact/partial-cache runners pass; see
  [the expression-control verification record](../changes/2026-10-20-expression-control.md).
  The latest source checkpoint additionally passes the typed-propagation native,
  source-removed QZI, Windows COFF and old-cache rejection runner documented
  in [its verification record](../changes/2026-10-20-typed-try-propagation.md).
  The loop-cleanup checkpoint adds 23 native/QZI destructor-trace cases,
  Windows COFF emission, and old-source-cache invalidation; all pass.
  Conditional cleanup adds 31 native/source-removed-QZI cases, Windows COFF
  emission, an old-cache leak reproducer and correct subsequent exact hit.
  Six isolated STD memory tests and ordinary quazifetch build/run also pass;
  its Linux RAM reporting uses `MemAvailable` and includes reclaimable cache.
  Source initialization adds 39 rejected programs and 57 native/QZI/COFF checks,
  deterministic unmatched-value traps, and old-cache rebuild/exact-hit execution.
  See [the full verification record](../changes/2026-10-22-source-initialization.md).

LSP implementation is inspected in
[`workspace.rs`](../../src/lsp/workspace.rs),
[`goto_def.rs`](../../src/lsp/goto_def.rs),
[`references.rs`](../../src/lsp/references.rs) and
[`server.rs`](../../src/lsp/server.rs). Source changes since the older snapshot
have removed the blocking analysis failures; this refresh does not claim that
four new LSP features were implemented in this documentation commit.

Four LSP requirements remain unchecked: broad incomplete-source behavior,
measured caching/performance targets, real-client Unicode/formatting coverage,
and reproducible Zed/Helix feature-request validation. Unit tests do not prove
those broader contracts.

Ownership remains **9/16**. Concrete call/argument facts, statement graphs,
selected expression controls and raw-storage retention are prerequisites of
D-014; they do not complete flow-sensitive interprocedural loans, recursive
call effects, structural destruction or verified artifacts. The existing
lexical-cleanup point now has native regression coverage for abrupt loop exits
as well as fallthrough and explicit return. Runtime flags now replace the
compile-time return snapshot and preserve executed-path initialization/movement;
the earlier terminal-CFG corrections remain covered. Structured source paths now
check local reads by declaration identity; general partial-place initialization,
interprocedural effects and structural destruction remain open. This is not a
general destruction guarantee.

The other checklist item counts retain their listed scope. This score excludes
workspace-wide documentation/editor production-readiness milestones, full STD
validation and Windows-host execution. STD CLI working-tree changes and unrelated
compiler artifacts were preserved. Graphify query/update and changed Markdown
link checks were used; no publication or external operation is performed.
