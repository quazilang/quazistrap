# 21-quazifetch

A Windows/Linux system-information example built on Quazi's cross-platform
standard library. It avoids shell commands, `popen`, libc-specific bindings,
and application-level `free()` calls.

## Run and check

```sh
qz run
qz check
```

Run `qz check` (or `qz build -c`) on both Windows and Linux when validating a
cross-platform change. The current command targets the host and has no
cross-target flag.

| Value | Linux | Windows |
|---|---|---|
| Hostname | `uname(2)` intrinsic, normalized lowercase | `GetComputerNameA` intrinsic, normalized lowercase |
| Memory | `sysinfo(2)` intrinsic | `GlobalMemoryStatusEx` intrinsic |
| CPU | `/proc/cpuinfo` via `std.fs` | CPUID processor brand leaves |
| OS release | Kernel release | shared kernel build number plus `GetProductInfo` edition |
| Shell | `$SHELL` | Toolhelp parent-process ancestry |
| Terminal | `$TERM_PROGRAM` / `$TERM` | `WT_SESSION` plus parent-process ancestry |
| Packages | Package databases and package directories | Package-manager-owned package directories |
| Files | Linux syscalls via `std.fs.File` | Win32 handles via `std.fs.File` |

Package output includes the discovered count, for example `apt (425)` or
`choco (3)`. Debian/APK counts come from their installed-package databases;
Pacman, XBPS, Nix, Flatpak, Chocolatey, Scoop, and WinGet portable-package
counts use manager-owned directories through `std.fs.count_entries`. When no
Windows package-manager metadata exists, the example counts the per-user
AppX/MSIX store and reports `appx (N)`. Restricted/minimal Windows sessions
fall back to the system package cache, then the immediate Program Files entries,
reported explicitly as `package-cache (N)` or `programs (N)`. No package manager
or shell is executed, and the label always identifies what was actually counted.

Windows Terminal is detected from `WT_SESSION` when inherited, then from the
process ancestry. A program launched through an ordinary console correctly
reports `Windows Console` instead of claiming Windows Terminal.

## Ownership

`String` and `std.fs.File` are owning values whose `free` destructors are found
by Whole Program Analysis. The compiler inserts reverse-order cleanup at normal
scope exits and early returns, transfers ownership on return/by-value calls, and
releases the old value on reassignment. Method receivers are borrowed, so this
example contains no manual `.free()` calls.

Explicit lifetime syntax is not implemented yet. The current model is lexical
automatic destruction plus move checking; raw pointers remain an unsafe escape
hatch confined here to ownership-transfer helpers and standard-library internals.
