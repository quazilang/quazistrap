# Quazi Language Server

Audience: editor users and tooling developers.

The contained Quazi language server is started with:

```text
qz lsp
```

It communicates over standard input/output using the Language Server Protocol.
Editors should start one process per workspace. The server negotiates
incremental text synchronization and also accepts full-document replacements.
The server reports its version from the compiler package.

## Supported Features

- Generation-gated diagnostics after open, incremental changes, and
  full-document replacements. Incremental ranges and optional replacement
  lengths are validated as UTF-16 positions; malformed ranges do not replace
  the last coherent document snapshot. Save republishes the current diagnostic
  snapshot, and closing a document clears its published diagnostics.
- Hover information from semantic types and constant evaluation.
- Go-to-definition for semantic call targets, including methods, plus
  best-effort local declarations and compiler-loader-backed local, package,
  and standard-library imports.
- Completion for `std.*` module paths and public module symbols.
- General identifier completion from the current document's semantic symbol
  snapshot. Suggestions include functions, types, variables, and parameters
  declared before the cursor.
- Whole-document formatting.
- Document symbols for user declarations in the current open document.
- Workspace-symbol search across successfully parsed `.qz` files under local
  workspace folders (or the legacy local `rootUri`). Results contain matching
  top-level declarations only. Unsaved open buffers temporarily override their
  on-disk file snapshot; saving updates it. The index never follows symlinks
  or scans outside the client-selected roots.
- Signature help for functions known to the current semantic snapshot.
- References for compiler-resolved bindings across the current document's
  configured local, package, and standard-library import graph. Rename uses
  the same loader-backed source map, but is offered only when every affected
  source is beneath a client-negotiated workspace root; it never edits a
  dependency or the standard library. Renaming an imported declaration updates
  its import selector while preserving a local `as` alias and its uses.
- Full-document semantic tokens for lexical tokens and known semantic symbols.
- Type inlay hints for unannotated local `var` and `const` declarations. Hints
  use the compiler-resolved type and are limited to the range requested by the
  editor; declarations with an explicit type receive no duplicate hint.
- A `quickfix` code action for W03 unused-import diagnostics on a
  single-selector import. It removes that complete import declaration; broader
  import forms intentionally receive no unsafe bulk-removal action.

Completion uses the most recent successful semantic analysis. While a document
has a parse error, only the filesystem-backed `std.*` path completion is
available. The current general completion is not yet a full lexical-scope
resolver: it can include declarations from earlier source regions that are not
visible at the cursor. Editors must treat it as best-effort completion rather
than a name-resolution guarantee.

## Current Limitations

- The transport honors JSON-RPC `$/cancelRequest` for pending LSP requests.
  Loader-backed definition, reference, and rename requests bridge cancellation
  to their blocking worker and poll it while traversing imports, resolving
  public exports, and lexing/parsing loader sources. Diagnostics attach a token to each open-document generation:
  a replacement or close interrupts tokenization, parsing, and semantic
  analysis between top-level declarations, pass boundaries, and long
  pass-internal collection/traversal boundaries; cancellation is never
  published as a source diagnostic. Compiler and loader snapshots otherwise
  run on Tokio's blocking pool, so they do not occupy an async server worker.
  Individual filesystem reads, pass-input snapshots, and semantic work within
  one top-level declaration remain atomic, so an already-running unit can
  finish before its result is discarded. Workspace initialization scans and saved-file index updates use
  the same pool. A completed loader snapshot is also discarded if a captured
  open buffer changed or closed while it was running. Automatic filesystem
  watching is not implemented; the workspace-symbol snapshot is built during
  initialization, and externally changed files are picked up when the server
  is restarted or opened and saved through the LSP.
- Go-to-definition follows semantic bindings across the current document's
  configured local, package, and standard-library import graph. Open file
  buffers override disk text for every loaded source before spans are rebased
  to LSP locations. Namespace and wildcard import forms remain limited by the
  compiler's current semantic annotations.
- Loader-backed definition, reference, and rename snapshots are rebuilt for
  each request. Caching and performance targets remain follow-up work.
  Namespace and wildcard imports still depend on the compiler's available
  semantic annotations.
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
