# Rejecting the unavailable `Deserialize` derive

Audience: Quazi users and compiler maintainers.

`@derive(Deserialize)` previously passed semantic analysis while generating no
decoder. That false-success state could let a program appear to opt into typed
deserialization even though the language cannot yet express a receiverless
`Deserialize.from_json` method and D-012 has not defined object policies for
unknown, missing, or duplicate fields.

The compiler now reports `S14` at the containing `@derive` attribute whenever
`Deserialize` is requested, including alongside `Serialize`. Use the explicit
`std.codec.decode_bool`, `decode_i64`, and `decode_string` functions for the
currently supported scalar decoding surface.

This is a deliberate compatibility correction for programs that used
`Deserialize` as a no-op marker. Ordered derive metadata is still retained
internally, so a future decoder can consume the same declaration information
after its resource limits, field policy, ownership, and trait contract are
specified.

Focused semantic regressions cover a `Deserialize`-only request and a combined
`Serialize, Deserialize` request; both must fail rather than exposing an
unimplemented generated API.
