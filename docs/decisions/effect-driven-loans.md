# D-031: the first safety rule driven by effect summaries

Decided 2026-11-27 (asks В37, recommended option А, chosen by the user).

The rule "a temporary loan of an argument `&x`/`&x!` ends after the call"
uses the D-014 effect summaries: it applies to every call whose summary is
fully described (no unmodeled effects, transitively) and whose argument is
neither returned nor captured — methods, calls through a module path, and
QZI dependencies with a verified ownership certificate (a dependency
without one is an error asking for a rebuild). Before, it applied only to
simple calls of source functions. More programs are accepted; nothing that
was rejected before changes.

Soundness rests on the summaries plus a language fact pinned by a test
(`safe_code_cannot_store_references_beyond_a_call`): safe code cannot store
a reference in a struct field, enum payload, generic argument or owned
aggregate, so `ret` and closure captures are the only escape channels.

Rejected alternative (Б): complete the summaries and certificates first.
