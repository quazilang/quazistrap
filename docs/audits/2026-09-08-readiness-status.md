# Production-readiness status — 2026-09-08

Audience: maintainers planning the next local checkpoints.

## Evidence reviewed

- Compiler repository test suite: 510 tests pass after restoring the tracked
  immutable QZI v2–v6 fixtures and verifying their recorded SHA-256 values.
- Tree-sitter corpus and workspace conformance are passing; the grammar now
  checks maintained compiler and standard-library `.qz` sources.
- The contained LSP has 41 focused tests covering synchronization, symbols,
  local workspace indexing, path boundaries, and relative-import definition
  behavior.
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

## Not complete

The production-readiness plan is **not complete**. The following requirements
remain unproven or explicitly deferred:

| Milestone | Current status | Required next evidence |
| --- | --- | --- |
| Process API | Deferred by D-011 because safe argument/handle marshalling belongs in compiler/runtime support. | Approved runtime contract, Linux/Windows implementation, failure and cleanup tests. |
| Civil time | Only monotonic `Duration`/`Instant` are shipped. | Calendar, UTC, zone, ambiguity, and serialization contract plus deterministic tests. |
| Concurrency | Native thread primitives remain experimental. | Structured lifetime, result/error/panic propagation, cancellation policy, synchronization contract. |
| Serialization | Bounded scalar decode and limited `Serialize` exist; derived `Deserialize`, options, collections, and nested structs do not. | Receiverless decoding design and bounded object policy with compiler/std tests. |
| LSP | Workspace symbols, loader-backed definitions, and loader-backed cross-file references exist for local/package/std imports. Rename produces one atomic workspace edit only when every resolved occurrence is inside client-negotiated workspace roots. Compiler-backed inferred type hints are available for unannotated local declarations, and W03 single-selector imports offer a safe removal quick fix. The transport honors pending-request cancellation. | Cooperative cancellation of CPU-bound compiler work and runtime smoke coverage beyond Neovim. |
| Documentation | Language specification (9 pages), progressive tutorial (10 chapters), and practical guides (4 guides) are now written. API coverage accounts for every `std` module. Offline compiler tests validate repository-local Markdown paths and heading fragments; every tutorial Quazi fence now visibly identifies itself as runnable, contextual, or intentionally invalid, and runnable fences plus complete chapter fixtures are compiler-checked through bytecode lowering. | Continue correcting any newly discovered API drift; fragments are explicitly scoped rather than treated as standalone programs. |
| Editors | Local integration repositories exist. Neovim 0.12 has a headless real-client smoke that opens a `.qz` project, starts `qz lsp`, requests hover, and performs the shutdown/exit protocol path. | Versioned runtime smoke validation for VS Code, Zed, and Helix; Zed and Helix runtimes are unavailable locally. |

No open item should be closed solely because current compiler tests pass: each
needs the specific contract and evidence described above.
