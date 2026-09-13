# API reference surface alignment

Date: 2026-09-09

Audience: Quazi users and documentation maintainers.

The canonical API reference now names the remaining public surface found in a
source-to-reference audit: the full fixed-width C alias set and `c_bool` in
`std.ffi`; `Url` component accessors and `HttpRequest.body_text()` in
`std.net`; `object_field_with_limits` and JSON error display text; codec error
display text; and dynamic-library error display text.

Compatibility: documentation-only. These APIs were already public; the update
does not change their behavior.

Verification:

```bash
cargo test --offline docs::tests
```
