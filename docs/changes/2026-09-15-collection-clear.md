# In-place collection cleanup

Audience: users of `std.collections` and runtime maintainers.

`Map.clear(&Map!)` and `Set.clear(&Set!)` release backing storage and retain an
empty reusable owner. `Map.free(self: Map)` and `Set.free(self: Set)` are now
the consuming destructors used by lexical cleanup.

## Compatibility and migration

Programs that inspected a collection or called `free()` more than once must
use `clear()` instead. A final release may be omitted in favor of scope cleanup
or expressed with consuming `free()` when control leaves the owner immediately.

## Rationale

This aligns collection release with the ownership model used by `String` and
`CString`, and lets compiler-generated cleanup recognize only actual consuming
destructors. It does not add generic element destruction; that remains part of
the structural-drop milestone.
