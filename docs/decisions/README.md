# Maintainer decisions

Audience: Quazi maintainers.

These questions define compatibility-sensitive language contracts. They must be
resolved before dependent implementation and documentation can be called stable.

## D-001: generic value layout

Choose between compiler-sized inline generic storage, boxed generic elements, or
an explicit temporary restriction to one-word plain-copy values. Word width
alone is insufficient: ordinary Quazi aggregates are one-word heap handles, but
shallowly loading one creates another apparent owner. Full support therefore
needs per-monomorphization size/alignment/move/drop metadata and ownership-correct
APIs (`get` must borrow or clone; `take` may transfer). That requires an ABI/QZI
layout design. The restriction is safer short-term but source-breaking for
owned-element uses in the standard library.

## D-002: receiver ownership

Decide whether ordinary `self: T` methods borrow, consume, or depend on an
explicit receiver marker. Mutation-returning owners such as `map = map.insert()`
cannot be made sound until receiver and return ownership are unambiguous.

## D-003: destruction and explicit close

Define whether destruction is structural, trait-based, or both; its order; move
suppression; behavior on assignment/return/panic/thread exit; and how explicit
`close`/`free` prevents later automatic destruction.

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
