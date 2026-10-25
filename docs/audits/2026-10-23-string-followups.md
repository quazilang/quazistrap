# String checkpoint follow-up findings

Audience: compiler developers continuing the full production-readiness goal.

The [runtime String checkpoint](../changes/2026-10-23-runtime-string-abi.md)
replaces unsafe spelling/layout assumptions with the selected declaration and
checked field offset. Independent review also confirmed a separate visibility gap.

## P1: private namespace aliases leak between source files

Check-only fixture: `/tmp/qz-string-crossfile-review-qn2t5qyp/case`.
The selected custom prelude publicly exports text.String. helper.qz imports
prelude.text as ns privately. main.qz imports only helper, then declares a
comparison accepting ns.String. The current installed compiler accepts check,
even though main does not import ns. No executable was emitted or run.
Log: `/tmp/quazi-string-followup-private-alias.log`.

The global import and type binding tables permit one file to borrow another
file's alias. Rebinding ns to a different namespace is already rejected S05;
the confirmed case is an unimported alias becoming visible, not an observed
counterfeit String layout execution.

Resolution must include the source file of each type spelling. Alias bodies
resolve in their declaration file; generic arguments resolve at their call site.
Publish canonical annotation/signature/field types, or retain explicit source
context into deferred checks and codegen. A mutable current-file cursor is not
sufficient for recursive aliases or concrete specialization closure. Preserve
declaration binders before imported-name lookup.

Regression requirements: reject helper-private aliases in main; accept a valid
namespace import in each file; preserve aliases/re-exports/cfg, generic shadowing,
header generation, loader-backed LSP navigation and QZI interfaces. Verify codegen
uses the same canonical identity. Do not hide the defect by accepting arbitrary
qualified leaf names or merely rejecting all qualified types.

## Remaining audit work

The 2026-10-24 [alias-definition prerequisite](../internals/source-type-context.md)
canonicalizes stored alias bodies in their defining file, preserves public
implicit prelude exports and stops caller-import remapping during expansion.
The global raw source tables remain: this is progress toward the fix, not
closure of the P1 finding above. QZC30 invalidates earlier source chunks;
QZI14 remains Unverified.

Logical import redirection, including symlinks below an unchanged runtime root,
needs a dedicated cache audit. The new root identity hashes selected canonical
roots; it is not a certificate of every logical import lookup. This is an audit
boundary, not a newly verified symlink reproducer.

General numeric normalization/coercion, generic equality/reference consistency,
physical f16 storage, complete loans/effects, recursive structural destruction,
artifact certification and the wider tooling/documentation milestones remain
open. This checkpoint does not establish full production readiness.

## Declaration-context prerequisite (2026-10-24)

[Stored declaration normalization](../changes/2026-10-24-declaration-type-context.md)
now preserves defining-source signatures, fields, payloads, trait methods and
imported copies. Generic alias formals have private identities, and function
parameter/return/retention seeds use source spans. 981 Rust tests and the focused
native/source-removed-QZI/COFF regression pass. QZC31 supersedes QZC30;
QZI14 remains Unverified. This does not close the P1 namespace leak above:
remaining raw annotations, validation, deferred call-site actuals and source
codegen still require coordinated migration.
