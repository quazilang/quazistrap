# LSP (`src/lsp/`)

Basic server is running.

## Capabilities

- ✅ Diagnostics (publish on open/change/save)
- ✅ Hover (type + const value, fallback to symbol table)
- ✅ Goto Definition (naïve: searches symbol table by name, no scoping)
- ✅ Completion (trigger on `.` — **only** for `std.*` chains via filesystem scanning)
- ✅ Document formatting

## Position Model

- Compiler `Span.start`/`Span.end` offsets count Unicode scalar values. LSP
  `Position.character` counts UTF-16 code units. Use `span.rs` conversion
  helpers at the protocol boundary; do not compare a byte offset from editor
  input directly with a compiler span.

## Missing

- ✅ General identifier completion from the current semantic snapshot
- ❌ Scoped/resolving goto-definition
- ❌ Find references
- ❌ Rename symbol
- ❌ Code actions / quick fixes
- ❌ Inlay hints
- ❌ Workspace symbols
