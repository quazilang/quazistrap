# Modules, Libraries, and Packages

A `.qz` file defines a module. `foo.qz` resolves as `foo`; `foo/mod.qz` is the
gateway for a directory-backed module. Public APIs use `pub`; gateways expose
children through `pub import`. There is no `reexport` keyword and no `::` path.

Imports may select a module, one item, several items, an alias, or a local file:

```quazi
import std.io;
import math.Vector;
import math.{dot, normalize};
import math.normalize as unit;
import ./generated.schema;
```

Project dependencies are named in `quazi.toml`; the dependency key becomes the
root import name. A dependency may be a local project/path, one source file,
QZI library, Git repository, or downloaded archive. Internet dependencies need
explicit `type = "git" | "archive" | "source" | "qzi"`.

QZI is portable Quazi Instruction bytecode. Library QZI includes package
metadata, its public source interface, relocations, and executable chunks, so
consumers do not need original source for supported APIs. Generic template
bodies remain source-published until QZI supports exporting them.

`quazi.lock` records resolved package identity/revisions/checksums. QZC is only
the disposable compilation cache. Full manifest syntax, artifacts, linking,
targets, and cache paths are in [PROJECTS.md](PROJECTS.md); QZI details are in
[LIBRARIES.md](LIBRARIES.md).
