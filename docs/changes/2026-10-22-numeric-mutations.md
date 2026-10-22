# Numeric mutation checks and borrowed builtin indexing

Audience: Quazi users and compiler developers.

The compiler now rejects non-numeric ++/-- and arithmetic compound assignment,
including owning structs, enums, arrays, functions and strings. Generic numeric
helpers remain supported, with requirements checked for every proven concrete
specialization. Raw pointer +=/-= retains integer offsets. Missing type evidence
fails closed. See the [implementation contract](../internals/numeric-mutations.md).

Borrowed builtin indexing now retains its element type. Floating array/slice
updates select floating arithmetic, borrowed slice reads load both descriptor
slots, and borrowed bytes remain immutable. A numeric reference RHS is loaded
when a compound assignment has a generic target.

QZC25 forces older source caches to rebuild. QZI13 is unchanged and Unverified.
General arithmetic typing/coercion, narrow-result normalization and complete
ownership/effect proof remain open; this checkpoint covers mutations only.

Verification uses `/tmp/quazi-named-rust-bin` wrappers, explicit RUSTC and
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`:

- `cargo test --offline -q`: **923 passed**, including 17 added semantic tests
  for concrete/generic constraints, aliases, declaration slots, analyzer reuse,
  cancellation, budgets and borrowed builtin indexing.
- `cargo build --offline -q` and direct Clippy-driver `cargo check --offline -q`
  pass with existing warnings.
- `python3 scripts/check_numeric_mutations.py --before-qz /tmp/quazi-numeric-before-5bb6c53`:
  **24 invalid programs** rejected by check/native/QZI/Windows COFF; **7 valid
  checks** execute natively and from QZI after source removal, and emit COFF.
  Three borrowed byte/slice bounds cases trap natively and from source-removed
  QZI. Old unsafe owner arithmetic is compiled only and never executed;
  rebuilding rejects it. A safe old floating-array wrong result is corrected
  after cache invalidation and remains correct on a new exact hit.

Existing scalar13, operand27valid/6invalid, initialization39invalid/57valid plus
traps, loop23, conditional31, propagation9invalid/8valid, expression36,
retention12invalid/7valid, named-argument and dynamic-vtable/cache runners pass.
`cargo install --path . --offline --force` updates the local optimized compiler;
its numeric/cache/bounds runner and six isolated STD memory tests pass.
Installed ordinary quazifetch build/run passes without S10, with old-cache miss
and new exact hit; Windows COFF emission also passes. Its RAM reports
MemAvailable-based usage. Windows-host execution is not claimed.
Graphify AST update passes: 5,407 nodes and 12,129 edges; no .qz semantic
extraction. Changed-region formatting and Markdown links pass.
Independent review found no further blocker after descriptor and reference-RHS
corrections. Logs: `/tmp/quazi-numeric-*.log`.

README statuses and the roadmap audit are updated before committing. The rubric
remains **73.2% complete / 26.8% remaining**, Ownership9/16 and LSP16/20: no new
roadmap requirement is completed by these correctness fixes.
