# std.os

Audience: Quazi application developers.

std.os is a small current-process and host-information module. Shell-free
Linux, Windows and NetBSD child-process management is available through
[`std.process`](process.md). See
the [process runtime decision](../decisions/process-runtime.md) for the
cross-platform contract.

## Process control

exit(code) terminates the current process and does not return. sleep(ms)
blocks for at least approximately the requested millisecond duration subject to
operating-system scheduling; it is unsuitable for precise timing. Use
std.time.Instant for elapsed-time measurement. yield_cpu() requests a scheduler
yield through the Linux syscall layer or Windows Sleep(0). It must not be used
as a cross-platform synchronization primitive.

`sleep` accepts the full `u64` millisecond range on both supported targets. On
Windows, where one `Sleep` call accepts only a 32-bit duration and its all-ones
value means “infinite”, longer waits are issued as finite chunks of at most
4,294,967,294 milliseconds. This avoids truncation and never requests the
Windows infinite-wait sentinel.

getpid() returns the current process identifier through Linux getpid or Windows
GetCurrentProcessId. getppid() returns the parent on Linux and NetBSD and `-1` on Windows. This limitation is tracked as
standard-library portability debt rather than a portable parent-PID contract.

## Arguments

`args()` returns `Result[Array[String], FsError]`: the program name followed by
its arguments as owned UTF-8 strings, available anywhere (not only through
`main(args)`). Linux reads `/proc/self/cmdline`, NetBSD the startup's
`ps_strings`; Windows splits `GetCommandLineW` like the C runtime. An
argument that is not valid UTF-8 returns `InvalidData`.

## Environment and host information

env(name, fallback) returns an owned environment value or an owned copy of
fallback when the variable is absent, cannot be read, is too large for the
current Windows buffer, or allocation fails. It distinguishes none of these
conditions, and it must not be used for security decisions.

unsafe getenv(name) exposes a borrowed native null-terminated pointer. It is
unsafe because native lifetime and encoding rules apply; ordinary code should
use env instead.

set_env(name, value) and remove_env(name) change this process's environment
and that of children started afterwards; env_present(name) reports whether a
variable is set, even to an empty value. They return `Result[bool, EnvError]`:
`set_env` gives `Ok(true)`, `remove_env` gives `Ok(true)` when the variable was
set and `Ok(false)` when it was not. A name that is empty or contains `=` is
`InvalidName`; `Native(code)` carries a platform error; `EnvError.message()`
gives display text. On Linux each change
publishes a new copy of the environment under a writer lock, so concurrent
readers and writers in other threads are safe; replaced copies stay allocated
because a reader may still hold one
([runtime state](../internals/runtime-state.md)). Windows uses
`SetEnvironmentVariableA` (ANSI code page: non-ASCII text is not preserved),
and NetBSD libc `setenv`/`unsetenv`.

| API | Current behavior |
| --- | --- |
| name() | "Linux", "Windows" or "NetBSD" on supported targets, otherwise "Unknown". |
| version() | Owned display-oriented release text. Linux currently returns "Linux" without a kernel release; Windows derives edition/build text from compiler-provided Windows metadata. It is not a stable machine-readable version API. |
| cpu_name() | Best-effort owned CPU-brand text, or "unknown" on failure. |
| shell() / terminal() | Best-effort interactive-environment names. Linux reads SHELL/TERM; Windows uses environment hints and a bounded parent-process scan whose native snapshot handle is released once. They may be "unknown", "Console", or "Windows Console" and must not drive security or compatibility policy. |
| hostname() | Owned host name from the platform intrinsic, or "unknown" on failure. |
| memory_total() / memory_available() | Physical-memory counters in bytes. Linux availability uses `/proc/meminfo`'s `MemAvailable`, including reclaimable memory; unavailable, missing, malformed or inconsistent procfs data falls back to `sysinfo.free_ram`. Windows uses `MEMORYSTATUSEX.available_physical`. Values can change immediately after return; separate calls are not an atomic snapshot. |

## Working directory

`os.current_dir()` returns the current working directory as an owned `String`
(`Result[String, fs.FsError]`); paths longer than the initial 4 KiB buffer are
retried with a larger one up to 1 MiB. `os.set_current_dir(path)` changes it
for the whole process (every thread) and returns `Result[bool, fs.FsError]`.
Both are implemented in `std.fs` (`fs.current_dir`, `fs.set_current_dir`):
`getcwd`/`chdir` on Linux and NetBSD, `GetCurrentDirectoryA` /
`SetCurrentDirectoryA` (ANSI code page paths) on Windows.

## Unix-specific functions (`std.posix`)

These live in `std.posix` (since 2026-11-17; they used to be exported through
std.os). They wrap Linux syscalls or NetBSD libc and return raw native results
rather than structured errors. On Windows they issue no native call:
signed-result APIs return `-1`, while the unsigned identity and umask APIs
return `u32::MAX` as an unsupported-target sentinel. `unsafe
linux_environment()` returns the current Linux `envp` array (see
[runtime state](../internals/runtime-state.md)).

- unsafe cwd(buf, size) writes a null-terminated current-directory path to
  caller-provided storage and returns a byte count including the terminator or a
  negative failure value. It is Unix-only; the caller must allocate writable
  storage of at least size bytes.
- getuid(), getgid(), geteuid(), and getegid() return Linux numeric identities.
  They do not model Windows security tokens.
- kill(pid, sig) sends a Linux signal and returns the raw syscall status;
  conventional signal numbers include SIGINT=2, SIGKILL=9, and SIGTERM=15.
  It is not a portable process-control API.
- umask(mask) changes the process-wide Linux file-creation mask and returns
  the previous mask. It affects other threads and all subsequent file creation,
  so libraries should not modify it implicitly.

Portable code uses std.os: `current_dir`/`set_current_dir`, `args`,
`env`/`set_env`/`remove_env`, `getpid`/`getppid`. Migration: replace
`os.getuid()` (and `getgid`, `geteuid`, `getegid`, `kill`, `umask`, `cwd`)
with `posix.getuid()` after `import std.posix;`.
