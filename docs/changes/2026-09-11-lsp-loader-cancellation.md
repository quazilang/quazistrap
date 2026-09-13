# Loader-backed LSP cancellation

## Motivation

Cross-file definition, reference, and rename requests build a fresh loader
snapshot. A cancelled request previously discarded that snapshot only after
the loader finished, wasting work on obsolete import graphs.

## Change

The loader has a cancellable entry point with a typed operational outcome.
It polls while traversing import graphs, resolving public module exports,
lexing and parsing import-discovery and merged sources, and processing loaded
source collections. Loader-backed LSP analysis preserves that outcome through
semantic analysis, so cancellation is never reported as a parse or loader
diagnostic.

Individual filesystem reads and parser work within one top-level declaration
remain atomic. Expensive inner semantic loops are also still a separate
follow-up.

## Compatibility

The existing non-cancellable loader API and its error messages are unchanged.
This only improves responsiveness for cancelled LSP requests.

## Verification

- `cargo test --offline loader::tests`
- `cargo test --offline lsp::`
- `cargo test --offline`
