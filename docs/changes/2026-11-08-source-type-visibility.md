# Per-file type name visibility

Audience: language users, library maintainers and compiler developers.

Type names are now resolved in the source file that spells them. Previously all
loaded declarations shared one global table, so a file could use a type it never
imported, and an import or namespace alias made in one file (`import text as ns`
in `helper.qz`) leaked into every other file. Completely unknown type names in
annotations, such as `fn take(value: Missing)`, were also accepted when the
function was never called.

The new `semantic/type_visibility.rs` pass runs after declaration types are
canonicalized. It walks every type spelling and value-position type head in
enabled items: function signatures and bodies, struct fields, enum payloads,
trait methods, aliases, foreign globals, `impl` targets, local annotations,
casts, call and method type arguments, struct literals, `Type.method()` and
`Type.Variant` receivers, and enum patterns. A spelling is accepted when it is
declared in the same file, bound by that file's authenticated imports (including
namespace spellings), a public type of the loader-selected prelude package, a
builtin, or a lexical generic binder (`Self` inside traits and `impl`s). Local
variables and parameters shadow value-position type heads. Other spellings fail
with `S04`, either `unknown type 'Name'` or
`type 'Name' is not imported into this file`.

See the [visibility rules](../language/modules.md#type-name-visibility) and the
[migration notes](../migrations/source-type-visibility.md).

## Compatibility

Programs that relied on the leak now fail to check. Add an explicit import in
each file that spells the type. The runnable I/O tutorial example now imports
`std.fs.{File, FsError}`. Parser-only clients without a loader source map keep
their previous global lookup. Source cache QZC32 replaces QZC31 so previously
accepted cached chunks are re-analyzed; QZI14 is unchanged and remains
Unverified.

Types provided only by QZI library interfaces have no source declaration and
remain visible in every file of a program that links the library.

## Verification

- `cargo test --offline -q` passes 992 tests. New regressions cover unimported
  types in parameters, struct literals and array annotations; namespace imports,
  generic binders and locals named like a type; and unknown annotations. The
  loader regression now requires `S04` for a prelude-private import alias and for
  another file's private namespace alias.
- Two test helpers built import evidence with `Span::new(start, end, 1, 1)`
  instead of `Span::new(line, col, start, end)`. Their qualified-name proofs were
  silently discarded and the tests passed only through the global leak; the
  helpers now build correct spans.
- Every example under `examples/` gives the same `qz check` result as the
  previous compiler (examples 30 and 31 fail with the same pre-existing `S10`
  ownership errors). A program importing every `std` module reports exactly the
  same diagnostics with both compilers.
