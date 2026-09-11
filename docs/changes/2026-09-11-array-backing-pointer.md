# Array backing-pointer accessor

`Array[T]` now provides `unsafe as_ptr() -> *T` for APIs that require a
borrowed pointer to contiguous element storage.

## Why

Some runtime-backed interfaces need a pointer-and-length slice rather than an
implementation-defined `Array` object. The accessor exposes the existing
contiguous backing storage without adding a process-specific collection type.

## Safety and compatibility

The pointer is valid only until the array is reallocated, freed, or cleaned up
with its owner. It may be accessed only for `len()` elements; writes must
preserve valid initialized values and the ownership invariants of `T`. It is
unsafe because callers can otherwise create dangling pointers or violate those
requirements. Existing array behavior is unchanged.

## Verification

The compiler suite parses and type-checks the prelude alongside its normal
source and documentation checks.
