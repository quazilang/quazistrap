# 23-standard-library-tour

A small developer report built from Quazi's ergonomic primitive APIs. It uses
Unicode-aware strings, slicing, checked parsing, `Result` matching, and
dependency-free `std.math` helpers.

```sh
qz run
```

The example deliberately has no `.free()` calls. Its owned `String` temporaries
are released by compiler-inserted scope cleanup.
