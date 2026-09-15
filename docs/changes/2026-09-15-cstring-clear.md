# In-place `CString` cleanup

Audience: users of `std.ffi` and runtime maintainers.

`CString.clear(self: &CString!)` releases the owned NUL-terminated allocation
and leaves a valid empty owner. It is repeat-safe and is the operation to use
when a C string must remain available for reuse after its storage is released.

`CString.free(self: CString)` remains the consuming destructor. A direct call
transfers the owner and prevents later use; lexical scope cleanup invokes that
operation for a live owner. This separates reusable reset from final disposal
and keeps consuming receiver effects uniform across resource-owning types.

## Compatibility and migration

Code that inspected a `CString` or called `free()` repeatedly must use
`clear()` instead. Code that is finished with the owner may omit the explicit
call and rely on scope cleanup, or use consuming `free()` immediately before
transferring control.

## Verification

The `std.ffi` regression constructs a C string, clears it, verifies its null
pointer and zero length, and clears it again. Compiler receiver-capability
tests cover exclusive and consuming receiver effects.
