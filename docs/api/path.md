# `std.path`

Audience: Quazi application developers.

`Path` holds a path as text and manipulates it lexically; nothing touches the
file system, so `normalize` does not resolve symbolic links. `/` separates
components everywhere; Windows builds also accept `\` and treat `C:\...` and
`\\server\...` as absolute. Indexes count Unicode scalars.

| API | Semantics |
| --- | --- |
| `Path.new(text)` / `as_str()` | Wraps a copy of `text`; returns the text. |
| `is_absolute()` | Rooted at a separator (after a Windows drive). |
| `join(child)` | Appends `child` with one separator; an absolute `child` replaces the path and an empty base yields `child`. |
| `file_name()` | `Option[String]`: the last component, ignoring trailing separators; `None` for a root or a path ending in `..`. |
| `parent()` | `Option[Path]`: the path without its last component (`a` gives the empty path, `/top` gives `/`); `None` for a root or empty path. |
| `extension()` / `stem()` | Split the file name at its last `.`; a leading dot (`.bashrc`) is part of the stem and gives no extension; `notes.` has an empty extension. |
| `components()` | `Array[String]`: the root (if any) then each component, skipping empty ones and `.`. |
| `normalize()` | Drops `.` and duplicate separators and collapses `x/..` pairs; leading `..` of a relative path are kept, a root swallows `..`, and an empty result is `.`. |

```quazi
import std.path.Path;

fn main() i32 {
    const file = Path.new("/srv/data/report.tar.gz");
    if (file.extension().unwrap().as_str() != "gz") { ret 1; }
    if (Path.new("a/./b/../c").normalize().as_str() != "a/c") { ret 2; }
    ret 0;
}
```
