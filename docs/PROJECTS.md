# Projects and `quazi.toml`

`quazi.toml` separates package identity from output artifacts. One package may
expose one library and several binaries.

```toml
[package]
name = "acme"
version = "0.1.0"

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

Legacy `package.type` plus `[build].entry` remains accepted only when artifact
tables are absent. New manifests use `[lib]` and `[[bin]]`. `qz new --lib` and
`qz init --lib` generate this form.

`--target x86_64-linux` and `--target x86_64-windows` select backend, output,
cache namespace, and linker. CLI linker wins over target `[link]`, then base
`[link]`, then defaults. Linux and Windows use in-process linkers for compiler
objects. Native libraries or custom linker flags select an external linker.
`libc = true` is explicit opt-in; no C runtime is inferred.

QZC means Quazi Compilation Cache. Each artifact/target snapshot lives at
`target/quazi/<target>/<artifact>/incremental.qzc`. It stores exact input hashes
plus linked QZI. `quazi.lock` alone stores dependency resolution. QZC is always
safe to delete.
