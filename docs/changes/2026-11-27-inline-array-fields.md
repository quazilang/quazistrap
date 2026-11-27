# Fixed arrays can be struct fields

Date: 2026-11-27

Audience: Quazi users; first stage of the fat `str` plan
([internals](../internals/fat-str.md)).

A plain (non-generic, non-`repr(C)`) struct may now have a fixed-array
field of plain elements, stored inline:

```quazi
struct Sample { head: i64, pair: [i64; 2], tail: i64 }
```

Field offsets count register slots (`pair` takes 16 bytes). Reading the
field copies the array; `s.pair[i] = v` and `s.pair[i] += v` write the
struct itself, also through `&Sample!`; `s.pair = [..]` replaces the whole
array. Before, any multi-slot field was S14.

Still rejected: fixed arrays of owning elements (`[String; 2]`; use
`Array[T]`), writing an element of a nested array field, and generic structs
instantiated with a multi-slot field type.

Regression: `scripts/check_fixed_arrays.py` (fields case).
