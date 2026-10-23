# Runtime String and formatting migration

Audience: language users and custom runtime maintainers.

Owned-string operations now recognize the actual String exported by the selected
prelude. Declaring another struct called String, including one with a data field,
does not grant string comparison, builtin view conversion or implicit formatting.
Use the runtime String or provide explicit ordinary source conversions.

Builtin as_str/as_string no longer interprets an arbitrary reference as text.
Implicit format arguments must be numbers, Bool, string views, runtime String or
supported references to those values. Convert a raw pointer to an integer when
its numeric address is intended. Convert an aggregate explicitly to an appropriate
view or runtime String; an implementation of a method with an unrelated result
layout does not authorize a raw heap pointer to be used as UTF-8 text.

Custom preludes may publicly re-export a differently named defining struct as
String. The selected declaration must be public, nongeneric, nonunion and use
the ordinary owned aggregate layout, with exactly one str data field. Its checked
offset may be nonzero. Invalid selected contracts fail S14. Constructor aliases
now use the original declaration's layout. Nested references to owned String
need explicit dereferences before comparison; one reference layer remains supported.

Rebuild old source caches and QZI bodies to acquire these checks. QZC29 invalidates
earlier source chunks automatically; QZI14's wire format and Unverified ownership
status are unchanged. Switching selected builtin runtime roots also invalidates
exact/reusable caches. Source namespace isolation remains incomplete; see
[the open visibility finding](../audits/2026-10-23-string-followups.md).
