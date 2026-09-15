# Plan-completion re-audit — 2026-09-14

Audience: maintainers continuing the local production-readiness plan.

## Conclusion

The production-readiness plan is **not complete**. This re-audit updates the
2026-09-12 evidence after the receiver-capability checkpoints, but it does not
turn the D-014 design decision into an implementation claim. The compiler has
local shared/exclusive receiver checks; it does not yet have consuming receiver
effects, flow-sensitive interprocedural loans, structural destruction, or
QZI/QZC ownership summaries.

## New evidence since the prior audit

- `5e3f7d2` makes `Array` queries shared and updates exclusive. It also fixes
  generic dispatch through `&Array[T]` and retains aggregate const-field
  metadata so an exclusive method or indexed write cannot mutate a `const`
  field.
- `fd88809` makes non-consuming `String` observers, parsing, and
  transformations shared. Its loader regression exercises actual prelude
  `&String` dispatch for access, `push_str`, `get`, slicing, and parsing.
- The contained compiler test suite passes 594 tests with the pinned offline
  toolchain. `examples/08-dynamic-arrays` and `examples/22-quazifetch` lower
  with the changed prelude.
- The separate Tree-sitter repository's corpus passes 24/24, and its workspace
  conformance check succeeds. This is stronger current evidence than the old
  resume warning that its grammar describes a legacy language.

## Milestone status

| Milestone | Current evidence | Completion status | Required evidence before closing |
| --- | --- | --- | --- |
| 1. Workspace and contradiction audit | Dated baselines and re-audits cover compiler, standard library, Tree-sitter, LSP, and editor surfaces. | Maintained, not final. | Final cross-project contradiction review after behavior stabilizes. |
| 2. Semantics decisions | D-001 through D-014 resolve important directions; D-004 through D-009 and D-013 remain open. | Incomplete. | Resolve compatibility, platform, panic, concurrency, TLS, time-zone, and formatting decisions where they gate APIs. |
| 3. Critical regressions | 594 compiler tests; focused loader/prelude, native, and artifact regressions. | Substantial, not a release gate. | Repeat target-scoped end-to-end verification after ownership and platform work. |
| 4–5. Canonical documentation | Specification, API, tutorials, guides, decisions, migrations, and change records are present and checked. | Content substantially complete. | Synchronize contracts with later behavior; no documentation website is in scope. |
| 6. Standard-library foundations | Time, UTF-8 boundaries, narrow Linux process support, networking repairs, and receiver migration checkpoints exist. | Incomplete. | Implement D-014 before extending resource-retaining APIs; then Windows process, civil time, TLS, concurrency, and remaining lifecycle work. |
| 7. Tree-sitter | Separate grammar, queries, generated parser, 24-case corpus, and workspace conformance pass. | Substantially complete. | Broaden invalid, ambiguous, and injection coverage as syntax changes. |
| 8. Contained LSP | Incremental synchronization, navigation, rename, symbols, tokens, formatting, and cancellation have focused tests. | Substantially complete. | Real-client protocol coverage for Unicode/formatting plus stated performance targets. |
| 9. Editor integrations | Separate projects exist; some VS Code and Neovim smoke coverage exists. | Incomplete. | Reproducible Zed and Helix runtime validation. |
| 10. Tutorial and guides | Ten-chapter tutorial and practical guides are compiler-checked. | Complete content checkpoint. | Keep examples synchronized with the language. |
| 11. Final review and verification | Checkpoint reviews are reconciled. | Incomplete. | One final cross-project review and target matrix after unresolved milestones close. |

## D-014 boundary verified during this re-audit

`Option` and `Result` predicates now use shared receivers through the first
generic aggregate-projection slice: a match on `&Enum` may inspect a variant
discriminant with unit or wildcard payload patterns, but may not bind a
payload. This makes tag-only status queries sound without an API-specific
special case. Borrowed payload binding, aggregate materialization, and
general aggregate projection remain D-014 work.

## Required sequence

1. Implement D-014 in compiler phases: a capability model including consuming
   receivers, direct-call effects and flow-sensitive loans, then verified
   QZI/QZC ownership summaries and structural destruction.
2. Migrate legacy resource and enum APIs only when their operation can be
   represented by that capability/effect model; do not infer ownership from
   method names or add per-API compiler exceptions.
3. Complete the remaining platform and policy foundations, then run the final
   target-scoped verification and editor-client review.

## Explicit non-claims

- Shared `Array` and `String` receivers do not prove generic element ownership,
  consuming receiver effects, or structural cleanup.
- A green compiler suite does not prove Windows runtime behavior, TLS policy,
  civil-time correctness, or real-client Zed/Helix integration.
- Existing QZI v9 and QZC v7 artifacts still lack D-014 ownership summaries;
  QZI-only dependencies therefore cannot establish a safe borrow boundary.
