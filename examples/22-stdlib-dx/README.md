# 22-stdlib-dx

An executable regression example for Quazi's ergonomic primitive APIs. It
checks rune lengths, byte lengths, negative indexing, Python-style slicing,
content comparison, ASCII case conversion, generic checked parsing, integer
exponentiation, and dependency-free `std.math` helpers.

```sh
qz run
```

The example deliberately has no `.free()` calls. Its owned `String` temporaries
are released by compiler-inserted scope cleanup.
