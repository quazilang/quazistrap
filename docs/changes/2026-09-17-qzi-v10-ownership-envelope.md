# QZI v10 ownership-artifact envelope

QZI v10 adds a mandatory ownership-artifact section and QZC v8 invalidates
older incremental caches. The canonical envelope binds the metadata, public
interface, symbolic call relocations, and bytecode with domain-separated
SHA-256 digests. Readers reject missing, malformed, reordered, or stale
envelopes before linking.

The initial envelope state is explicitly unverified. It is not a serialized
`CallableOwnershipSummary`, ownership certificate, or permission for borrowed
values to cross a QZI boundary. D-014 still requires whole-program effects,
provenance, escape facts, and a verifier before QZI-only libraries can take or
return safe references.

Existing compatible QZI v2-v9 files remain readable as legacy artifacts, but
they have no ownership envelope. Regression tests cover round-tripping,
digest tampering, canonical section order, legacy decoding, and QZC format
invalidation.
