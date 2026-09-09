# Compiler-backed LSP type inlay hints

The contained LSP now advertises `textDocument/inlayHint` and returns type
hints for unannotated local `var` and `const` declarations.

The implementation reparses the open source to identify declarations that do
not have a written type, then joins each exact declaration span with the
semantic report. This ensures displayed types are compiler-resolved rather
than guessed from source text or a same-named binding in another scope. Hints
use UTF-16 protocol positions and honor the editor's requested visible range.

Focused tests cover nested declarations, explicit-type suppression, semantic
reference types, Unicode-safe positions, and range filtering.
