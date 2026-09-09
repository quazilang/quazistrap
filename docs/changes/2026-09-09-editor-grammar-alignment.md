# Editor grammar alignment

Date: 2026-09-09

Audience: editor users and tooling maintainers.

The Zed and Helix integrations now pin the current immutable
`tree-sitter-quazi` revision. The previous revision predated compiler-supported
attribute and relative-import forms, opaque field attributes, named arguments,
raw byte strings, and the current `for`-based loop syntax.

Both integrations now ship the canonical highlight query. This also fixes the
Helix query's obsolete `while` node reference, which prevented that query from
loading with the current grammar.

Each integration has a `tests/check-canonical-assets.sh` check. In a sibling
workspace it verifies that the manifest's revision matches the local canonical
grammar checkout and that its highlight query is byte-for-byte identical to
the canonical query.

Compatibility: language identity (`quazi`), scope (`source.quazi`), `.qz`
association, and `qz lsp` launch command are unchanged. Editors gain parsing
and highlighting for current supported syntax. This does not replace runtime
validation in the unavailable Zed and Helix editors.

Verification:

```bash
cd ../zed-quazi
bash tests/check-canonical-assets.sh
cargo check --offline
cargo fmt -- --check

cd ../helix-quazi
bash tests/check-canonical-assets.sh

cd ../tree-sitter
npm test
npm run test:workspace-conformance
```
