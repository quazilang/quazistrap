# Libraries, QZI, and incremental builds

This document fixes the first stable library model. Three files have three jobs:

- `quazi.toml` is the human-written dependency request.
- `quazi.lock` records the exact resolved dependency graph.
- `incremental.qzc` is disposable local compiler cache state.

`quazi.lock` is portable and should be committed. QZC is machine/compiler
specific, lives under `build/` by default, and must not be committed.

## Imports

Quazi names always use `.`:

```quazi
import math.vector.Vec2;
pub import math.vector.{Vec2, dot};
```

`pub import` imports a name and exposes it from the current module. There is no
separate `reexport` keyword.

The file configured by `[lib].path` is the package entry point. Its own `pub`
declarations are package exports directly, so `pub fn factorial` there is
imported with `import my_package.factorial;`. No extra module or `pub import` is
required. Other source files remain modules; the entry point must `pub import`
their declarations when they should become direct package exports.

## Manifest dependencies

The TOML key is the Quazi identifier used by `import` (letters, digits, and
`_`, without a leading digit). A dependency has exactly one of
`path` or `url`. A local project, `.qz`, or `.qzi` path can infer its type.
Internet dependencies require an explicit type.

The CLI normally infers the manifest key from package metadata or the URL:

```sh
qz add ../math
qz add https://example.org/net.git --type git
qz add ../math --alias numbers
```

There is one add form: the positional value is always the path or URL. Package
identity comes from `quazi.toml`, QZI metadata, or the downloaded source.
Use `--alias <name>` when the local import name should differ from the package
identity. Only the alias is needed in the manifest. The compiler discovers the
identity from source-project or QZI metadata and records it in `quazi.lock`.

```toml
[dependencies]
local_math = { path = "../math" }
single = { path = "vendor/single.qz" }
compiled = { path = "vendor/compiled.qzi" }
numbers = { path = "../math" }

net_git = { type = "git", url = "https://example.org/net.git", version = "v1.2.0" }
codec = { type = "archive", url = "https://example.org/codec.tar.gz", checksum = "sha256:..." }
one_file = { type = "source", url = "https://example.org/one_file.qz", checksum = "sha256:..." }
fast_math = { type = "qzi", url = "https://example.org/fast_math.qzi", checksum = "sha256:..." }
```

| Type | Materialized form |
|---|---|
| `path` | local Quazi project directory |
| `git` | checked-out project directory |
| `archive` | extracted project directory |
| `source` | one `.qz` module |
| `qzi` | compiled QZI library plus embedded public interface |

Downloads are stored in `<out_dir>/deps`. Archives are extracted
with traversal checks. File/archive dependencies are SHA-256 verified when a
checksum is requested; the resolved checksum is written to `quazi.lock`. Git
dependencies accept `version = "<tag>"`, `version = "<commit hash>"`, or
`version = "latest"`. Tags and hashes resolve to a locked commit. `latest`
fetches and rewrites that lock on every resolution. QZI files retain their
metadata version and are SHA-256 pinned in `quazi.lock`. Run `qz fetch` to
materialize and verify dependencies, and `qz deps` to inspect the result.

Build dependency trees show logical import names such as `std.io` and
`qz_test_lib`, never host filesystem or package-cache paths. Git downloads use
Git's reported percentage as an accurate progress bar followed by the same green
status diamond as build stages; failures use a red diamond.

## QZI (format 17)

`qz build --qzi` and `qz build path/to/main.qz -i` produce one QZI that serves
Linux, Windows, and NetBSD from a single compilation. Import resolution
determines which files each system uses; the files are parsed once and
analyzed and lowered once. Shared code is one body for every target. A
function whose `@cfg` statements or conditional variants differ is compiled
once per distinct group of targets, and those bodies carry disjoint target
sets. Calls are recorded by name, so selecting a target links shared code to
that target's implementation. The build prints a single analysis and
bytecode stage and never names a target in its summary. Project builds resolve
imports before reusing a QZI whose sources, configuration and dependencies
are unchanged. Both `--no-incremental` and `--debug` bypass this cache. See
[the internal contract](internals/qzi.md).

`qz build app.qzi --qzi -o copy.qzi` validates and copies the complete current
QZI. To combine code and dependencies in a new QZI, build the source project
that declares those dependencies.

Target-specific type aliases of built-in types (such as `std.ffi.c_long`)
and functions whose signatures differ per target are supported. Structs,
enums and traits whose definitions differ per target, and methods whose
signatures differ, are rejected with an explanation.

`qz build app.qzi --target x86_64-netbsd` selects the NetBSD bodies: the
native binary contains only them and the shared code, so no Linux syscalls
reach NetBSD output and no NetBSD libc reaches Linux output. QZI
dependencies of a project are selected the same way, and native generation
needs no source files.

If code needed on a supported target is missing there, the build names the
caller, the missing declaration and the target, and preserves any existing
output. The successful build summary says `QZI`.
Earlier partial QZI artifacts can still be read for the targets they contain.
One artifact carries one entry signature.

Layout: magic, 17, the target names, one module in the sectioned v14 layout
holding every distinct body (one metadata section, interface, call
relocations, ownership certificate, bytecode), one target mask per body,
interface overrides for targets whose public interface differs, and a
SHA-256 of the whole file. Format 17 requires the module's ownership
certificate (schema 2): one effect record per body, recomputed from the
bytecode and compared on every read. Format 16 carried only the unverified
envelope and is rejected with a request to rebuild from source. The
certificate is verified, but downstream ownership analysis does not consume it
yet. `scripts/check_qzi.py` checks the contract; see
[ownership certificates](internals/ownership-certificates.md).

Shared references to multi-slot fixed arrays can refer to stack or heap
storage. A library interface that exchanges these references requires the
current reference representation marker. Rebuild an older library from
source if the compiler reports a fixed-array reference ABI mismatch. Existing
raw-pointer buffers retain their slot order, and the QZI container layout is unchanged.

### Dependencies inside a project QZI

Projects are combined through dependencies, not through a separate
container: a project's QZI links the code of its `quazi.toml` dependencies
(recursively) into the same one artifact, which compiles without them.
Aliasing a compiled library preserves its target-specific bodies and interfaces.
A library whose public interface differs per target is seen through one
interface whose differing declarations carry `@cfg(target_os = ...)`.
Interface cache entries are keyed by interface content.
Format 15, an early container holding one complete program per operating
system, is retired; reading such a file asks to rebuild it from source.

## QZI module layout and compatibility

QZI is the bytecode container for executables and compiled libraries. The
module embedded in v17 uses the sectioned layout from v14:

1. package name, version, `executable`/`library` kind, and entry signature;
2. a source-visible public interface;
3. symbolic call relocations;
4. a mandatory ownership artifact (in v17, a certificate); and
5. bytecode chunks and constant pools.

V7 recorded unsigned integer division, remainder, and relational semantics in
instruction flags. This prevents high-bit `u64` and `usize` values from being
miscompiled as signed integers. Every `Lea` also carries an explicit nonzero
address-taken block length so register allocation cannot recycle referenced
slots. V8 adds the phase-2 layout-query intrinsic boundary:
`quazi.size_of[T]()` and `quazi.align_of[T]()` are assigned stable intrinsic IDs,
and current codegen resolves them to constants in each monomorphized wrapper.
V10 adds a canonical, content-bound ownership-artifact envelope covering the
metadata, public interface, relocations, and bytecode. Its current state is
explicitly unverified: it detects stale or malformed artifacts but does not yet
prove D-014 effects or permit borrowed values through a QZI-only dependency.
V14 adds explicit floating Pow operand representations; integer power lowers
to checked portable instruction loops. Pre-v14 artifacts containing Pow require
a source rebuild. See [the power migration](migrations/power.md). QZC28 rebuilds
older source caches for the new checks and lowering.
The current reader decodes supported v2-v13 files without legacy Pow; link compatibility is checked
separately. V1
requires a source rebuild because it omitted frame metadata, and legacy files
with implicit `Lea` metadata, parameterized v6 trait interfaces, or v6 public
runtime-`any` interfaces require rebuilding. Compilers must reject newer QZI
generations rather than silently ignore unsupported semantics. QZC v13 invalidates older exact
hits and reusable chunks so cached QZI cannot omit dynamic table layouts or bypass match-arm string-view provenance checks, indexed and concrete-generic dense-element read checks,
or corrected ownership transfers.

V12 uses one slot-reference convention for explicit borrowed receivers and
ordinary safe-reference parameters; shared string views remain representation
identical. Older ambiguous reference/aggregate interfaces, missing callable
interfaces on parameterized libraries, and opaque outbound calls require a
source rebuild. Proven closed scalar/raw-pointer/shared-string contracts retain
compatibility. See [the receiver migration](migrations/reference-receiver-abi.md).
V13 retains exact ordered implementation symbols for dynamic vtables. Source
and QZI-only native builds share descriptor-driven emission; old layout-free
table addresses require a source rebuild. Private trait implementations stay
out of the public interface while their bytecode remains internal. See the
[v13 migration](migrations/qzi-v13-vtables.md) and
[developer contract](internals/dynamic-vtable-layout.md).

V11 adds the original ownership ABI generation to content-bound metadata.
Older allocating or ambiguous ownership contracts require a source rebuild;
namespace rewriting or reserialization retains their origin. See
[owned formatting and QZI v11](migrations/owned-formatting.md).

QZI v7 is also the affine function-value ownership boundary. Compatible older
artifacts remain readable only when they expose no owned `fn` contract and
contain no legacy closure or named-function-forwarder chunks. Those artifacts
must be rebuilt from source so callers and callees agree on environment cleanup.

The frontend checks a QZI dependency against its embedded interface. The QZI
linker resolves symbolic calls by dotted name and combines bytecode. A
precompiled library therefore avoids recompiling its source.
Library WPO starts from the public API and retains its private dependency
closure, so unused prelude/STD code is not packed. When several QZI inputs carry
the same dependency chunk, the linker deduplicates only bytecode-equivalent
definitions and rejects conflicting ones.

Executable and library QZI use one container. Library metadata does not make a
file execute automatically; execution still requires an entry point. Future
JIT and dynamic loading can use the same metadata/interface/symbol foundation,
but runtime unloading and version negotiation are not implemented yet.

Generic bodies are not yet a stable binary-library ABI. The compiler rejects a
QZI library with public generic declarations and tells the author to distribute
source until generic templates gain a portable representation.

## QZC v39

QZC means **Quazi Compilation Cache**. Project builds cache native targets
and QZI at the following paths:

```text
build/quazi/<architecture>-<os>/<artifact>/incremental.qzc
build/quazi/qzi/<artifact>/incremental.qzc
```

After a successful build it stores compiler/target identity, canonical input
paths and SHA-256 hashes, validated linked QZI, backend metadata, and reusable
pre-WPO function chunks keyed by source hash and semantic-context fingerprint.
QZC v39 rebuilds derived codecs after `@serde` keys gained `*:` and began
to apply to `@derive(Serialize, Deserialize)` structs and enum variants.
QZC v38 rebuilt calls after panic lowering changed from spelling-based
dispatch to the prelude-selected declaration. QZC v37 rebuilt calls after
layout intrinsic resolution moved to declaration identity. QZC v36 rebuilt calls
that could be lowered as a same-named enum variant.
QZC v35 had refreshed the fixed-array reference layout.

For a native target, an exact hit skips frontend and WPO and reuses linked
bytecode. On a partial hit the
compiler parses and analyzes the complete program for sound WPO, restores
unchanged pre-WPO function chunks, compiles only functions in changed source
files, then reruns global reachability, inlining, constant propagation, dead
elimination, register allocation, QZI linking, and native emission over both.
Changing any declaration signature, type/import context, target, manifest,
lockfile, or QZI dependency conservatively invalidates all function chunks.
Corrupt caches are ignored. `--no-incremental` bypasses reads and writes;
`qz clean` removes the configured output directory.

Normal build output reports `cache lookup` as `hit`, `partial`, or `miss`.
The bytecode stage reports restored and compiled function counts, then `cache
write` reports `saved`. These stages follow the same
`--silent`, `--no-progress`, `--no-color`, and `--no-unicode` output controls as
the compiler stages.

QZC deliberately stores pre-WPO QZI units, not native objects. This keeps WPO
whole-program: cached functions never bypass the final global optimization pass.
Generic specializations and closure-owning functions are conservatively rebuilt.
