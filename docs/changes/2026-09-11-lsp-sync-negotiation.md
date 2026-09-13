# LSP text synchronization negotiation

The contained Quazi language server now explicitly advertises the complete
text-document notification contract it consumes: document open/close, save,
and incremental changes.

## Why

The server previously advertised only the legacy incremental-sync value. That
value describes changes but cannot request open/close or save notifications,
despite the server relying on all three to create document state, clear stale
diagnostics, and refresh a saved workspace snapshot. Clients following the
protocol could therefore omit required notifications.

## Compatibility

The server now returns `TextDocumentSyncOptions` with `openClose: true`,
`change: Incremental`, and `save: true`. Existing clients that already send
these notifications continue to work. Protocol-conformant clients can now
reliably use diagnostics, close cleanup, and save-time indexing without
client-specific assumptions. Full-document replacement changes remain
accepted for compatible clients.

## Verification

The server capability regression asserts the exact options shape. Run:

```text
env RUSTC=$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc \
  $HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo test --offline \
  lsp::server::tests::advertises_open_close_incremental_and_save_synchronization
```
