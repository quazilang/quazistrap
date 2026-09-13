# Exact semantic binding spans for LSP edits

The compiler now records the exact identifier spelling that resolves a binding
alongside each expression annotation. This keeps the annotation's existing
whole-expression span for analysis and code generation while giving tooling a
safe range for a direct identifier or function callee.

Same-document references, rename, and go-to-definition now use that exact
binding span. A request on a call's parentheses or argument list no longer
selects the callee, and a rename of `helper(42)` edits only `helper` rather than
the whole call expression.

This is an internal tooling-correctness improvement with no language or API
compatibility change. Cross-file references and rename remain unsupported;
they additionally require stable declaration and import-selector occurrences
across loader snapshots.

Regression coverage verifies exact call-callee spans, exact rename edits, and
that call delimiters do not resolve a callee.
