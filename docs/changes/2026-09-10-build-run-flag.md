# Build `-r` flag documentation correction

Audience: Quazi users and documentation maintainers.

The tutorial and project guide previously described `qz build -r` as a release
build. The current CLI defines `-r` as `--run`: it executes the native artifact
after a successful build. Quazi does not currently expose a release-profile
optimization flag; `-s` only strips debug symbols.

Compatibility: documentation-only. The CLI behavior did not change.

Verification:

```text
qz build --help
cargo test --offline docs::tests
```
