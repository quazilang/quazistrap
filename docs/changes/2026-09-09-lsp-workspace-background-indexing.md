# LSP workspace background indexing

Date: 2026-09-09

Audience: editor users and tooling maintainers.

Workspace initialization scans and save-time workspace-symbol index analysis
now run on the LSP's Tokio blocking pool. A saved document's result is applied
only if the open document still has the captured generation, so a later edit
cannot be overwritten by an older background save analysis.

Compatibility: LSP responsiveness improves; language and command-line compiler
behavior are unchanged. This does not add cooperative compiler cancellation.

Verification:

```bash
cargo test --offline lsp::
```
