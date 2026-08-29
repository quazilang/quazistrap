# Compiler checkpoint resume

Audience: the next assistant continuing the current `feat/test` worktree.

## Read first

- `/home/nam/Projects/qz/RESUME.md`
- `/home/nam/Projects/qz/AGENTS.md`
- `/home/nam/Projects/qz/quazistrap/AGENTS.md`
- `/home/nam/Projects/qz/quazistrap/src/semantic/AGENTS.md`
- the full active objective linked from the workspace resume

Do not commit this checkpoint unless the user explicitly authorizes the exact
commit scope. Preserve the many unrelated/prior dirty changes.

## Implemented in this checkpoint

- Added internal parser AST `TypeKind::Error`; source syntax cannot produce it.
- Preserved trait parameter names and full method signatures for dynamic calls
  and QZI source interfaces.
- Replaced prelude trait placeholder `any` results with `Self`.
- Rejected runtime `any` in functions, fields, enums, traits, aliases, casts,
  locals, globals, impl targets, and generic arguments.
- Restricted `any` to a final variadic parameter on an explicit `@format`
  function; its body cannot access the pseudo-parameter.
- Removed universal `any` and broad named-type compatibility.
- Added contextual closure typing for typed bindings, returns, assignments,
  constructor payloads, and positional or named arguments across bare/module
  functions, function values, inherent methods, and dynamic trait methods.
- Made `Some`/`None`/`Ok`/`Err` inherit surrounding `Option`/`Result` types,
  including closure payloads, and added a successful-analysis invariant that
  rejects unresolved `Error` types before codegen.
- Recovered concrete builtin enum payload types in match bindings.
- Added dynamic trait signature preservation and object-safety checks.
- Added a codegen invariant rejecting `Any`/`Error` expression annotations.
- Added public QZI rejection for runtime `any`, while allowing erased `@format`
  tails and preserving trait receiver parameter names.
- Added user docs, migration/change records, decision D-010, and audit status.

## Current verified state

- Builtin constructor symbols now use internal `Error`, and contextual
  constructor analysis happens before payload analysis. Constructor identity is
  gated on the actual zero-span builtin symbol rather than a name suffix.
- Bare/called unconstrained `None` now carries `Option[Error]`; the general
  empty-enum generic wildcard was removed. Direct typed `None` remains valid.
- `TraitMethodSignature` now stores method generics; trait symbols store trait
  generics.
- `validate_trait_impl_conformance` was added. It substitutes trait generics and
  `Self`, assumes an implicit receiver when the trait declaration omits one,
  requires every method, and checks exact runtime parameter/return shapes.
- Dynamic calls reject generic traits, generic methods, non-receiver `Self`, and
  `Self` returns.
- Trait conformance rejects an unsafe implementation of a safe trait method, so
  dynamic dispatch cannot bypass the unsafe-call boundary.
- Direct named function arguments receive contextual parameter types, and
  named-argument validation reuses the expected type.
- Trait conformance compares the runtime signature and excludes the
  compiler-erased final `@format ...args: any` pseudo-parameter.
- QZI rejects public generic methods without source template bodies, imports
  impl-only modules into materialized gateways, preserves v7 trait receiver
  names, and explicitly rejects ambiguous parameterized trait interfaces or
  public runtime-`any` interfaces from v6.
- Linux thread spawn checks allocation and `pthread_create`, frees on failure,
  and returns zero. Joining zero is a no-op on Linux and Windows.
- Shared references now use a conservative lexical model: directional and
  invariant compatibility, local/parameter-only address-of, no return/owned
  storage/rebinding/closure capture, and function-long owner borrow protection.
- Scalar address-of emits explicit one-slot `Lea` metadata; every current
  codegen `Lea` has a nonzero block length. QZI rejects implicit metadata and
  QZC v4 invalidates stale exact-hit caches and pre-ownership closure chunks.
- Ref-to-ref casts preserve exact runtime shape. Shared-reference method calls
  are rejected for direct, dereferenced, and parameter receivers until receiver
  mutability exists. Generic arguments and match results cannot manufacture
  references, and inferred owned locals participate in move-while-borrowed
  checks.
- Legacy QZI v2-v5 bytecode is scanned for missing `Lea` metadata before it can
  enter the linker. Register compaction fills low free slots around high pinned
  registers without saturating or aliasing.

## Verification completed

```text
cargo test --offline semantic::tests
cargo test --offline bytecode::interface::tests
cargo test --offline
git diff --check
cargo fmt -- --check
```

The latest full suite passed 425/425. `git diff --check` passes. `cargo fmt --
--check` remains red on
pre-existing dirty formatting differences; do not reformat unrelated files.

`cargo fmt -- --check` already had unrelated formatting failures in
`backend/target.rs`, `lexer/mod.rs`, `loader.rs`, and `parser/format.rs`, plus
some prior dirty compiler sections. Do not run whole-repo formatting blindly;
format only owned lines/files without rewriting unrelated user changes.

```text
cd examples/32-testing
../../target/debug/qz test --no-color
```

This passed 6/6. The current CLI also built `/tmp/quazi-thread-project` as Linux
ELF and Windows COFF objects. `examples/04-closures` also checks, builds, and
runs with explicit closure binding types. A native register-pressure program
under `/tmp/quazi-reference-project` also returns success through `&local`.

## Remaining follow-up

- Final read-only semantic and register-allocation re-reviews challenged the
  reference checkpoint. Their cast, method, inferred-owner, generic/match,
  legacy-QZI, high-pinned-register, regression-quality, and documentation
  findings were implemented and covered by focused tests. The semantic review's
  final aggregate-alias finding was also fixed, and both reviewers confirmed no
  remaining blockers in the reference checkpoint scope.
- Historical QZI v2-v5 compatibility still lacks immutable golden files. V1
  omitted required frame metadata and now fails explicitly; v6 trait ambiguity
  also fails cleanly instead of guessing.
- Decide separately whether trait declarations should carry `@format` metadata
  for variadic formatting through `dyn Write`; current conformance correctly
  compares erased runtime shape, but this user-facing dynamic-call capability is
  not established.
## Closure ownership checkpoint

The closure P0 is now resolved conservatively:

- `fn` values are affine environment owners. Passing, returning, and assigning
  transfer ownership; calls borrow; self-assignment is rejected.
- Scope exit, replacement, discarded function temporaries, immediately called
  temporaries, and consumed function parameters emit environment cleanup.
- Returned function owners are deactivated in the producer and freed by the
  caller. Assignment statements no longer free the newly stored environment.
- Closure chunks reserve hidden environment r0 and all user parameter registers
  before allocating capture registers. Nested closure IDs are unique and
  repeated named-function values share one forwarder.
- Runtime callable signatures are exact. Captures, closure parameters, and
  results are restricted to immutable plain scalars; owned `fn` values cannot be
  nested in aggregates or supplied as generic arguments until recursive cleanup
  exists.
- Conditional moves, function-valued match results, and consuming assignment
  expressions are rejected until cleanup state is path-sensitive. Same-shape
  casts propagate ownership transfer.
- QZC is v4 and excludes chunks that depend on synthetic closure/forwarder
  companions from partial restoration. QZI v7 is the affine ownership boundary;
  pre-v7 public callable contracts and legacy synthetic chunks require rebuild.

Verification: `cargo test --offline` passes 425/425; the closure example runs;
the native closure-pressure project runs successfully on Linux and emits a
verified x86-64 COFF object for Windows; example 32 passes 6/6 tests; example 29
checks and builds. Hugo-friendly language, memory, migration, change, audit, and
maintainer Markdown has been updated without adding site machinery.

The next audited P0 is generic `Array[T]`/`Box[T]` one-word storage and recursive
destruction. Three independent closure reviews (code generation, semantics, and
test design) completed their post-fix passes with no release blockers. Suggested
future hardening includes a true two-file partial-cache link/run fixture and
allocator instrumentation, but neither blocks this conservative checkpoint.

The generic-storage investigation has started but implementation is intentionally
paused at maintainer decision D-001. Native evidence confirms ordinary named
structs survive `Array`/`Box` as heap handles; fixed `[i32; 3]` elements lose all
but the first register. Owned handles remain unsound because `get` shallow-copies,
`set` does not drop the replaced element, and `free` only releases the backing
buffer. Merely changing the stride cannot repair this. The short-term safe route
restricts canonical prelude `Array[T]`/`Box[T]` to plain-copy slot types but
breaks `std.net.Headers`; the complete route requires runtime layout metadata,
multi-register generic ABI, borrowed/cloning access, take semantics, and
recursive drop glue with a QZI/cache boundary. The baseline audit and D-001
Markdown now state this corrected evidence.
