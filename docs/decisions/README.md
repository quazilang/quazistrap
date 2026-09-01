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
legacy artifacts require a source rebuild. Depends on D-002 (receiver
ownership) and D-003 (destruction model), both resolved below.

Original question: compiler-sized inline generic storage, boxed generic
elements, or an explicit temporary restriction to one-word plain-copy values.
Word width alone is insufficient: ordinary Quazi aggregates are one-word heap
handles, but shallowly loading one creates another apparent owner. Full support
therefore needs per-monomorphization size/alignment/move/drop metadata and
ownership-correct APIs (`get` must borrow or clone; `take` may transfer). That
requires an ABI/QZI layout design. The restriction is safer short-term but
source-breaking for owned-element uses in the standard library.

## D-002: receiver ownership

Resolved 2026-08-29: **explicit receiver markers**. `self: &T` takes a shared
borrow, `self: &mut T` takes an exclusive mutable borrow, and a plain
`self: T` consumes the receiver by move. Ownership is therefore unambiguous at
every call site, and mutating methods no longer need to return shallow owner
copies (the unsound `map = map.insert()` idiom). Standard-library signatures
migrate mechanically; this extends the conservative reference model rather
than adding a new concept.

Original question: decide whether ordinary `self: T` methods borrow, consume,
or depend on an explicit receiver marker. Mutation-returning owners such as
`map = map.insert()` cannot be made sound until receiver and return ownership
are unambiguous.

## D-003: destruction and explicit close

Resolved 2026-08-29: **structural destruction with a Drop hook**. The compiler
recursively destroys owned fields in reverse declaration order; an explicit
`Drop` hook (the existing `free(self)` convention formalized) runs first for
the aggregate itself. Moves and reassignment keep exactly-once semantics: the
replaced value is destroyed before the new one is installed, and moved-from
values are suppressed. Calling `free()` explicitly acts as a move-out that
suppresses later automatic destruction. Panic terminates the process without
unwinding, so destructor guarantees apply to normal control flow only; this is
documented behavior, not an implicit promise.

Original question: define whether destruction is structural, trait-based, or
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
