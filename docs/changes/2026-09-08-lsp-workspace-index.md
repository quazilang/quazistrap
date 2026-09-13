# Persistent local workspace symbols and relative definitions

The contained LSP now indexes parseable `.qz` files under client-negotiated
local workspace folders, or the legacy local `rootUri` when no folders are
provided. `workspace/symbol` therefore no longer requires a user to open every
file first.

The index is read-only and intentionally bounded: it skips `.git`, never
follows symlinks, and requires both the presented path and canonical target to
remain below a selected root. Unsaved open buffers override their disk snapshot
for workspace symbols; saving updates the snapshot. An unparsable open buffer
does not fall back to stale disk declarations.

Go-to-definition now follows explicit relative leaf imports such as
`import ./helpers.answer as answer` to a unique public top-level declaration
in the target file. It does not yet resolve wildcard, module, package,
dependency, or `std` imports: those require compiler-loader source mapping to
produce correct cross-file ranges.

This is additive editor behavior. No language syntax, compiler semantics, or
standard-library API changed. Focused LSP tests and the full compiler test
suite cover the workspace boundary, disk/open overlay precedence, relative
aliases, private targets, and stale-target avoidance.
