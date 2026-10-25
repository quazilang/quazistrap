# Source context for declaration types

Audience: compiler and tooling developers.

Transparent alias bodies resolve imported names in their declaration file.
The actual arguments of an alias application resolve in the caller's context.
These contexts must remain distinct during recursive expansion.

After declaration and import authentication, `semantic/type_context.rs`
canonicalizes each stored alias body using its source's authenticated type
imports and the selected prelude's public implicit imports. It uses an empty
alias table, preserving forward applications, recursive definitions and lexical
alias parameters for the normal bounded expansion pass. The parsed AST stays
immutable. Stored alias formals are alpha-renamed to private, dot-free keys that
cannot be spelled in source. This prevents an imported nominal from becoming a
same-named formal after its qualifier is removed. Declaration symbols retain
readable formals; semantic alias expressions expand through the authoritative
alias table and enforce arity rather than consuming display metadata. An already
converted `@repr(C)` callback retains its `CFn` representation. Named types and
dynamic trait names participate in canonicalization and parameter substitution;
a dynamic trait actual must be a plain named type.

The loader marks exactly its canonical selected prelude entry with
`SourceFile.implicit_prelude`. Its public selectors, including wildcard and
public reexport chains, contribute the implicit environment. Private imports
do not. A file basename does not grant this role. Selector spans must belong
to their owning source and targets must match authenticated declarations.
Multiple implicit prelude markers fail with S14.

The shared alias resolver applies caller name mapping to source queries and
substitution values. It does not apply that mapping inside a canonical alias
body. Alias actuals are normalized before entering the alias frame. Alias-local
parameters and caller parameters remain separate. Parser-only clients without
loader metadata retain bare declaration names. Clients supplying imported alias
bodies must canonicalize them in their defining context first.

Source paths use indexed lookup; public-import and alias-owner spans use sorted
interval lookup. The pass polls cancellation while walking items, sources,
selectors and updates. It caps selector/alias sites at one million; individual
type traversals retain the shared depth, node, text and expansion budgets.
Cancellation is operational: callers discard partial analysis.

`semantic/declaration_types.rs` then normalizes original function/global symbols,
struct/serialization fields, enum payloads and trait signatures under their
source context and declaration binders. Authenticated import targets refresh
function/global copies, including legacy bare-name loading. Source lookup and
alias expansion share one resolver traversal: an imported `own.Word` cannot
become a lexical `Word` binder between two passes. Function parameter/return
checking and retention seed their types from source spans; trait conformance
protects method binders through the final runtime-shape comparisons.

Declaration normalization caps aggregate sites and type nodes at one million
and copied type-name text at 16 MiB. Repeated binder materialization is charged
before allocation. Exhaustion latches, records S14 and cannot publish success;
item, method, field and imported-record loops poll operational cancellation.
Alias preparation separately bounds parameter counts and mapping text.

Full source isolation remains incomplete. Raw local annotations, casts,
call-site generic actuals, parts of declaration validation and source codegen
still use global source binding tables. The known private namespace visibility
defect remains open, including unused unresolved aliases. Canonical nominal
names are not yet a complete declaration-identity IR. Do not infer module
isolation or general binder hygiene from this migration. See
[the open findings](../audits/2026-10-23-string-followups.md).

QZC31 rebuilds previous exact/reusable source chunks (the earlier alias-only
checkpoint used QZC30). QZI14 is unchanged and Unverified; no ownership or effect
certificate follows from type name resolution. See
[verification and compatibility](../changes/2026-10-24-declaration-type-context.md).
