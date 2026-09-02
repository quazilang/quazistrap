# std.os

Audience: Quazi application developers.

std.os is a small current-process and host-information module. It is not the
planned child-process API; see the [process runtime decision](../decisions/process-runtime.md)
for why process creation is intentionally deferred to compiler/runtime work.

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
GetCurrentProcessId. getppid() remains Linux-specific; Windows callers must not
rely on its current Unix-syscall-based result. This limitation is tracked as
standard-library portability debt rather than a portable parent-PID contract.

## Environment and host information

env(name, fallback) returns an owned environment value or an owned copy of
fallback when the variable is absent, cannot be read, is too large for the
current Windows buffer, or allocation fails. It distinguishes none of these
conditions, and it must not be used for security decisions.

unsafe getenv(name) exposes a borrowed native null-terminated pointer. It is
unsafe because native lifetime and encoding rules apply; ordinary code should
use env instead.

| API | Current behavior |
| --- | --- |
| name() | "Linux" or "Windows" on supported targets, otherwise "Unknown". |
| version() | Owned display-oriented release text. Linux currently returns "Linux" without a kernel release; Windows derives edition/build text from compiler-provided Windows metadata. It is not a stable machine-readable version API. |
| cpu_name() | Best-effort owned CPU-brand text, or "unknown" on failure. |
| shell() / terminal() | Best-effort interactive-environment names. Linux reads SHELL/TERM; Windows uses environment hints and a bounded parent-process scan. They may be "unknown", "Console", or "Windows Console" and must not drive security or compatibility policy. |
| hostname() | Owned host name from the platform intrinsic, or "unknown" on failure. |
| memory_total() / memory_available() | Physical-memory counters in bytes. Linux currently uses sysinfo.free_ram for “available”, which is not Linux’s broader reclaimable-memory estimate. Values can change immediately after return. |

## Unix-specific low-level functions

The following APIs directly use Linux syscalls even though they are currently
exported through std.os. They are target-specific and return raw native results
rather than structured errors. On Windows they do not issue Linux syscalls:
signed-result APIs return `-1`, while the unsigned identity and umask APIs
return `u32::MAX` as an unsupported-target sentinel.

- unsafe cwd(buf, size) writes a null-terminated current-directory path to
  caller-provided storage and returns a byte count including the terminator or a
  negative failure value. It is Linux-only; the caller must allocate writable
  storage of at least size bytes.
- getuid(), getgid(), geteuid(), and getegid() return Linux numeric identities.
  They do not model Windows security tokens.
- kill(pid, sig) sends a Linux signal and returns the raw syscall status;
  conventional signal numbers include SIGINT=2, SIGKILL=9, and SIGTERM=15.
  It is not a portable process-control API.
- umask(mask) changes the process-wide Linux file-creation mask and returns
  the previous mask. It affects other threads and all subsequent file creation,
  so libraries should not modify it implicitly.

The std.fs page documents the portable filesystem surface. Code that requires
portable current directories, identities, environment mutation, arguments,
working-directory changes, or child-process management must wait for an
explicitly designed API rather than depending on these syscall wrappers.
