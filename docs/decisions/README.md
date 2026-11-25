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
explicit shared (`self: &T`), exclusive (`self: &T!`), or consuming
(`self: T`) capabilities. Resolved direct inherent calls now implement the
bare-receiver move and cleanup transfer. Legacy API migration, associated
Index dispatch, indirect/dynamic calls, and interprocedural effects remain
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
Recursion handling decided 2026-11-15 (В16А): a second panic while handling
one prints `panicked while processing a panic` and exits with 101 without
calling the handler again.

Resolved 2026-11-18 (В28): the fatal contract stays — `@panic_handler fn(PanicInfo) !`
runs once, the stack is not unwound, local destructors are not guaranteed,
the default exits 101 and a qoroutine panic reaches `join()` (D-019). The
maintainer asked for readable diagnostics: backtraces name their functions
and `QUAZI_TRACE=2` adds crash registers (Linux implemented; Windows on
2026-11-19, verified under Wine; NetBSD panic backtraces with names on
2026-11-25, verified on NetBSD 11.1 — NetBSD has no signal crash handler yet).

## D-007: concurrency model and terminology

Define native Quazi thread/task semantics, result and panic propagation,
cancellation, structured cleanup, synchronization, and communication. Naming
research is technical context rather than legal advice; do not copy another
language's model by implication.

Blocked on implementation of [D-014](whole-program-ownership.md): no safe
concurrency surface can be finalized before borrow transfer, ownership effects,
and QZI-only composition are enforceable.

Direction chosen 2026-11-18 (В29А): cooperative cancellation; a consumed
`JoinHandle[T]` waits when destroyed; an isolated panic is returned as an
error from `join()` after guaranteed cleanup. Implementation stays blocked on
D-014; qoroutine rules from D-019 are unchanged.

Implemented 2026-11-19 for `std.thread.spawn` ([`std.thread`](../api/thread.md)):
the parts of В29А that do not widen what crosses a thread boundary. A panic
on a spawned thread returns `Err(ThreadError.Panicked)` from `join()`; the
thread's block, closure and stack are released, while destructors in the
abandoned frames do not run (no unwinding, as for qoroutines). A dropped
`JoinHandle` joins; `JoinHandle.cancel` / `thread.cancel_requested()` are the
cooperative cancellation surface, and the prelude `Channel[T]` (moved into
thread closures) is the communication surface. Mechanism: the prelude's `__qo_isolate`
runs the thread body on the same OS stack through `quazi.context.switch` and
registers it by OS thread id (`quazi.thread.current`); `panic` returns there
instead of calling the handler. Moving owned values into threads and
cross-thread channels remain blocked on D-014 (closure captures and results
keep the current rules).

## D-008: TLS and trust policy

Select a maintained TLS backend and define certificate/hostname verification,
trust-store source, protocol policy, backend/version support, and update model.

Resolved 2026-11-18 (В30А): system TLS libraries and the system trust store,
with supported versions and dependencies documented per platform. Certificate
and hostname verification are on by default; the test matrix includes a local
TLS server, an untrusted certificate and a wrong hostname. Implemented on Linux
(2026-11-19) as [`std.tls`](../api/tls.md) over OpenSSL 3; Windows (Schannel)
and NetBSD remain open.

## D-009: time-zone data

Define whether civil-time support bundles, discovers, or delegates an IANA time
zone database and how updates/versioning work. Monotonic duration APIs can proceed
independently of this decision.

Resolved 2026-11-18 by [D-025](maintainer-answers-2026-11-18.md) (В25А):
`std.timezone` discovers the system IANA database (`$TZDIR`,
`/usr/share/zoneinfo`, `/etc/zoneinfo`, `/usr/share/lib/zoneinfo`), so updates
come with the operating system; nothing is bundled. Windows has no database
unless `TZDIR` names one; POSIX TZ rules work everywhere.

## D-010: runtime dynamic values

Current stabilization rule: `any` is reserved and cannot carry runtime values
because the VM, native ABI, and QZI format do not define a tag, payload layout,
ownership, or checked downcast. Formatting functions take `...args: Display`
([D-024](display-variadics.md)) instead of the removed `@format ...args: any`. A future dynamic-value
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

Resolved 2026-10-09: [dynamic formatting returns owned String](formatting-ownership.md). Implementation and migration remain pending.

## D-014: whole-program ownership and escape discipline

Resolved 2026-09-12: [whole-program ownership and escape discipline](whole-program-ownership.md).
Quazi uses capability-based, whole-program escape analysis rather than source
lifetime annotations. QZI-only dependencies require compiler-verified
ownership summaries; implementation and artifact-versioning remain pending.

## D-015: borrowed dense-container elements

Accepted 2026-09-16: [generic dense-container element borrows use a
compiler-validated provenance contract](borrowed-elements.md). The required
address operation and QZI/QZC compatibility update land together with D-014
ownership summaries; no `Array`-specific compiler exception is permitted.

## D-016: typed power operator

Accepted 2026-10-22: [typed power with exact checked integer arithmetic](power-operator.md).
Known negative integer exponents are compile-time errors; runtime negative
integer exponents panic. Floating-base negative powers remain fractional.
Implementation is pending.

## D-017: tail values and `if` expressions

Assessed and implemented 2026-11-08: [adopt with modifications](expression-oriented-blocks.md).
A function body's final expression may omit `;` and supplies its value; tail
position extends into final `if`/`else` branches and `unsafe` blocks. `if` with
a mandatory `else` and single-expression branches is a value expression,
defined as a two-arm Boolean `match`. `ret` stays the only early exit. General
block expressions are deferred until value-carrying block scopes exist.

## D-018: module-qualified type identities

Proposed 2026-11-08: [give types module identities like functions](module-type-identity.md)
so same-named types in different modules stop colliding (`S05`). Not implemented.

## D-020: string interpolation in every literal

Decided 2026-11-09: [every string literal interpolates `{expr}`, `{{`/`}}`
escape braces, no `f"..."` prefix; `async`/`await` leaves the plan in favor of
`qo`](string-interpolation.md).

## D-019: qoroutines, channels and OS threads

Recorded 2026-11-08: [keyword `qo`, built-in `Qoroutine[T]`/`Channel[T]`, M:N
scheduler, method-based channels, `std.thread` with `JoinHandle`/`Mutex`/
`AtomicI64`](qoroutines.md). Open points (type spelling, panic policy,
preemption staging, closure spawns, JIT scope) are tracked in the workspace
`asks.txt`.

## D-021: maintainer answers of 2026-11-10

Decided 2026-11-10: [numeric to_str, consuming match, plain variadics, thread.spawn captures, casts, fs and process requests](maintainer-answers-2026-11-10.md).

## D-022: checked integer arithmetic

Decided 2026-11-15: [integer `+ - * /` and unary minus panic on overflow; constants that overflow are compile errors; `wrapping_*` wrap explicitly](checked-integer-arithmetic.md).

## D-023: maintainer answers of 2026-11-15

Decided 2026-11-15: [to_str removal, consuming match, plain variadics, closures, nested panics, process timeouts, deserialization errors, module constants, trait bounds before generic Map](maintainer-answers-2026-11-15.md).

Decided 2026-11-16: [Display variadics replace `@format`](display-variadics.md) (D-024).

## D-025: maintainer answers of 2026-11-18

Decided 2026-11-18: [generic values move unless `T: Copy`, `@derive(Copy)` and `AsRef`, system time zone data, streaming `Reader`/`Writer`](maintainer-answers-2026-11-18.md).

## D-026: attribute-driven marshalling

Accepted 2026-11-18: [`@serde("json", "ini")` structs, field renames, per-format traits, documents](serde-marshalling.md).
