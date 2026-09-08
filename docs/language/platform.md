# Platform and target behavior

Audience: language users and implementers.

## Supported targets

Quazi currently supports two production target triples:

| Target | Architecture | OS | ABI |
| --- | --- | --- | --- |
| `x86_64-linux` | x86-64 | Linux | SysV |
| `x86_64-windows` | x86-64 | Windows | Win64 |

Select a target with `--target`:

```bash
qz build src/main.qz --target x86_64-linux
qz build src/main.qz --target x86_64-windows
```

The default target matches the host system.

## Conditional compilation

`@cfg(key="value")` gates declarations and imports on the build target:

```quazi
@cfg(target_os="windows") fn separator() str { ret "\\"; }
@cfg(target_os="linux") fn separator() str { ret "/"; }
```

Block form gates multiple declarations:

```quazi
@cfg(target_os="windows") {
    import std.windows;
    const platform_name: str = "Windows";
}
```

Conditional imports are excluded from dependency discovery when the condition
does not match, preventing cross-platform dependencies from loading on
incompatible targets.

### Available keys

| Key | Supported values |
| --- | --- |
| `target_os` | `"linux"`, `"windows"` |
| `target_arch` | `"x86_64"` |
| `target_abi` | `"sysv"`, `"win64"` |

## Platform-specific behavior

### Integer sizes

`isize` and `usize` are 64 bits on all currently supported targets (x86-64).

### Integer division

Integer division by zero panics with `"integer division by zero"`. Integer
remainder by zero panics with `"integer remainder by zero"`. With
`std = false`, the panic runtime is omitted and behavior falls back to the
target's hardware divide trap.

### Floating-point division

IEEE-754 behavior on all targets. Division by zero produces signed infinity;
`0.0 / 0.0` produces NaN.

### Console I/O

On Linux, console I/O uses UTF-8 through file descriptors.

On Windows, `std.io` distinguishes a console from a redirected handle:

- Consoles receive UTF-16 through `WriteConsoleW`.
- Files and pipes receive UTF-8 through `WriteFile`.

This prevents box-drawing characters from displaying incorrectly in Windows
terminals.

### File system

`std.fs` operations use Linux syscalls on Linux and Win32 APIs on Windows.
Path separators, file metadata, and directory iteration adapt to each
platform.

### Networking

`std.net` uses raw syscalls on Linux and Winsock (`ws2_32`) on Windows.
TLS/SSL is not yet available; HTTPS requests return `TlsUnavailable`.

### Sleep and time

`std.os.sleep` uses the `nanosleep` syscall on Linux and `Sleep` on Windows.
Windows sleep durations above `0xFFFFFFFE` milliseconds are split into finite
chunks; `0xFFFFFFFF` is reserved for the infinite sentinel.

Monotonic time uses `clock_gettime(CLOCK_MONOTONIC)` on Linux and
`GetTickCount64` on Windows.

### Process and OS information

`std.os` functions select the appropriate platform implementation
automatically. Functions without a Windows equivalent return documented
sentinel values (typically `-1`).

## Linker behavior

Plain Linux and Windows binaries use in-process ELF/PE linkers with no
implicit native libraries. The built-in linker is selected by default or
explicitly with `--linker builtin`.

Archives, shared libraries, `-l` flags, an explicit linker path, or
`QUAZI_LINKER` select an external linker (`ld.lld`/`mold`/`ld` on Linux,
`lld-link`/`link` on Windows). libc/CRT libraries are never added
implicitly; `libc = true` in `quazi.toml` is an explicit opt-in.

## Implementation-defined behavior

- Struct layout (without `@repr(C)`) is compiler-defined and not stable.
- Integer overflow uses the target's native wrap behavior (not checked).
- `f64` math functions in `std.math` are lightweight approximations, not
  correctly rounded scientific implementations.
- ASCII-only case conversion until Unicode tables are shipped.

## Experimental and incomplete features

The following features exist but have known limitations:

- **Concurrency**: `std.thread` provides spawn/join but structured lifetime,
  result propagation, cancellation, and synchronization are not designed.
- **Serialization**: Limited `@derive(Serialize)` exists; `Deserialize`,
  generic structs, nested types, and collections are not yet supported.
- **Process management**: Deferred by D-011 pending safe runtime marshalling.
- **Civil time**: Only monotonic `Duration`/`Instant` exist; calendar, UTC,
  and time zones are not implemented.
- **Generic storage**: Values wider than 255 register slots require an
  explicit indirect-value design that is not yet implemented.
