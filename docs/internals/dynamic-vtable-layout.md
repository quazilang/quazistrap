# Portable dynamic vtable layouts

Audience: compiler, runtime, and tooling contributors.

## Contract

A lowered `dyn Trait` value contains the concrete value in its first word and
a vtable address in its second. Borrowed trait receivers receive the first
word's address; value receivers receive its contents. The receiver ABI remains
the slot-reference convention introduced in QZI v12.

QZI v13 retains the table's exact layout in a target-neutral constant:
`VtableLayout { type_name, trait_name, methods }`. `methods` contains emitted
Quazi chunk symbols in semantic trait-slot order, not alphabetical order or
native symbol spellings. Codegen obtains that order from the analyzed trait.
Both source and artifact-only native emission consume these descriptors.
The backend does not reconstruct layouts from a `SemanticReport` or the public
interface, which can omit private types, traits, and implementation bodies.

The descriptor lives beside its bytecode use so cloning, inlining, and raw
incremental-unit serialization preserve it. A separate module section was
considered, but would require parallel plumbing through the chunk-only linker,
backend, and cache APIs. Keeping the descriptor with its constant also binds it
through the existing ownership-envelope bytecode digest without a second schema.

## Encoding and validation

Constant tag 8 encodes a nonempty type name, a nonempty trait name, a `u16`
method count, and that many nonempty method symbols. Each string is UTF-8 with
a `u16` byte length. There are at most 256 slots because `VtblLoad` addresses
slots with one byte. A zero-method marker trait is valid. Decoding checks
lengths, UTF-8, count bounds, and remaining bytes before allocating the method
list. Only an outer QZI generation of at least 13 admits this constant; the
sectioned container continues to use its established inner chunk encoding.

Encoding and raw cache decoding validate descriptor shape without requiring
all implementation chunks. Partial cache units can legitimately omit those
chunks. Complete QZI linking and native emission then require every method
symbol to exist. Repeated identical `(type, trait)` layouts share one native
table; conflicting ordered method lists fail explicitly, including conflicts
between different modules or caller chunks. The layout is executable metadata,
not a proof that a method satisfies an ownership or effect contract.

## Names, optimization, and native emission

Library namespace aliases rewrite matching prefixes in type identities, trait
identities, and every method symbol. Root-function qualification uses its exact
old-to-new function map for method symbols; it does not rename source type or
trait identities. Bare identities therefore retain the compiler's existing
program-wide naming policy. Distinct artifacts with conflicting bare identities
fail linking rather than silently choosing a table.

Descriptor method symbols count as function-address roots during dead-inline
elimination. Initial source reachability retains the observed trait target set
and ordinary dependencies; pre-WPO cache units reenter the same optimization
pipeline. QZC v13 invalidates both exact hits and reusable units from v12, which
could contain layout-free table addresses.

Native table names hex-encode the type and trait UTF-8 bytes separately. This
preserves punctuation and component boundaries rather than collapsing distinct
identities through underscore substitution. Table relocations resolve each
Quazi method through the backend's actual chunk-to-native-symbol map, including
unmangled and intrinsic/export spellings. Relocation errors propagate instead
of being ignored. Marker tables reserve one zero word to keep their symbol
addressable. Linux ELF and Windows COFF use the same descriptor and slot order.

## Public source interface boundary

Implementation bytecode remains available to descriptors even when its source
identity is private. The interface writer omits implementations with proven
same-source private nominal targets or traits and no explicitly public methods.
It keeps explicit public methods on public targets as inherent declarations
when the trait itself is private. Public methods on private nominal targets
instead produce an actionable export error: make the target public or consume
its source package. Previously these cases could emit declarations referring
to types or traits absent from the interface.

Privacy proof here uses the source-file identity and direct nominal declaration
name. Excluded dependencies, other files' same-spelled names, aliases, and
nested generic argument spellings do not establish private identity. They retain
the existing export checks rather than causing silent omission. General source
interface visibility closure through aliases and composite headers remains a
separate requirement; this local projection is not that resolver.

## Compatibility and remaining work

Historical tag-4 `VtableAddr` constants remain decodable for inspection. They
lack implementation order and fail current linking/native emission with
source-rebuild guidance. Unrelated compatible v12 artifacts retain their ABI;
reserialization preserves the original ownership ABI generation. See the
[v13 migration](../migrations/qzi-v13-vtables.md).

This change preserves already-lowered dynamic values. It does not add general
source coercions, generic dynamic implementations, ownership-certified QZI
effects, dynamic cleanup, or D-014/D-015 provenance support. Existing alias and
constant-local dynamic-construction gaps and unused-method warning precision
remain separate work. Windows object verification is not Windows host execution.
