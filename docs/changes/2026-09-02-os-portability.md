# Portable current-process yield and process ID

Audience: Quazi users and standard-library maintainers.

std.os.yield_cpu now maps to the Linux scheduler-yield syscall on Linux and
Sleep(0) on Windows. std.os.getpid now uses Linux getpid or Windows
GetCurrentProcessId rather than issuing a Linux syscall on both targets.
`std.thread.yield_now` now follows the same target-specific scheduler mapping.
`std.thread.current_tid` now returns `-1` on Windows instead of lowering the
Linux-only `gettid` syscall on that target.
The remaining Linux-only `std.os` syscall wrappers (`getppid`, `cwd`, identity,
signal, and umask helpers) now likewise return documented unsupported-target
sentinels on Windows instead of issuing Linux syscall numbers.

This is a compatibility correction: callers on Windows now receive a real
current process ID and a scheduler-yield request instead of target-inappropriate
syscall lowering. The public signatures do not change. getppid, working
directory, Unix identity, signal, and umask functions remain target-specific
and are explicitly documented as such.

On Linux, std.os.sleep is also now self-contained when the compiler uses its
built-in linker: its millisecond delay lowers to the nanosleep system call
instead of an unresolved libc usleep reference. The existing Windows Sleep
lowering now preserves the public u64 duration: waits longer than one finite
DWORD Sleep interval are split into finite chunks, avoiding both truncation and
the `INFINITE` sentinel.

Linux TCP sends now use the socket send operation with its no-SIGPIPE flag,
turning a closed peer into the documented `NetError.BrokenPipe` result instead
of allowing a process-wide signal termination.

Verification includes a Linux native smoke that requires a positive process ID
and a Windows x86-64 COFF compile smoke. The compiler and standard-library
diffs are checked independently.
