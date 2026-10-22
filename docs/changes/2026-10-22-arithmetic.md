# Arithmetic requirements, reference loads and floating remainder

Audience: language users and compiler developers.

Eager + - * / % and unary negation now enforce supported operand categories,
including every proven concrete generic specialization. This closes owner-handle,
boolean and enum arithmetic accepted by the old compatibility-only checks.
Declaration-slot facts are shared with numeric mutations. Numeric generics,
str-view concatenation and byte pointer offsets remain supported; owned String
concatenation is rejected because its previous result representation was wrong.
See [the contract](../internals/arithmetic.md) and [migration](../migrations/arithmetic.md).

Alias/reference operands now carry loaded value types. Shared string wrappers
remain thin views; exclusive string references load their VM slot exactly once,
including explicit dereference through a shared wrapper. Numeric and pointer
references load independently of a generic peer's still-symbolic type.

Generic division/remainder and compound forms retain the hidden panic function
for integer specializations, preserving exit101 instead of fallback trap. Four
native/source-removed QZI zero-divisor regressions verify that dependency.

Floating remainder now emits guarded FPREM reduction instead of integer IDIV.
Both native emission and constant propagation preserve signed zero and handle
nonfinite/zero-divisor results. QZC26 rebuilds older source chunks; QZI13 remains
unchanged and Unverified. No ownership/effect certificate follows.

Verification uses `/tmp/quazi-named-rust-bin`, explicit RUSTC and
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`:

- `cargo test --offline -q -- --test-threads=1`: **934 passed**. The first
  parallel run passed931 and hit one unrelated test-runner ETXTBSY (`Text file
  busy`); subsequent serial runs passed, including the final added regressions.
- `cargo build --offline -q` and direct Clippy-driver `cargo check --offline -q`
  pass with existing warnings.
- `python3 scripts/check_arithmetic.py --before-qz /tmp/quazi-arithmetic-before-5ae615b`:
  **34 invalid** sources rejected by check/native/QZI/Windows COFF; **7 valid**
  groups execute natively and from source-removed QZI, and emit Windows COFF.
  Coverage includes generic/renamed impl arithmetic, aliases, numeric/pointer
  references, nested shared/exclusive views and unsigned/signed magnitude edges.
- **1,019 host bit comparisons per path**, native and source-removed QZI,
  exercise a dynamic C-exported remainder against host fmod. Inputs include
  seeded bit patterns, signed zeros, subnormals, huge exponent gaps, NaNs,
  infinities and zero divisors. NaNs compare by classification; finite values
  and signed zeros compare exact bits. Repeated calls catch x87 stack leaks.
  Shared-library tests use explicit external linking with a non-executable stack.
- Old owner arithmetic is compiled only, never executed. New source rejects its
  cached version; a safe old floating remainder wrong result rebuilds correctly
  and remains correct on a new exact hit. An exploratory old alias-reference
  fixture faulted despite valid scalar inputs; it was removed from cache
  execution coverage and is not claimed as a safe previous-compiler test.

Existing numeric-mutation24invalid/7valid+3bounds, scalar13, operand27+6,
initialization39+57+traps, loop23, conditional31, propagation9+8, expression36,
retention12+7, named and dynamic/cache runners pass. Independent reviewers
challenged string result shapes, pointer reference loads, nested view dereference
and floating reduction; findings were corrected and covered by regressions.
The optimized compiler is installed locally with `cargo install --path .
--offline --force`; its arithmetic/cache/host runner passes. STD memory6 and
ordinary quazifetch build/run pass without S10, with old-cache miss/new exact
hit and Windows COFF emission. Graphify query/AST update passes: 5,453 nodes,
12,248 edges; no .qz semantic extraction. Changed-region formatting and Markdown
links pass. Migration's owned String operation is also built and executed.
Logs: `/tmp/quazi-arithmetic-*.log`.

Mixed numeric coercion/normalization, bitwise/Pow constraints, generic ordering
proof and full ownership/effect/artifact contracts remain open. README statuses
and audit are refreshed before committing. The 33-item/78-requirement rubric
remains **73.2% complete / 26.8% remaining**, Ownership9/16 and LSP16/20.
