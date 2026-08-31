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
| Neovim | `../nvim-quazi/` | native Neovim 0.11+ LSP configuration | headless configuration test |
| Helix | `../helix-quazi/` | configuration/runtime query package | static configuration review; Helix binary unavailable |
| Zed | `../zed-quazi/` | Zed language extension | `cargo check` and formatting check |

The canonical grammar is the separate [`tree-sitter`](../../../tree-sitter/)
repository. Its immutable grammar revision is pinned by integrations that need
to fetch a parser. Consult each project README for installation, compatibility,
troubleshooting, local development, and packaging instructions.

## LSP boundary

The server is currently reliable for versioned **full-document** updates only.
It provides current-document diagnostics, hover, definitions, completion,
formatting, symbols, signature help, references, rename, and semantic tokens.
Workspace indexing, cross-file references/rename, incremental synchronization,
and cancellation are not implemented. See [the LSP contract](lsp.md).
