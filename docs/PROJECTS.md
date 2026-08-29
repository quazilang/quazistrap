# Projects and `quazi.toml`

`quazi.toml` separates package identity from output artifacts. One package may
expose one library and several binaries.

```toml
[package]
name = "acme"
version = "0.1.0"
out_dir = "build"
std = true
crash_handler = true
mangling = true

[lib]
name = "acme"
path = "src/lib.qz"

[[bin]]
name = "acme_cli"
path = "src/main.qz"

[dependencies]
local_math = { path = "../local_math" }
wire = { url = "https://example.invalid/wire.qzi", type = "qzi", version = "1.2.0" }

[link]
linker = "builtin" # builtin, auto, or executable path
libc = false
objects = []
libraries = []
library-paths = []
flags = []

[target.x86_64-windows.link]
libraries = ["user32"]
```

Library name must be a Quazi identifier matching `package.name`; binary names
are output names. `qz build --lib` selects the library;
`qz build --bin acme_cli` selects a binary. One artifact is automatic. With
several artifacts, the binary matching `package.name` is default; otherwise
selection is required. Libraries emit target-neutral QZI by default.

Package runtime/codegen switches default to `true`. `std = false` omits both
automatic prelude injection and `std` resolution. `crash_handler = false`
keeps process startup but omits crash-handler registration. `mangling = false`
uses bare native function names; duplicate bare names are compile errors.
These fields replace removed `@no_std`, `@no_crash`, and
`@no_mangle`/`@no_mangling` source attributes.

Legacy `package.type` plus `[build].entry` remains accepted only when artifact
tables are absent. New manifests use `[lib]` and `[[bin]]`. `qz new --lib` and
`qz init --lib` generate this form.

`--target x86_64-linux` and `--target x86_64-windows` select backend, output,
cache namespace, and linker. CLI linker wins over target `[link]`, then base
`[link]`, then defaults. Linux and Windows use in-process linkers for compiler
objects. Native libraries or custom linker flags select an external linker.
`libc = true` is explicit opt-in; no C runtime is inferred.

QZC means Quazi Compilation Cache. Each artifact/target QZC v5 snapshot lives at
`build/quazi/<target>/<artifact>/incremental.qzc` by default. It stores exact-hit
linked QZI plus source-hashed pre-WPO function chunks for partial rebuilds. The
compiler still reruns complete-program analysis and WPO after restoring chunks.
V5 rejects caches created before concrete generic value shapes were validated.
`quazi.lock` alone stores dependency resolution. QZC is always safe to delete.
`[package].out_dir` changes the `build` root; it must remain inside the project.
