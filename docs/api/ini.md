# `std.ini`

`std.ini` provides a small, deterministic INI reader. It keeps the validated
source text as an owned `IniDocument` and performs lookups on demand. It does
not preserve comments or formatting as editable structure, and it does not
provide writing, interpolation, schema validation, or nested-value syntax.

## Parsing and document ownership

`IniDocument.new()` creates an empty document. `IniDocument.parse(source)`
creates a document from one complete source string, returning
`Result[IniDocument, IniDecodeError]`. `IniDocument.parse_string(source)`
replaces the text of an existing document and returns `Result[bool, IniError]`.

Parsing is atomic: if `parse_string` fails, the document retains its prior
validated content. `stringify()` returns a fresh owned `String` containing the
exact validated source; it does not normalize whitespace or line endings.
Call `free()` when the document is no longer needed.

`parse_string` requires exclusive access to the document. Lookups,
`stringify`, and the `require_*` helpers use shared access, so they cannot run
while an exclusive parse is in progress.

```quazi
import std.ini.IniDocument;

fn main() i32 {
    var config = IniDocument.new();
    config.parse_string("[server]\\nport = 8080\\nenabled = true\\n").unwrap();

    const port: i32 = config.require_i32("server", "port").unwrap();
    const enabled: bool = config.require_bool("server", "enabled").unwrap();
    if port != 8080 || !enabled { ret 1; }

    config.free();
    ret 0;
}
```

## INI syntax

Blank lines and full-line comments beginning with `#` or `;` are ignored.
Whitespace surrounding section names, keys, and values is ignored. A section
is `[name]`; an empty section name returns `IniError.EmptySection(line)`. A
line with only one of the required brackets returns `InvalidSection(line)`.

All other non-ignored lines must contain `=`. The first `=` separates the key
from the value, so later equals signs remain part of the value. An absent
separator returns `MissingSeparator(line)`, and an empty key returns
`EmptyKey(line)`. Line numbers are one-based. Duplicate key definitions are
allowed; the last definition within a section wins. Values may be empty.

There is an implicit unnamed section before the first section header. Query it
with `""` as the section name.

## Lookup

`get(section, key)` returns `Option[String]`: `Some` contains a fresh owned
value, including an empty value, while `None` means the key is absent.
`contains(section, key)` distinguishes an absent key from an empty value, and
`has_section(section)` tests whether a section header exists.

`require_string`, `require_i32`, `require_u64`, and `require_bool` convert a
required key into the requested result type. Missing keys return
`IniDecodeError.MissingValue(section, key)`. Invalid numeric text, or boolean
text other than exact lowercase `true` or `false`, returns
`IniDecodeError.InvalidValue(section, key)`. `IniDocument.parse` wraps source
syntax failures as `IniDecodeError.Syntax(IniError)`.

The returned strings and error section/key names are owned values. Keep the
document alive only for subsequent queries; values returned from `get` or
`require_string` do not borrow its backing source.

## Typed marshalling (`@serde("ini")`)

`@serde("ini")` on a struct (D-026) generates `IniSerialize` and
`IniDeserialize`. Scalar and `String` fields are `key = value` lines before
the first section, `Option` fields may be absent, and a struct field is the
section `[key]`; keys default to field names and `@serde("key")` or
`@serde("ini:key")` renames them.

```quazi
import std.ini;

@serde("ini")
struct Server { host: String, port: u16, }

@serde("ini")
struct Config { name: String, @serde("max_users") limit: u32, server: Server, }

fn main() i32 {
    var doc = ini.IniDocument.parse("name = demo\nmax_users = 40\n\n[server]\nhost = localhost\nport = 8080\n").unwrap();
    var config: Config = doc.decode[Config]().unwrap();
    config.limit = config.limit + 1;
    doc.encode(config).unwrap();
    ret 0;
}
```

`IniDocument.load(path)`, `decode[T]()`, `encode(value)` (consumes it),
`save()` and `save_as(path)` work like `codec.Document`. `ini.read[T]` and
`ini.read_optional[T]` read one value through `IniValue` (integers, floats,
`bool`, `String`); a missing key is `MissingValue`, a malformed one
`InvalidValue`, a value with a line break `MultilineValue`, and file errors
`Io`. Sections do not nest and INI has no arrays; both are compile errors.
