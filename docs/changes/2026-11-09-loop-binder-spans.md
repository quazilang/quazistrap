# Loop binder diagnostics point at the binder

Audience: language users and tool authors.

`for i : 0..n { ... }` declared `i` with the span of the whole loop statement,
so W01 ("unused variable") underlined from `for` to the end of the loop: the
renderer printed one caret per character of the multi-line span, far past the
end of the line. The same span reached LSP features that use symbol spans.

`ForLoop::Each` now records `var_spans` (one per binder, including the second
binder of `for index, item : ...`), and semantic analysis declares binders with
them. A semantic regression checks the W01 span of each binder.
