# QZI ownership certificates — implementation design

Audience: compiler developers.

Status: stages 1 and 2 are implemented (QZI format 17, ownership schema 2).
Stage 3 has started: verified records of QZI dependencies compose into source
parameter effects ([consumption](#consumption)). Rejection at safe ownership
boundaries and the remaining D-014 gaps below are open; see
[D-014](../decisions/whole-program-ownership.md#qzi-only-libraries).

## Source facts that exist today

The source pipeline now derives, per callable, the facts a certificate must
summarize:

- [typed place accesses](source-control-flow.md): reads, writes, moves and
  borrows of classified places (binding root, storage, field/deref/index
  projections), `Escape` for `ret place`/`ret &place`, and closure `Capture`;
- [`parameter_effects`](ownership-call-graph.md#parameter-effects): parameter
  slots read, written (and written through a reference), moved, borrowed,
  returned by value or as a borrow, captured, global writes, and an
  `unmodeled` count plus `callee_unmodeled` for anything not covered;
- fixed-point composition of callee writes through references and of results
  handed out by `ret callee(p)`.

These map onto D-014's required summary contents as follows:

| D-014 item | Source fact | Gap |
|---|---|---|
| 1. capabilities, reads, writes, consumption, destruction | signature capabilities; `read`/`written`/`moved`/`borrowed` | destruction of overwritten values and drop effects |
| 2. result ownership and reference provenance | `returned`/`returned_borrow` (direct and through returned calls) | provenance through other expressions (result retention covers parameter roots) |
| 3. escape edges | `ret`, closure captures, global writes | safe code cannot store a reference in a struct field, enum payload, generic argument, owned local aggregate or closure capture (S10; `safe_code_cannot_store_references_beyond_a_call`), so the remaining channels are raw pointers in `unsafe` code, `str` views (result retention) and foreign/task boundaries |
| 4. closed indirect targets, transitive export effect | `unmodeled`, `callee_unmodeled`, composition | closing indirect/trait targets |
| 5. exported type layout, move and drop facts | existing QZI interface layout | move/drop facts per exported type |

## Certificate record

One record per exported callable and specialization, in canonical order:

- the qualified callable identity and specialization (`OwnershipCallableId`);
- receiver/parameter capabilities and result ownership;
- the effect sets above as sorted slot lists, plus escape kinds;
- `complete`: true only when `unmodeled == 0` and `callee_unmodeled` is false
  and every indirect target is closed;
- a content binding: a hash over the callable's public interface entry and the
  bytes of every chunk the record covers.

The records live in a new QZI section under a new major QZI version, with a
matching QZC version bump, per D-014.

## Verification

D-014 requires that the downstream compiler verify the summary against the
retained bodies instead of trusting it. The proposed verifier recomputes a
conservative summary from bytecode and accepts the record only if the record
is a superset (every recomputed effect is claimed):

1. **Register provenance.** Track which registers hold a parameter value or an
   address derived from one (field offsets, `Lea`, pointer arithmetic).
2. **Accesses.** A store through a parameter-derived address is a write
   through a reference; a load is a read; passing a parameter-derived register
   to a callee composes that callee's verified record for the slot.
3. **Escapes.** Returning a parameter-derived address is a returned borrow;
   storing one into a heap object, global or closure environment is an escape.
4. **Unknowns.** Indirect calls without closed targets, foreign calls and
   unclassified instructions make the recomputed summary incomplete, which a
   `complete` record cannot claim.

Bytecode verification is independent of the source pipeline, so it is
validated differentially first: for every source-built program in the test
suites, the bytecode summary must be no smaller than `parameter_effects`.

## Staged plan

1. Bytecode provenance analysis computing the effect sets for compiled
   chunks, with differential tests against `parameter_effects`. Started:
   `src/bytecode/effects.rs` runs a flow-sensitive dataflow over each chunk
   (per-register parameter-slot masks; `Lea` of the frame's own registers is
   local), records loads, stores through parameter-derived addresses,
   returned and stored slots, direct calls with per-argument masks and
   arguments to unknown targets, and composes callee writes through
   parameters to a fixed point. Its tests compile programs without inlining,
   map register slots to source parameters through `fn_value_layouts`, and
   require the bytecode writes and returned slots to cover the source
   `written_through_reference` and `returned_borrow` sets.
2. The QZI certificate section and canonical encoding, with serialization
   tests and the major-version/QZC bump. Done: see
   [Encoding (schema 2)](#encoding-schema-2).
3. Verification on import (done: every decode recomputes the records) and
   consumption (started: [consumption](#consumption)); a missing, malformed,
   stale or incomplete record at a safe ownership boundary is rejected with a
   rebuild diagnostic once a safety rule depends on these effects.
4. Migration documentation and fixtures for previous QZI versions, which stay
   rejected wherever verified metadata is required.

Public generics stay source dependencies (D-014) until templates gain an
equally complete specialization/effect contract.

## Encoding (schema 2)

QZI format 17 requires the embedded module's ownership section in schema 2:

- `u16` schema 2, `u8` state 1, the four content-binding digests of schema 1
  (metadata, interface, relocations, bytecode);
- `u32` record count, equal to the number of function bodies, then one record
  per body in bytecode order: `u16` parameter register count and five `u64`
  slot sets — `reads`, `writes_through`, `returned`, `stored`, `unknown`.

Slot sets index parameter *register* slots (a multi-slot parameter or a
hidden sret pointer occupies several), with slots past 63 sharing bit 63.
A record is complete when `unknown` is empty.

Records come from `chunk::ownership_records`: `bytecode::effects` per chunk,
then `effects::compose` over direct calls. A call resolves through its
relocation symbol to *every* body of that name, so target-specific bodies of
a universal QZI are combined conservatively; a symbol without a body is an
unknown target. Composition carries a callee's reads, writes through, stores
and unknown uses of a slot to the slots of the matching caller argument.

The certificate is a deterministic function of the bytecode and relocations.
Serialization therefore regenerates it from the final payloads (namespace
aliasing or target projection re-certify instead of rebinding a proof), and
decoding recomputes it and rejects any difference as stale or forged. Schema 1
modules (QZI module versions 10–14 outside a format-17 container) still decode
as `Unverified`; format 16 containers are rejected with a rebuild request.

Not yet covered by records: source-level consumption/destruction, closure
captures and escapes beyond `stored`, closed indirect targets and exported
type move/drop facts (the gap table above). Exported callables are not
distinguished from private ones; every body has a record.

## Consumption

When a project loads a QZI dependency, `chunk::certified_effects` maps each
body's verified record by its (aliased) chunk name — the namespaced name the
consumer calls. Same-named bodies (target copies, or one name provided by
several dependencies, such as a bundled submodule) are combined by union;
bodies with different slot counts combine into an unverified record. The map travels through
`ModuleResolver`, `LoadResult` and `analysis::analyze_program_with_dependencies`
to the analyzer, which gives bodiless non-generic callables
[source parameter effects](ownership-call-graph.md#parameter-effects) from it.
Composition then carries, for example, a QZI function's write through a
reference into its source callers. A dependency without a certificate
contributes an unverified record (every slot unknown) for each of its bodies,
so no verified record of the same name from another dependency can stand in
for it and its callers stay `callee_unmodeled` (fail closed).
No safety rule consumes parameter effects yet; until one does, rejection of
uncertified dependencies has nothing to protect.
