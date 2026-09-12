# Win64 process-runtime lowering

Audience: compiler and runtime maintainers.

This note specifies the remaining Win64 implementation of D-011. It is an
implementation design, not a claim that Windows process support is shipped.

## Boundary

`quazi.process.spawn(program, args_ptr, args_len, handle_out, error_out)`
returns `0` or `-1`. It must clear `*handle_out` and `*error_out` before work,
write the first Windows error code on failure, and return only an owning process
handle on success. It must not invoke a shell or use `PATH` lookup.

## Command construction

`CreateProcessW` receives `program` as non-null `lpApplicationName` and a
separate, mutable UTF-16 command buffer. Convert every source string with
`MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, ..., -1, ...)`; conversion
failure is a native spawn error. The command sequence is `program` followed by
the supplied argument elements.

For each UTF-16 element, quote an empty string or one containing space, tab, or
quote. In a quoted element, a run of `n` backslashes before a quote emits
`2n+1` backslashes before the quote; a run of `n` trailing backslashes emits
`2n` before the closing quote. This is the Microsoft command-line parsing rule
and cannot be replaced by joining strings with spaces.

## Standard handles

Read the three standard handles, duplicate each valid one as inheritable with
`DuplicateHandle(..., TRUE, DUPLICATE_SAME_ACCESS)`, and put only those
duplicates in a `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`. Build a zeroed
`STARTUPINFOEXW`: `cb = 104`, `STARTF_USESTDHANDLES`, duplicate standard handles
at offsets 80/88/96, and the attribute-list pointer at 104. Pass
`EXTENDED_STARTUPINFO_PRESENT` and `bInheritHandles = TRUE` to `CreateProcessW`.
This avoids mutating parent handle flags and avoids leaking unrelated
inheritable handles to children.

## Ownership and cleanup

The lowering allocates UTF-16 program, command, and scratch buffers plus the
attribute list from the process heap. It closes duplicate handles after
creation, destroys an initialized attribute list before freeing it, closes the
returned thread handle, and frees all temporary allocations on every path. On
success it retains only `PROCESS_INFORMATION.hProcess`. Capture `GetLastError`
before cleanup, as cleanup calls may overwrite it.

The remaining operations use Win32 handles directly: wait uses
`WaitForSingleObject(INFINITE)`, `GetExitCodeProcess`, then `CloseHandle`;
try-wait uses timeout zero; terminate calls `TerminateProcess` without closing;
close probes with timeout zero, terminates only a live process, waits, and
closes. Windows reports `ExitStatus.Code` only.

## Verification

Required target-native tests cover paths and arguments with spaces, quotes, and
non-ASCII UTF-8; nonzero exits; a missing executable; live and exited
`try_wait`; forced terminate; repeated consuming calls; and no inherited
non-standard handles. Cross-compilation alone is insufficient.
