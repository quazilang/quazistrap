# Runtime String identity and representation

Audience: compiler developers.

Owned-string operations require the declaration exported as String by the
loader-selected prelude. A name, qualified suffix, field shape, or QZI interface
spelling alone grants no runtime role. With no selected export, an ordinary
source struct called String remains an ordinary aggregate.

The loader preserves the canonical defining file and original declaration name
through public re-exports, aliases, wildcards, cfg and overlays. SourceFile marks
that defining declaration with runtime_string. SourceTypeImport records actual
source spellings, selector spans and canonical export targets, including namespace
members. Full selector paths matter: fake/string.qz cannot authenticate a
declaration from another string.qz just because their basenames match.
Traversal is cancellable and bounded; parsed source snapshots and export queries
are reused. Nested public namespace requests are resolved from actual source
spellings rather than an unbounded namespace expansion.

After declarations, semantic analysis matches each import target to its actual
public declaration and global symbol span. Type alias conflicts fail. Every
explicit selector must discharge its own provenance obligation, even when
another import already introduced the same local name. Shared alias resolution
protects declaration binders before lookup; qualified aliases and nominals use
declaration-backed bindings, without a leaf-name fallback.

CheckedStringAbi contains the selected ResolvedBinding and data_offset. Its
declaration must be a public, nongeneric, nonunion ordinary source struct, with
one alias-resolved str data field and representable total allocation/field offset.
The offset is computed from the ordinary VM-slot layout; data need not be first.
Malformed selected contracts fail S14. Analyzer reuse clears all derived proofs.

Source/concrete ordering checks, parse routing, String-specific borrow handling,
display coercion and native lowering consume this identity. Codegen verifies
the declaration span and uses the checked data offset. It never defaults String
extraction to field zero. Imported constructor aliases also resolve before field
ordering, allocation sizing, stores and associated method dispatch. A type
parameter cannot impersonate a concrete static namespace by sharing its spelling. One owner-reference layer can be loaded
for string comparison; nested owner references are rejected until the necessary
load sequence is supported. Shared string-view wrappers retain their existing
representation exception.

Builtin as_str/as_string fallback accepts string views, not arbitrary references.
Real String methods remain resolved ordinary source methods. Implicit formatting
accepts numbers, Bool, views and the checked owner, including supported references.
Deferred generic arguments are checked against every proven concrete identity
with bounded, cancellable declaration facts. Raw pointers, slices and unrelated
aggregates need explicit conversion. Codegen independently rejects unsupported
UTF-8 coercions and unproven owned formatter results. This is no implicit user
Display dispatch or whole-program effect proof.

QZC29 rebuilds older exact/reusable source chunks. Cache compiler identity also
includes selected canonical builtin std/prelude roots, so runtime-root overrides
cannot retain old bodies. Ordinary source content remains covered by input
hashes. QZI14 is unchanged and remains Unverified. Newly emitted QZI already
contains checked lowering; old bodies need source rebuilding to acquire these
checks. Interface spelling alone cannot become the selected runtime declaration.
No artifact ownership/effect certificate is inferred.

The global source binding table still exposes private namespace aliases across
files. This is a separate confirmed visibility defect, not a completed module
isolation contract. See [follow-up evidence](../audits/2026-10-23-string-followups.md),
[migration](../migrations/runtime-string-abi.md) and
[verification](../changes/2026-10-23-runtime-string-abi.md).
