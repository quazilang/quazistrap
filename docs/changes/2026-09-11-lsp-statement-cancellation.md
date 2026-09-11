# LSP statement-boundary semantic cancellation

The contained language server now cancels superseded semantic analysis while a
single function, method, or nested block is being checked.

## Why

Earlier cancellation checks stopped before each top-level declaration and
between whole semantic passes. A long function body could therefore keep an
obsolete diagnostic worker active until its entire declaration had been
analyzed.

## Behavior and compatibility

The existing typed checkpoint now flows through semantic item, block, and
statement traversal. It is polled before each reachable statement, including
statements in nested blocks. Cancellation is still an operational result, not
a source diagnostic; callers discard the partial analyzer state. Individual
expressions, filesystem reads, and pass-input snapshots remain atomic, so this
does not claim preemptive interruption of arbitrary CPU work.

Normal compiler analysis continues to use an infallible checkpoint and keeps
the same API and diagnostic behavior. No Quazi syntax or runtime behavior
changes.

## Verification

A semantic regression cancels a one-function program during its body and
asserts that not every expression was annotated. Run:

```text
env RUSTC=$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc \
  $HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo test --offline \
  cancellable_analysis_stops_inside_a_single_function_body
```
