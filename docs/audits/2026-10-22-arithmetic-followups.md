# Remaining operator and shared-library findings

Audience: compiler and tooling developers.

This follow-up audit accompanies the [arithmetic checkpoint](../changes/2026-10-22-arithmetic.md).
It records current gaps without assigning roadmap completion credit.

| Priority | Finding | Evidence | Impact and next action |
| --- | --- | --- | --- |
| P0 | Bitwise, shifts and power accept owning handles | Current debug `qz check` accepts `Owner & Owner`, `Owner << Owner`, `Owner ** Owner`; fixtures `/tmp/quazi-arithmetic-followups-4_56viv7/0` through `2` | Compatibility alone is insufficient. Extend operation-specific deferred constraints and concrete checks before allowing emission. No invalid executable was built or run. |
| P1 | Generic ordering lacks concrete category requirements | Fixture `3` in the same directory accepts `less[Owner]` with body `a < b` | Preserve numeric/string ordering, reject unsupported concrete categories after specialization closure, and test aliases/reference representation. |
| P1 | External shared output lacks GNU_STACK metadata | `/tmp/quazi-arithmetic-contract-audit-n0oqszf0/stack/lib.so`, built with `qz build --shared-lib`; `readelf -W -l` shows no GNU_STACK; current host ctypes loader rejects executable-stack enablement | Establish non-executable-stack metadata/default external-link flags. The arithmetic host fixture explicitly uses `-z noexecstack`. Plain builtin-linked executable stack guarantees are a separate supported path. |

The operator fixtures use `struct Owner{value:i32,}` and initialized values.
They are check-only: accepted typing does not authorize executing their invalid
owning results. Bitwise Bool behavior has implementation evidence but lacks a
clear canonical contract; obtain a maintainer decision if changing that behavior.
Pointer differences likewise need an explicit result contract rather than silent
C-style scaling or pointer-to-integer assumptions.

Mixed integer/float coercion, narrowed expression-result normalization, physical
f16 storage, general typed effects, recursive destruction and verified artifacts
remain open. Keep these findings separate from the corrected eager arithmetic,
reference-load and floating-remainder paths. Source caches now use QZC26;
QZI13 remains Unverified.
