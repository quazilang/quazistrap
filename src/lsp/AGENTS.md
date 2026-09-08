# LSP (`src/lsp/`)

Basic server is running.

## Capabilities

- ✅ Diagnostics (publish on open/change/save)
- ✅ Full-document and UTF-16-safe incremental text synchronization
- ✅ Hover (type + const value, fallback to symbol table)
- ✅ Goto Definition (semantic call targets plus best-effort lexical priority)
- ✅ Completion for `std.*` chains plus identifiers in the current semantic snapshot
- ✅ Document formatting
- ✅ Flat document symbols from the semantic snapshot
- ✅ Signature help for current-document and loaded standard-library functions
- ✅ Same-document references and rename for semantically resolved bindings
- ✅ Full-document semantic tokens for lexical tokens and known semantic symbols

## Position Model

- Compiler `Span.start`/`Span.end` offsets count Unicode scalar values. LSP
  `Position.character` counts UTF-16 code units. Use `span.rs` conversion
  helpers at the protocol boundary; do not compare a byte offset from editor
  input directly with a compiler span.

## Missing

- ✅ General identifier completion from the current semantic snapshot
- ❌ Fully scoped/resolving goto-definition for all binding uses
- ❌ Cross-file references and rename
- ❌ Code actions / quick fixes
- ❌ Inlay hints
- ✅ Persistent workspace symbols for parseable local `.qz` files under the
  negotiated workspace roots; open buffers override their on-disk snapshots.

Workspace indexing is read-only: normalize only local file workspace folders
(or the legacy `rootUri`), never follow symlinks, skip `.git`, and retain no
stale symbol snapshot after a file fails to parse. It provides symbols only;
do not reuse its independently analyzed reports for cross-file navigation,
references, or rename without a source-map/span-rebasing design.
