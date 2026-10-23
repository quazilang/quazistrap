# Checked runtime String identity

Audience: language users and compiler developers.

Owned-string operations now require the actual declaration exported as String
by the selected prelude and a validated layout. A counterfeit String could
previously pass numeric/string category checks and have an integer or raw-pointer
field interpreted as a text view. The same assumptions affected arbitrary
reference as_str fallback, implicit formatting and injected formatter results.
See [the contract](../internals/runtime-string-abi.md) and
[migration](../migrations/runtime-string-abi.md).

Loader provenance retains full canonical export targets, selector spans and
original names through aliases, re-exports, namespaces, overlays and cfg.
Qualified type resolution has no global leaf fallback. Explicit selectors cannot
borrow another selector's proof. Conflicting aliases fail. String extraction
uses the checked data offset; renamed constructors use the defining type's layout.
Known and concrete generic format arguments reject unsupported UTF-8 coercions.
Nested owner references fail rather than collapsing two required loads into one.

QZC29 invalidates older exact/reusable source chunks. Cache identity includes
selected builtin runtime roots; root/layout switches rebuild and new hits use
the selected runtime. QZI14 remains unchanged and Unverified. Rebuild old QZI
bodies from source for these checks; no ownership/effect certificate is added.
Private namespace-alias isolation remains a confirmed separate gap, with
[evidence and next steps](../audits/2026-10-23-string-followups.md).

Verification uses `/tmp/quazi-named-rust-bin`, explicit RUSTC and
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`:

- `cargo test --offline -q -- --test-threads=1`: **969 passed**.
- `cargo build --offline -q` and direct Clippy-driver `cargo check --offline -q`
  pass with the existing 23 compiler warnings.
- `python3 scripts/check_string_abi.py --before-qz /tmp/quazi-string-before-07e3049`:
  **25 negative programs** rejected by check/native/QZI/Windows COFF, including
  counterfeit layouts, references, direct/nested wrong-module imports, selector
  reuse, alias conflicts and unsupported generic formatting. A valid re-export
  control passes. The old counterfeit cached program is compiled only, never run;
  current source rebuilding rejects it.
- Actual String comparison, generic aliases, shared references, format and parse
  pass native/source-removed QZI/COFF. Two custom runtimes verify nonzero data
  offsets and Text exported as String, including constructor and associated
  method dispatch. Generic namespace binders cannot impersonate the runtime. Runtime-root switches alternate methods
  and field order, require misses, and verify new exact hits execute the correct
  selected runtime. Loader tests cover full paths, nested public namespaces,
  private exports, overlays, cfg, cycles, ambiguity and cancellation. Analyzer
  reuse clears the proof. Backend consumer tests reject absent proof and use
  offset 8 while preserving formatter/argument cleanup.

Earlier arithmetic/operator/mutation/scalar/operand/initialization, loop and
conditional cleanup, propagation, expression control, retention, named argument,
dynamic/cache, ELF stack and power runners pass. The installed optimized compiler
passes the String runner/cache controls; six isolated STD memory tests and ordinary
quazifetch build/run/new exact hit/Windows COFF pass. Windows runtime execution
is not verified on this Linux host. Graphify query/AST update passes: 5,653 nodes
and 12,848 edges, with no .qz semantic extraction. Logs: `/tmp/quazi-string-*.log`.

Independent reviews found and helped close forged selector paths, alias conflicts,
same-name selector reuse and nested owner-reference load mismatches. The private
cross-file alias finding remains open. README statuses and progress are updated
before committing, unchanged **73.2% complete / 26.8% remaining**, Ownership9/16,
LSP16/20, 33 items and 78 requirements. No new roadmap requirement receives credit.
