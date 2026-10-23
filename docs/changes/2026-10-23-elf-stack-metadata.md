# ELF object stack requirements

Audience: users building native objects, archives and shared libraries.

Quazi-generated ELF objects now contain one empty `.note.GNU-stack` section
with no allocation, write or execution flags. It is SHT_PROGBITS despite its
conventional name. External linkers can now derive that these objects do not
require an executable stack, including when building ordinary shared libraries.
Previously the missing metadata could yield executable-stack requirements or no
PT_GNU_STACK header and cause the host loader to reject a library.

This follows the [GNU ld stack-marker contract](https://sourceware.org/binutils/docs/ld.html).
The fix belongs to the shared object producer and covers source, QZI and cached
bytecode emission, startup/no-start objects and static archives. Windows COFF
gets no ELF marker. The builtin ELF linker already emits RW PT_GNU_STACK and
ignores this nonallocated input metadata. No QZI or QZC version bump is needed;
current versions remain QZI14 / QZC28. Existing objects and archives must be
rebuilt to acquire the marker; already linked libraries must be relinked.

External native objects and explicit linker options still determine the final
external-link policy. An explicit user `-z execstack` remains effective. The
producer does not force every external link to use a non-executable stack.
See [the linker contract](../LINKER.md).

Verification uses `/tmp/quazi-named-rust-bin`, explicit RUSTC and
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`:

- The new `elf_objects_declare_non_executable_stack` Rust regression fails
  before the fix and passes afterward. It covers Linux and Windows, both
  startup modes and QZI round trips.
- `cargo test --offline -q -- --test-threads=1`: **954 passed**.
- `cargo build --offline -q` and direct Clippy-driver `cargo check --offline -q`
  pass with existing warnings.
- `python3 scripts/check_elf_stack.py --before-qz /tmp/quazi-stack-before-bc317fd`
  verifies raw section type, empty size and zero flags; source/QZI object and
  static archive markers; builtin and external ld executable execution with
  RW stacks; default shared-library loading with an exported answer; explicit
  executable-stack override inspected without loading; and COFF absence.
  The previous compiler's object lacks the marker. Installed release verification
  adds `--qz /home/amapekibert/.cargo/bin/qz --expect-old-cache-hit`: its valid old
  exact source-cache hit now emits a loadable shared library with new metadata.
  Debug/release cache identities intentionally differ. The optimized compiler is
  installed with `cargo install --path . --offline --force`.
- `python3 scripts/check_arithmetic.py` and `python3 scripts/check_power.py`
  pass after removing both source and QZI shared-library noexecstack workarounds.
  These cover 1,019 remainder host bit comparisons per path, 204 unsigned and
  203 signed exact integer power cases, 209 approximate floating power cases,
  source-negative checks, native/QZI execution, panic paths and Windows COFF.

Independent read-only review and integration execution found no blocking defect.
Graphify query/AST update passes: 5,592 nodes and 12,626 edges; no .qz semantic
extraction. Logs: `/tmp/quazi-stack-*.log`.
README statuses/progress are refreshed before committing; the rubric remains
**73.2% complete / 26.8% remaining**, Ownership9/16, LSP16/20. Stack metadata
strengthens an existing output contract and does not complete another roadmap
requirement. String ABI identity, general numeric normalization/coercion,
effects, structural destruction and verified artifacts remain open.
