# `==` on struct and enum values is rejected

Audience: language users.

`==` and `!=` accepted struct and enum operands but compared their heap-handle
addresses. `Kind.A == Kind.A`, `error == NetError.TimedOut` and comparisons of
two equal structs were always false. `std.net` relied on this in
`TcpStream.shutdown`, so every shutdown mode closed both halves.

Equality on struct and enum values (including `Option` and `Result`) is now
`S06: ==/!= cannot compare T values; use match or a comparison method`. Scalars,
`bool`, strings, references, raw pointers and generic parameters are unchanged.

Migration: replace `value == Enum.Variant` with
`match value { Enum.Variant => true, _ => false }`, or add an explicit method.

Verification: semantic regression for enums, structs, scalars and generics; the
std `tcp_shutdown_read_keeps_the_sending_half_open` test fails when `Read` maps
to both halves and passes with the `match`-based mapping.
