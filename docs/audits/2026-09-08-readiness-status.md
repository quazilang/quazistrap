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

## Completed recent checkpoints

- Historical QZI fixtures are tracked despite the general generated-bytecode
  ignore rule, restoring a compilable test baseline.
- Tree-sitter conformance rejects parser errors and covers compiler-supported
  multi-index and semicolon-separated declaration syntax.
- LSP workspace symbols index parseable local files beneath selected roots;
  open buffers override disk snapshots. Relative leaf imports resolve a unique
  public target declaration, with all broader imports deliberately excluded.

## Not complete

The production-readiness plan is **not complete**. The following requirements
remain unproven or explicitly deferred:

| Milestone | Current status | Required next evidence |
| --- | --- | --- |
| Process API | Deferred by D-011 because safe argument/handle marshalling belongs in compiler/runtime support. | Approved runtime contract, Linux/Windows implementation, failure and cleanup tests. |
| Civil time | Only monotonic `Duration`/`Instant` are shipped. | Calendar, UTC, zone, ambiguity, and serialization contract plus deterministic tests. |
| Concurrency | Native thread primitives remain experimental. | Structured lifetime, result/error/panic propagation, cancellation policy, synchronization contract. |
| Serialization | Bounded scalar decode and limited `Serialize` exist; derived `Deserialize`, options, collections, and nested structs do not. | Receiverless decoding design and bounded object policy with compiler/std tests. |
| Standard-library tests | `qz test` collects every `src/*.qz` file as an independent root. That bypasses the target-gated `std.windows` import, so Linux runs fail on Windows-only FFI declarations before INI tests execute. | Make test-root discovery respect target-gated module reachability, then add Linux and Windows regression coverage for the standard-library test command. |
| LSP | Workspace symbols and a narrow relative definition path exist. | Loader-backed source map for package/std imports, cross-file references/rename, cancellation, code actions, inlay hints, real-editor smoke coverage. |
| Documentation | API coverage is substantially improved but tutorial, language specification, and guide requirements are incomplete. | Runnable progressive tutorial, exhaustive supported-language reference, checked links/examples. |
| Editors | Local integration repositories exist. | Versioned runtime smoke validation for each supported editor. |

No open item should be closed solely because current compiler tests pass: each
needs the specific contract and evidence described above.
