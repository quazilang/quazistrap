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
are safe — each file is loaded at most once.
