# Confirmed safe-reference receiver ABI mismatch

Audience: compiler maintainers. Status: the demonstrated receiver corruption
is fixed by the canonical slot-reference migration described in the
[change record](../changes/2026-10-14-reference-receiver-abi.md). The evidence
below records the original defect; whole-program ownership remains incomplete.

## Evidence

A normal safe-reference argument is a pointer to a register slot holding the
value. Unary `&owner!` emits `Lea` of the slot holding the aggregate handle.
Struct methods with explicit `self: &Owner` or `self: &Owner!` currently receive
the heap aggregate handle directly. Their field loads/stores use that handle.

Consequently this source passes analysis but `edit` writes through the owner
slot, rather than through its heap handle:

```quazi
struct Owner { text: str }
impl Owner {
    fn edit(self: &Owner!) void { self.text = "changed"; }
}
fn inspect(owner: &Owner!) void { owner.edit(); }
fn main() void {
    var owner = Owner { text: "before" };
    inspect(&owner!);
}
```

The readback probes in `/tmp/quazi-provenance-{direct,one,two}.qz` build but
exit 101 after SIGSEGV. Their `.log` files show the fault. The larger
`/tmp/quazi-provenance-native.qz` builds and exits one instead of mutating the
caller's owner. `-d -i` output in `/tmp/quazi-provenance-debug.log` shows the
slot `Lea` forwarded to `Owner.edit`, whose `FieldStore` lacks a handle load.
This mismatch predates the local provenance checkpoint. Moving the reference
through one or two aliases preserves the mismatch. It is not an inliner-only
problem; the retained `Owner.edit` chunk already expects the wrong physical
representation for the forwarded argument.

The separate `/tmp/quazi-provenance-local.qz` direct-owned-receiver probe passes
Linux native and QZI execution and Windows COFF emission. This scopes the
failure; it does not establish general borrowed-reference runtime correctness.

## Required fix and verification

Unify safe-reference and explicit receiver representation, retaining slot
references for dereference/replacement semantics. Auto-borrow owned receivers,
load aggregate handles before field and dense-metadata access, and audit
shared/exclusive/enum receiver forwarding and aliases together. A dispatch-only
extra load based on source reference type is insufficient: existing struct
method `self` already holds a handle and would be dereferenced twice.

Treat any physical contract change as an ownership ABI migration. Audit QZI
origin preservation and legacy rejection, invalidate QZC, and document source
rebuilding. Do not introduce API-name or `self`-spelling exceptions.

Add native readback regressions for free-function exclusive parameters, one and
two transferred aliases, direct owned receiver calls, and forwarding through
`self` and its aliases. Cover direct field mutation, replacement through a
reference, shared observations, enum receivers, Linux source/QZI execution,
and Windows COFF emission; execute on Windows when a host is available.
Do not substitute static-dispatch assertions for pointer correctness.

This must precede claims of safe dense-element access, D-014 effect completeness,
or general resource API readiness. Parameter-derived aggregate retention and
artifact effect verification remain separate unresolved requirements.

## Implemented correction and remaining artifact gap

The current compiler auto-borrows owned explicit receivers, forwards ordinary
slot references, and loads aggregate handles for field/dense metadata access.
The original direct and transferred-alias native repros now exit zero. QZI v12
and QZC v12 prevent incompatible old artifacts/caches from bypassing that change.
A genuine v11 writer fixture locks down source-rebuild rejection.

Dynamic source dispatch also uses the concrete fat-value slot, retains target
methods, and passes its source report through native test emission. Source
native readback and Windows object emission pass. However, QZI-only dynamic
vtables still lack retained layout metadata: the backend currently constructs
vtables from a source SemanticReport. A standalone QZI round trip cannot supply
that report. This remains required artifact work; portable dynamic dispatch is
not established by the static receiver QZI fixture. Dynamic cleanup, accurate
source reachability warnings, and D-014 effect proof remain separate work.

## Follow-up (2026-10-15)

QZI v13 resolves the retained-layout gap above. Ordered table descriptors now
survive serialization, linking, namespace aliases, optimization and QZC units;
native source and QZI builds use the same descriptors. The original dynamic
fixture now executes through QZI and emits Windows COFF without a source report.
See [the change and verification record](../changes/2026-10-15-portable-vtables.md).
Ownership effects and dynamic cleanup remain open.
