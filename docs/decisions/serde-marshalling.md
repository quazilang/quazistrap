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
  compiler generate that format's `Serialize` and `Deserialize` impls.
  Supported: `json`, `ini`. Other names (`yml`, `hocon`) are reserved and
  rejected until they are implemented.
- A field needs no attribute when its key equals its name. `@serde("key")`
  renames it in every format; `@serde("fmt:key", ...)` renames it per format
  and takes precedence for that format. The attribute may precede the field
  or follow its type, like other field attributes.
- The older `@derive(Serialize, Deserialize)` with `@json(name="...")` stays
  as JSON-only shorthand.

## Traits

Each format module owns its traits, so a struct can support one format
without the others:

- `std.codec.Serialize` — `fn to_json(self: Self) Result[String, EncodeError]`;
  `std.codec.Deserialize` — `@associated fn from_json(source: str)
  Result[Self, DecodeError]`.
- `std.ini.Serialize` / `std.ini.Deserialize` — the same shape for INI.

`@associated` marks a trait method without a receiver; generic code calls it
through a bound: `fn load[T: codec.Deserialize](text: str) Result[T, ...] {
ret T.from_json(text); }`.

## Documents

```quazi
var doc = json.Document.load("user.json")?;   // read a file
var user: User = doc.decode[User]()?;
user.age = user.age + 1;
doc.encode(user)?;                            // replace the document value
doc.save()?;                                  // write back to its path
```

`Document.parse(text)`, `doc.text()` and `doc.save_as(path)` complete the
surface; `ini.Document` mirrors it. Encoding consumes the value (`Serialize`
takes `self` by value) until borrowed enum payloads allow encoding through
`&Self`.
