# HTTP receiver ownership and dense-element read guards

Audience: Quazi users and compiler contributors.

## Behavior

`UdpDatagram.text`, request `method`/`target`/`host`/`body_text`, and response
`status`/`reason`/`body` now use shared receivers. Observing these values keeps
their owners available. A retained text view still freezes its owner; views of
temporary owners cannot be retained.

HTTP connection `read`, `read_request`, `respond`, and `close`, and server
`accept` and `close`, use exclusive receivers. Use mutable local bindings for
these operations. Connection cleanup still uses its structural `TcpStream`
field destructor; server cleanup retains its consuming source hook. Repeated
early close retains the underlying invalid-handle sentinel. `serve_once`
remains consuming and keeps its connection in a mutable local. Request `send`
captures the response limit before consuming the request through `encode`.

The compiler's metadata-derived owning-element prohibition now covers `[]`,
including observation such as `items[0].value`, projected roots, aliases, and
borrowed receivers. Index expressions retain their resolved method identity.
The rejection proof independently instantiates the declared receiver/return
signature; it does not enable new borrowed-reference indexing ABI paths.
Indexed replacement evaluates the storage and index without materializing the
old element as a second owner. Exclusive element removal remains available.

Generic helpers no longer bypass the prohibition. The borrow pass retains
matching generic element reads under the caller's binder context, then checks
proven concrete specializations after emission dependency closure. Concrete
identity provenance propagates to newly instantiated dependencies, including
previously registered symbolic spellings. Checked alias substitution, reset on
analyzer reuse, and cooperative cancellation apply to the deferred check.
Plain specializations and uninstantiated templates remain accepted. Indexed
write-target typing does not register a fictitious Index call, and it leaves
nested storage/index reads checked. Named Index registration now applies the
shared checked specialization limits and retains concrete provenance even when
the Index result itself is plain.

Native verification also exposed two lowering defects. Inferred calls to
retained generic storage wrappers now transfer an argument only when its
fixed declared parameter consumes and its actual caller type owns. Callee
binders stay protected from same-named aliases and caller substitutions;
scalar/reference arguments and variadic pointer/count packing cannot clear
owner guards. Fixed-array ownership follows its element. Discarded assignment
results now retain destination storage, preserving an owner just deposited by
indexed replacement.

This strengthens a source diagnostic. It does not introduce an element-loan
API, certify QZI ownership effects, or change QZI encoding. QZC advances to
cache epoch v10 so v9 exact hits and reusable chunks cannot bypass the new
source validation or ownership lowering. Previously
emitted unsafe QZI artifacts are not repaired by the source check; rebuild source.

## Remaining integration work

The full `std.net` module remains unbuildable: `Headers` still reads owned
`Header` values and returns escaping views, and HTTP codecs still perform
unsupported field moves. Checked D-015 element addressing, D-014 provenance and
artifact summaries, and structural destruction remain required. The first
accepted D-015 slice excludes field-projection roots; simply annotating
`self.items` access as borrowed cannot establish the required lifetime.

The strengthened guard also exposes an owning indexed read in `std.json`
(`array` reads `Array[String]` elements). Full standard-library tests currently
fail analysis with 30 diagnostics in `net.qz`, one in `json.qz`, one concrete
prelude `Array.index` diagnostic, and two unrelated unfinished CLI
errors. Preserve the CLI work; do not weaken ownership to make this matrix green.

## Verification

From the compiler root, with the pinned Rust toolchain and bfd linker:

```sh
rust_bin="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin"
RUSTC="$rust_bin/rustc" RUSTFLAGS='-C link-arg=-fuse-ld=bfd' "$rust_bin/cargo" test --offline contiguous -q
TMPDIR="$(mktemp -d /tmp/quazi-owned-reads-XXXXXX)" RUSTC="$rust_bin/rustc" RUSTFLAGS='-C link-arg=-fuse-ld=bfd' "$rust_bin/cargo" test --offline -q
RUSTC="$rust_bin/rustc" RUSTFLAGS='-C link-arg=-fuse-ld=bfd' "$rust_bin/cargo" build --offline -q
RUSTC="$rust_bin/rustc" RUSTFLAGS='-C link-arg=-fuse-ld=bfd' "$rust_bin/cargo" clippy --offline -q
python3 ../std/scripts/check_net_receivers.py "$PWD/target/debug/qz"
target/debug/qz build prelude/tests/owned-element-replacement.qz --no-incremental -o /tmp/qz-owned-element-replacement -r
target/debug/qz build prelude/tests/owned-element-replacement.qz --no-incremental -i -o /tmp/qz-owned-element-replacement.qzi
target/debug/qz build /tmp/qz-owned-element-replacement.qzi --no-incremental -o /tmp/qz-owned-element-replacement-roundtrip -r
target/debug/qz build prelude/tests/owned-element-replacement.qz --no-incremental -c --target x86_64-windows -o /tmp/qz-owned-element-replacement.obj
git diff --check
```

The default Rust linker wrapper references a missing Nix store file; a reviewer
attempt with plain `cargo test --offline prior_cache_versions_cannot_bypass_dense_element_read_validation -q`
failed before test execution. The pinned/bfd checks above are the verified path.

The indexed-read regression fails before the fix. A native-source generic
`extract[Item](Array[Item])` probe emitted QZI before the deferred check and now
fails with S10 before bytecode generation. Focused regressions cover aliases,
shared/exclusive indexing, non-consuming observation, owning indexed replacement,
exclusive removal, generic direct/transitive reads, renamed impl binders,
same-named global aliases, uninstantiated templates, and analyzer reuse.

The generic storage-call regression fails with the previous transfer lowering
and passes after the fix. Scalar, borrowed, variadic, and fixed-array argument
regressions preserve a pending temporary receiver's cleanup. The actual prelude
`owned-element-replacement` fixture previously crashed from premature frees;
it now passes Linux native execution and QZI round-trip execution, plus Windows
COFF emission. Its storage push, indexed replacement, and exclusive removal
transfer owners without materializing an Index read.

The isolated standard-library verifier passes ten Linux native tests (seven
new receiver tests and three existing transport tests), three specific negative
lifetime checks, a QZI native round trip running the seven new tests, and Windows
COFF emission. Its Linux socketpair test reads twice through one connection and
observes peer EOF after implicit connection destruction. Windows is emitted,
not executed.

The verifier copies exact production declarations, owner fields, selected
methods, and transport implementation into a temporary project. It excludes
unfinished header/HTTP codecs and unrelated CLI sources. It does not establish
successful server accept, HTTP parsing/responding, `serve_once`, the request
send response-limit behavior, or complete aggregate leak freedom. The full
`qz test --no-color --no-unicode` in the standard-library root remains red at
analysis. See the [migration guide](../migrations/http-receivers.md).
