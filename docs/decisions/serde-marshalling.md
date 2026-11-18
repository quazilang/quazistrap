# D-026: attribute-driven marshalling (`@serde`)

Status: accepted 2026-11-18 from the maintainers' sketch (Go-like
marshalling, "do something similar but more carefully"). It refines D-012:
serialization stays static (compiler-generated impls, no runtime
reflection).

## Source form

```quazi
@serde("json", "ini")            // formats this struct marshals to/from
struct User {
    id: usize,                   // key `id`: a field matches by its name
    name: String,
    @serde("user_age")           // rename for every format
    age: u8,
    level: u8 @serde("json:lvl", "ini:level"),   // per-format names
}
```

- `@serde(...)` on a struct lists the formats. Each listed format makes the
  compiler generate that format's serialize and deserialize impls; the file
  imports the format's module (`std.codec` for JSON, `std.ini` for INI).
  Supported: `json`, `ini`. Any other format name is rejected on the
  struct until a built-in codec implements it.
- The older `@derive(Serialize, Deserialize)` with `@json(name="...")` stays
  as JSON-only shorthand. `@json(name=...)` wins over `@serde` keys.

## Keys

Amended 2026-11-18 by maintainer answer В27Б: `@serde` is the one universal
attribute for wire keys, so formats added later (and ORM-style layers built
on serialization) read the same metadata.

```quazi
@derive(Serialize, Deserialize)
struct Square {
    side: f64 @serde("json:alpha", "toml:alpha"),
    aaaa: f64 @serde("*:bbbbb"),
}

@derive(Serialize, Deserialize)
enum Shape {
    Circle(f64) @serde("circle"),
    Empty,
}
```

- A field or enum variant needs no attribute when its key equals its name.
- `@serde("format:key")` names the key for one format. `@serde("*:key")`
  names it for every format; the bare `@serde("key")` means the same. For
  a format, its own key wins over the `*` key, which wins over the name.
- Text before the first `:` is the selector, so a key that itself contains
  `:` is written with one: `@serde("*:urn:id")`. Built-in format names are
  lowercase; `"JSON:key"` is rejected as a misspelling. INI keys cannot
  contain `=`, `[`, `]`, `;`, `#`, line breaks or surrounding spaces.
- A field or variant gives each selector (`*` or a format name) at most one
  key. Keys are nonempty strings without NUL; a format name is an
  identifier. Names without a built-in codec (`toml` above) are accepted on
  keys and ignored by the built-in codecs, so another format can use them.
- Keys need a serialized owner: a struct with `@serde(...)` formats or a
  `Serialize`/`Deserialize` derive, or an enum with that derive. Keys are
  unique per requested format.
- `@serde` replaces the earlier `@tag("name")` on enum variants, which is now
  rejected with a pointer to `@serde`.

## Traits

Each format module owns its traits, so a struct can support one format
without the others:

- `std.codec.Serialize` — `fn to_json(self: Self) Result[String, EncodeError]`;
  `std.codec.Deserialize` — `@associated fn from_json(source: str)
  Result[Self, DecodeError]`.
- `std.ini.IniSerialize` — `fn to_ini_writer(self: Self) IniWriter`;
  `std.ini.IniDeserialize` — `@associated fn from_ini_section(document:
  &IniDocument, section: str) Result[Self, IniDecodeError]`. Scalars and
  `String` are `key = value` lines, `Option` fields may be absent, and struct
  fields are `[key]` sections; sections do not nest and INI has no arrays.
  (Trait names carry the format because the trait registry keys traits by
  their short name.)

`@associated` marks a trait method without a receiver; generic code calls it
through a bound: `fn load[T: codec.Deserialize](text: str) Result[T, ...] {
ret T.from_json(text); }`.

## Documents

```quazi
var doc = codec.Document.load("user.json")?;  // read a file
var user: User = doc.decode[User]()?;
user.age = user.age + 1;
doc.encode(user)?;                            // replace the document value
doc.save()?;                                  // write back to its path
```

`Document.parse(text)`, `doc.text()` and `doc.save_as(path)` complete the
surface. `ini.IniDocument` mirrors it (`load`, `parse`, `decode[T]`,
`encode`, `save`, `save_as`, `stringify`). The JSON document lives in
`std.codec` because `std.json` cannot import the codec traits (`std.codec`
already imports `std.json`). Encoding consumes the value (`Serialize`
takes `self` by value) until borrowed enum payloads allow encoding through
`&Self`.
