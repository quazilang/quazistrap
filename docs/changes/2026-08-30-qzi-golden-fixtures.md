# QZI Historical Golden Fixtures and the v2 Chunk-Header Fix

Date: 2026-08-30

## Motivation

The documented compatibility window claimed the current reader retains
compatible QZI v2-v6 bytecode, but no immutable evidence backed the claim:
every legacy-reading test serialized a fresh artifact and edited its header.
Real historical writers encode details those synthetic tests cannot capture,
so the compatibility promise was unverified — and, for v2, false.

## Behavior

- Immutable golden fixtures now live in
  `src/bytecode/fixtures/qzi/` with per-artifact provenance (writer commit,
  date, source program, generation command, sha256) in their README. Each
  fixture was produced by building the historical compiler from its recorded
  commit with no modifications and compiling a small era-valid program.
- `golden_*` tests in `src/bytecode/chunk.rs` decode every fixture and lock
  the expected result: era feature tours from v2 through v6 must decode,
  including sectioned v6 executables (with non-empty relocations) and a v6
  library container with metadata, interface, and a legitimately empty
  relocation section; every legacy `Lea` artifact must be
  rejected with the explicit source-rebuild error.
- Confirmed and fixed with real evidence: the reader assumed the v3+ chunk
  header (with its flags byte) for v2 artifacts and misaligned on every real
  v2 file. v2 serialization writes only `param_count` and `reg_count`; the
  reader now uses the v2 header layout and defaults the flags to zero.
- Authentic historical behavior is now documented rather than guessed: v3
  stores `@api` symbols as plain string constants (the backend's scalar
  legacy `CallExt` lowering covers them, matching v3's scalar-only FFI), and
  v3 never persisted `@export` symbols, so exported entry points from that
  era are unrecoverable by any reader.

## Compatibility

Reading a real v2 artifact now succeeds instead of failing with a misleading
`unknown const tag` error. Because v2 artifacts carry no chunk flags,
intrinsic wrapper chunks inside them lose compilation-local symbol scoping;
the QZI linker's equivalent-chunk deduplication absorbs this. No current
(v7) artifact is affected, and no Quazi source change is needed.

## Verification

`cargo test --offline` passes 439/439, including the eight new `golden_*`
fixture tests. Fixture generation was verified deterministic (repeated
era-writer builds are byte-identical) and every artifact's magic and version
byte was checked against its recorded writer commit.
