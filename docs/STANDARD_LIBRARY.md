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
- `std.collections.array`: growable `Array[T]`; map/set currently use `usize`.
- `std.fs`: owned files, whole-file reads, metadata, directories, path helpers.
- `std.os`: environment, hostname, OS/version, CPU, memory, shell, terminal.
- `std.net`: TCP, HTTP/1.1 client requests, and local one-request servers.
- `std.thread`: spawn/join primitives.
- `std.time`: monotonic durations and elapsed-time instants; wall-clock and
  civil-time APIs are intentionally not yet provided.
- `std.ffi`: C scalar aliases, `CStr`, `CString`, callbacks, null pointers.
- `std.unix` / `std.windows`: platform-specific low-level operations; portable
  application code should prefer `std.fs`, `std.os`, `std.io`, and `std.net`.

Fallible APIs return `Result`; absence uses `Option`. Resource-owning values
clean up at scope exit. Details for text/primitives are in
[PRIMITIVE_APIS.md](PRIMITIVE_APIS.md); networking is in [NETWORK.md](NETWORK.md);
randomness is in [RANDOM.md](RANDOM.md).

Public failures use domain enums, never unexplained operating-system integers.
`std.fs` returns `FsError`; `std.net` returns `NetError`; console input returns
`ReadError`; parsing returns `ParseError`. `message()` provides user-facing text,
while `Native(code)` preserves an unclassified platform code for diagnostics.
