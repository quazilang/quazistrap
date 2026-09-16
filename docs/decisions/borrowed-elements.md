# D-015: borrowed dense-container elements

Audience: Quazi maintainers.

Status: accepted implementation contract, 2026-09-16. This refines the
generic-layout direction in D-001 and the provenance requirements in D-014;
it does not claim that the compiler support is implemented.

## Decision

A safe borrow of a generic dense-container element is a compiler-validated
container contract. It must be declared through the existing
`@contiguous_elements` storage metadata, not inferred from an `Array` name, a
method spelling, or a field layout.

An accessor participating in this contract has a shared receiver, one integer
index, and returns exactly `&T`, where `T` is the element parameter of the
annotated container. Its body may use only the compiler-private checked dense
element-address operation. That operation is tied to the annotated pointer,
length, and element layout, so it performs bounds checking and cannot be
recreated by a safe raw-pointer cast. Other containers with a coincidentally
similar shape receive no special treatment.

## Ownership and escape rules

The returned reference carries its container root, element projection, and
index provenance. The first implementation slice accepts only a local or
parameter as that root. Temporary values, field projections, indexed roots,
call results, foreign values, and QZI-only references are rejected until the
whole-program effect and summary model can prove their lifetime.

While the borrowed element is live, its root has a shared loan. Safe code may
read it, including its length, but may not move, free, assign, replace,
reallocate, or obtain an overlapping exclusive borrow. Borrowed elements may
only cross calls whose resolved capability summaries accept the matching shared
reference. They may not be stored, captured, returned through an unknown call,
or passed across an opaque/dynamic/foreign boundary. The initial region is
lexical; later D-014 effect work may prove shorter regions without weakening
these rules.

## Lowering and compatibility

Loading an element copies its representation and therefore cannot implement an
element borrow. The compiler needs a distinct checked element-address lowering
that receives the storage base, index, and monomorphized stride. Adding that
operation changes QZI/QZC executable semantics. It must land with the D-014
artifact-version and ownership-summary work, including rejection or source
rebuild of older artifacts; a partial opcode is not a compatible intermediate
state.

## Mutation and destruction follow-up

Mutating APIs require an exclusive or consuming container capability. A
replacement destroys the prior owned element exactly once after a successful
replacement; removal transfers the removed owner exactly once and destroys
every remaining owner exactly once; container destruction recursively destroys
all remaining elements exactly once. These rules apply to every concrete
specialization, including nested containers and multi-slot elements.

## Required evidence

- Owned, plain, multi-slot, and nested element borrows use the address path,
  never a value-load followed by a local reference.
- A live element borrow freezes every mutating or consuming operation on its
  root and allows independent shared reads.
- Temporary, projected, returned, captured, opaque-call, and foreign escape
  attempts are rejected unless future D-014 summaries prove them safe.
- A same-shaped but uncontracted container cannot obtain the privileged safe
  address operation.
- Replacement, removal, and final destruction are checked for exact-once
  behavior on Linux and Windows for owned and nested elements.
