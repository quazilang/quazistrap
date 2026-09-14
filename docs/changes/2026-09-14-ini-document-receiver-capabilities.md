# INI document receiver capabilities

`std.ini.IniDocument` now expresses whether an operation only inspects its
validated source or replaces it. Lookup, conversion, and serialization methods
use shared receivers; `parse_string` uses an exclusive receiver.

## Why

An atomic reparse replaces the owned source text, while lookups only derive
fresh owned values from it. Explicit receiver capabilities prevent safe code
from combining those operations through conflicting loans. This is one staged
standard-library migration required before bare `self: T` can take on the
planned consuming D-014 meaning.

## Compatibility

Calls on mutable local documents continue to work. A shared `&IniDocument`
view supports `get`, `contains`, `has_section`, `stringify`, and all
`require_*` helpers, but must end before `parse_string`. `free` retains its
legacy by-value spelling until consuming receivers are implemented.

## Verification

The source-level `std.ini` suite runs through the contained compiler, and the
canonical documentation suite checks the public API reference.
