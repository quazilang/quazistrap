# Tutorial runtime-boundary guidance

Audience: Quazi users and documentation maintainers.

The final tutorial chapter now directs users to the shipped monotonic-time,
blocking-network, experimental-thread, and diagnostics contracts. It also
states that child-process execution has no supported public API yet, rather
than implying shell execution or undocumented runtime internals are usable.

Compatibility: documentation-only. No language or library behavior changed.

Verification:

```text
cargo test --offline docs::tests
```
