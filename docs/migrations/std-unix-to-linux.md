# `std.unix` is now `std.linux`

Date: 2026-11-19

Audience: Quazi users of the low-level standard-library bindings.

`std.unix` mapped names directly to Linux x86-64 system calls, so it was
never a Unix-wide module (NetBSD has its own libc bindings in `std.netbsd`,
and `std.posix` is the layer shared by both). It is renamed to `std.linux`
at the maintainer's request; the functions and their signatures are
unchanged.

Change imports and qualified calls:

```text
import std.unix;        ->  import std.linux;
unix.write(1, p, n)     ->  linux.write(1, p, n)
import std.unix.open;   ->  import std.linux.open;
```

Portable programs should keep using `std.os`, `std.fs`, `std.process` and
`std.posix` instead of either platform module.
