# Exact integer and typed floating power

Audience: language users and compiler developers.

The approved [D-016](../decisions/power-operator.md) is implemented: integer power
uses exact checked squaring, known negative integer exponents fail compilation,
runtime negative exponents and overflow panic, and floating negative exponents
produce floating results. Large integers no longer pass through double or C pow.
Either floating operand selects f64. Nonnumeric generic actuals are rejected.
See [the contract](../internals/power.md) and [migration](../migrations/power.md).

QZC28 invalidates older exact/reusable source caches. QZI14 records floating
operand representations; earlier artifacts containing Pow require rebuilding.
Compatible older non-Pow artifacts preserve their ownership origin. Artifacts
remain Unverified. Fractional floating results are approximate, with no correctly
rounded libm promise. No implicit external pow symbol remains.

Verification uses `/tmp/quazi-named-rust-bin`, explicit RUSTC and
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`:

- `cargo test --offline -q -- --test-threads=1`: **953 passed**.
- `cargo build --offline -q` and direct Clippy-driver `cargo check --offline -q`
  pass with existing warnings.
- `python3 scripts/check_power.py --before-qz /tmp/quazi-power-before-64b2760`:
  **12 invalid programs** rejected by check/native/QZI/Windows COFF;
  **8 value groups** pass native/source-removed QZI/Windows COFF;
  **13 language panic cases** exit 101 in native/source-removed QZI execution.
  Covers every integer width, signed MIN, unsigned MAX, exact large integers,
  huge exponents, operand sequencing, aliases, references and transitive generics.
- Dynamic exports compare **204 unsigned and 203 signed integer cases** exactly,
  and **209 floating cases** against the host within approximation tolerance,
  per native/source-removed QZI shared library. Additional exact controls cover
  integer exponent parity above 2^53, signed MIN and 2.0 ** -1074. Existing ELF
  stack metadata work remains separate; these host-loaded fixtures explicitly
  request noexecstack. No unresolved pow import exists.
- Direct backend execution covers 768 finite floating samples, special values,
  x87 stack/control preservation and both ABI encodings without calls/relocations.
  QZI tests cover all 256 flag bytes and old-generation compatibility.
- Old nonnumeric Pow is compiled to bytecode only, never executed. The current
  compiler rejects its cached source; exact integer rebuilding misses old caches
  and subsequent exact hits execute correctly.

Earlier arithmetic, operator, numeric mutation, scalar memory, operand,
initialization, loop/conditional cleanup, propagation, expression control,
retention, named argument and dynamic/cache runners pass. The installed optimized
compiler passes the complete power runner/cache checks. Six isolated STD memory
tests pass; ordinary quazifetch builds/runs, misses its older cache, hits the new
cache, and compiles to Windows COFF. Windows runtime is not verified on this host.
Graphify query and AST update pass: 5,578 nodes and 12,584 edges; no .qz
semantic extraction. Logs: `/tmp/quazi-power-*.log`.

Independent reviews found no remaining confirmed defect in the checked integer
algorithm or floating backend. Review recommendations added width, overflow,
sequencing and signed differential cases. README statuses and the roadmap audit
are refreshed before committing: unchanged **73.2% complete / 26.8% remaining**,
Ownership9/16, LSP16/20, 33 items and 78 requirements. String runtime identity,
general numeric coercion/normalization, effects, recursive destruction and
verified artifacts remain open; this checkpoint does not claim production readiness.
