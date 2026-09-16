# Standard-library ownership migration status — 2026-09-15

Audience: maintainers continuing the D-014 ownership rollout.

## Scope and conclusion

The standard library has progressed through four safe, non-structural
migration slices: reusable `String` and `CString` cleanup, source-backed INI
replacement, and single-handle socket operations. This does **not** complete
the standard-library or D-014 plan. Full `std` test compilation remains blocked
by aggregate projections and owned collection elements that require structural
destruction and place-level move tracking.

## Completed slices

- `String.clear(&String!)` and `CString.clear(&CString!)` release their own
  raw allocation while retaining an empty reusable owner. Their `free(self)`
  hooks remain consuming destructors.
- `IniDocument.parse_string` and parser loop replacement paths use in-place
  cleanup rather than consuming a value that remains live.
- `TcpStream`, `TcpListener`, and `UdpSocket` I/O, shutdown, accept, and close
  use exclusive receivers; handle inspection is shared and `free(self)` stays
  consuming. UDP and TCP-listener lifecycle regressions exercise a real local
  bind, idempotent close, and final consuming cleanup.
- DNS parsing releases temporary `CString` storage through `clear()` on every
  error and success path. Scalar `Option[usize]` parser state is extracted once
  before later control-flow decisions.
- Source regressions now use `String.clear()` when they inspect a released
  string and no longer use an owner after consuming `free()`.
- `Map` and `Set` now use exclusive `clear()` for reusable backing-storage
  release and consuming `free(self)` for final destruction. Their scalar-only
  storage avoids claiming generic element destruction.

## Remaining boundary

The current `qz test <focused-name>` invocation still compiles all `std`
sources before it can run one test. Its remaining ownership diagnostics are
not socket or string-receiver failures. They concentrate in:

1. `Headers` and HTTP request/response code, which move `Header` or `Headers`
   fields and indexed owned elements. This requires borrowed element access,
   place-level moves, and exact-once container destruction.
2. `UdpDatagram.address`, which attempts to move an owned aggregate field and
   needs a supported borrowed-result or consuming aggregate design.
3. Legacy `Child` tests, whose control flow treats potentially consuming
   `wait`/`try_wait` paths as reusable. Their contract must be reconciled with
   D-011 rather than weakened ad hoc.

Generic `Array` and `Box` do not gain an in-place `clear()` in this phase: a
generic operation that merely frees backing storage would leak or bypass
destruction of owned elements. `Map` and `Set` are the narrow scalar-slot
exception documented by their API; broad generic element cleanup belongs to
the coordinated structural-destruction implementation, not a receiver-name
exception.

In particular, `Array.get(&Array[T]) -> T` is presently a raw load. The
compiler now rejects owned-element calls with S10 rather than allowing it to
materialize a shallow second owner, but the API still cannot be used to inspect
an owned `Header` as though it were a borrowed element.
That would materialize a second owner, while `Array.set` cannot yet destroy a
replaced owned element. Do not work around the HTTP diagnostics by changing
receiver spelling or by cloning from `Array.get`; the required solution is a
borrowed-element/provenance mechanism plus exact-once generic element cleanup.

## Verification evidence

- `cargo test --offline docs::` in `quazistrap` passes all 13 documentation
  checks after the API and migration records.
- The pinned `qz test` front end reports no exclusive-receiver diagnostics for
  the socket lifecycle regression, no resolver-temporary diagnostics, and no
  diagnostics in the migrated String/raw-owner test sources. It cannot execute
  them until the remaining full-std structural diagnostics are resolved.
- `git diff --check` passed before every independent compiler-doc and std
  commit in this migration sequence.

## Required next sequence

1. Expand the constrained D-003 field-cleanup pilot into complete structural
   destruction with place-level move state, enum/array/generic glue, outer
   storage release, and migrated manual hooks.
2. Add borrowed collection-element and aggregate-projection support only with
   the matching loan/provenance checks.
3. Migrate `Headers`, HTTP values, and `UdpDatagram` against that mechanism.
4. Reconcile the `Child` state transition APIs/tests against D-011, then run
   the full standard-library harness and target-specific lifecycle tests.
