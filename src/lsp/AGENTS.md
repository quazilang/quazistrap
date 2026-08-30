# LSP (`src/lsp/`)

Basic server is running.

## Capabilities

- ✅ Diagnostics (publish on open/change/save)
- ✅ Hover (type + const value, fallback to symbol table)
- ✅ Goto Definition (semantic call targets plus best-effort lexical priority)
- ✅ Completion (trigger on `.` — **only** for `std.*` chains via filesystem scanning)
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
- ❌ Workspace symbols
