# Production-readiness status — 2026-09-08

Audience: maintainers planning the next local checkpoints.

## Evidence reviewed

- Compiler repository test suite: 510 tests pass after restoring the tracked
  immutable QZI v2–v6 fixtures and verifying their recorded SHA-256 values.
- Tree-sitter corpus and workspace conformance are passing; the grammar now
  checks maintained compiler and standard-library `.qz` sources.
- The contained LSP has 52 focused tests covering synchronization, symbols,
  local workspace indexing, path boundaries, relative-import definition
  behavior, blocking-pool execution, and stale-overlay rejection.
- Canonical `std` API coverage includes the public INI contract. No compiler
  INI behavior was introduced; INI parsing remains entirely in `std.ini`.
- The canonical Linux `qz test ini` invocation passes all five source-level
  `std.ini` tests. Test-root discovery now keeps target-gated modules behind
  their normal import edges.

## Completed recent checkpoints

- Historical QZI fixtures are tracked despite the general generated-bytecode
  ignore rule, restoring a compilable test baseline.
- Tree-sitter conformance rejects parser errors and covers compiler-supported
  multi-index and semicolon-separated declaration syntax.
- LSP workspace symbols index parseable local files beneath selected roots;
  open buffers override disk snapshots. Relative leaf imports resolve a unique
  public target declaration, with all broader imports deliberately excluded.
- `qz test` adds source files as independent roots only when they declare an
  `@test`; all `tests/` files remain roots for diagnostics. This preserves
  discovery of unimported tests without loading target-disabled modules.
- LSP definition requests now use the compiler loader's configured import
  graph and effective per-file source map, including canonical open-buffer
  overlays, for local, package, and standard-library bindings.

## Follow-up evidence (2026-09-10)

- The standard-library socket owners now invalidate before issuing their native
  close operation. `TcpStream`, `TcpListener`, and `UdpSocket` ignore repeated
  `close()`/`free()` calls instead of forwarding the `-1` sentinel to Linux or
  Winsock. A source-level UDP regression binds an ephemeral local socket and
  verifies invalidation through explicit close, repeated close, and `free()`.
  This is a resource-safety correction, not evidence that the broader network
  timeout, streaming, or TLS milestones are complete.
- Prelude `Array[T]` and `Box[T]` now invalidate their raw pointers after an
  explicit release, making repeated `free()` calls no-ops. The compiler's
  generic-specialization closure now also carries ordinary helper and intrinsic
  calls from a generic template into each concrete specialization; this keeps
  `Array.push()`'s realloc path reachable. Source regressions cover repeat-safe
  release for `Array`, `Box`, `Map`, and `Set`. This corrects raw-owner release
  behavior only; element destruction and general aggregate ownership remain
  deferred.
- `Thread.join()` now invalidates its wrapper before the native join, preventing
  a repeated call from reusing released pthread/Windows-handle storage. The
  experimental concurrency contract remains incomplete because structured
  lifetime, result/panic propagation, cancellation, and synchronization policy
  still require a maintainer decision.
- The process-runtime decision now records the concrete public-contract gates:
  argument and executable lookup, exit-status representation, termination,
  close/destruction of a running child, and a multi-field spawn result ABI.
  This narrows the implementation path but intentionally does not approve a
  `std.process` API.
- Neovim and VS Code each have isolated real-client hover smoke coverage; Zed
  and Helix synchronize their copied grammar assets with the canonical
  Tree-sitter revision and verify that alignment statically. Their editor
  runtimes remain unavailable in this workspace.

## Not complete

The production-readiness plan is **not complete**. The following requirements
remain unproven or explicitly deferred:

| Milestone | Current status | Required next evidence |
| --- | --- | --- |
| Process API | D-011 accepts compiler/runtime ownership of child-process creation, but the public argument, exit-status, termination, close/destruction, and spawn-result contracts remain unapproved. | Maintainer-approved contract, Linux/Windows implementation, and failure/cleanup tests. |
| Civil time | Only monotonic `Duration`/`Instant` are shipped. | Calendar, UTC, zone, ambiguity, and serialization contract plus deterministic tests. |
| Concurrency | Native thread primitives remain experimental. | Structured lifetime, result/error/panic propagation, cancellation policy, synchronization contract. |
| Serialization | Bounded scalar decode and limited `Serialize` exist; derived `Deserialize`, options, collections, and nested structs do not. | Receiverless decoding design and bounded object policy with compiler/std tests. |
| LSP | Workspace symbols, loader-backed definitions, and loader-backed cross-file references exist for local/package/std imports. Rename produces one atomic workspace edit only when every resolved occurrence is inside client-negotiated workspace roots. Compiler-backed inferred type hints are available for unannotated local declarations, and W03 single-selector imports offer a safe removal quick fix. Compiler and workspace-index snapshots run on Tokio's blocking pool; loader snapshots are discarded if their included open buffers change or close, and save-time index writes are generation-checked. The transport honors pending-request cancellation. Neovim 0.12 and VS Code 1.133 have isolated real-client hover smokes. | Cooperative cancellation of CPU-bound compiler work and real-client feature-request coverage for Zed and Helix. |
| Documentation | Language specification (9 pages), progressive tutorial (10 chapters), and practical guides (4 guides) are now written. API coverage accounts for every `std` module. Offline compiler tests validate repository-local Markdown paths and heading fragments; every tutorial Quazi fence now visibly identifies itself as runnable, contextual, or intentionally invalid, and runnable fences plus complete chapter fixtures are compiler-checked through bytecode lowering. | Continue correcting any newly discovered API drift; fragments are explicitly scoped rather than treated as standalone programs. |
| Editors | Local integration repositories exist. Neovim 0.12 has a headless real-client smoke that opens a `.qz` project, starts `qz lsp`, requests hover, and performs the shutdown/exit protocol path. VS Code 1.133 has an isolated extension-test host that opens a temporary `.qz` file and requests its literal hover through the VS Code provider API. Zed and Helix pin the current canonical Tree-sitter revision and statically verify their copied highlight query against it. | Versioned runtime validation for Zed and Helix; their runtimes are unavailable locally. |

No open item should be closed solely because current compiler tests pass: each
needs the specific contract and evidence described above.
