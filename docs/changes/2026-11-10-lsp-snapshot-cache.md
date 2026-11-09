# LSP navigation snapshots are cached

Audience: editor users and compiler developers.

Definition, references and rename analyze the document's whole import graph
(including the standard library) through the loader. Every request rebuilt
that snapshot: about 150 ms in a release build for a document importing
`std.io` and `std.math`, about 1 s in a debug build.

`src/lsp/snapshot_cache.rs` keeps up to 8 snapshots, one per document. A
snapshot records the text of every file it loaded and of the project and
dependency `quazi.toml` manifests; it is reused only while each of them,
read from the open buffers or from disk, is unchanged. Opening a document
clears the cache because a new buffer can change which file an unchanged
import resolves to. Existing stale-result checks (the requesting buffer and
the captured overlays must still match when the result is returned) are
unchanged.

`scripts/check_lsp_performance.py` drives `qz lsp` over stdio and enforces
the targets documented in `docs/tooling/lsp.md` (release build: first request
<= 1000 ms, warm median <= 20 ms) and checks that an unsaved edit produces
fresh references. Measured: 150 ms cold, 0.6 ms warm.

Follow-up the same day (review): a file created on disk could change which
file an unchanged import resolves to (`src/util.qz` shadows `src/util/mod.qz`;
a directory import loads every file in it) without changing any recorded
input. Snapshots now also record the file names of each loaded file's
directory and its ancestors up to the containing project, dependency or std
root, plus `quazi.lock` files; the warm request stays at 0.7 ms.
