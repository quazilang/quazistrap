# Plan-completion audit — 2026-09-12

Audience: maintainers deciding the next local checkpoint.

## Conclusion

The production-readiness plan is **not complete**. This audit distinguishes
implemented behavior from a resolved design decision and from evidence that is
still unavailable. In particular, D-014 defines the ownership model but does
not implement its analysis, artifact contract, or structural destruction.

The older `implementation_plan.md` is not a completion authority: its LSP
entries predate incremental synchronization, cross-file navigation, background
workspace indexing, and cooperative cancellation. The dated audits, decisions,
source regressions, and current repository state are stronger evidence.

## Evidence reviewed

- `docs/audits/2026-09-08-readiness-status.md` and the dated change records.
- `docs/decisions/README.md`, especially D-013 and D-014.
- The contained compiler's LSP, QZI, semantic, and runtime source/test
  surfaces; the separate `std`, Tree-sitter, and editor repositories.
- Local history through the Linux `std.process` checkpoint and the D-014
  ownership decision on 2026-09-12.

## Milestone evidence

| Milestone | Evidence of progress | Completion status | Required evidence before closing |
| --- | --- | --- | --- |
| 1. Workspace and contradiction audit | Dated workspace baselines and the current readiness audit record compiler, standard-library, tooling, and editor findings. | Maintained, not a final audit. | A final cross-project contradiction review reconciled against the then-current source and runtime results. |
| 2. Authoritative semantics decisions | D-001, D-002/D-003 through D-014, and D-011/D-012 establish several critical directions. | Incomplete. | Resolve D-004 stability, D-005 platform tiers, D-006 panic, D-008 TLS, D-009 time-zone data, and D-013 formatting ownership. |
| 3. Regressions and critical fixes | Compiler regressions, historical QZI fixtures, source-level standard-library tests, native Linux smokes, and Windows COFF checks cover many corrected defects. | Substantially complete, not a final stabilization gate. | Repeat target-appropriate end-to-end checks after remaining ownership and platform work; do not use the compiler unit suite as broad platform proof. |
| 4–5. Documentation structure, specification, API, and changes | Canonical Markdown is separated under `docs/`; language pages, tutorial, guides, API coverage ledger, decisions, migrations, and change records are present. Markdown links/headings and runnable tutorial fences have automated checks. | Content substantially complete. | Keep implementation/API contracts synchronized as later changes land; no documentation website is in scope under the explicit local-work clarification. |
| 6. Standard-library foundations | Monotonic `Duration`/`Instant`, bounded JSON work, UTF-8 boundary checks, networking repairs, and narrow Linux child processes are implemented and documented. | Incomplete. | Implement and verify D-014 before expanding resource APIs; then complete Windows process support, civil time, TLS policy/backend, concurrency, and the remaining serialization/net lifecycle contracts. |
| 7. Tree-sitter | Separate project has grammar, queries, corpus, generated parser, and workspace conformance checks. | Substantially complete. | Broaden invalid/ambiguous/injection coverage and retain conformance as language syntax changes. |
| 8. Contained LSP | Incremental synchronization, diagnostics, hover, completion, signatures, symbols, semantic tokens, loader-backed navigation, guarded rename, inlay hints, quick fixes, and cancellation have focused regressions. | Substantially complete. | Real-client protocol coverage for formatting and Unicode edge cases, explicit performance/caching targets, and documented namespace/wildcard limitations. |
| 9. Editor integrations | Separate VS Code, Neovim, Helix, and Zed projects exist; VS Code and Neovim have real-client hover smokes. | Incomplete. | Run reproducible Zed and Helix runtime validation when those editors are available; static grammar alignment is not equivalent. |
| 10. Tutorial and guides | Ten tutorial chapters and practical guides are present; examples are labeled and compiler-checked in the documented scope. | Complete content checkpoint. | Maintain runnable examples as the language evolves. |
| 11. Independent review and final verification | Several checkpoint-specific independent reviews were performed and reconciled. | Incomplete. | Perform one final cross-project review and the full, target-scoped verification matrix after the unresolved milestones are closed. |

## Blocking and dependency order

1. Implement D-014 as a compiler feature: explicit receiver capabilities,
   direct-call ownership effects and flow-sensitive loans, then QZI/QZC
   ownership summaries and their verification. A documentation decision alone
   must not unblock resource-retaining process work or safe concurrency.
2. Obtain the still-required compatibility and platform decisions before
   committing their dependent public APIs. D-013 is specifically a maintainer
   choice, not a compiler bug that may be silently redesigned.
3. After D-014, choose one bounded foundation at a time (for example Windows
   process support, civil time, TLS, or `Deserialize`) with source, artifact,
   native-target, documentation, and migration evidence.
4. Finish real-editor validation and the final cross-project audit only after
   those behavior changes stabilize.

## Explicit non-claims

- Linux `std.process` support does not prove Windows process support, embedded
  native-library process initialization, pipe/callback/captured-output safety,
  or asynchronous process ownership.
- QZI v9 and QZC v7 carry no D-014 ownership summary. Existing QZI-only
  dependencies therefore cannot yet be used as proof of a safe borrowed-call
  boundary.
- The current function-local move checker and local cleanup are not
  whole-program escape analysis or structural destruction.
- Zed and Helix static integration checks are not editor-runtime validation.

This audit is a planning and evidence record. It does not close any open
milestone merely because its surrounding documentation or a narrow test suite
is present.
