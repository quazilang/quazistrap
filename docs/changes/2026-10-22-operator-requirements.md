# Bitwise, shift and concrete ordering checks

Audience: language users and compiler developers.

Bitwise operators and shifts now reject unsupported types, including through
concrete generic instantiations. & / pipe / ^ preserve integer pairs and Boolean
pairs; shifts require integer pairs. Generic ordering now validates actual
categories: integer pairs, float pairs or recognized strings, returning Bool.
Mixed float/integer ordering remains rejected. See [the contract](../internals/operator-requirements.md).

Known Boolean reference operands load their stored values even beside a generic
operand. Declaration parameters named String retain their parameter identity;
nested exclusive string references cannot masquerade as owned String values.
Actual standard String content comparisons remain supported. This preserves
existing Boolean and string semantics rather than defining a new ordering for
owners or Boolean values. Call an explicit comparison method for structured
values; use an integer representation explicitly when bit manipulation is intended.

QZC27 rebuilds older source chunks. QZI13 remains unchanged and Unverified;
rebuild old QZI bodies from source for these checks and corrected loads. This
checkpoint is not a complete operator/representation/effect proof. Pow lowering,
String ABI identity, numeric conversion and normalization remain open.

Verification uses `/tmp/quazi-named-rust-bin`, explicit RUSTC and
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`:

- `cargo test --offline -q -- --test-threads=1`: **939 passed**, with five
  added regressions for category pairs, every concrete instantiation,
  declaration slots/shadowing, analyzer reuse and nested references.
- `cargo build --offline -q` and direct Clippy-driver `cargo check --offline -q`
  pass with existing warnings.
- `python3 scripts/check_operator_requirements.py --before-qz /tmp/quazi-operators-before-40b1c2a`:
  **37 invalid** programs rejected by check/native/QZI/Windows COFF;
  **5 value groups and 2 owned-string comparisons**, with a prelude-loaded
  String-binder shadowing control, pass native/source-removed QZI/Windows COFF.
- Old owner bitwise code is compiled only, never run; rebuilding rejects its
  old cache. The old safe Boolean-reference fixture masks an aligned valid
  slot address instead of loading true; it returns a wrong Boolean result.
  New rebuilding misses the old cache and executes correctly, including a
  subsequent exact hit. No invalid pointer or owning result is executed.

Earlier arithmetic34invalid/7valid+4panics+1019host-bit comparisons perpath,
mutation24invalid/7valid+3bounds, scalar13, operand27+6, initialization39+57+traps,
loop23, conditional31, propagation9+8, expression36, retention12+7, named and
dynamic/cache runners pass. `cargo install --path . --offline --force` updates
the local optimized compiler; its operator/cache runner passes. STD memory6
and ordinary quazifetch build/run/cache/Windows COFF pass. Logs:
`/tmp/quazi-operators-*.log`.

Independent review found the String-binder and nested-exclusive-reference
bypasses; both were fixed and covered. Graphify query/AST update passes:
5,474 nodes, 12,308 edges, no .qz semantic extraction. Changed-region formatting
and Markdown links pass. README statuses and roadmap audit are refreshed before
committing, unchanged **73.2% complete / 26.8% remaining**, Ownership9/16,LSP16/20.
See [remaining findings](../audits/2026-10-22-operator-followups.md).
