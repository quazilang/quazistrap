# Editor integrations

Audience: editor users and tooling developers.

Quazi editor support is maintained in separate repositories beside the
compiler. Every integration uses the `.qz` extension, language identifier
`quazi`, and starts the contained server as `qz lsp` where its host supports
LSP. They do not bundle the compiler or assign semantic meaning to opaque field
attributes.

| Editor | Project | Integration type | Local verification |
|---|---|---|---|
| VS Code | `../vscode-quazi/` | extension with language registration and LSP launcher | manifest parsing and `npm pack --dry-run` |
| Neovim | `../nvim-quazi/` | native Neovim 0.11+ LSP configuration | headless configuration and real-client LSP smoke on Neovim 0.12 |
| Helix | `../helix-quazi/` | configuration/runtime query package | canonical grammar-asset check; Helix binary unavailable |
| Zed | `../zed-quazi/` | Zed language extension | canonical grammar-asset check, `cargo check`, and formatting check |

The canonical grammar is the separate [`tree-sitter`](../../../tree-sitter/)
repository. Its immutable grammar revision is pinned by integrations that need
to fetch a parser. Consult each project README for installation, compatibility,
troubleshooting, local development, and packaging instructions.

## LSP boundary

The server accepts versioned UTF-16-safe incremental updates and compatible
full-document replacements.
It provides diagnostics, hover, definitions, completion, formatting, symbols,
signature help, references, rename, semantic tokens, inferred type hints, and
safe unused-import quick fixes.
Workspace-symbol search covers parseable local `.qz` files below the selected
workspace roots; unsaved open buffers override their disk snapshots. The index
does not follow symlinks or scan outside the selected roots. Cross-file
references and rename follow loader-backed imports when edits remain in
client-negotiated workspace roots. JSON-RPC transport cancellation is supported
for pending requests; superseded diagnostic analyses cooperatively stop at
lexer, parser, and semantic pass/top-level-item boundaries. Loader traversal
and expensive semantic inner loops remain non-cooperative. See [the LSP
contract](lsp.md).
