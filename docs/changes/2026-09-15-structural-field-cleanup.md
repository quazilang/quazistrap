# Constrained structural field cleanup

Audience: compiler maintainers and users relying on lexical cleanup.

The bytecode generator now performs compiler-generated cleanup of eligible
fields for an acyclic, non-generic, non-`repr(C)` struct that has no source
consuming `free(self)` hook. Fields are visited in reverse declaration order,
and each terminal hook must be a semantically recorded consuming receiver.

## Deliberate boundary

A source `free(self)` hook remains the complete cleanup route for its type;
automatic traversal never runs beside it. The implementation intentionally
defers recursive layouts, generic specializations, arrays, enums, `dyn`,
closure environments, physical aggregate-storage release, and place-level move
state. The existing partial-move restrictions therefore remain active.

This is a verified D-003 pilot, not completion of structural destruction.
