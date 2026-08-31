# Tooling rebaseline — 2026-08-31

Audience: language maintainers and tooling developers.

This rebaseline supersedes only the tooling-status assertions in the
2026-08-29 workspace baseline; historical evidence in that report is preserved.

## Resolved since the baseline

- The separate Tree-sitter repository is now Quazi-native and has a corpus
  covering current shipped syntax, including opaque field attributes.
- Separate VS Code, Neovim, Helix, and Zed integration repositories now exist.
- The contained LSP supports current-document symbols, signature help,
  semantic tokens, references, and rename. Full-document lifecycle processing
  is version-gated, clears diagnostics on close, and uses UTF-16 formatting
  ranges.

## Still open

- Documentation specification, standard-library API reference, tutorial, and
  guides remain placeholders rather than complete canonical documents.
- The LSP has no workspace index, cross-file navigation, incremental sync,
  cancellation, code actions, or inlay hints.
- `std.time` and `std.process` are absent; thread creation still exposes a
  zero-handle failure sentinel rather than a typed error.
- The support matrix, compatibility policy, panic model, concurrency model,
  TLS trust policy, and time-zone policy remain maintainer decisions.

## Next sequencing

Keep the separate grammar and editor packages aligned with the contained LSP.
Before expanding new standard-library subsystems, establish their ownership,
platform, failure, and test contracts. Complete structured language/API/tutorial
documentation in independently reviewable sections rather than duplicating the
legacy flat guides.
