# Migrating shared-reference code

Quazi now enforces a conservative shared-reference model while lifetime
parameters and mutable references remain undesigned.

Replace value-to-reference conversions with an explicit borrow of a local or
parameter:

```quazi
var value: i32 = 7;
var reference: &i32 = &value;
```

Reference pointees are invariant, so use the exact target type instead of
relying on integer widening through a reference. Reference bindings are lexical
and cannot be rebound. Create a new binding in the narrower scope instead.

Since 2026-11-17 a function may return a plain `&T` that is a reference
parameter, `&parameter.field` through one, or a reference-returning call made
only with such reference arguments; any other non-string reference
result is an error. Structs, enums, arrays, owned generic values, and closures
cannot retain references. Return an owned
value or redesign the operation as a callback that consumes the reference
synchronously. `str` and `&str` keep their existing string-view compatibility.

Do not dereference `&Aggregate` into a local aggregate alias. Pass ownership to
an operation that needs aggregate methods, or expose a scalar read API until
Quazi gains immutable aggregate receivers and lifetime-aware views. This makes
`std.random.choose` consume its `Array[T]` for now.

Address-of supports local variables, parameters, and fields of them
(`&record.field`, borrowing all of `record`). Code such as `&items[index]`,
`&*pointer`, or `&(left + right)` must be rewritten to borrow a named local
copy, with the understanding that it is a copy rather than an alias to the
original element.

QZI dependencies containing `Lea` instructions without explicit address-taken
block metadata cannot be repaired after register allocation. Rebuild those
artifacts from source with the current compiler. Incremental QZC caches are
invalidated automatically by the v3 cache version.
