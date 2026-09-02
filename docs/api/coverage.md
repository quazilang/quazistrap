# Standard-library API coverage

Audience: Quazi users and documentation maintainers.

This page is an honest coverage ledger for the canonical `std` gateway. It
prevents the API index from implying that undocumented modules are stable.

| Module | API page | Current documentation status |
| --- | --- | --- |
| `std.codec` | [codec](codec.md) | Initial typed JSON encoding/decoding contract. |
| `std.fs` | [fs](fs.md) | Current owned-file and text-read surface. |
| `std.json` | [json](json.md) | Bounded JSON validation and composition. |
| `std.net` | [net](net.md) | IPv4 sockets and limited HTTP helpers. |
| `std.os` | [os](os.md) | Current-process and host-information boundaries. |
| `std.thread` | [thread](thread.md) | Experimental native-thread ABI. |
| `std.time` | [time](time.md) | Monotonic duration and instant foundation. |
| `std.core` | [core](core.md) | Low-level intrinsic, allocation, and UTF-8 safety boundary; allocation-backed `str` results remain unstable under D-013. |
| `std.io` | [io](io.md) | Current UTF-8 console input/output and writer contracts. |
| `std.collections` | [collections](collections.md) | Experimental `usize` Map/Set with in-place updates and audited single-owner cleanup. |
| `std.ffi` | [ffi](ffi.md) | C ABI aliases, raw-pointer boundary, and explicit C-string ownership. |
| `std.math` | [math](math.md) | Dependency-free integer helpers and finite-approximation `f64` math. |
| `std.random` | [random](random.md) | CSPRNG values, unbiased ranges, and current generic-collection limits. |
| `std.dylib` | [dylib](dylib.md) | Owned native-library handles and unsafe raw-symbol boundary. |
| `std.unix` | [platform internals](platform-internals.md) | Linux-only raw syscall implementation boundary; not a portable API. |
| `std.windows` / `std.win32_core` | [platform internals](platform-internals.md) | Raw Win32 implementation boundaries; not portable application APIs. |

All modules exported by the canonical gateway now have a current reference or
a deliberate implementation-boundary note. These pages describe the present
surface; experimental ownership, generic-storage, and formatting limits remain
separately recorded in the linked module pages and decisions.
