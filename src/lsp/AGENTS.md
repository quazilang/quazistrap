# LSP (`src/lsp/`)

Basic server is running.

## Capabilities

- ✅ Diagnostics (publish on open/change/save)
- ✅ Hover (type + const value, fallback to symbol table)
- ✅ Goto Definition (naïve: searches symbol table by name, no scoping)
- ✅ Completion (trigger on `.` — **only** for `std.*` chains via filesystem scanning)
- ✅ Document formatting

## Missing

- ❌ General identifier completion (non-std)
- ❌ Scoped/resolving goto-definition
- ❌ Find references
- ❌ Rename symbol
- ❌ Code actions / quick fixes
- ❌ Inlay hints
- ❌ Workspace symbols
