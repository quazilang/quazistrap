# Migrating to per-file type visibility

Audience: Quazi users upgrading source code.

Each source file must now import every type it spells, unless the type is
declared in that file or is a public prelude type. See the
[visibility rules](../language/modules.md#type-name-visibility).

If analysis reports `S04: type 'Name' is not imported into this file`:

- Import the type in the file that reports the error, for example
  `import geometry.Point;` or `import std.fs.{File, FsError};`.
- Alternatively import the module and spell the type through it:
  `import geometry;` followed by `geometry.Point`.
- An alias such as `import geometry as shapes;` is private to the file that
  declares it. Repeat the import in each file that uses `shapes.Point`.

If analysis reports `S04: unknown type 'Name'`, the name is not declared
anywhere in the program. Correct the spelling or add the missing declaration.

Source caches older than QZC32 are invalidated automatically. See the
[change record](../changes/2026-11-08-source-type-visibility.md).
