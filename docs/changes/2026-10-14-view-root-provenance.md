# String-view roots across matches and capability transfers

Audience: Quazi users and compiler contributors.

## Behavior and motivation

A match result can no longer hide a borrowed string view of a consumed enum
payload. Returning that view, storing it in an aggregate, or binding a view of a
temporary matched owner produces S10. Pattern bindings have their own lexical
scope, and result provenance uses the same scoped environment as arm checking.
Qualified enum constructors without a result annotation still contribute a
temporary storage root. The I/O tutorial fixture now prints the owned file
contents inside the match arm, matching the chapter's existing example.

Local owner roots identify the declaration, rather than only its name and scope
depth. Branch merges match that identity and retain the longest enclosing loan
region. Reusing a name cannot revive a view of a different declaration.

Borrowed parameters retain their slot origin. A view derived from an exclusive
parameter freezes mutation through the parameter until the view's lexical scope
ends. Moving that exclusive capability to a local binding transfers its active
root: valid view observations remain accepted and mutation while borrowed stays
rejected. Nested shadowing and chains of capability transfers are covered.

## Compatibility and implementation boundary

Rebuild affected source. Previously accepted escaping match-result views are
now rejected. Keep the owner live for the whole use, or consume/print the owned
value within its match arm. QZC advances to v11, invalidating v10 and older exact
hits and reusable chunks. QZI stays v11; old QZI bodies are not repaired by
these source checks.

This is local hardening toward [D-014](../decisions/whole-program-ownership.md),
not its effect solver or an artifact certificate. Call-result view provenance
still conservatively unions borrowed inputs. Exact source result summaries,
aggregate provenance propagation, indirect/recursive effect composition, and
QZI body verification remain required before D-015 collection loans can rely
on them. Named aggregate parameters are not recursively certified.

Parameter-derived aggregate retention remains existing, unverified behavior.
A blanket parameter-storage ban was tested and rejected: it also rejects
synchronous `PanicInfo` construction in the prelude and overtaints copying
helpers. No panic-name/type exception was introduced. A complete implementation
must carry origins through all aggregate forms and prove whether callees retain
those origins. Existing local/temporary escape checks remain enforced; this
checkpoint does not establish safe interprocedural aggregate retention.

## Verification

Commands run from the compiler repository with the pinned Rust toolchain:

```sh
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
export RUSTFLAGS='-C link-arg=-fuse-ld=bfd'
cargo test --offline string_view_tests -q
TMPDIR="$(mktemp -d /tmp/quazi-provenance-suite-XXXXXX)" cargo test --offline -q
cargo build --offline -q
cargo clippy --offline -q
rustfmt --edition 2024 --check src/semantic/borrow.rs src/incremental.rs
git diff --check
```

Focused regressions cover enum-payload return/storage/temporary escapes,
shadowed owner identity, branch merges, exclusive-parameter view freezing,
capability transfer and nested shadowing, loan release, and existing allocation
constructors. Cache tests reject prior exact hits and reusable units. The full
suite also checks tutorial fixtures and runnable fences against real dependencies.
Native and cross-target outcomes are recorded in the current resume checkpoint.

The final full suite passes 738/738; the focused view suite passes 19/19. Build
and clippy succeed with existing warnings. New match and exclusive-parameter
regressions fail against the previous implementation.

Additional CLI probes (temporary source fixtures are recorded in `RESUME.md`):

```sh
target/debug/qz build /tmp/quazi-provenance-local.qz --no-incremental -o /tmp/quazi-provenance-local -r
target/debug/qz build /tmp/quazi-provenance-local.qz --no-incremental -i -o /tmp/quazi-provenance-local.qzi
target/debug/qz build /tmp/quazi-provenance-local.qzi --no-incremental -o /tmp/quazi-provenance-local-roundtrip -r
target/debug/qz build /tmp/quazi-provenance-local.qz --no-incremental -c --target x86_64-windows -o /tmp/quazi-provenance-local.obj
target/debug/qz build /tmp/quazi-provenance-match-negative.qz --no-incremental -i -o /tmp/quazi-provenance-match-negative.qzi
```

The local-owner positive probe passes native and QZI execution and Windows
COFF emission. The negative match probe correctly fails with S10. Windows host
execution was unavailable. A separate forwarded-reference mutation probe fails
at runtime: general slot references and struct receiver handles disagree.
That confirmed unresolved bug is documented in the
[reference receiver ABI audit](../audits/2026-10-14-reference-receiver-abi.md);
QZI/COFF emission of that probe succeeded but does not establish runtime safety.
