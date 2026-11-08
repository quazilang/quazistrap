# `qz check` analyzes every artifact

Audience: language users.

Without `--bin` or `--lib`, `qz check` analyzed only the default entry of a
project. In `examples/26-http-client-server` it therefore passed while the
`http-server`, `http-client` and `http-url-client` binaries failed to build.

`qz check` now analyzes the distinct entry of every `[lib]` and `[[bin]]`
artifact and fails if any of them has errors. `--bin <name>` and `--lib` still
check a single artifact. `--target` applies to every checked entry.

Verification: `ProjectConfig::artifact_entries` has a unit test over example 26;
a project whose second binary calls an unknown function now fails `qz check`
(the previous compiler exited 0), and `qz check --bin <first>` still passes.
