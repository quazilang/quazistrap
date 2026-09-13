# Complete guarded match-arm diagnostic spans

Parser spans for guarded match arms now include the expression after `=>`.

## Why

The parser previously ended a guarded arm at its guard expression. Semantic
diagnostics that identify an arm—such as a mismatched result type or an
unreachable arm—therefore highlighted only `pattern if guard`, omitting the
result expression responsible for the diagnostic context.

## Compatibility

This changes diagnostic source ranges only. Quazi syntax, parsing, and runtime
behavior are unchanged. Editors and diagnostic consumers now receive the full
`pattern if guard => expression` span.

## Verification

Parser and semantic regressions assert that guarded-arm spans end at the
right-hand expression and that a result-type mismatch includes that expression.

```text
env RUSTC=$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc \
  $HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo test --offline \
  guarded_match_arm
```
