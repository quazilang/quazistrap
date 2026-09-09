# LSP unused-import quick fix

The contained LSP now advertises `textDocument/codeAction` and provides a
`quickfix` for compiler warning W03 when it identifies an unused,
single-selector import.

The action reparses the current source and requires the compiler warning span
to match that complete import declaration before it offers an edit. Its edit
removes the declaration and following line ending. Wildcard and multi-selector
imports receive no bulk-removal action, since deleting them could remove
bindings that remain in use.

Focused coverage verifies the exact UTF-16 edit range, the W03 diagnostic
association, and kind filtering requested by the client.
