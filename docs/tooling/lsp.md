# Quazi Language Server

Audience: editor users and tooling developers.

The contained Quazi language server is started with:

```text
qz lsp
```

It communicates over standard input/output using the Language Server Protocol.
Editors should start one process per workspace and use full-document text
synchronization. The server reports its version from the compiler package.

## Supported Features

- Diagnostics after open, full-document change, and save.
- Hover information from semantic types and constant evaluation.
- Go-to-definition for semantic call targets, including methods, plus
  best-effort resolution of local declarations in the current document.
- Completion for `std.*` module paths and public module symbols.
- General identifier completion from the current document's semantic symbol
  snapshot. Suggestions include functions, types, variables, and parameters
  declared before the cursor.
- Whole-document formatting.
- Document symbols for user declarations in the current open document.
- Signature help for functions known to the current semantic snapshot.
- References and rename for semantic bindings within the current open document.
- Full-document semantic tokens for lexical tokens and known semantic symbols.

Completion uses the most recent successful semantic analysis. While a document
has a parse error, only the filesystem-backed `std.*` path completion is
available. The current general completion is not yet a full lexical-scope
resolver: it can include declarations from earlier source regions that are not
visible at the cursor. Editors must treat it as best-effort completion rather
than a name-resolution guarantee.

## Current Limitations

- Text synchronization is full-document only; incremental edits are not
  negotiated.
- Cross-file references and rename, workspace symbols, inlay hints, code
  actions, cancellation, and workspace-wide indexing are not
  implemented.
- Diagnostics and navigation operate on the current document only. Imported
  source locations are not yet exposed as cross-file locations.
- Formatting and position conversion need a real-editor protocol smoke suite
  before they can be treated as stable across all Unicode input.

## Verification

Run the focused LSP checks from the compiler repository:

```bash
env RUSTC=$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc \
  $HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo test --offline lsp::
```

The compiler test suite also compiles the server implementation. Real editor
integration remains a separate validation requirement for each editor package.
