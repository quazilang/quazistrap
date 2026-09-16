# Reject unsafe owned contiguous-container reads

Audience: Quazi users and standard-library maintainers.

A shared accessor explicitly marked `@contiguous_element_value_read` on any
`@contiguous_elements` container remains available
for Plain element types. Returning that container's owned element type by
value is an S10 error: the current raw-load implementation would create a
second shallow owner of the element's resources. The compiler derives this
from the compiler-validated accessor contract, container metadata, resolved
receiver/result types, and declared element-parameter position; it does not
recognize `Array`, `get`, or a first-generic-parameter convention.

This is a safety boundary, not a replacement collection API. Reading owned
elements safely requires the planned provenance-tracked borrowed-element
operation, and replacement/removal/freeing require exact-once element cleanup.
Existing APIs that depend on owned contiguous-container element reads,
including HTTP headers, remain unavailable until that coordinated
implementation lands.
