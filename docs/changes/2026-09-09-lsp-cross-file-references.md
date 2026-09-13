# Loader-backed cross-file LSP references

The contained LSP now resolves references through the compiler loader's full
configured import graph. It reuses the loader's effective source map to rebase
merged semantic spans into UTF-16 locations in each owning file, including
unsaved open-document overlays.

Rename uses the same resolved occurrence set. It updates declaration spellings,
non-aliased uses, and import selectors; a local `as` alias and its uses retain
their local name. It returns one multi-file workspace edit only if every edited
source is inside a client-negotiated workspace root. This deliberately prevents
a request on an imported symbol from partially renaming local call sites or
editing the standard library and package dependencies.

Focused regressions cover a Unicode-bearing importer, an import-selector
cursor, local-alias preservation, grouped multi-file edits, the
`includeDeclaration` reference setting, and rejection when an affected file is
outside the permitted edit set.
