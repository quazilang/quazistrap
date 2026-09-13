# Imported inherent-method dispatch

Date: 2026-09-10

Audience: Quazi application developers and compiler maintainers.

Calls to inherent methods on imported named types now compile correctly. The
compiler previously retained an imported display name such as `fs.File` while
its implementation chunk was indexed under the declared type name,
`File.close`. Analysis accepted a call such as `file.close()`, but code
generation then reported that no direct or trait method resolved it.

The resolver now uses the qualified name to find the public method, then records
the declared implementation-chunk key for reachability, direct dispatch, and
generic specialization metadata. It does not add a fallback search across
unrelated type names.

Compatibility: source previously accepted by analysis but rejected during code
generation now builds. No API syntax or runtime behavior changes.

Verification:

```bash
cargo test --offline imported_named_type_method_uses_the_declared_impl_chunk
cargo test --offline imported_generic_named_type_method_uses_the_declared_impl_chunk
```

The regression builds namespaced ordinary and generic types, then calls their
inherent methods from an importer without falling back to vtable dispatch. A
Windows x86-64 COFF smoke also imports `std.fs.File`, opens a file, and calls
`close()`.
