# Plan-progress audit — 2026-09-16

Audience: maintainers continuing the local production-readiness plan.

## Conclusion

The production-readiness plan remains **incomplete**. Recent checkpoints make
the compiler safer and clarify the ownership boundary; they do not implement
D-014's whole-program ownership analysis, generic element destruction, or QZI
ownership summaries. A passing compiler suite must not be used as evidence for
the unimplemented platform, TLS, civil-time, concurrency, or editor-runtime
work.

## Evidence reviewed

- Compiler commit `9ba5772` permits only an exact direct
  `ret self.field` from an eligible consumed receiver. Its generated cleanup
  excludes that one field for that return and retains sibling cleanup. The
  compiler suite verifies alternate return branches and destructor reachability.
- Compiler commit `1553568` rejects `Array.get` for owned elements with S10.
  The old raw-load implementation would otherwise create a shallow second
  owner. Plain element reads remain valid.
- Compiler commit `10d5853` makes contiguous-container cleanup roots recursive.
  A `Buffer[Buffer[Token]]` now retains both concrete `Buffer.free`
  specializations, including when the outer cleanup is reached through a
  generic caller. Symbolic template edges remain available for normal
  monomorphization; the compiler never falls back to an unresolved generic
  storage-release hook.
- The standard-library harness reaches the HTTP `Headers` boundary: its
  `Array[Header]` operations need scoped borrowed elements and exact-once
  replacement/removal/freeing. It is intentionally not treated as a passing
  standard-library suite while those diagnostics remain.
- The contained compiler's `cargo test --offline` passes 633 tests at the
  generic-removal checkpoint. Canonical
  Markdown checks pass 13 tests.
- The separate Tree-sitter corpus passes 28/28 and its workspace conformance
  command passes. This supersedes the obsolete resume warning about example 33.

## Milestone status

| Milestone | Current status | Evidence required before closure |
| --- | --- | --- |
| 1. Workspace and contradiction audit | Maintained, not final. | Cross-project review after behavior stabilizes. |
| 2. Semantics decisions | Incomplete. | Resolve remaining platform, panic, TLS, time-zone, and formatting decisions. |
| 3. Critical regressions | Substantial. | Target-scoped end-to-end checks after ownership/platform work. |
| 4–5. Canonical documentation | Substantially complete. | Keep contracts synchronized; no documentation website is in scope. |
| 6. Standard-library foundations | Incomplete. | D-014 element/provenance/destruction work; then platform and lifecycle work. |
| 7. Tree-sitter | Substantially complete. | Retain conformance and expand invalid/ambiguous coverage with syntax changes. |
| 8. Contained LSP | Substantially complete. | Real-client Unicode/formatting and performance evidence. |
| 9. Editor integrations | Incomplete. | Reproducible Zed and Helix runtime validation. |
| 10. Tutorial and guides | Complete content checkpoint. | Keep examples synchronized. |
| 11. Final review | Incomplete. | Final target matrix and independent cross-project review. |

## Required next ownership sequence

1. Implement D-015's provenance-tracked scoped borrowing for generic
   container elements. Its compiler-validated contract rejects an
   `Array`-specific escape hatch and requires an artifact-compatible address
   operation; owned values may not be copied from an `Array` read.
2. Prove the new compiler-validated generic removal lowering against runtime
   ownership cases (including nested containers and out-of-bounds execution),
   then re-evaluate resource-owning collection APIs such as HTTP headers.
   Replacement, removal, and final recursive destruction are now implemented
   at the bytecode-contract level.
3. Extend the capability/effect model through direct and indirect calls, then
   design and verify the matching QZI/QZC ownership-summary format.

These are compiler and runtime prerequisites, not standard-library receiver
spelling changes. Do not bypass them with API-specific compiler exceptions or
shallow copies.
