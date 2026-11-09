# LSP features keep working while a document does not parse

Audience: editor users and compiler developers.

While a call or statement is half typed the document has a parse error. The
server published that error and kept no analysis for the new text, so hover,
completion and signature help either returned nothing (a document opened in
that state) or answered from the previous text's offsets.

`src/lsp/recovery.rs` builds parseable approximations: the error line, the
previous non-blank line (an unfinished line is usually reported at the next
token, such as a closing brace), both, and finally the enclosing top-level
item are blanked in turn; blanking turns every character except line breaks
into a space, so offsets and lines stay those of the edited text. The first
candidate that parses is analyzed in the same background task and becomes the
document's report; only the parse error is published.

Limits: positions inside the blanked region have no answers, loader-backed
references and rename still need a parseable document, and when no candidate
parses the previous report is kept.

Verification: unit tests for the candidates and for a recovered analysis that
keeps declarations before and after the broken line at their offsets, and
`scripts/check_lsp_incomplete_source.py` (signature help inside the
unfinished call, hover before and after it, completion with the recovered
function's local, definition) over stdio. LSP 18/20, plan 97.9%.
