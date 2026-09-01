# UTF-8 validation at file and socket text boundaries

Audience: Quazi users and standard-library maintainers.

## Change

`str` and `String` remain valid UTF-8 by contract. `std.fs.read_to_string` now
validates bytes read from a file before taking ownership as a `String`; malformed
input returns `FsError.InvalidData`.

The text-returning TCP and UDP receive methods now make the same check. Invalid
input returns the new `NetError.InvalidUtf8` variant after releasing the raw
receive buffer. TCP fills an incomplete final scalar when it still fits in the
requested limit, and `receive_all` validates only after collecting its raw
stream bytes, so a valid scalar split across system reads is not rejected. Raw
socket receive methods remain unsafe pointer APIs for binary protocols; callers
that need arbitrary bytes must keep them in byte buffers rather than converting
them to text.

The validator rejects truncated sequences, overlong encodings, UTF-16 surrogate
encodings, values above U+10FFFF, and invalid continuation bytes. It accepts
valid one- through four-byte UTF-8 sequences.

## Compatibility and migration

This is a safety correction. Programs that previously treated invalid file or
network bytes as `String` now receive a structured error. Handle
`FsError.InvalidData` or `NetError.InvalidUtf8`, or use a binary/raw receive
path when the protocol is not text.

## Verification

- Linux runtime smoke checks a valid four-byte scalar, rejects overlong and
  surrogate encodings, writes malformed bytes to a file, and confirms
  `fs.read_to_string` returns an error.
- The same program is compile-checked for the Windows target, exercising the
  shared standard-library source on both supported targets.
