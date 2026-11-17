# `std.posix`

Audience: language users.

`std.posix` holds Unix-specific process facilities that are not part of the
portable `std.os` surface (moved there on 2026-11-17). They wrap Linux
syscalls or NetBSD libc and return raw native results; on Windows the signed
functions return `-1` and the unsigned ones `u32::MAX`.

| API | Behavior |
| --- | --- |
| `getuid()`, `getgid()`, `geteuid()`, `getegid()` | Numeric real and effective user and group IDs. |
| `kill(pid, sig)` | Sends a signal and returns the raw status (`0` on success). |
| `umask(mask)` | Sets the process-wide file-creation mask and returns the previous one. |
| `unsafe cwd(buf, size)` | Writes the working directory into caller storage; prefer `os.current_dir()`. |
| `unsafe linux_environment()` | The current Linux `envp` array (see [runtime state](../internals/runtime-state.md)). |

See [`std.os`](os.md#unix-specific-functions-stdposix) for migration notes.
