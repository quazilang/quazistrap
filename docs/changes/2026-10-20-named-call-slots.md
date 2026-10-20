# Named arguments follow resolved declaration slots

An accepted call such as `value.digits(second = 2, first = 1)` previously
passed its operands in source order when the parameter table was keyed by
`Type.digits` but codegen looked up `digits`. An unrelated function with that
short name could also supply the wrong ordering. Positional calls returned 12
while the corresponding named calls returned 21.

Codegen now indexes declaration slots from semantic call argument facts by
source span. All concrete specializations must agree on that mapping. Supplied
operands are stably ordered by parameter slot, receiver operands remain with
method dispatch, and repeated variadic slots retain every supplied operand.
Unavailable, incomplete or inconsistent evidence fails explicitly. No bare-name
fallback or emitted-specialization spelling determines argument binding.

This preserves the existing emission policy; it does not establish a language
evaluation-order guarantee, new ownership permissions or artifact verification.
Function values whose types erase parameter names cannot support this mapping;
their named calls fail during codegen. Improving that semantic diagnostic is
separate work. QZI13 is unchanged; QZC16 invalidates older source caches.

Validation used the existing stable Rust toolchain through temporary ELF-loader
wrappers (`PATH=/tmp/quazi-named-rust-bin:$PATH`,
`RUSTC=/tmp/quazi-named-rust-bin/rustc`,
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`):

- `cargo test --offline -q`: 834 passed, including three focused regressions.
- `cargo test --offline -q prior_cache_versions_cannot_bypass_ownership_validation`:
  passes after adding v15 to the rejected versions.
- `cargo build --offline -q`: passes with existing warnings.
- `python3 scripts/check_named_arguments.py --qz target/debug/qz`: passes
  native execution, QZI-only execution, Windows COFF emission and an aliased
  scalar QZI library import after removal of its defining source. Shared,
  exclusive, consuming and legacy receivers, static methods, generic callers,
  module aliases, short-name collisions, closures and variadics are covered.
  The preceding compiler fails this fixture at native exit 2.
- `cargo clippy --offline -q` could not launch its Cargo/driver subprocesses:
  the original Nix ELF loader disappeared. Running `cargo check --offline -q`
  with `RUSTC_WORKSPACE_WRAPPER=/tmp/quazi-named-rust-bin/clippy-driver` invokes
  the same installed Clippy driver successfully, with existing warnings.
- Independent read-only review, changed-region formatting and `git diff --check`
  pass. Graphify AST update ran; its index excludes Quazi source semantics.

Separately, STD commit `d3e3a0b` changes Linux `std.os.memory_available()` to
prefer `MemAvailable` over free RAM. Six deterministic parser fixtures pass
`python3 ../std/scripts/check_os_memory.py target/debug/qz`. Quazifetch builds
and runs from source and QZI, survives repeated cache hits and emits a Windows
object. Its live used-RAM estimate follows fastfetch within sampling changes.
Windows-host execution and full STD validation are not claimed.
