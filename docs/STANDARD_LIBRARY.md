# Standard Library Guide

The prelude supplies `String`, `Array`, `Box`, `Option`, `Result`, common
traits, formatting, parsing errors, compile-time layout queries, and panic
support. Other modules are explicit imports.

- `std.io`: UTF-8-safe console output; owned fallible `read`, `readln`, `readkey`.
- `std.fmt`: `{}` formatting and format specifications used by I/O functions.
- `std.string`: owned/borrowed UTF-8 operations, slicing, search, case helpers,
  trimming, and generic checked parsing.
- `std.math`: dependency-free integer combinatorics and floating approximations.
- `std.random`: OS-CSPRNG-backed secure values, unbiased ranges, choice,
  shuffling, probabilities, and random bytes.
- `std.collections`: growable `Array[T]`, generic `HashMap[K, V]` and
  `HashSet[K]` with owned keys and values, and the `usize`-keyed `Map`/`Set`.
- `std.json` / `std.codec`: JSON validation and typed, derive-backed encoding
  and decoding with input and depth limits.
- `std.ini`: INI documents with typed lookups.
- `std.tls`: verified TLS client streams and HTTPS (Linux).
- `std.dylib`: runtime-loaded shared libraries with unsafe symbol casts.
- `std.fs`: owned files, whole-file reads, metadata, directories, path helpers.
- `std.path`: the structured `Path` type (join, parent, normalization).
- `std.stream`: `Reader`/`Writer` byte streams and buffered adapters.
- `std.process`: shell-free child processes (`Command`, `Child`, output capture).
- `std.os`: environment, hostname, OS/version, CPU, memory, shell, terminal.
- `std.net`: TCP, HTTP/1.1 client requests, and local one-request servers.
- `std.thread`: spawn/join primitives.
- `std.time`: durations, monotonic instants, wall-clock `SystemTime`, civil
  `DateTime`/`UtcOffset` and RFC 3339 text.
- `std.timezone`: IANA time-zone rules for civil times in named zones.
- `std.ffi`: C scalar aliases, `CStr`, `CString`, callbacks, null pointers.
- `std.linux` / `std.windows`: platform-specific low-level operations; portable
  application code should prefer `std.fs`, `std.os`, `std.io`, and `std.net`.

Fallible APIs return `Result`; absence uses `Option`. Resource-owning values
clean up at scope exit. Details for text/primitives are in
[PRIMITIVE_APIS.md](PRIMITIVE_APIS.md); networking is in [NETWORK.md](NETWORK.md);
randomness is in [RANDOM.md](RANDOM.md).

Public failures use domain enums, never unexplained operating-system integers.
`std.fs` returns `FsError`; `std.net` returns `NetError`; console input returns
`ReadError`; parsing returns `ParseError`. `message()` provides user-facing text,
while `Native(code)` preserves an unclassified platform code for diagnostics.
