# Collections use one in-place owner

`std.collections.Map` and `Set` no longer return a replacement container from
`insert` or `remove`. Method receivers are borrowed by Quazi, so the former API
could return a second owner for the same backing allocations and crash during
scope cleanup.

`insert` now updates the existing collection and returns `Result[bool, Error]`:
the boolean reports whether a new entry was added. `remove` updates in place and
returns whether it removed an entry. Existing callers must remove assignments
such as `map = map.insert(key, value)?` and use `map.insert(key, value)?`
instead.

Growth rehashes directly into fresh raw storage, then replaces the original
owner's pointers. Linux native smokes cover normal cleanup, explicit early
release, removal, and growth; a Windows x86-64 COFF smoke also compiles.
