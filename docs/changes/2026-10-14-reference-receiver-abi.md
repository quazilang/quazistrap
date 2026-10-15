# Canonical safe-reference receiver ABI

Audience: Quazi users, library authors, and compiler contributors.

## Problem and resulting behavior

Passing `&owner!` through a normal function and then calling a struct method
could overwrite the caller's owner handle. Normal references addressed a value
slot, while explicit struct receivers expected the heap handle itself. The
[original audit](../audits/2026-10-14-reference-receiver-abi.md) records a real
Linux SIGSEGV and the bytecode establishing this representation mismatch.

Explicit shared and exclusive method receivers now use the ordinary safe
reference ABI: a pointer to addressable value slots. Calls borrow owned receiver
storage or forward an existing reference. Aggregate field reads/writes,
bitfields, flexible-tail addresses, and generated dense-container metadata
operations load the heap handle from the referenced slot before accessing it.
Direct method calls, `Index`, indexed replacement/compound updates, format/parse
shortcuts, and compiler-generated iterator calls use the same adaptation.
Legacy/value receivers retain value arguments, materializing a referenced value
when appropriate. No receiver/API-name cast workaround was introduced.

Shared `str` and `&str` remain representation-identical string views. Taking or
reading a shared string reference does not add a pointer level. String-reference
methods follow this exception; checked `parse[T]()` uses its declared
`parse_<suffix>` receiver capability, including shared `String` views. Explicit
custom `parse` methods keep their own declaration capability. Named Index
inference unwraps borrowed receivers before substituting concrete element types
and registering setter specializations; mutation and owning-read restrictions
still use the original capability.

Dynamic source dispatch passes the concrete value slot in word zero of its fat
value to borrowed trait receivers. Observed trait target sets retain their
methods and ordinary call dependencies. Discovery reads semantic `Dyn` facts
without resolving arbitrary template annotations outside their binder scopes.
Native test emission now retains the source report needed to build vtables.

## Storage regressions found during resumed review (2026-10-15)

Independent review reproduced two more accepted-source miscompilations: an
exclusive scalar method on `owner.field` modified a loaded temporary, and
indexing an exclusive fixed-array reference overwrote the reference register
instead of the pointee element. Projected receivers now evaluate their base
once and borrow actual field storage, including nested handle replacement.
Scalar qword C fields support this ABI, including packed fields; narrow fields,
bitfields, and inline C aggregates receive S14 for exclusive receiver calls.

Fixed-array reference reads, plain writes, compound writes, and increment use
the existing slot pointer with bounds checks. The checked address is fixed
before evaluating the assignment RHS, so an RHS that changes the index cannot
redirect the store or bypass its guard. A native regression failed before these
fixes (`/tmp/quazi-resume-places-before.log`) and passes afterward. The added
`prelude/tests/reference-receiver-places.qz` fixture covers field mutation,
nested replacement, forwarded field mutation, literal/dynamic array indexes,
RHS index mutation, and packed qword C fields. A semantic regression rejects
narrow C fields and bitfields.

## Compatibility

QZI advances to v12 and QZC to v12. Pre-v12 reference/aggregate or unavailable
interfaces and opaque outbound calls are rejected with source-rebuild guidance
when compatibility cannot be established. Parameterized old libraries must
retain real callable declarations, not just a nonempty interface encoding.
Proven closed scalar/raw-pointer/shared-string contracts retain compatibility.
Older bytes can still be decoded for inspection; decoding does not authorize
current linking. Original ownership ABI generation survives namespace aliasing
and reserialization. Cache invalidation covers exact hits and reusable chunks.
See [the migration](../migrations/reference-receiver-abi.md).

A genuine v11 shared-receiver library is retained as an immutable fixture, with
writer source, revision, size, and SHA-256 in its
[fixture record](../../src/bytecode/fixtures/qzi/README.md). It decodes but fails
current linking. Its actual raw-handle `FieldLoad` without a preceding slot load
prevents the regression from being reduced to a synthetic version-byte test.

## Verification

Run from the compiler root:

```sh
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
export RUSTFLAGS='-C link-arg=-fuse-ld=bfd'
cargo test --offline receiver -- --nocapture
cargo test --offline golden_v11_receiver -- --nocapture
TMPDIR="$(mktemp -d /tmp/quazi-slot-ref-suite-XXXXXX)" cargo test --offline -q
cargo build --offline -q
cargo clippy --offline -q
target/debug/qz build prelude/tests/reference-receivers.qz --no-incremental -o /tmp/quazi-slot-ref-native -r
target/debug/qz build prelude/tests/reference-receivers.qz --no-incremental -i -o /tmp/quazi-slot-ref.qzi
target/debug/qz build /tmp/quazi-slot-ref.qzi --no-incremental -o /tmp/quazi-slot-ref-roundtrip -r
target/debug/qz build prelude/tests/reference-receivers.qz --no-incremental -c --target x86_64-windows -o /tmp/quazi-slot-ref.obj
target/debug/qz build prelude/tests/reference-receivers-dynamic.qz --no-incremental -o /tmp/quazi-slot-ref-dynamic -r
target/debug/qz build prelude/tests/reference-receivers-dynamic.qz --no-incremental -c --target x86_64-windows -o /tmp/quazi-slot-ref-dynamic.obj
target/debug/qz build prelude/tests/reference-receiver-places.qz --no-incremental -o /tmp/quazi-resume-places -r
target/debug/qz build prelude/tests/reference-receiver-places.qz --no-incremental -i -o /tmp/quazi-resume-places.qzi
target/debug/qz build /tmp/quazi-resume-places.qzi --no-incremental -o /tmp/quazi-resume-places-roundtrip -r
target/debug/qz build prelude/tests/reference-receiver-places.qz --no-incremental -c --target x86_64-windows -o /tmp/quazi-resume-places.obj
python3 ../std/scripts/check_net_receivers.py "$PWD/target/debug/qz"
git diff --check
```

The resumed full compiler suite passes 752/752; build and clippy succeed with
existing warnings. Independent reviewers checked receiver lowering and artifact
compatibility, then reviewed the field/array fixes and RHS address stability.

The native fixture checks nonzero field offsets, source/receiver forwarding,
reference aliases, direct field mutation, pointee replacement/readback, shared
observations, generic named receivers, indexed writes/reads/compound updates,
dense removal, scalar reference methods, shared string references, borrowed
String formatting, and checked parsing. A second fixture checks dynamic source
receiver readback. The previously crashing direct program and larger alias/view
probe now exit zero. The prior binary still crashes on the same direct program,
recorded in `/tmp/quazi-slot-ref-before-native.log`.

The isolated real STD receiver suite checks 10 native cases, three negative
lifetime probes, QZI execution, and Windows COFF emission. Full STD compilation
remains outside that script and still requires the pending Headers/JSON migration.
Formatting is limited to owned changes; existing codegen/test-runner formatting
differences remain preserved. Windows host execution was unavailable.

## Remaining requirements

This corrects representation and source native behavior, not D-014 provenance
or effect certification. Envelopes remain `Unverified`; structural destruction,
parameter-derived aggregate retention, and D-015 element loans remain incomplete.
Pointee replacement readback is tested, not complete replacement cleanup.

At this checkpoint, QZI-only dynamic vtables lacked retained layout metadata.
This artifact gap is resolved by the later
[QZI v13 change](2026-10-15-portable-vtables.md). The original evidence follows.
 The dynamic source
fixture emitted QZI, but compiling that artifact failed to resolve
`__vtable_DynamicCounter_Readable` because no source report is available. This was
a confirmed artifact requirement rather than a passing portable dispatch
test. Dynamic cleanup and source unused-method warning precision remain open.

Additional investigation found existing source-analysis gaps for a generic alias
sharing a function binder's spelling and for method-local generic substitution.
The lowering regression supplies alias metadata independently to test binder
isolation without confusing that separate source-analysis gap with receiver ABI
correctness. These broader generic issues remain follow-up work.
