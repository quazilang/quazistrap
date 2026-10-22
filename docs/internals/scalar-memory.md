# Scalar memory conversions and numeric updates

Audience: compiler developers.

Raw-pointer scalar loads/stores share `c_memory_access` with C fields and
foreign globals. Resolved aliases and concrete generic substitutions determine
width, signedness and conversion. Signed narrow integer reads extend; unsigned
reads zero extend. Native f32 uses a four-byte access plus `FLOAT_FLAG` to convert
between external single precision and internal f64 VM slots. f64 uses a qword.

Safe references address VM value slots. An `&f32!` dereference therefore reads and
writes a qword without the native f32 memory-conversion flag. Never select the
raw-pointer representation merely from the expression result's float type.
Inline @repr(C) aggregate pointers retain the storage-address representation
introduced by the [operand checkpoint](operand-snapshots.md).

Numeric `++`/`--` evaluates the place once. Floating updates use floating 1.0 and
flagged Add/Sub, across local variables, referenced slots, raw pointers, fields,
array elements and foreign globals. The previous value is captured before the
store so postfix locals return their old value. Plain integer locals keep their
existing Inc/Dec path. Address width/conversion and arithmetic flags are separate
properties: a VM-slot f32 update still requires floating arithmetic.

This checkpoint proves the corrected memory footprint and arithmetic selection.
It does not define general f32 intermediate rounding, narrow integer overflow,
or normalization of assignment/compound/prefix expression results after a
narrow store. Those existing value-model questions need a consistent contract.
Native f16 memory conversion is also unimplemented. Non-numeric `++`/`--` acceptance
and generic numeric obligations remain a confirmed semantic-checking gap; no
ownership/effect certificate follows from these emission corrections.

QZC24 invalidates old exact and reusable source chunks. QZI13 representation is
unchanged and Unverified. Old QZI bodies must be rebuilt from source to receive
the fix; compatible instruction encoding does not repair their old lowering.
See the [verification record](../changes/2026-10-22-scalar-memory.md).
