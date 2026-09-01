# Foundations rebaseline — 2026-09-01

Audience: language maintainers and tooling developers.

This report updates the affected findings from the 2026-08-29 workspace
baseline and the 2026-08-31 tooling rebaseline. Historical evidence remains in
those reports; this page records only the current state and remaining work.

## Resolved or materially reduced

| Area | Evidence | Current contract |
|---|---|---|
| Text safety | `std` `960d109`; UTF-8 smoke on Linux and Windows compile-only | `String` is not constructed from invalid file, TCP, or UDP text bytes. TCP aggregation handles scalars split across system reads without exceeding its limit. |
| Monotonic time | `std` `5b043cd`; API docs `a1f940f` | `Duration` uses normalized seconds/nanoseconds and checked arithmetic; `Instant` is monotonic on Linux and Windows. |
| Conditional imports | compiler `cb18d1a`; std `d8b8668`; grammar `6eedcd7`, `1a223cf` | Disabled imports are filtered before loading and source, standard library, and Tree-sitter agree on conditional import syntax. |
| Editor protocol surface | `f575d6b`; focused `cargo test lsp::` 30/30 | The contained LSP supports generation-gated full-document updates and workspace-symbol search over successfully analyzed open documents. |
| Child-process architecture | D-011, `3c551a0` | A public process API must be built on runtime primitives; a std-only fork/exec or `CreateProcess` facade is not considered support. |

## Still open

- The runtime process primitives, public `std.process`, and their Linux/Windows
  test matrix are not implemented.
- The LSP has an open-document symbol index only. Persistent workspace indexing,
  cross-file references/rename, incremental synchronization, cancellation, code
  actions, and inlay hints remain open.
- Wall-clock time, calendars, civil-time parsing/formatting, time zones, and
  time-zone data policy remain deliberately deferred by D-009.
- Structural destruction, full resource lifecycle guarantees, and the complete
  panic/concurrency model require their remaining compiler/runtime work despite
  the resolved decisions D-002 and D-003.
- The language specification, comprehensive API reference, tutorial, and guides
  are not yet complete. A documentation website remains intentionally excluded
  by maintainer direction.

## Priority sequence

1. Implement and test the runtime-owned child-process primitives described by
   D-011 before exposing `std.process`.
2. Establish destruction and panic/concurrency runtime behavior before promising
   broad RAII or cancellation semantics in further standard-library APIs.
3. Complete canonical language/API/tutorial documentation alongside verified
   implementation contracts, then add persistent LSP indexing and remaining
   editor protocol work.

## Verification record

- `cargo test --manifest-path Cargo.toml lsp::` — 30 focused LSP tests passed.
- UTF-8 regression program — Linux build/run passed; Windows x86-64 COFF
  compile-only passed.
- Tree-sitter import corpus — 6/6 passed at the conditional-import checkpoint.

These checks are checkpoint evidence, not a claim that the full production
readiness objective is complete.
