# `std.ffi`

Audience: Quazi developers writing a narrow, audited foreign-function boundary.

`std.ffi` supplies C ABI vocabulary and explicit C-string ownership helpers.
It is not a binding generator and does not make a foreign API safe. Prefer a
typed standard-library wrapper when one exists; otherwise keep raw-pointer
operations inside a small `unsafe` adapter with the foreign library's lifetime,
threading, and error rules documented alongside it.

## C ABI aliases

Fixed-width aliases such as `c_int`, `c_uint`, `c_short`, `c_uchar`,
`c_float`, and `c_double` map to their corresponding Quazi fixed-width scalar
types. `c_size`, `c_ssize`, `c_ptrdiff`, `c_intptr`, and `c_uintptr` follow the
target pointer width.

`c_long` and `c_ulong` are target dependent: they are 32-bit on Windows and
64-bit on Linux and macOS. Use the fixed-width aliases when a foreign contract
requires an exact size. `c_void` is `void`; `va_list` is an opaque ABI marker
only—Quazi does not marshal C variadic arguments through it.

## Raw pointers

`unsafe nullptr[T]()` creates a typed null `*T`. It is unsafe because the
result is a raw pointer, not because creating its zero representation accesses
memory. Passing, dereferencing, retaining, or freeing it remains governed by
the foreign API's contract and Quazi's unsafe rules.

## Borrowed `CStr`

`unsafe CStr.from_ptr(ptr)` wraps a foreign NUL-terminated `*c_char` without
copying or freeing it. The caller must prove that `ptr` remains readable and
NUL-terminated for every use of the `CStr`. `unsafe as_ptr()` exposes the same
borrowed raw pointer; it neither extends lifetime nor transfers ownership.

## Owned `CString`

`CString` owns a separately allocated, NUL-terminated byte buffer. It is the
appropriate input form when foreign code requires a C string and does not take
ownership of the pointer.

- `try_from(bytes)` copies exact bytes and rejects the first embedded NUL as
  `CStringError.InteriorNul(index)`; allocation failure is
  `CStringError.AllocationFailed`.
- `try_from_str(text)` applies the same check to UTF-8 text.
- `unsafe from_unchecked(text)` is only for legacy `str` input where an
  embedded NUL cannot be checked. The caller must ensure the foreign API can
  safely consume the resulting truncation-prone C string.
- `len()` is the stored byte length, excluding the terminating NUL.
- `as_c_str()` creates a borrowed `CStr` valid only while the owning `CString`
  remains alive. `unsafe as_ptr()` exposes its raw pointer under the same
  lifetime restriction.

Normal local scope cleanup invokes `CString.free()` once. Call `free()` only
for an earlier release and never use the `CString`, its `CStr`, or its raw
pointer afterward. Do not call `core.free` on a pointer borrowed from `CStr`;
only the matching `CString` owner may release its allocation.

```quazi
import std.ffi;

fn open_name() Result[usize, CStringError] {
    var name: CString = CString.try_from_str("config.json")?;
    // Pass name.as_ptr() only to a foreign function that borrows it for the call.
    ret Ok(name.len());
}
```
