# Signedness-correct integer lowering

Audience: Quazi language users and tooling developers.

Quazi now preserves unsigned integer semantics through semantic constant
folding, QZI optimization, and x86-64 lowering. Division and remainder use
unsigned machine instructions for `u8`, `u16`, `u32`, `u64`, and `usize`;
ordered comparisons use unsigned conditions; and right shift is arithmetic for
signed integers and logical for unsigned integers.

All integer shift counts use their low six bits, matching the 64-bit QZI slot
model. Signed `i64::MIN / -1` wraps to `i64::MIN`, and the corresponding
remainder is zero; constant folding and native execution now agree. Propagated
negative constants remain signed 64-bit constant-pool values instead of being
re-encoded as unsigned 16-bit immediates.

Previously, high-bit unsigned values were interpreted as negative during
division, remainder, and ordered comparison. In particular, a high-bit `usize`
could bypass a source-level `index >= length` check. This was a silent
miscompilation and memory-safety risk.

The QZI container version is now 7 because integer instructions use a newly
defined signedness flag. The compiler still reads compatible QZI v2-v6
bytecode. QZI v1 omitted required frame metadata and must be rebuilt from
source; parameterized v6 trait interfaces likewise require rebuilding so v7
can preserve receiver identity. Older compilers do
not understand v7 and must not consume it.

Source code normally needs no migration. Rebuild checked-in or cached QZI files
with the current compiler before distributing them to v7 consumers. Projects
that intentionally rely on signed reinterpretation of an unsigned high-bit
value must use an explicit signed cast.

Regression coverage verifies semantic constant folding, bytecode constant
propagation, ordinary and compound code generation, signed and unsigned right
shift selection, native SysV/Win64 instruction selection, QZI v7 round-tripping,
and native high-bit arithmetic through the `qz test` example suite.
