# Documentation verification

Date: 2026-09-09

Audience: documentation maintainers and contributors.

The compiler test suite now checks canonical documentation offline. It rejects
missing repository-local inline Markdown link targets and ATX-heading
fragments, including links that cross documentation sections. External URLs
and links into sibling repositories are deliberately not fetched: their
availability and checkout state are not deterministic in this repository's
compiler tests. Reference-style links, multiline destinations, setext
headings, and renderer-specific HTML heading behavior remain outside this
small dependency-free checker.

Tutorial snippets are intentionally not compiled by this checker. The current
tutorial mixes complete programs with partial declarations and illustrative
fragments; source-controlled fixtures for the complete tutorial programs remain
the next documentation-verification checkpoint.

Compatibility: documentation-only. The checker adds no language or runtime
behavior.

Verification:

```bash
cargo test --offline docs::tests
```
