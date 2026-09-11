# LSP semantic-loop cancellation

The contained language server now also cooperatively cancels superseded
document analysis while semantic passes traverse large collections and graphs.
Cancellation remains an operational result: it never becomes a source
diagnostic or a partial semantic report.

## Why

The earlier semantic cancellation checkpoint polled at pass and top-level-item
boundaries. Several passes can still do substantial work after entering one
pass: tree-shaking and inline recursion walk call graphs, exhaustiveness
checks walk match arms and enum variants, lazy-import hints process access
sets, generic-specialization closure expands dependency edges, and dependency
reporting copies the completed graph. An edit during one of those walks could
keep an obsolete LSP analysis worker occupied until the whole walk completed.

## Behavior and compatibility

Each of those collection and graph traversals now receives the existing typed
checkpoint and returns `Cancelled` immediately when it is observed. The normal
compiler analysis API remains non-cancellable, and completed analyses retain
the same diagnostics and optimization metadata. No Quazi source syntax or
language behavior changes; this is an LSP responsiveness improvement.

Individual filesystem operations, pass-input snapshots, lexer/parser
processing within one top-level declaration, and semantic checking within a
single top-level declaration remain atomic boundaries. Cancellation is
cooperative, not preemptive, so newly added long semantic traversals must
receive the same typed checkpoint before they can be described as
interruptible.

## Verification

Focused regressions cancel during tree-shake call-edge traversal and before
dependency-graph copying. The complete semantic suite confirms unchanged
completed-analysis behavior. Run:

```text
env RUSTC=$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc \
  $HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo test --offline semantic::
```
