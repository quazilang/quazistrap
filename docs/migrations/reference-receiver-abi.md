# Safe-reference receiver ABI migration

Audience: Quazi users and library maintainers.

Rebuild source libraries using explicit borrowed receivers with the current
compiler. QZI v12 and QZC v12 establish a consistent slot-reference convention:
`&T` and `&T!` point to addressable value storage both as ordinary parameters
and as method receivers. Field operations read the aggregate handle from that
storage. Source syntax is unchanged. Shared `str` and `&str` retain their
representation-identical view contract.

Previously, forwarding a reference parameter to a struct method could corrupt
the caller's value because struct methods expected a raw heap handle. Direct
owned calls, forwarded references, aliases, and receiver-to-receiver calls now
use the same physical representation. Do not cast or duplicate owners to adapt
the old ABI manually.

Current readers still decode older supported QZI formats. Current linking rejects
pre-v12 reference/aggregate or unavailable contracts and opaque outbound call
boundaries unless compatibility is established. A parameterized legacy library
must retain actual callable declarations, not merely a nonempty interface
section. Closed scalar/raw-pointer/shared-string artifacts can remain compatible.
Namespace aliasing and reserialization preserve original ownership ABI generation;
rewriting an old artifact cannot upgrade its ABI. Rebuild from its source when
the compiler requests it. Incremental caches are invalidated automatically.

Exclusive methods on source fields now borrow the actual field slot, so scalar
mutation and nested owner replacement reach the original storage. Narrow C
fields, bitfields, and inline C aggregates cannot provide that slot and receive
S14. For plain scalar C fields, copy into a local variable, mutate the local,
then assign back explicitly; ordinary scalar qword fields support direct calls.
Fixed-array references retain their pointee address for indexing and mutation.

The ownership envelope remains `Unverified`. This migration does not certify
D-014 effects, complete structural destruction, or enable borrowed dense-element
APIs. The later [QZI v13 migration](qzi-v13-vtables.md) adds portable dynamic
vtable layouts. See the
[change record](../changes/2026-10-14-reference-receiver-abi.md) for evidence.
