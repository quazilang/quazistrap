# Reliable full-document LSP lifecycle

Audience: editor users and tooling developers.

## What changed

The contained `qz lsp` server now treats a full-document notification as a
versioned document generation. An analysis result is published only while its
source text and version still match the open document. Older `didChange`
notifications are ignored, and a late analysis can no longer replace newer
diagnostics or semantic data.

`didClose` now removes the document from the server and publishes an empty
diagnostic set for its URI. Formatting replacement ranges now end at the
document's actual UTF-16 position, including documents with supplementary
Unicode characters or a trailing newline.

## Compatibility

The server still advertises full-document synchronization only. Editors may
observe corrected diagnostic clearing and formatting ranges; there is no source
compatibility impact. Incremental synchronization, cancellation, workspace
indexing, and cross-file navigation remain unsupported.

## Verification

Focused Rust tests cover rejected out-of-order updates and UTF-16 end positions
for emoji and trailing newlines. Run:

```bash
cargo test --quiet lsp::
```
