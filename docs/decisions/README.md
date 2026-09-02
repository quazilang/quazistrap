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

Open. The current compiler treats ordinary method receivers, including
`self: T`, as borrowed. It does not implement `&mut` receiver syntax or a
receiver move at a method call. APIs must therefore either mutate their sole
owner in place (as `std.collections` now does) or avoid returning an owning
alias. The older claim that explicit `&`/`&mut`/consuming receivers were
implemented was not true of the current compiler.

Decision required: decide whether methods gain explicit shared/mutable/consuming
receivers, how calls record a receiver move, and which legacy APIs migrate.
Until then, mutation-returning owners such as `map = map.insert()` are invalid;
new APIs must not expose that pattern.

## D-003: destruction and explicit close

Open. Current scope cleanup recognizes a named type's `free(self)` method and
suppresses that cleanup after an explicit `free()` call. It does not yet
recursively destroy owned fields/elements, track aggregate-place moves, or
provide a formal Drop hook. The earlier claim that structural destruction was
implemented was not true of the current compiler.

Decision required: define whether destruction is structural, trait-based, or
both; its order; move suppression; behavior on assignment/return/panic/thread
exit; and how explicit `close`/`free` prevents later automatic destruction.

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

Resolved 2026-09-01: [child-process creation belongs to the runtime](process-runtime.md).

## D-012: serialization

Resolved 2026-09-01: [serialization uses static typed derives, not runtime reflection](serialization.md).

## D-013: formatting result ownership

Open: [choose an owned result contract for dynamic formatting](formatting-ownership.md).
