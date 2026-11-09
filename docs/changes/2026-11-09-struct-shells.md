# Struct storage shells are released

Audience: language users and compiler developers.

Every struct value is a heap handle. Structural destruction only produced a
drop action when some field needed destruction, so a struct with only scalar
fields (and no `free` hook) was never released: each one leaked its storage
(a whole page with the mmap-per-allocation runtime allocator). A loop that
created 40000 such structs grew from 2060 to 40065 mapped pages.

`structural_drop_action_for_named` now returns a structural action even when
no field needs destruction, so the existing structural cleanup releases the
shell (`ReleaseStorage`) after its fields. Ownership tracking is unchanged:
analysis already treated these structs as owned (moves, no moves out of
fields), so only the missing release was added. Nested scalar structs release
both shells.

Concrete generic structs (`Pair[i64]`, `Named[T]` with `T` known, including
`String` fields) follow the same structural rule since a follow-up the same
day: codegen receives the declared struct type parameters
(`struct_generic_params`) and substitutes the arguments into the field types;
templates with unresolved parameters keep no action.

Still open (checklist item "complete structural destruction"): enum values
(`Option`, `Result`, user enums) and C-layout structs keep leaking their
shells.

Verification: `scripts/check_struct_release.py` (2060 -> 40065 pages before,
60 -> 65 after); a codegen regression checks that a nested struct releases two
shells without calling a non-consuming `free(&T!)`; all tests, e2e scripts,
examples (also run) and the std suite pass.
