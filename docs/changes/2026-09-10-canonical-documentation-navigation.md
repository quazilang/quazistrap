# Canonical documentation navigation

Audience: Quazi users and documentation maintainers.

The documentation and API-reference indexes now identify the structured
`docs/` sections as the current canonical contracts. Their prior wording said
the documentation was still being reorganized and directed users to legacy
flat pages as authoritative, contradicting the completed specification and
standard-library coverage ledger.

The flat pages remain available for historical context and stable inbound
links, but the structured language, API, tutorial, guide, tooling, and
internals sections define current behavior.

Compatibility: documentation-only. No language or standard-library behavior
changed.

Verification:

```text
cargo test --offline docs::tests
```
