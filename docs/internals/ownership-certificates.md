# QZI ownership certificates — implementation design

Audience: compiler developers.

Status: design for the remaining D-014 artifact requirement. Nothing here is
implemented yet; current QZI keeps the unverified ownership envelope described
in [D-014](../decisions/whole-program-ownership.md#qzi-only-libraries).

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
| 3. escape edges | `ret`, closure captures, global writes | aggregate/heap storage of parameter-derived references, foreign and task boundaries |
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
   chunks, with differential tests against `parameter_effects`.
2. The QZI certificate section and canonical encoding for exported callables,
   with serialization tests and the major-version/QZC bump.
3. Verification on import; a missing, malformed, stale or incomplete record at
   a safe ownership boundary is rejected with a rebuild diagnostic.
4. Migration documentation and fixtures for previous QZI versions, which stay
   rejected wherever verified metadata is required.

Public generics stay source dependencies (D-014) until templates gain an
equally complete specialization/effect contract.
