# Migrating compiled libraries to QZI v10

Audience: maintainers distributing compiled Quazi libraries.

QZI v10 adds a mandatory, canonical ownership-artifact envelope and advances
the incremental cache format to QZC v8. The envelope includes domain-separated
SHA-256 bindings for the module metadata, public interface, symbolic call
relocations, and bytecode. A reader rejects a missing, malformed,
non-canonical, or stale envelope before linking.

The only v10 envelope state currently emitted is explicitly **unverified**.
It is validation infrastructure, not a D-014 ownership proof: QZI-only calls
remain opaque for borrowed values until a later compiler emits and verifies
the required effect summary and certificate. Do not treat rebuilding to v10 as
permission to pass, return, or retain safe borrowed references across a QZI
boundary.

Rebuild distributed `.qzi` artifacts with the current compiler. Existing QZI
v2-v9 files remain readable for their compatible bytecode semantics but carry
no ownership artifact; QZC v7 caches are discarded automatically. No source
syntax changes are required.
