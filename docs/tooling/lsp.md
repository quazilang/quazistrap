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
- Go-to-definition for symbols resolved by the current document analysis.
- Completion for `std.*` module paths and public module symbols.
- General identifier completion from the current document's semantic symbol
  snapshot. Suggestions include functions, types, variables, and parameters
  declared before the cursor.
- Whole-document formatting.

Completion uses the most recent successful semantic analysis. While a document
has a parse error, only the filesystem-backed `std.*` path completion is
available. The current general completion is not yet a full lexical-scope
resolver: it can include declarations from earlier source regions that are not
visible at the cursor. Editors must treat it as best-effort completion rather
than a name-resolution guarantee.

## Current Limitations

- Text synchronization is full-document only; incremental edits are not
  negotiated.
- References, rename, signature help, workspace symbols, semantic tokens,
  inlay hints, code actions, cancellation, and workspace-wide indexing are not
  implemented.
- Diagnostics and navigation operate on the current document only. Imported
  source locations are not yet exposed as cross-file locations.
- Formatting and position conversion need a real-editor protocol smoke suite
  before they can be treated as stable across all Unicode input.

## Verification

Run the focused completion checks from the compiler repository:

```bash
$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo test --offline lsp::completion
```

The compiler test suite also compiles the server implementation. Real editor
integration remains a separate validation requirement for each editor package.
