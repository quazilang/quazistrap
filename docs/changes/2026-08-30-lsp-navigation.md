# LSP Same-Document Navigation

Audience: editor users and tooling maintainers.

## Change

The contained `qz lsp` server now advertises and implements document symbols,
signature help, references, rename, and full-document semantic tokens in
addition to diagnostics, hover, completion, definition, and formatting.
Definition resolution now prefers the semantic target of calls, including
methods, before applying its existing current-document fallback.

References and rename use the semantic binding identity recorded during type
checking: canonical symbol name, declaration span, and symbol kind. This keeps
shadowed locals separate and also distinguishes a function from a parameter
with the same spelling. Function and method call edits select only the callee
identifier rather than the enclosing call expression.

Parameter declarations now retain their own identifier span in the parser AST.
This improves the semantic symbol metadata used by tooling; it does not change
language syntax or runtime behavior.

## Compatibility and Migration

This is an additive LSP protocol change. Editors that already start `qz lsp`
need no configuration migration. The server still uses full-document text
synchronization and only analyzes open documents, so references and rename do
not cross file boundaries yet. Semantic-token ranges and lengths use LSP UTF-16
code units.

## Verification

- `cargo test --offline` with the pinned stable `rustc`: 478 tests passed.
- Focused LSP regressions cover shadowed locals, method definitions, direct
  function calls, generic-call signature help, invalid rename identifiers, and
  a parameter sharing its function's name. Semantic-token tests cover
  symbol classification and Unicode UTF-16 token lengths.

See the [LSP guide](../tooling/lsp.md) for the supported protocol surface and
remaining limitations.
