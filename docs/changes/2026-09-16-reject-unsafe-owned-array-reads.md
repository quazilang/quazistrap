# Reject unsafe owned contiguous-container reads

Audience: Quazi users and standard-library maintainers.

A shared accessor on any `@contiguous_elements` container remains available
for Plain element types. Returning that container's owned element type by
value is an S10 error: the current raw-load implementation would create a
second shallow owner of the element's resources. The compiler derives this
from container metadata, resolved receiver/result types, and declared
element-parameter position; it does not recognize `Array`, `get`, or a
first-generic-parameter convention. This deliberately conservative boundary
also rejects same-shaped shared methods until the compiler-private element
address operation can prove a result's provenance.

This is a safety boundary, not a replacement collection API. Reading owned
elements safely requires the planned provenance-tracked borrowed-element
operation, and replacement/removal/freeing require exact-once element cleanup.
Existing APIs that depend on owned contiguous-container element reads,
including HTTP headers, remain unavailable until that coordinated
implementation lands.

Likewise, `Array.set` is currently `unsafe`: replacing an initialized slot
would otherwise overwrite an owned element without its exact-once destructor.
The eventual generic replacement operation will restore a safe exclusive
receiver API after it can destroy the prior element before storing the new one.
