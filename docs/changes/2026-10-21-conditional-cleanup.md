# Runtime cleanup for conditional owners

Audience: Quazi users and compiler developers.

Supported local owners now carry a runtime cleanup flag. A skipped initializer
leaves the flag clear; successful initialization or replacement sets it; a move
or destruction clears it on the executed path. Cleanup tests the flag before
reading the value. This fixes crashes after skipped initialization and leaks
after an untaken consuming branch. Loop-body declarations reset their flags
on each iteration. Reverse lexical destruction order is preserved.

Previously `DropLocal.active` was a mutable code-generation boolean. Compiling
one branch changed cleanup for every later path, even when that branch never
ran. Runtime flags replace that state, including the old return-time snapshot.
The binding lookup checks both the name and its current register, so copying
a scalar shadow cannot suppress cleanup of an outer owner. Flags follow the
entire parameter ABI prefix, including variadic pointer/count and hidden sret
slots; allocating them between parameters would corrupt incoming arguments.

Consumption requests are claimed when an expression begins and applied after
its evaluation, in the executed branch. Returning, binding or passing a `match`
result transfers only the selected owner. Post-evaluation field/receiver
transfers compare the selected handle with guarded candidate bindings.
Zero-argument calls can share a source span with their callee identifier; the
callee must not claim consumption intended for the call result.

Pending argument owners remain in full-expression slots until a call commits.
Groups, casts and selected match arms preserve these slots if a later argument
fails through `?`. Independent review found and reproduced a leak in the first
draft of this change when wrapper consumption cleared those slots too early;
the final implementation separates binding moves from committed transfers.

**QZC21** invalidates older exact and reusable source caches. QZI13 and the
physical calling convention remain unchanged. Rebuild older QZI bodies from
source; native emission cannot insert ownership flags into existing bytecode.
See [migration guidance](../migrations/conditional-cleanup.md).

Verification uses the stable toolchain through the existing
`/tmp/quazi-named-rust-bin` ELF-loader wrappers, with
`RUSTC=/tmp/quazi-named-rust-bin/rustc` and
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`:

- `cargo test --offline -q`: **873 passed**, zero failed/ignored, including a
  new zero-argument callee-environment regression. Seven existing cleanup
  assertions now inspect reachable instructions: constant propagation folds
  guard branches but retains unreachable destructor instructions in the chunk.
- `cargo build --offline -q` and
  `RUSTC_WORKSPACE_WRAPPER=/tmp/quazi-named-rust-bin/clippy-driver cargo check --offline -q`:
  pass with existing warnings; newly introduced lint findings were corrected.
- `python3 scripts/check_conditional_cleanup.py --before-qz /tmp/quazi-conditional-drop-before`:
  **31 cases** pass natively and from QZI after source removal, with Windows
  x86-64 COFF emission. Cases cover skipped/taken initialization and consumption,
  replacement, loop flag reset, selected match transfer/self-replacement,
  pending argument failure, scalar/owner shadows, supported structural cleanup,
  mixed owner/scalar/variadic parameters and a repeated zero-argument closure.
  The old compiler builds the isolated fixture but exits 7 because an untaken
  consuming branch suppresses cleanup. The new compiler misses that old cache,
  runs correctly, then executes a correct exact hit.
- Existing loop cleanup (23 traces), typed propagation (9 rejected/8 valid),
  expression control (36 checks), result retention (12 rejected/7 valid), named
  arguments and dynamic-vtable/cache integrations pass.
- `python3 scripts/check_os_memory.py <matching-qz>` in the separate STD checkout:
  **6 passed**. Ordinary quazifetch build/run passes with no S10 diagnostic;
  Linux used memory tracks installed fastfetch through `MemAvailable`.

Independent read-only design/implementation review, changed-region formatting,
Markdown links and Graphify query/AST update were checked. Windows emission
does not establish Windows-host runtime behavior. No performance benchmark or
full STD validation is claimed.

Runtime cleanup flags do not prove that ordinary reads are definitely
initialized. The source type checker's branch/loop initialization merge remains
a separate audit item. General partial moves, recursive generic/enum destruction,
interprocedural loans/effects and artifact verification remain D-014/D-015 work.
QZI13 envelopes remain Unverified; this change grants no new loan permissions.
