# Incremental LSP text synchronization

Audience: Quazi editor users and tooling maintainers.

The contained `qz lsp` server now negotiates incremental text synchronization.
It applies all edits in a `didChange` notification in order, using the LSP's
UTF-16 positions and optional replacement lengths. Full-document replacement
notifications remain supported.

Malformed ranges, replacement lengths that do not match the replaced UTF-16
text, and positions within a surrogate pair are rejected without replacing the
last coherent document snapshot. Older document versions remain ignored.
Diagnostic publication is serialized with document generation changes, so an
older analysis cannot republish diagnostics after a newer change or document
close has supplied the authoritative diagnostics.

This is protocol-compatible for clients that previously sent full text: they
may continue to do so. Clients that support incremental updates can now avoid
sending an entire document for every edit. Cross-file analysis, persistent
workspace indexing, cancellation, and code actions remain outside this
checkpoint.

Focused LSP tests cover ordered multi-edit batches, UTF-16 supplementary
characters, malformed ranges, replacement-length validation, full-document
replacement, and stale-version rejection. The generation-ordered publication
path is kept in the same critical section as the document state transition.
