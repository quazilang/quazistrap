# Retention through raw storage and byte copies

Source result analysis now separates pointer identity from stored contents.
Copying a `str` handle through allocated memory retains its input owner;
copying ordinary UTF-8 bytes into fresh storage produces independent contents.
This removes the false owner loan from prelude range copies, including a
source helper returning `value[1:4]` followed by release of the original owner.

The inherited unfinished refinement lost dependencies after helper-returned
allocations. It also confused loaded pointer values with the cells containing
them and missed writes through uncertain aliases. Storage identities now
distinguish bindings, direct allocation sites and unmodeled expression sites.
Explicit and implicit projection loads recover stored targets; uncertain
memory reads/writes share weak facts through branches and loop convergence.
An empty source parameter summary never establishes returned storage freshness.
Intrinsic handling follows resolved declarations, including named argument
slots, and preserves tracked storage through string-pointer round trips.

See [the source retention contract](../internals/source-result-retention.md#raw-storage-and-copying).
This remains a conservative dependency model. Exact byte/projection effects,
external/global state, destruction and artifact-body verification are not
certified. Unsafe raw operations retain their existing validity obligations.
No reference escape, borrowed element API, pointer/integer cast, or projected
address-of syntax is enabled. D-014 and D-015 remain incomplete.

QZI stays v13 with unverified ownership envelopes. QZC v15 invalidates older
exact hits and reusable units so source checks rerun. Caches rebuild
automatically; ordinary source needs no migration.

## Evidence and verification

The valid-source pre-fix memory suite failed five of its original eight tests
with the inherited refinement. Those tests and adjacent cases now pass. The
earlier saved `(&view) as *u8` reproducer in the previous resume was not valid
source: S06 already rejects that cast. It was evidence of a summary discrepancy
on rejected input, not a runtime safety defect. New tests use type-correct
allocated view slots or the supported direct-address argument conversion.

The previous committed compiler rejects the new owned-range fixture with S10
(`/tmp/quazi-memory-retention-range-before.log`). The current compiler executes
it from source and QZI after freeing the original owner. Negative handle-copy
and alias fixtures reject owner destruction before any execution. Those two
negative CLI fixtures also reject with the previous committed compiler; their
pre-fix regression evidence is in the focused summary tests, not an old CLI
acceptance claim.

Using the pinned stable Rust toolchain in PATH and
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`:

- `cargo test --offline -q`: 831/831 pass with a fresh temporary TMPDIR.
- `cargo test --offline semantic::retention`: 39/39 pass, including 23 memory
  regressions. Existing cancellation and new intrinsic-registry reset tests pass.
- `cargo test --offline prior_cache_versions_cannot_bypass_ownership_validation`:
  exact and reusable v14 caches are rejected along with earlier versions.
- `cargo build --offline -q` and `cargo clippy --offline -q`: pass with existing
  warnings. The new private-interface warning was corrected.
- `python3 scripts/check_result_retention.py target/debug/qz`: 12 unsafe cases
  reject during check/build/QZI emission; seven valid cases run from source and
  QZI and emit x86-64 Windows COFF.
- `python3 scripts/check_dynamic_vtables.py target/debug/qz`: source/QZI native,
  Windows COFF, source-removed library and incremental cache paths pass.
- `python3 ../std/scripts/check_net_receivers.py /DISK/000vse/Projects/QUAZI-PROJECTS/quazilang/quazistrap/target/debug/qz`:
  10 isolated real STD tests, three negative lifetime cases, QZI execution and
  Windows COFF emission pass. Full STD remains unverified.
- Owned Rust formatting, changed Markdown relative links and `git diff --check`
  pass. No unrelated formatting or user STD CLI changes are included.

An initial whole-suite run found an alias-field mutation regression in the new
address evaluator; reference projections now address their pointee. Tests also
cover implicit field/index/iteration reads, temporary/aggregate helper results,
known/unknown aliasing, named copy slots, weak fills, string-pointer round trips,
byte-pointer local aliases, branch/loop allocations and static result precision.
Two independent read-only reviewers challenged these paths and verified native
copy independence plus owner-loan rejection. No Windows-host execution is claimed.

Logs are `/tmp/quazi-memory-retention-*.log`; independent fixtures are
`/tmp/quazi-retention-native-audit` and `/tmp/quazi-retention-alias-audit`.
Graphify query and AST update were used; the local graph has 4,914 nodes and
10,806 edges. Its integrity diagnostic reports 63 dangling endpoint edges and
43 undirected endpoint collisions. It is a navigation aid, excludes Quazi source
semantics and does not certify ownership.
