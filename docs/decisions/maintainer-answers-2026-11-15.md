# D-023: maintainer answers of 2026-11-15

Status: decided by the maintainers on 2026-11-15 (asks.txt rounds 4-6).

- **В9В.** `to_str()` is removed everywhere; `to_string()` remains
  (implemented, `adf6660`).
- **В10А.** A by-value `match` consumes its scrutinee; `match &r` borrows
  (implemented 2026-11-16: enum drop glue destroys the active payload,
  bindings own payloads, `?` consumes owned operands;
  `scripts/check_consuming_match.py`). Struct patterns move bound fields
  out; a struct with its own `free` hook cannot be destructured that way.
  Recursive enums keep shell-only release.
- **В11В.** No `@format`: variadics must work normally and formatting must not
  need an attribute (implemented 2026-11-16 as `...args: Display` and `...args`
  forwarding, [D-024](display-variadics.md)).
- **В14Г.** Closures must capture properly, including owned values
  (pending).
- **В15А.** Float to integer `as` truncates and saturates, NaN is 0
  (implemented 2026-11-10, D-022 neighbours).
- **В16А.** A panic while handling a panic prints `panicked while processing a
  panic` and exits with 101 (implemented, `5f03491`).
- **В17.** Process timeouts are wanted now: `Child.wait_timeout` and
  `wait_or_terminate` (implemented in std, Linux); this supersedes D-011's
  deferral.
- **В18А.** Safety fixes may reject programs that relied on the holes.
- **В19В.** Deserialization: field access uses `Option`, and an unknown key is
  an error rather than silently ignored (implemented 2026-11-16:
  `@derive(Deserialize)` with UnknownField/DuplicateField/MissingField and
  `Option` fields, `json.object_field` returns `Option`; D-012 step 4).
- **В20А.** Module-level constants, and more of them in std (implemented,
  `0f8fe6e`).
- **В21А.** `RESUME.md` is not kept in product repositories; session notes
  live in the workspace (implemented).
- **В22Б.** Trait bounds first, then a generic `Map[K, V]`. Bounds are done
  (2026-11-16, `docs/language/types.md#trait-bounds`, `scripts/check_trait_bounds.py`);
  the generic `Map` is pending. Generic values are still treated as copyable
  by the ownership pass (asks В23).

## 2026-11-16 follow-up

- **QZI packaging.** `qz pack` and QZI format 15 are removed. One project is
  one unified QZI (format 16), and projects are combined through
  `quazi.toml` dependencies, which are linked into that artifact. This
  replaces the qzifix.txt request to keep a separate multi-QZI container.
