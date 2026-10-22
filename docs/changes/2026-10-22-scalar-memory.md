# Raw f32 memory and floating increment/decrement

Audience: Quazi users and compiler developers.

Reads and writes through `*f32` now use native four-byte storage and convert to
and from internal floating VM values, matching C fields and foreign globals.
Aliases and concrete generic pointees use the resolved scalar representation.
Previously an eight-byte access could corrupt neighboring storage. Safe `&f32!`
references keep their internal qword-slot representation.

Floating ++ and -- now add/subtract floating 1.0. Earlier lowering modified the
integer encoding of a floating value. Prefix/postfix updates across local,
referenced, raw, field and foreign-global storage select floating arithmetic;
postfix local values survive the subsequent write. Integer-local emission keeps
its existing fast path. See the [internal contract](../internals/scalar-memory.md).

**QZC24** rebuilds old exact/reusable source caches. QZI13 is unchanged and
Unverified; rebuild old QZI bodies from source. This change does not define
general intermediate f32 rounding, overflow or narrowed assignment-result
normalization, nor implement physical f16 memory. Non-numeric increment typing
and generic numeric constraints remain separate semantic work.

Verification uses the Rust wrappers under `/tmp/quazi-named-rust-bin`, explicit
`RUSTC`, and `RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`:

- `cargo test --offline -q`: **906 passed**, including two new regressions for
  native-vs-VM float storage and floating update instructions/foreign globals.
- `cargo build --offline -q` and
  `RUSTC_WORKSPACE_WRAPPER=/tmp/quazi-named-rust-bin/clippy-driver cargo check --offline -q`
  pass with existing warnings.
- `python3 scripts/check_scalar_memory.py --before-qz /tmp/quazi-raw-access-before-838cd8f`:
  **13 checks** pass natively, from QZI after source removal, and as Windows COFF.
  Neighbor sentinels catch oversized writes; cases include raw loads/stores,
  compound and prefix/postfix operations, aliases, generic pointees, signed-byte
  storage, f32 safe references, fields and f32/f64 generic/local arithmetic.
  The previous compiler's initialized, allocated-buffer read returns a wrong
  result; current source rebuilding misses its old cache and executes correctly
  on both the rebuild and a new exact hit. No invalid pointer is executed.

The existing operand (27 valid/6 invalid), expression (36), initialization
(39 invalid/57 valid plus unmatched traps), loop (23) and conditional (31)
cleanup, typed propagation (9 invalid/8 valid), retention (12 invalid/7 valid),
named-call and dynamic/cache regressions pass. `cargo install --path . --offline
--force` installs the optimized compiler locally and passes with existing
warnings. Its scalar runner/cache tests pass; separate STD
`python3 scripts/check_os_memory.py <installed-qz>` passes all six tests.
Ordinary installed `qz build`/`qz run` in quazifetch pass, including an old-cache
miss and new exact hit; `qz build --no-incremental --silent -c --target
x86_64-windows -o /tmp/quazi-raw-access-quazifetch.obj` emits COFF. Windows-host execution and
complete STD validation are not claimed. Logs: `/tmp/quazi-raw-access-*.log`.

Independent review confirms storage width and arithmetic are kept separate;
it also identified the open result-rounding contract above. Graphify query/AST
update, changed-region formatting and Markdown links are checked before commit.
README status/progress is refreshed before the commit. The unchanged 33-item,
78-requirement rubric stays **73.2% complete, 26.8% remaining; Ownership 9/16,
LSP 16/20**. These corrections strengthen existing requirements.
