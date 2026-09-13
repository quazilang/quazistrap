# LSP Semantic-Analysis Cancellation

The contained language server now cooperatively cancels a superseded document
analysis at semantic pass boundaries and between top-level declarations. A
cancelled analysis produces no partial semantic report and no source
diagnostic.

## Why

Document generations already interrupted lexing and parsing, but an edit that
arrived after parsing could leave semantic analysis running until it completed.
That consumed a blocking-pool worker for a result that the server would later
discard.

## Compatibility

This is an LSP responsiveness improvement only. It does not change Quazi
source syntax, diagnostics for completed analyses, or the command-line
compiler's non-cancellable analysis API. Editors may observe that obsolete
diagnostics disappear sooner after an edit; the next coherent document
generation remains authoritative.

## Remaining boundary

The loader's recursive import traversal and its merged-program analysis do not
yet poll cancellation. A cancelled definition, references, or rename request
can therefore still run to completion before its snapshot is discarded.
Long semantic collection and graph traversals now poll during their work. The
remaining atomic semantic boundary is checking within a single top-level
declaration; cancellation is cooperative rather than preemptive.

## Verification

The semantic regression cancels while walking top-level items. The focused LSP
analysis regression confirms cancellation remains operational rather than a
parse diagnostic. Run:

```text
env RUSTC=$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc \
  $HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo test --offline \
  cancellable_analysis_stops_between_top_level_items

env RUSTC=$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc \
  $HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo test --offline \
  lsp::analysis::tests::cancelled_analysis_does_not_create_a_parse_diagnostic
```
