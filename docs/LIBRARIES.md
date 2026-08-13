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

## QZI v6

QZI is both the portable bytecode container and compiled-library format. It is
not a package-manager archive. A v6 file contains:

1. package name, version, `executable`/`library` kind, and entry signature;
2. a source-visible public interface;
3. symbolic call relocations;
4. bytecode chunks and constant pools.

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

## QZC v1

QZC means **Quazi Compilation Cache**. A project uses one cache file:

```text
build/quazi/<architecture>-<os>/default/incremental.qzc
```

After a successful build it stores compiler/target identity, canonical input
paths and SHA-256 hashes, validated linked QZI, and backend build metadata.

On the next build, an exact match skips parsing, analysis, optimization, and
bytecode generation. The backend reuses QZI and links requested native output.
Any changed input, dependency, manifest, lockfile, compiler identity, or corrupt
cache causes a safe full rebuild. `--no-incremental` bypasses reads and writes;
`qz clean` removes the configured output directory.

Normal build output reports `cache lookup` as `hit` or `miss`, then `cache
write` as `saved` after a successful cold build. These stages follow the same
`--silent`, `--no-progress`, `--no-color`, and `--no-unicode` output controls as
the compiler stages.

QZC v1 is an exact warm-build cache, not per-function incremental WPO. WPO stays
unchanged: every cache miss still analyzes and optimizes the complete linked
program. Fine-grained query caching can evolve later with dependency
fingerprints and invalidation tests; QZC already supplies its container.
