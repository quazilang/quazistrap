# LSP background compiler snapshots

Date: 2026-09-09

Audience: editor users and tooling maintainers.

Document diagnostics and loader-backed definition, reference, and rename
requests now run compiler work on Tokio's blocking pool. This keeps expensive
parsing, loading, and semantic analysis from occupying the asynchronous LSP
workers.

Loader-backed results are accepted only when every captured open document is
still open with the same source text. A request therefore returns no stale
locations or rename edit after an included unsaved buffer changes or closes
while analysis is in progress.

Cancellation remains a transport-level boundary: it can stop response waiting,
but cannot preempt a compiler task that has already begun. The compiler has no
cooperative cancellation API yet.

Compatibility: LSP responsiveness and stale-result safety improve. The
language and command-line compiler behavior are unchanged.

Verification:

```bash
cargo test --offline lsp::
```
