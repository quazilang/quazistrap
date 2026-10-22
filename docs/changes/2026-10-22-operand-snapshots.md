# Stable operands, places and pending owner storage

Audience: Quazi users and compiler developers.

Eager operands now capture binding-backed values before later mutations.
Computed assignment places evaluate once before the RHS, and compound assignment
captures the prior scalar value. Calls, aggregates, comparison branches, range
bounds and match results preserve their physical value shape. Slice aliases keep
pointer and length together. Typed C fields/flexible-array places preserve their
memory conversion flags. Raw pointers to C aggregates preserve the inline
storage address when dereferenced, rather than loading its first word.

Pending field and consuming-receiver ownership transfers begin at operand
evaluation. Scoped source storage pins reject release/replacement while an
indirect callee, computed place or match traversal still needs an allocation;
view observations retain shared loans. Plain scalar writes remain permitted.
Consumed owned direct assignment results also consume the new destination owner.
Borrow checking follows declaration-slot argument/field schedules.

Independent reviews found late field transfer, consuming fallback and flexible
f32 conversion gaps in drafts; each received a correction and regression.
See [the implementation contract](../internals/operand-snapshots.md) and
[user migration](../migrations/operand-snapshots.md).

**QZC23** invalidates old exact/reusable source caches. QZI13 remains unchanged
and Unverified; older bytecode bodies need a source rebuild.

Verification (toolchain via `/tmp/quazi-named-rust-bin`, explicit `RUSTC` and
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`):

- `cargo test --offline -q`: **904 passed**, including 60 contained-LSP tests.
  Seven new Rust regressions cover pending lifetime regions, declared schedules,
  consumed assignment results, opaque reference lifetimes and C f32 places.
- `cargo build --offline -q` and
  `RUSTC_WORKSPACE_WRAPPER=/tmp/quazi-named-rust-bin/clippy-driver cargo check --offline -q`:
  pass with existing warnings.
- `python3 scripts/check_operand_snapshots.py --before-qz /tmp/quazi-operands-before-87e66dc`:
  **27 valid** checks native, source-removed QZI and Windows COFF, including
  destructor counts, C callbacks and flexible f32 memory. **6 invalid** lifetime
  cases reject through check/native/QZI/COFF. A safe previous-compiler wrong-result
  fixture misses its old cache, rebuilds correctly, then executes on a new hit.
- `python3 scripts/check_initialization.py`: **39 invalid/57 valid** plus unmatched
  traps; loop cleanup **23 traces**, conditional cleanup **31 traces**, typed try
  propagation **9 invalid/8 valid**, expression control **36 checks**, result
  retention **12 invalid/7 valid**, named arguments and dynamic-vtable/cache
  runners pass. Commands are the corresponding `scripts/check_*.py`; retention
  and dynamic runners take `target/debug/qz` as a positional argument.
- Separate STD `python3 scripts/check_os_memory.py <matching-qz>`: **6 passed**.
  Quazifetch `qz build --no-incremental --no-color --no-unicode`, ordinary-cache
  miss/hit builds and native run pass, without S10. Windows object emission with
  `qz build --no-incremental --silent -c --target x86_64-windows -o /tmp/quazi-operands-quazifetch.obj`
  passes. RAM is comparable to installed fastfetch in adjacent live samples.

Independent reviews, changed-region formatting, local Markdown links, scoped
Git diffs, and Graphify query/AST update pass. Logs: `/tmp/quazi-operands-*.log`.
No Windows-host execution or complete STD validation is claimed.

README statuses and percentages are refreshed **before** the commit. The roadmap
rubric retains 33 items/78 requirements: **73.2% complete, 26.8% remaining;
Ownership 9/16, LSP 16/20**. These fixes strengthen existing requirements rather
than completing normalized effects, general partial-place state, complete
recursive destruction or verified artifacts.
