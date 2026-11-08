# `std.time` wall-clock time, UTC calendar and RFC 3339

Audience: language users.

`std.time` gains `SystemTime` (Unix wall-clock time, years 0001-9999, no leap
seconds), `DateTime` (validated proleptic-Gregorian UTC fields with ISO weekday),
RFC 3339 formatting and parsing (numeric offsets normalized to UTC),
`Duration.from_minutes`/`from_hours`, and `compare` on `Duration`, `Instant` and
`SystemTime`. `TimeError` gains `InvalidFormat` and `OutOfRange`; code matching
`TimeError` exhaustively must handle them.

Named time zones, offset-preserving values, timers and deadlines remain open.
See the [API reference](../api/time.md). Implemented in std `feat(time)`;
verified by nine deterministic `@test` functions and Windows/NetBSD builds.
