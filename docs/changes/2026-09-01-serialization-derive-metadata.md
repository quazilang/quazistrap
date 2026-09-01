# Serialization derive metadata

Audience: language users and compiler maintainers.

## Change

The semantic report now records stable, ordered metadata for a struct carrying
`@derive(Serialize)`, `@derive(Deserialize)`, or both. Each record includes the
requested derive traits, declared generic parameters, union status, field order,
alias-resolved field types, optional `@json(name="...")` wire names, and a
parser-independent copy of every field attribute.

This is compiler-internal infrastructure for the static JSON derives selected
in [D-012](../decisions/serialization.md). It does not add runtime type
reflection, comptime execution, generated `impl`s, a public `std.codec` trait,
or working struct serialization yet.

## Validation

`@json` is reserved for a field of a struct that requests one of the JSON
serialization derives. Its only accepted form is a single non-empty string
name:

```quazi
@derive(Serialize, Deserialize)
struct AddArgs {
    force: bool,
    output: String @json(name="out"),
}
```

The compiler now rejects misplaced or malformed `@json`, multiple `@json`
attributes on one field, duplicate serialization derives, and duplicate
effective JSON keys (including a renamed field colliding with another field's
default name). `Serialize` and `Deserialize` currently diagnose generic
structs and unions as unsupported; their first supported shape remains a
non-generic named struct.

## Compatibility and migration

No generated implementation exists at this checkpoint, so no existing program
gains a serialization method. Programs that used `@json` as an unrelated opaque
field attribute must rename that attribute; it is now reserved for the planned
JSON derive surface. Existing non-serialization custom attributes remain
opaque metadata.

## Verification

Focused semantic tests cover ordered metadata, opaque attribute retention,
type-alias resolution, malformed/misplaced attributes, duplicate keys and
derives, generic structs, and unions. The broader compiler test suite remains
the integration gate before a release.
