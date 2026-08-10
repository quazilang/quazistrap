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
| Hostname | `uname(2)` intrinsic | `GetComputerNameA` intrinsic |
| Memory | `sysinfo(2)` intrinsic | `GlobalMemoryStatusEx` intrinsic |
| CPU | `/proc/cpuinfo` via `std.fs` | `PROCESSOR_IDENTIFIER` |
| Packages | Known package databases | Package-manager environment/path probes |
| Files | Linux syscalls via `std.fs.File` | Win32 handles via `std.fs.File` |

Package output reports detected package-manager families instead of executing
package managers. This is deterministic, safe, lightweight, and shell-free.

## Ownership

`String` and `std.fs.File` are owning values whose `free` destructors are found
by Whole Program Analysis. The compiler inserts reverse-order cleanup at normal
scope exits and early returns, transfers ownership on return/by-value calls, and
releases the old value on reassignment. Method receivers are borrowed, so this
example contains no manual `.free()` calls.

Explicit lifetime syntax is not implemented yet. The current model is lexical
automatic destruction plus move checking; raw pointers remain an unsafe escape
hatch confined here to ownership-transfer helpers and standard-library internals.
