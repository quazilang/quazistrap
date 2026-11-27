# D-030: `str` is a pointer and a length, without a terminating NUL

Decided 2026-11-27 by the maintainer ("без 0"), replacing the deferred
plan of 2026-11-26 that kept the NUL terminator.

`str` becomes two 8-byte slots `{ptr, len}`:

- `bytes_len()` reads `len` (O(1)); the `StrLen` opcode reads the slot
  instead of scanning for a zero byte.
- No NUL terminator is guaranteed. A `str` may therefore view the middle of
  another string: slicing (`{ptr + a, b - a}`) needs no copy.
- Every operation that creates a `str` records its length: literals (known
  at compile time), `String.as_str()` (from `String.len`), `str_from_ptr`
  (one `strlen` when crossing in from C), program arguments, environment
  values, formatting and concatenation results.
- C interop is explicit, as in Rust (`&str` versus `CStr`): passing a `str`
  where C expects `char*` goes through `std.ffi.CString`/`CStr`, or the
  compiler builds a temporary NUL-terminated copy at an `@api` boundary.
  Raw code that relied on a terminator (`core.str_as_ptr(s)` handed to C or
  scanned to zero) must change.
- `@repr(C)` structs keep a single `char*` where C has one; a `str` field
  there is not the fat value.
- QZI format and QZC cache versions bump; older artifacts are rejected with
  a rebuild request.

Rejected alternative: keep the NUL terminator (zero-copy FFI, but slices
must copy).
