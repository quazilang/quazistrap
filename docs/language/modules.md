# Modules and imports

Audience: language users and implementers.

## Module system

Each `.qz` file defines a module. A directory module uses `mod.qz` as its
gateway file. Module and symbol paths always use `.`, never `::`.

Non-entry files get module-qualified function names (e.g., `bar.foo`). Entry
files keep bare names.

## Import syntax

### Module import

```quazi
import std.io;
```

Imports the module; access members through `io.println(...)`.

### Single item import

```quazi
import std.io.println;
```

Imports one declaration directly into scope.

### Multi-item import

```quazi
import store.{find, save};
```

Imports several declarations from the same module.

### Aliased import

```quazi
import store.find as lookup;
```

Creates a local alias for an imported declaration.

### Relative import

```quazi
import ./local_helper.value;
```

`./` forces resolution relative to the current file's directory rather than
the package root.

### Conditional import

```quazi
@cfg(target_os="windows")
import std.windows;
```

Imports gated by `@cfg` are excluded from dependency discovery when the
condition does not match the selected target.

### Re-export

```quazi
pub import codec.Decoder;
```

`pub import` imports a name and simultaneously exposes it from the current
module. There is no separate `reexport` keyword.

### Standard library import

`import std` imports the standard-library gateway, giving access to all
public standard-library modules. Individual modules are imported with
`import std.io`, `import std.fs`, etc.

Standard-library resolution checks the compiler's manifest directory for
`std/`, then the user installation at `~/.quazi/std`
(`%USERPROFILE%/.quazi/std` on Windows).

## Visibility rules

- `pub` on a function, type, constant, or import makes it visible outside its
  file.
- Without `pub`, a declaration is private to its file.
- A directory module's `mod.qz` gateway controls what children are visible
  through `pub import`.

### Type name visibility

Every type spelling is resolved in the source file that contains it. This
applies to signatures, fields, enum payloads, aliases, local annotations,
casts, generic arguments, struct literals, static calls such as `Point.new()`,
and enum patterns such as `Color.Red`. A type name is visible when it is:

- declared in the same file, with or without `pub`;
- introduced by one of the file's own imports, including `as` aliases,
  wildcard imports, and namespace spellings such as `geometry.Point` after
  `import geometry;`;
- a public type of the implicit prelude (`String`, `Array[T]`, `Option[T]`,
  `Result[T, E]`, and the other [prelude types](types.md#prelude-types));
- a generic parameter of the enclosing declaration, or `Self` inside a trait or
  `impl`.

Imports are private to the importing file. Importing `geometry` in
`helper.qz` does not make `geometry.Point` visible in `main.qz`, even when
`main.qz` imports `helper`. An unknown type name, or a type that the file did
not import, is rejected with `S04`:

```quazi
// helper.qz
import geometry as shapes; // visible only inside helper.qz

// main.qz
import helper.make;
fn area(p: shapes.Point) i32 { ret 0; } // S04: not imported into this file
```

Limitation: types supplied only by precompiled QZI libraries have no source
declaration. They are currently visible in every file of a program that links
the library; per-file selector checking for them is not implemented yet.

## Collision handling

Importing a name that collides with a local declaration is a compile error.
Use `as` to alias the import:

```quazi
import bar.foo as b_foo; // avoids collision with local fn foo
```

## Package structure

A `quazi.toml` manifest defines a package. The dependency key becomes the root
import name:

```toml
[dependencies]
local_math = { path = "../math" }
```

```quazi
import local_math.factorial;
```

### Library entry points

The file configured by `[lib].path` is the package entry point. Its `pub`
declarations are direct package exports:

```quazi
// src/lib.qz
pub fn factorial(n: i32) i32 { ... }
```

Consumers import with `import my_package.factorial;`. Other source files
remain internal modules; the entry point must `pub import` their
declarations to make them direct package exports.

### Dependency types

| Type | Source |
| --- | --- |
| `path` | Local project directory or single `.qz`/`.qzi` file |
| `git` | Checked-out Git repository |
| `archive` | Extracted archive |
| `source` | Single downloaded `.qz` module |
| `qzi` | Compiled QZI library |

Internet dependencies require an explicit `type`. Local paths can infer
their type. See [projects](../PROJECTS.md) for full manifest syntax.

### Lock file

`quazi.lock` records resolved dependency identity, revisions, and checksums.
It is portable and should be committed. `qz fetch` materializes and verifies
dependencies; `qz deps` inspects the result.

## Deduplication and circular imports

The loader deduplicates imports via canonical-path tracking. Circular imports
are safe — each file is loaded at most once. A file whose import resolves to itself (for example
`import ./x;` inside `x.qz`) is rejected with `'<path>' imports itself`.
