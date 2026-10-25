# Defining context for stored declaration types

Audience: language users, library maintainers and compiler developers.

Stored function signatures, foreign globals, struct fields, enum payloads and
trait method signatures now resolve imported types in the file declaring them.
Imported function/global copies inherit the original record rather than
resolving its raw spelling at the import site. Function parameter bindings,
return expectations, retention facts and trait conformance use the same
source-aware resolution.

Previously, a method parameter `T` could be mistaken for a private import named
`T`, allowing an implementation with a different parameter type to satisfy a
trait. Such implementations now fail with S14. Qualified `own.Word` remains the
imported alias even inside a declaration whose generic parameter is `Word`.
Stored generic aliases use private parameter identities, preventing
`type Pick[Record] = own.Record` from capturing the nominal `Record`. Symbol
metadata and parsed source preserve user-facing parameter names. Dynamic trait
parameters share alias substitution; non-named actuals fail explicitly. Bare
generic aliases used as expressions must supply their required arguments.

Source cache QZC31 rebuilds previous exact/reusable chunks. QZI14 is unchanged
and remains Unverified. Rebuild existing artifacts from source for current
checks. No source syntax changes are required for valid programs.

Verification: 981 serial offline Rust tests pass. Focused regressions cover
stored/imported records, binder collisions, immutable ASTs, method conformance,
alias-expression arity, dynamic substitution, cancellation and aggregate work
limits. The declaration-context CLI runner confirms that the previous installed
compiler accepts the invalid trait during check only; the new compiler rejects
it during check/native/QZI/Windows COFF. A valid imported signature runs natively
and from QZI after all fixture source files are removed, and emits Windows COFF.

This migration is incomplete: private namespace aliases still leak through
remaining global tables. Local annotations, casts, call-site type arguments,
raw declaration validation and source codegen require further migration. This
checkpoint does not prove module isolation, ownership/effect certificates or
production readiness. See [the implementation boundary](../internals/source-type-context.md).
