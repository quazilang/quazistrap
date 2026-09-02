# Standard-library API reference

Audience: language users.

Status: structure established; exhaustive public-API documentation is in
progress. See [the current standard-library guide](../STANDARD_LIBRARY.md) for
the presently documented subset.

See the [coverage ledger](coverage.md) for every exported `std` module and the
remaining API-reference work.

Documented modules:

- [`std.core`](core.md) — low-level intrinsics, raw memory, and UTF-8 boundaries.
- [`std.io`](io.md) — UTF-8 console input and output helpers.
- [`std.collections`](collections.md) — experimental integer-keyed maps and sets.
- [`std.ffi`](ffi.md) — C ABI aliases and explicit C-string ownership.
- [`std.math`](math.md) — dependency-free integer and `f64` helpers.
- [`std.random`](random.md) — operating-system-backed random sampling.
- [`std.dylib`](dylib.md) — owned native-library handles and unsafe symbols.
- [Platform implementation boundaries](platform-internals.md) — unsafe Unix and Win32 internals.
- [`std.thread`](thread.md) — experimental native threads and callback ABI.
- [`std.time`](time.md) — monotonic durations and elapsed-time instants.
- [`std.json`](json.md) — bounded JSON validation and token encoding.
- [`std.codec`](codec.md) — typed JSON serialization foundation.
- [`std.fs`](fs.md) — owned file handles, text reads, and directory operations.
- [`std.net`](net.md) — IPv4 TCP/UDP and bounded HTTP/1.1 helpers.
- [`std.os`](os.md) — host information and current-process boundaries.
