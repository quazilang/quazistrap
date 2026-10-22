# Source initialization and checked match execution

Audience: Quazi users and compiler developers.

Local reads now require initialization on every normal path reaching them.
Branches, guarded matches, short-circuit conditions and loop exits join by
declaration identity; deferred closures cannot initialize their outer scope.
C updates run after the body. Terminal statement annotations align analysis and
emission. See [the contract](../internals/source-initialization.md) and
[migration guide](../migrations/source-initialization.md).

Checking follows emitted named-argument and struct-field declaration slots;
indirect callees and assignment place operands precede arguments/RHS. Duplicate
fields reject. Place validation consumes checked operands once, and range bounds
precede the loop binder. Match snapshots its scrutinee before guards, uses checked
discriminants, validates substituted payload representations, loads borrowed
scalars correctly and traps on unmatched values while preserving Void fallthrough.
Boolean patterns test zero/nonzero truthiness, integer patterns exact equality,
float patterns IEEE equality and strings the runtime first-NUL boundary.

Known results cannot erase executed effects. Foldable annotations require
elidable evaluation; unknown float multiplication by zero is not absorbing.
Constant propagation treats every Lea-addressed VM slot as runtime memory in
both dataflow and rewrite, preventing stale constants after aliased writes.

Independent read-only review found draft schedule, scrutinee-alias and absorbing
fold gaps; each received a focused correction and regression. The native pointer
fixture also reproduced stale aliased constant propagation on the previous
compiler. An existing ownership-negative fixture now annotates its wrapped enum
so it continues testing S10 with a proven payload type, rather than failing only
on incomplete type inference. The old floating-zero folding assertion was
corrected to preserve IEEE semantics.

**QZC22** invalidates previous exact/reusable chunks. QZI13 and physical ABI are
unchanged; rebuild older QZI bodies from source. Its envelope remains Unverified.

Verification uses the stable toolchain via `/tmp/quazi-named-rust-bin`, with
`RUSTC=/tmp/quazi-named-rust-bin/rustc` and
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`:

- `cargo test --offline -q`: **897 passed**, zero failed/ignored, including
  60 contained-LSP tests. Initialization tests cover declaration identity,
  reachability, conditions, exits, pattern domains and budget exhaustion;
  constprop tests cover scalar aliases, whole addressed blocks and successors.
- `cargo build --offline -q` and
  `RUSTC_WORKSPACE_WRAPPER=/tmp/quazi-named-rust-bin/clippy-driver cargo check --offline -q`:
  pass with existing warnings.
- `python3 scripts/check_initialization.py --before-qz /tmp/quazi-initialization-before`:
  **39 invalid** sources rejected by check/native/QZI/Windows COFF; **57 valid**
  checks pass natively and from QZI after source removal, with Windows COFF
  emission. Unmatched values deterministically trap in native/QZI, and Void
  preserves fallthrough. The previous compiler builds the invalid-read source;
  current analysis rejects its old cache without executing that read. Separate
  safe guard and pointer fixtures reproduce old wrong results, rebuild correctly
  after an old-cache miss, and execute correctly on new exact hits.
- `python3 scripts/check_loop_cleanup.py`: **23 traces** pass;
  `python3 scripts/check_conditional_cleanup.py`: **31 traces** pass;
  `python3 scripts/check_try_propagation.py`: **9 invalid/8 valid** pass;
  `python3 scripts/check_expression_control.py`: **36 checks** pass.
- `python3 scripts/check_result_retention.py target/debug/qz`: **12 invalid/7 valid**;
  `python3 scripts/check_named_arguments.py` and
  `python3 scripts/check_dynamic_vtables.py target/debug/qz`: pass, including
  source-removed libraries and exact/partial caches.
- In the separate STD checkout,
  `python3 scripts/check_os_memory.py <matching-qz>`: **6 passed**. Quazifetch
  `qz build --no-incremental --no-color --no-unicode` and its native executable
  pass; `qz build --no-incremental --no-color --no-unicode --silent -c --target x86_64-windows -o /tmp/quazi-init-quazifetch.obj`
  emits x86-64 COFF. Ordinary quazifetch builds also miss the old cache, execute
  correctly, then hit the new 131-function cache and execute correctly again.
  Linux RAM uses MemAvailable; live output and installed
  fastfetch are comparable, with sampling and accounting differences.

Changed-region formatting, local Markdown links, scoped diffs and Graphify
query/AST update are checked before commit. Logs are `/tmp/quazi-init-*.log`.
No Windows-host runtime or full STD validation is claimed.

README statuses were updated before the commit. The unchanged 33-item,
78-requirement rubric remains **73.2% complete, 26.8% remaining; Ownership 9/16,
LSP 16/20**. General operand value snapshots, canonical Boolean storage/equality,
partial-place initialization, typed interprocedural effects, recursive destruction
and verified artifacts remain open. The broad workspace goal stays active.
