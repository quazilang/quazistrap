# Markdown fence verification

Date: 2026-09-09

Audience: documentation maintainers and contributors.

The offline documentation verifier now recognizes Markdown fenced code blocks
using either backticks or tildes, with the opening fence's width and marker
type preserved until a matching closing fence. Headings and inline links inside
those blocks are excluded from validation, as they are source text rather than
Markdown structure.

This closes false failures (and false heading targets) for valid tilde or
longer-width fences. The checker remains deliberately small and
dependency-free; it is not a complete Markdown renderer.

Compatibility: documentation-validation behavior only. No language or runtime
behavior changes.

Verification:

```bash
cargo test --offline docs::tests
```
