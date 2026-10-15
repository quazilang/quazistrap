# Retain dynamic vtables in portable bytecode

Audience: Quazi users, library maintainers, and compiler contributors.

## Problem and behavior

The source-native dynamic receiver fixture executed correctly but its emitted
QZI failed linking on `__vtable_DynamicCounter_Readable`. Native vtables were
reconstructed exclusively from a source `SemanticReport`, which artifact-only
builds cannot supply. The old compiler reproduces that failure in
`/tmp/quazi-vtables-before.log`.

QZI v13 retains exact implementation chunk symbols in semantic trait-slot order
with each table constant. Source and QZI native builds use that same descriptor.
Dead-inline elimination retains table method targets, including small methods
also called directly. Final linking rejects inconsistent layouts and missing
implementation symbols. The backend uses actual native function names and
propagates table-relocation errors. Private implementation details need not be
exposed through the public source interface.

The real aliased-library regression also exposed a source-interface leak:
private trait implementations were emitted without the private nominal
declarations needed to analyze them. Same-source nominal privacy now omits
internal-only impl declarations, preserves explicit public methods on public
targets as inherent declarations, and reports unsupported exports on private
targets. Alias and generic-argument spellings never justify silent omission.
The underlying implementation chunks remain internal artifact dependencies.

The [developer contract](../internals/dynamic-vtable-layout.md) specifies the
encoding, limits, namespace policy, partial-cache boundary, and deterministic
symbol construction. A chunk-local descriptor was chosen over a new module
section to preserve existing linker/backend/cache interfaces and bytecode
content binding. QZC advances to v13; source syntax and receiver ABI stay the
same. See the [migration](../migrations/qzi-v13-vtables.md).

## Verification

From the compiler root, using the pinned Rust toolchain and bfd linker:

```sh
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
export RUSTFLAGS='-C link-arg=-fuse-ld=bfd'
cargo test --offline -q vtable
cargo test --offline -q
cargo build --offline -q
cargo clippy --offline -q
target/debug/qz build prelude/tests/dynamic-vtable-layout.qz --no-incremental -o /tmp/quazi-portable-vtables -r
target/debug/qz build prelude/tests/dynamic-vtable-layout.qz --no-incremental -i -o /tmp/quazi-portable-vtables.qzi
target/debug/qz build /tmp/quazi-portable-vtables.qzi --no-incremental -o /tmp/quazi-portable-vtables-roundtrip -r
target/debug/qz build /tmp/quazi-portable-vtables.qzi --no-incremental -c --target x86_64-windows -o /tmp/quazi-portable-vtables.obj
python3 scripts/check_dynamic_vtables.py "$PWD/target/debug/qz"
git diff --check
```

Final verification passes 770 compiler tests, including 13 focused vtable
regressions and 13 interface tests. Build and clippy succeed with existing
warnings. The integration script passes imported dispatch, unmangled native
names, cold/exact/partial/updated-exact caches (one restored and one compiled
unit), and an aliased QZI library with private dynamic dispatch after deleting
its defining source.

The permanent native regression serializes, decodes, and links the chunks, then
compiles and executes with no source report. The fixture uses nonalphabetical
trait slots, two implementations with different field layouts, shared and
exclusive methods, repeated tables, an inline candidate called directly and
dynamically, and an ordinary helper dependency. Backend tests inspect ELF/COFF
table symbols and relocations with unmangled native method names and marker
traits. Cache tests write/read QZC units and rerun optimization with restored
descriptors. Serialization tests cover namespace changes, content tampering,
generation downgrade, every truncated prefix, and cross-module resolution.

Windows host execution remains unavailable. Dynamic destruction, general source
coercion gaps, unused-method warning precision, and D-014/D-015 ownership proof
remain open. Ownership envelopes are still `Unverified`; this change resolves
the retained-layout artifact gap only.
