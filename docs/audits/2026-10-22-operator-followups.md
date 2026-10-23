# Remaining operator representation findings

Audience: compiler developers continuing production-readiness work.

The [operator checkpoint](../changes/2026-10-22-operator-requirements.md) closes
bitwise/shift category gaps and validates generic ordering categories. The
following findings were recorded at that checkpoint; no production-readiness claim follows.

| Priority | Finding | Evidence and impact | Next action |
| --- | --- | --- | --- |
| Resolved 2026-10-23 | Pow emission misinterprets numeric values and needs an external pow symbol | encoder Opcode::Pow always converts signed slots to doubles and back, ignoring unsigned/floating representation; source permits nonnumeric handles too | Implement the [approved typed-power contract](../decisions/power-operator.md), including checked integer arithmetic and negative-exponent diagnostics. Preserve exact integer arithmetic without hidden libc/libm linkage. |
| P0 | String recognition accepts counterfeit layouts | Check-only `/tmp/quazi-string-identity-audit-59q4dx22/source`: `struct String{value:i32,}` plus generic ordering is accepted; existing lowering interprets its first field as text | Establish shared, validated String identity/runtime ABI evidence for semantic analysis and lowering. No counterfeit executable was built or run. |
| P1 | External shared output lacks stack metadata | [previous audit](2026-10-22-arithmetic-followups.md), readelf and host-loader rejection | Emit the ordinary empty nonallocated .note.GNU-stack marker in ELF objects, then verify source/QZI objects, builtin/external executables and default shared loading. |

The maintainer approved [D-016](../decisions/power-operator.md): known negative
integer exponents are compile-time errors, runtime negative integer exponents
panic, integer multiplications are exact with overflow checks, and floating
bases support negative powers. This supersedes the earlier truncation selection.
Implementation and source/native/QZI/COFF verification are recorded in the
[typed-power checkpoint](../changes/2026-10-23-typed-power.md).

Existing Bool bitwise semantics are established by a dedicated test and Boolean
constant evaluator, so this checkpoint preserves them. Bool normalization,
shift-count/narrow-width normalization, generic equality operand consistency,
mixed arithmetic coercion, physical f16 storage, whole-program effects and
verified artifacts remain separate work. The source type-name convention is not
proof of String storage, effect or ownership validity.
