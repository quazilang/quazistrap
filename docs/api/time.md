# `std.time`

`std.time` provides monotonic durations and instants for measuring elapsed
time. It deliberately does not provide wall-clock timestamps, calendar dates,
time zones, or civil-time formatting; those require separate, platform-stable
contracts.

## `Duration`

`Duration` stores normalized `seconds: u64` and `nanoseconds: u32` values.
Its nanosecond component is always less than one billion.

- `zero()` constructs a zero duration.
- `from_nanoseconds`, `from_microseconds`, `from_milliseconds`, and
  `from_seconds` construct normalized values without treating calendar units as
  fixed durations.
- `seconds()` and `subsec_nanoseconds()` expose the normalized components.
- `checked_add` and `checked_sub` return `Option[Duration]`; overflow and a
  negative result return `None` rather than wrapping.

## `Instant`

`Instant.now()` returns `Result[Instant, TimeError]`. It reads a monotonic
clock: Linux uses `clock_gettime(CLOCK_MONOTONIC)` and Windows uses
`GetTickCount64`. The epoch is opaque, and values from different system boots
must not be compared or serialized as timestamps.

- `duration_since(earlier)` returns `ClockWentBackwards` when `earlier` is
  later than the receiver.
- `elapsed()` samples the same monotonic clock and returns the elapsed
  `Duration`.

The resolution is platform-dependent. Programs must not assert exact elapsed
values or rely on a wall-clock relationship.

## Errors

- `ClockUnavailable` means allocation or the platform clock call failed.
- `ClockWentBackwards` reports an invalid ordering supplied to
  `duration_since`.

`TimeError.message()` provides display text; applications should generally
propagate the structured variant instead of matching error strings.
