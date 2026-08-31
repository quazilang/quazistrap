# Quazi Tree-sitter Grammar

Audience: editor-integration and source-tooling developers.

The maintained grammar lives in the sibling `tree-sitter` repository. Its
package is `tree-sitter-quazi`, its language scope is `source.quazi`, and
Quazi source files use the `.qz` extension.

The grammar is a concrete-syntax parser: it accepts language syntax and emits
named nodes and fields, but it does not assign compiler semantics. In
particular, a struct or union field may include arbitrary postfix attributes:

```quazi
struct User {
    name: String @ini("username") @json(name="user_name"),
    flags: u32 @bits(width=3,):3 @wire("v1"),
}
```

Each `@…` is an `attribute` child of `struct_field`; its name and literal or
identifier arguments remain opaque. A consumer must not reject an attribute
just because it does not recognize its name. The authoritative language
semantics and argument syntax are in the [language guide](../LANGUAGE.md#attributes).

Run the grammar corpus from its repository with:

```bash
XDG_CACHE_HOME=/tmp npm test
```

Generated parser artifacts (`src/parser.c`, `src/grammar.json`, and
`src/node-types.json`) are committed. Regenerate them whenever `grammar.js`
changes, then run the corpus suite. Existing highlight queries already capture
all `attribute` nodes; editor packages should build on this grammar and start
the contained server with `qz lsp` for semantic features.
