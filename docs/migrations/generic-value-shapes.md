# Migrating Multi-Register Generic Values

Quazi now rejects a concrete generic specialization when one of its substituted
parameters or results cannot cross the current one-slot internal ABI safely.
Previously, such code could compile while preserving only the first register.

For example, this is diagnosed rather than miscompiled:

```quazi
var rows: Array[[i32; 3]] = Array.new() as Array[[i32; 3]];
rows.push([1, 2, 3]);
var row: [i32; 3] = rows.get(0);
```

Keep fixed arrays local and index them directly until multi-register generic ABI
lowering is available. If the data has a meaningful domain model, a named struct
is represented as one indirect handle and can cross the current ABI:

```quazi
struct Row { first: i32, second: i32, third: i32, }

var rows: Array[Row] = Array.new() as Array[Row];
rows.push(Row { first: 1, second: 2, third: 3 });
```

This representation rule does not make owned handles copyable. Container
borrowing, `take`, replacement, and recursive destruction are still being
stabilized. Avoid manufacturing aliases by repeatedly extracting an owned value,
and do not treat explicit `free()` as a cloning mechanism.

QZC v5 automatically ignores older incremental caches. No manual cache deletion
is required. QZI libraries with public generic templates must continue to be
published as source.

Generic constructors with no value arguments can now infer their type arguments
from an annotated binding:

```quazi
var values: Array[i32] = Array.new();
```

Remove older `as Array[i32]` inference workarounds when the surrounding binding
already states the intended type.
