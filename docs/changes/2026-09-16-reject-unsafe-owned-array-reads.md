# Reject unsafe owned Array reads

Audience: Quazi users and standard-library maintainers.

`Array.get` remains available for Plain element types. Calling it for an owned
element is now an S10 error: the current raw-load implementation would create
a second shallow owner of the element's resources.

This is a safety boundary, not a replacement collection API. Reading owned
elements safely requires the planned provenance-tracked borrowed-element
operation, and replacement/removal/freeing require exact-once element cleanup.
Existing APIs that depend on owned `Array` element reads, including HTTP
headers, remain unavailable until that coordinated implementation lands.
