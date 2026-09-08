# Tutorial API alignment

Date: 2026-09-09

Audience: Quazi learners and tutorial maintainers.

The collections and I/O tutorial chapters now use the APIs shipped by the
current standard library. `Map` and `Set` construction and insertion are
fallible and therefore use `unwrap()` in the compact teaching examples;
removal mutates in place and returns a boolean. I/O examples use `err` and
`errln`, and filesystem examples use `File.create`, `write_str`, `exists`, and
`count_entries` rather than unimplemented convenience APIs.

Chapter 10 reports its indexed byte-position count to stdout and reports an
input-file failure to stderr. Its test example now names `index_positions`.

Compatibility: documentation-only. These corrections describe existing
compiler and standard-library behavior.
