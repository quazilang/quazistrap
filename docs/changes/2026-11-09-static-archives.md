# Static archives in the built-in ELF linker

Audience: language users and compiler developers.

Passing a static archive (`qz build main.qz libhelpers.a`) used to require an
external linker. The built-in ELF linkers (Linux, NetBSD, and `qz run --jit`)
now accept `.a` archives of ELF objects:

- `backend/archive.rs` reads the archive with the `object` crate and selects
  members like a traditional Unix linker: a member is linked when it defines
  a still-undefined global symbol, repeatedly until nothing changes. Members
  nobody needs, and their own undefined references, stay out.
- Thin archives are rejected with an error. Windows `.lib` archives still need
  an external linker.
- A symbol no selected member defines is reported as an unresolved symbol.

Verification: a unit test builds ELF objects with `object::write` and an `ar`
archive, checking transitive selection and skipped members;
`scripts/check_static_archives.py` links C objects from a real `ar` archive
(transitive member after an unused one with a dangling reference), checks the
static output, `qz run --jit`, and the unresolved-symbol error. Built-in linker
plan item 8/9.
