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
- ✅ Loader-backed go-to-definition for semantic bindings across local,
  package, and standard-library imports. The loader receives canonical open
  document overlays and supplies the effective per-file source map used to
  rebase declaration spans to LSP locations.
- ✅ Loader-backed cross-file references and workspace-scoped rename
- ✅ Safe W03 unused-import quick fix for single-selector imports
- ✅ Compiler-backed inferred type inlay hints for unannotated local declarations
- ✅ Persistent workspace symbols for parseable local `.qz` files under the
  negotiated workspace roots; open buffers override their on-disk snapshots.

The tower-lsp transport handles `$/cancelRequest` for pending requests.
Compiler and loader snapshots run on Tokio's blocking pool, which keeps them
off async server workers, but this does not make them cooperatively
cancellable: an already-running parser, semantic analysis, or loader task
must finish. Before returning a loader-backed result, verify that every open
buffer included in its snapshot is still open with the same source text;
otherwise discard it. An explicit compiler cancellation boundary is still required
before claiming CPU-work interruption.

Cross-file references and rename use a fresh compiler-loader snapshot, not the
workspace-symbol index. Every loaded span is rebased through the loader's
effective source map, including unsaved open-document overlays. References can
include local, package, and standard-library import graphs; rename is offered
only when every affected file is beneath a client-negotiated workspace root,
so it cannot modify dependencies or the standard library.

Type inlay hints are derived from parsed `var`/`const` declarations that omit
a written type and from the matching semantic declaration span. They display
the compiler's resolved type, respect the client-requested visible range, and
do not guess types from identifier names or text.

The code-action provider offers `Remove unused import` only for a current W03
diagnostic on a parsed single-selector import. The edit removes the complete
import declaration and its line ending. It deliberately excludes wildcard and
multi-selector imports because deleting those declarations could remove used
bindings.

Workspace indexing is read-only: normalize only local file workspace folders
(or the legacy `rootUri`), never follow symlinks, skip `.git`, and retain no
stale symbol snapshot after a file fails to parse. It provides symbols only;
do not reuse its independently analyzed reports for cross-file navigation
without a source-map/span-rebasing design.
