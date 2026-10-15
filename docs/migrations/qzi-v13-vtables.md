# QZI v13 dynamic-dispatch migration

Audience: Quazi users and library maintainers.

Rebuild libraries and executables that contain `dyn` construction from source
with the current compiler. QZI v13 retains the ordered implementation symbols
needed to emit vtables without original source. Previously a dynamic executable
could compile from source but fail when its QZI was linked, because only the
source analysis report knew its table layout.

Source syntax and the v12 safe-reference receiver convention are unchanged.
Compatible artifacts without old layout-free table constants remain usable.
Artifacts with historical tag-4 table addresses receive an explicit
source-rebuild error; relabeling or reserializing them cannot recover missing
method order. Older compilers must reject the new QZI generation. QZC v13
automatically invalidates older exact hits and reusable codegen units.

Public methods of a same-source private nominal target now receive an
actionable QZI export error instead of producing an unusable interface. Make
the target public or distribute source. Explicit public methods on public
targets remain in the interface even when their trait is private.

The ownership envelope remains `Unverified`. Portable vtable emission does not
certify borrowed resource effects, dynamic cleanup, or arbitrary source `dyn`
coercions. See the [change record](../changes/2026-10-15-portable-vtables.md)
and [developer contract](../internals/dynamic-vtable-layout.md).
