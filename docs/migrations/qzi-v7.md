# Migrating compiled libraries to QZI v7

Audience: maintainers distributing compiled Quazi libraries.

QZI v7 adds an instruction flag for unsigned division, remainder, and ordered
comparison and preserves trait method parameter names in public source
interfaces. It is also the affine function-value ownership boundary. Rebuild
`.qzi` artifacts with the current compiler. The v7 compiler
continues to read compatible v2-v6 bytecode (now locked by immutable golden
fixtures from real historical writers), with two era caveats: v2 artifacts
carry no chunk flags, so their intrinsic wrapper chunks lose compilation-local
symbol scoping, and v3 artifacts keep `@api` imports as string metadata
covered by the scalar legacy lowering but never persisted `@export` symbols.
The reader rejects v6 trait interfaces
that declare parameters: v6 replaced `self` and every other parameter name with
`argN`, so consuming them under v7 could silently change receiver arity. Publish
that dependency as source if rebuilding it is not possible. The reader also
rejects v6 public interfaces containing runtime `any` before semantic analysis,
because direct QZI loading must not bypass the unrepresentable-type boundary.
Pre-v7 artifacts with public `fn` contracts or generated closure/forwarder
chunks are rejected because their callers and callees do not implement the v7
environment cleanup contract.
Older compilers reject v7 because they cannot preserve its integer semantics.

Every v7 `Lea` instruction must now carry an explicit nonzero address-taken
register-block length. This includes length `1` for a scalar local reference.
Artifacts with implicit zero metadata can contain already-reused stack slots,
so the reader cannot repair them and requests a source rebuild. Incremental QZC
is now v4, covering both address metadata and closure ownership changes.

Version 1 cannot be loaded safely. Its historical writer omitted function
parameter counts and register-frame sizes, so a current backend cannot recover
the calling convention or validate register operands. The reader reports an
explicit source-rebuild error instead of guessing. The pre-QZI artifact used
the separate `VBC` magic and is not a current QZI input format.

No Quazi source changes are expected. If a dependency is distributed as source,
normal compilation produces v7 automatically. If it is distributed as a QZI
artifact, replace the artifact and update any recorded checksum or lockfile
entry in the consuming project.

QZI still does not carry generic template bodies. Public generic functions,
types, traits, or methods must be distributed as source. Public runtime `any`
signatures are also rejected because `any` has no portable tagged
representation; see [the runtime `any` migration](runtime-any.md).
