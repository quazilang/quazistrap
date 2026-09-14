# C-string receiver capabilities

Borrowed-pointer accessors on `std.ffi.CStr` and `CString` now use shared
receivers. `CString.len` and `as_c_str` are shared as well.

## Why

These operations inspect an existing pointer or length without transferring or
destroying allocation ownership. A shared receiver preserves that distinction
and prevents a safe access from overlapping an exclusive operation on the
owner.

## Compatibility

Existing calls on local C strings are source-compatible. `CString.free`
retains its legacy by-value spelling until the D-014 consuming receiver effect
is implemented; callers must still obey the documented raw-pointer lifetime.

## Verification

The C-string source-level ownership regression compiles and runs with the
contained compiler, and canonical documentation tests validate the API page.
