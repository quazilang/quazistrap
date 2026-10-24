# Preserve defining-source identity in alias bodies

Audience: language users, library maintainers and compiler developers.

Transparent aliases now store imported type identities from their defining
source. Recursive expansion no longer applies caller imports to those stored
bodies. Actual alias arguments continue to resolve before entering the alias's
lexical parameter frame. This separates two contexts previously handled by one
global callback, preparing the compiler for source-scoped imports.

The loader explicitly identifies its selected implicit prelude entry. Public
exports, including renamed types and wildcard reexport chains, remain available
to alias definitions; private prelude imports do not enter that implicit
environment. Generic parameters take precedence. Stored alias symbols and
`@repr(C)` callback representation stay synchronized without mutating parsed AST.
Dynamic trait aliases participate in name canonicalization.

Source paths and owning intervals use indexed lookup. Cancellation remains an
operational result, with bounded selector/alias work and bounded type traversal.
Malformed selector spans cannot contribute implicit identity and multiple
implicit prelude markers fail closed.

This is one prerequisite for the source visibility fix. Function signatures,
fields, expression annotations, generic call sites and raw source codegen still
need coordinated scoping. The direct private namespace leak remains reproducible;
unused unresolved aliases are not a completed visibility check. No new ownership,
effect, artifact or production-readiness claim follows.

QZC30 rebuilds earlier source caches. QZI14 remains unchanged and Unverified.
See [the developer contract](../internals/source-type-context.md) and
[migration guidance](../migrations/source-alias-context.md).

Verification:

- `cargo test --offline -q -- --test-threads=1`: 975/975. The strengthened
  wildcard/reexport fixture also passes the focused filter
  `cargo test --offline -q aliases_keep_defining -- --test-threads=1`: 1/1.
- Restoring the previous caller-remapping behavior makes
  `canonical_alias_bodies_do_not_borrow_caller_imports` fail. Restored corrected
  behavior passes. This is a focused mutation check, not an old compiler rebuild.
- Debug build and direct Clippy check pass; existing warnings remain.
- `python3 scripts/check_string_abi.py`: invalid UTF-8/runtime identity cases
  rejected through check/native/QZI/COFF; valid aliases, custom runtime layouts,
  source-removed QZI and runtime-root/cache switches pass.
- STD `python3 scripts/check_os_memory.py <debug-qz>`: 6/6. Quazifetch builds,
  runs, takes a 131-function exact cache hit and emits Windows COFF.
- `python3 scripts/check_arithmetic.py`, `python3 scripts/check_power.py`
  and `python3 scripts/check_named_arguments.py` pass, including native,
  source-removed QZI and COFF controls. Installed release String/cache controls
  also pass; installed quazifetch repeats the 131-function cache hit in 1ms.
- Independent read-only design and implementation review reconciled context,
  bounds, provenance and effective regression coverage. Its final wildcard
  test-strengthening suggestion is included.

Commands use the local Rust wrappers described in RESUME.md. Logs are under
`/tmp/quazi-type-context-*.log`. README/checklist percentages remain 73.2%
complete and 26.8% remaining: this prerequisite completes no additional roadmap
requirement.
