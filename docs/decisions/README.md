# Maintainer decisions

Audience: Quazi maintainers.

These questions define compatibility-sensitive language contracts. They must be
resolved before dependent implementation and documentation can be called stable.

## D-001: generic value layout

Resolved 2026-08-29: **full runtime-layout implementation**. Generic storage
gains per-monomorphization size/alignment/move/drop metadata, an ABI that no
longer truncates multi-register values, ownership-correct element access
(`get` borrows or clones; `take` transfers), and recursive drop glue. The
change crosses the internal ABI, so QZI/QZC boundaries are bumped with it and
legacy artifacts require a source rebuild. It still depends on a future
receiver-ownership decision (D-002) and a complete destruction model (D-003).

Original question: compiler-sized inline generic storage, boxed generic
elements, or an explicit temporary restriction to one-word plain-copy values.
Word width alone is insufficient: ordinary Quazi aggregates are one-word heap
handles, but shallowly loading one creates another apparent owner. Full support
therefore needs per-monomorphization size/alignment/move/drop metadata and
ownership-correct APIs (`get` must borrow or clone; `take` may transfer). That
requires an ABI/QZI layout design. The restriction is safer short-term but
source-breaking for owned-element uses in the standard library.

## D-002: receiver ownership

Resolved 2026-09-12 by [D-014](whole-program-ownership.md): receivers are
explicit shared (`self: &T`), exclusive (`self: &mut T`), or consuming
(`self: T`) capabilities. The current compiler still treats ordinary receivers
as borrowed; grammar, call-site moves, and legacy API migration remain
implementation work.

## D-003: destruction and explicit close

Resolved 2026-09-12 by [D-014](whole-program-ownership.md): destruction is
structural, begins with a Drop hook, then destroys owned fields/elements in
reverse declaration order; moves and explicit `free`/`close` suppress later
destruction, and termination-only panic does not unwind. Current cleanup is
not yet that implementation.

## D-004: compatibility and stability

Define the current language stability level, supported source/QZI compatibility
window, deprecation policy, and whether safety corrections may break programs
that compiled only because of permissive typing.

## D-005: supported platforms

Define tiered support for x86-64 Linux, x86-64 Windows, macOS, and other targets,
including which standard-library modules and tests each tier guarantees.

## D-006: panic model

Choose termination-only versus unwinding, cleanup guarantees, thread behavior,
custom-handler exact signature/lifecycle, recursion handling, and exit status.

## D-007: concurrency model and terminology

Define native Quazi thread/task semantics, result and panic propagation,
cancellation, structured cleanup, synchronization, and communication. Naming
research is technical context rather than legal advice; do not copy another
language's model by implication.

Blocked on implementation of [D-014](whole-program-ownership.md): no safe
concurrency surface can be finalized before borrow transfer, ownership effects,
and QZI-only composition are enforceable.

## D-008: TLS and trust policy

Select a maintained TLS backend and define certificate/hostname verification,
trust-store source, protocol policy, backend/version support, and update model.

## D-009: time-zone data

Define whether civil-time support bundles, discovers, or delegates an IANA time
zone database and how updates/versioning work. Monotonic duration APIs can proceed
independently of this decision.

## D-010: runtime dynamic values

Current stabilization rule: `any` is reserved and cannot carry runtime values
because the VM, native ABI, and QZI format do not define a tag, payload layout,
ownership, or checked downcast. `@format ...args: any` remains a compiler-erased
call-site convention and does not create an `any` value. A future dynamic-value
feature requires an explicit maintainer decision covering representation,
ownership/destruction, trait interaction, casts and pattern matching, FFI, and
QZI compatibility; it must not restore universal implicit compatibility.

## D-011: child-process creation

Accepted 2026-09-01 and expanded with an approved first-release contract on
2026-09-10: [child-process creation belongs to the runtime](process-runtime.md).
The narrow Linux runtime and `std.process` facade are implemented. Platform
expansion and process features that can retain resources, callbacks, or
asynchronous state are blocked on [D-014](whole-program-ownership.md).

## D-012: serialization

Resolved 2026-09-01: [serialization uses static typed derives, not runtime reflection](serialization.md).

## D-013: formatting result ownership

Open: [choose an owned result contract for dynamic formatting](formatting-ownership.md).

## D-014: whole-program ownership and escape discipline

Resolved 2026-09-12: [whole-program ownership and escape discipline](whole-program-ownership.md).
Quazi uses capability-based, whole-program escape analysis rather than source
lifetime annotations. QZI-only dependencies require compiler-verified
ownership summaries; implementation and artifact-versioning remain pending.
