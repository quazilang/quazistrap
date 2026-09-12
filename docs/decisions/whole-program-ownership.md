# D-014: whole-program ownership and escape discipline

Audience: Quazi maintainers.

Status: resolved 2026-09-12. This is a language-design decision, not an
implementation claim. The current function-local move checker, receiver
behavior, destruction support, and QZI format do not yet implement it.

## Decision

Quazi will use whole-program ownership and escape analysis over the resolved
specialization call graph. It will not require Rust-style lifetime names or
per-function lifetime contracts in source. Whole-program visibility improves
precision; it does **not** relax the aliasing rules below.

Safe code uses capabilities rooted at a concrete storage place:

- An owned value has exactly one affine owner. Moving it transfers the owner
  and makes the source unusable. Only `Plain` values may be copied; an owned
  handle or aggregate is never shallow-copied.
- `&T` is a shared loan. Any number may coexist, but while any is live its
  root and all aliases cannot be moved, destroyed, mutated, or reallocated.
- `&mut T` is an exclusive loan. It is the only access to its root for its
  region: no other shared or exclusive loan, owner use, mutation, move, or
  destruction may overlap it. It may be temporarily reborrowed, freezing the
  parent exclusive loan until that child loan ends.

Method receivers are therefore explicit: `self: &T` borrows shared,
`self: &mut T` borrows exclusively, and `self: T` consumes. This resolves the
policy question in D-002; implementing its grammar and migrating legacy
receivers remains work. Ordinary current receivers do not acquire these
capabilities merely because this decision exists.

The analysis derives loan regions from real control flow and real call edges.
It tracks each loan's root, projections, and escape path flow-sensitively. A
reference may be returned, stored, or captured only when the analysis proves
that every reachable use ends before every move or destruction of its root.
This permits call-site-specific valid programs without lifetime annotations;
a temporary or local reference that can outlive its root remains an error.

Calls are solved to a conservative fixed point across resolved direct calls
and concrete generic specializations. Recursion is accepted only when that
fixed point proves the required effects. Indirect calls, callbacks, and trait
dispatch need a closed target set and composed effects; otherwise they are an
opaque boundary and safe code cannot pass or receive borrowed values there.
Raw pointers, foreign calls, and C-ABI boundaries remain `unsafe`; safe
reference-bearing ABI contracts are not introduced by this decision. `dyn`
values remain excluded until their vtable has both effect and destruction
metadata.

## Destruction and ownership transfer

This decision adopts the structural-destruction direction in D-003 as the
owner-side complement to escape analysis. On normal scope exit and replacement,
and for every non-transferred local during a return, the compiler runs a type's
Drop hook and then destroys its owned fields/elements in reverse declaration
order. A move, return, consuming receiver, or explicit `free`/`close` transfers
or consumes that owner and suppresses its later automatic destruction. D-011's
`Child.close` specifically consumes before it can report failure; other future
resource APIs must state whether an error consumes or leaves their owner for
retry. Panic remains termination-only: no unwinding cleanup is promised.

The compiler must track moves at place granularity. It may not destroy an
aggregate field/payload after that place was moved, and it may not move a
field/payload out of a still-owned aggregate except through a defined
consuming operation. This resolves D-003's policy question; drop glue and
place-level implementation remain prerequisites to claiming the guarantee.

## QZI-only libraries

Source availability is **not** required for safe library consumption. A
QZI-only dependency must instead carry a mandatory compiler-generated,
machine-checkable ownership summary for every exported callable and callable
dispatch slot. A downstream whole-program analysis composes those summaries
with the consumer's graph; it does not trust a hand-written lifetime-like
signature or inspect unavailable source.

Each summary is keyed by the qualified callable and concrete specialization
and is content-bound to the public interface and bytecode. It must record:

1. receiver and parameter capability (`Copy`, `Move`, shared borrow, or
   exclusive borrow), plus reads, writes, consumption, and destruction;
2. result ownership and, for a reference result, exact provenance from a
   parameter/root/projection or a proven static root;
3. every escape edge: return, aggregate/heap/global storage, closure or
   callback capture, foreign boundary, and process/thread/task transfer;
4. closed indirect-call targets and a transitive conservative export effect;
   private callee effects need not be exposed to consumers; and
5. exported-type layout, move, and drop facts needed by callers to construct,
   transfer, and destroy values.

The QZI must also contain compiler-generated typed ownership/effect IR or a
certificate sufficient for the downstream compiler to verify the transitive
export effect against the retained callable bodies. Canonical ordering, bounds,
version, and the content binding are necessary validation but are not proof of
effect correctness. A public summary is sufficient only after that verification
establishes it conservatively summarizes private callees. Missing, malformed,
stale, or pre-summary QZI is rejected at a safe ownership boundary with a
rebuild-from-source/current-compiler diagnostic; it is never silently treated
as an unknown-safe call. An explicitly `unsafe` opaque interface is the only
future escape hatch.

Implementing this contract requires a new QZI major version and matching QZC
invalidation, dedicated serialization/verification tests, and migration
documentation. Current QZI v2-v9 and QZC v7 carry no ownership-summary
section; they must be rejected whenever a D-014-aware safe ownership link needs
that metadata. The decision deliberately does not allocate future version
numbers or change legacy compilation behavior before that implementation lands.
Public generics, whose template bodies are currently source-only, remain source
dependencies until they receive an equally complete specialization/effect
contract.

## Process and concurrency boundary

No borrow may cross a thread, task, or independently running process boundary.
Such a boundary receives owned transfers only, after all loans rooted in the
transferred value have ended. A synchronous process-launch call may borrow its
program/argument inputs for the duration of that call, but captured output,
callbacks, asynchronous I/O, thread/task creation, and shared state require
this ownership analysis plus the sendability/synchronization policy of D-007.

D-011's existing narrow Linux `Child` operations retain their documented
single-owner contract, but D-011 is blocked from platform expansion and from
new asynchronous, callback, pipe, or shared-resource APIs until this decision
is implemented and audited. D-007 and all safe concurrency surface are blocked
on the same implementation; its scheduling and synchronization choices remain
separate decisions.

## Required implementation evidence

Before stabilizing any dependent API, add compiler and source regressions for
shared-versus-exclusive conflicts, move-after-borrow, returned/stored/captured
references, recursive and indirect calls, structural drop after partial moves,
QZI-only summary composition/rejection, and process/thread transfer attempts.
Run those against source dependencies and QZI-only dependencies on every
supported target. The D-011 and D-007 follow-up work must reference this
evidence rather than independently defining resource ownership.
