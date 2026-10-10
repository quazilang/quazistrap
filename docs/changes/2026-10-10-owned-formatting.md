# Owned formatting and full-expression cleanup

Audience: users migrating text APIs and compiler/runtime contributors.

D-013 selects `Display.to_string() -> String`. Primitive integer widths,
pointer-sized integers, floating-point values (including `f16`), booleans, and
text now construct owned strings through source implementations. High-bit
unsigned decimal conversion preserves unsignedness. `String` provides
inherent borrowed and owned conversions. The compiler no longer lowers
`to_string()` to an ownerless allocation intrinsic.

`fmt.format` returns owned UTF-8 text. It copies borrowed display arguments,
handles escaped braces and compiler-expanded format specifications, and walks
complete UTF-8 scalars. Direct formatting uses the pointer/count variadic ABI;
output wrappers borrow an owned formatted result through the write. Module
calls retain their resolved annotation instead of overwriting it with an
unresolved duplicate, keeping move checking and emission consistent.

Full-expression temporary owners occupy zero-initialized guarded slots before
control flow. Borrowed uses keep producers alive until the expression ends.
Actual call, struct-field, and enum-payload commits deactivate transferred
owners; merely starting argument evaluation does not transfer them. `?` exits
destroy previously evaluated, untransferred owners. This closes the temporary
`Ok(number.to_string())` dangling-payload regression.

Consuming source struct `free(self)` hooks return `void`; storage cleanup runs
after source resource teardown on each return path. Supported structural
cleanup releases parent storage after fields. Native Linux and Windows entry
stubs construct allocator-backed `Array[str]` headers for argument-taking main,
so their destruction never passes a stack header to the heap allocator.
File.close takes an exclusive receiver and invalidates the handle without
consuming its header, avoiding recursive automatic destruction.
Allocation-backed construction checks null before writing and terminates on
failure. String allocation helpers panic rather than silently returning empty
output. Allocation failure has no unwinding guarantee.

The `Index` trait and prelude wrappers use a shared receiver, matching borrowed
indexing. Generic index inference unwraps the receiver capability before
matching its concrete element type. Repeated Array/String reads retain their
owner; by-value index wrappers previously destroyed it during a read.

Local string-view provenance tracks owner roots, aliases, and temporary
producers. It rejects retained views from temporary owners, returns of
local/owned-parameter views, unsafe aggregate/closure escapes, and sibling
argument moves while an earlier view is live. Display variadics borrow owned
`String` arguments rather than consuming them. This remains a conservative
local check, not D-014 effect inference or an ownership certificate.

QZI v11 binds the original ownership ABI generation into metadata and preserves
it through aliasing and serialization. Older owning or ambiguous contracts
require rebuilding; compatible scalar/raw-pointer artifacts remain readable.
Historical v2-v6 golden decoding remains covered. QZC v9 invalidates v8 exact
hits and reusable chunks. Envelopes remain explicitly unverified.

## Verification and limits

The pinned Rust/bfd setup passes the 728-test compiler suite, build, and
clippy (existing warnings remain). Standalone fixtures in `prelude/tests/`
cover conversion independence, returns/moves, enum transfer, numeric widths,
unsigned limits, Unicode, dynamic templates, specs, escaped braces, core
helpers, and literal/dynamic/writer output. Linux native execution, portable
QZI round trips, and Windows COFF emission pass. Windows execution requires a
Windows host and has not been tested here.

Linux `strace` records two mappings/two unmaps for a scoped numeric conversion,
and 78 mappings/78 unmaps for the owned-format fixture. Injected failure of the
buffer or owner-header mapping terminates with `SIGILL` before null
dereference. A separate no-prelude native counter fixture records exactly 13
destructor calls across discard, borrowed chains, moves, short circuits, match
arms, loops, `?`, and constructor failure paths.

Whole canonical-stdlib `qz test` currently fails before running tests: 39 HTTP
ownership diagnostics in `std.net`, plus two diagnostics in user-authored
unfinished CLI code. Core helpers, codec numeric conversion, repeated File.close, argument-taking
main, and quazifetch have separate native evidence. Preserve the CLI changes; do not describe whole-stdlib
readiness as achieved. HTTP ownership migration, general scalar-only/generic/
enum storage destruction, unknown returned aggregate/function capture
provenance, and cross-call retention remain required work. The full
production-readiness objective remains open.
