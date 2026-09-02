# Platform implementation modules

Audience: maintainers of `std` platform wrappers—not ordinary application code.

`std.unix`, `std.windows`, and `std.win32_core` are low-level implementation
boundaries. They expose raw kernel/Win32 calls, integer handles, C-style paths,
and caller-sized buffers. They do not normalize errors, validate memory, or
provide portable behavior. Application code should use `std.fs`, `std.net`,
`std.os`, `std.thread`, or `std.dylib` instead.

## `std.unix`

`std.unix` maps names directly to Linux x86-64 syscalls. File descriptors are
`i32`; paths are NUL-terminated `str` values; status results retain the native
integer convention. Raw I/O, struct buffers, vector I/O, out-pointers,
directory buffers, and caller-controlled lengths are explicitly `unsafe`.

It is not a POSIX portability layer. It is unavailable as a meaningful API on
Windows: target-inappropriate syscall lowering must not be used as an emulated
fallback. Callers must supply the exact Linux structure layout, syscall flags,
and lifetime rules, and must handle interrupted or partial I/O themselves.

## `std.windows`

`std.windows` is a broad Win64 `@api` declaration layer. Opaque Windows
handles use `usize`; `BOOL` uses `i32`; `DWORD` uses `u32`; and out-parameters
remain raw pointers. Its declarations retain native `A` (byte-string) entry
points where applicable, so the caller is responsible for NUL termination and
the Windows API’s encoding expectations.

The module requires the correct native libraries (for example `kernel32`,
`user32`, `advapi32`, and `ws2_32`) for any declarations it uses. It does not
provide ownership wrappers or translate `GetLastError`; close the specific
kind of handle with the matching Windows API, not by guessing that
`CloseHandle` applies.

## `std.win32_core`

`std.win32_core` is the smaller pointer-correct substrate used internally by
the cross-platform standard library. Its functions are raw `unsafe` imports
for file handles, console conversion, environment/process lookup, Toolhelp,
Winsock, and dynamic-library primitives. Its `*u8`, `*u16`, and `**u8`
arguments must point to correctly sized, initialized Windows ABI storage for
the duration required by the native call.

Do not depend on this internal surface for portable application code. Its
signatures can change when `std.fs`, `std.net`, `std.os`, or `std.dylib` needs a
more precise native abstraction.
