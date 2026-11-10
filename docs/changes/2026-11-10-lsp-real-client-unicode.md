# Real-client Unicode and formatting protocol check

Audience: editor users and tooling developers.

Position conversion and formatting were only covered by unit tests and
stdio scripts that build LSP messages by hand. `../vim-quazi/tests/lsp-unicode.lua`
now drives the contained server through Neovim 0.12's own LSP client:

- incremental edits beside non-BMP text (`λ🙂 你好🎉`, emoji before code on
  the same line) are sent by the client in its UTF-16 encoding;
- the unknown-name diagnostic and a hover range after `🙂🙂` must land on the
  columns Neovim computes for the same text;
- whole-document formatting of a misindented line with trailing blanks must
  equal `qz fmt` output for the same text and keep the non-BMP text.

It passes against the current `qz`, as do the existing headless and LSP smoke
tests. LSP 19/20, plan 98.1%. Zed and Helix runtime validation remains open:
neither editor is available in this environment.
