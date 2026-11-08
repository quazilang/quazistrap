# `std.time`

`std.time` provides monotonic durations and instants for measuring elapsed
time, POSIX wall-clock time (`SystemTime`), proleptic-Gregorian UTC calendar
fields (`DateTime`), and RFC 3339 formatting and parsing.

Policy: wall-clock values count Unix seconds without leap seconds (a leap second
cannot be represented, and `:60` is rejected), cover years 0001 through 9999,
and use the proleptic Gregorian calendar. Named time zones and a time-zone
database are not provided; RFC 3339 numeric offsets are accepted when parsing
and normalized to UTC. Days, months and years are not `Duration` units.

## `Duration`

`Duration` stores normalized `seconds: u64` and `nanoseconds: u32` values.
Its nanosecond component is always less than one billion.

- `zero()` constructs a zero duration.
- `from_nanoseconds`, `from_microseconds`, `from_milliseconds`, and
  `from_seconds` construct normalized values without treating calendar units as
  fixed durations.
- `from_minutes` and `from_hours` use fixed 60- and 3600-second units and
  return `Option[Duration]`, `None` on `u64` overflow.
- `seconds()` and `subsec_nanoseconds()` expose the normalized components.
- `compare(&other)` returns -1, 0 or 1.
- `checked_add` and `checked_sub` return `Option[Duration]`; overflow and a
  negative result return `None` rather than wrapping.

These instance operations use shared receivers: they inspect a duration and
return a fresh value without changing the original duration.

## `Instant`

`Instant.now()` returns `Result[Instant, TimeError]`. It reads a monotonic
clock: Linux uses `clock_gettime(CLOCK_MONOTONIC)` and Windows uses
`GetTickCount64`. The epoch is opaque, and values from different system boots
must not be compared or serialized as timestamps.

- `duration_since(earlier)` returns `ClockWentBackwards` when `earlier` is
  later than the receiver.
- `elapsed()` samples the same monotonic clock and returns the elapsed
  `Duration`.
- `compare(&other)` returns -1, 0 or 1 for instants from the same boot.

The resolution is platform-dependent. Programs must not assert exact elapsed
values or rely on a wall-clock relationship.

## `SystemTime`

`SystemTime` stores Unix seconds (`i64`) and nanoseconds (`u32`) in UTC.

- `now()` reads the wall clock: `clock_gettime(CLOCK_REALTIME)` on Linux and
  NetBSD, `GetSystemTimeAsFileTime` (100 ns resolution) on Windows. The wall
  clock can jump; measure elapsed time with `Instant`.
- `unix_epoch()` and `from_unix(seconds, nanoseconds)` construct values;
  `from_unix` returns `None` for nanoseconds of one billion or more and outside
  years 0001-9999 (`-62135596800` through `253402300799`).
- `unix_seconds()`, `subsec_nanoseconds()` and `compare(&other)` inspect values.
- `checked_add(duration)` / `checked_sub(duration)` return `None` outside the
  supported range; `duration_since(&earlier)` returns `ClockWentBackwards` when
  `earlier` is later.
- `to_utc()` returns the `DateTime` fields.
- `format_rfc3339()` returns an owned `String` such as `2026-11-08T09:05:00Z`;
  a nonzero fraction is printed with nine digits (`...:00.000000005Z`).
- `parse_rfc3339(text)` accepts `YYYY-MM-DDTHH:MM:SS[.fraction]` followed by
  `Z` or `±hh:mm`; `T` and `Z` may be lowercase, the fraction has 1-9 digits,
  and the offset is applied to produce UTC. Malformed text, impossible dates,
  second 60 or offsets outside `00:00`-`23:59` return `InvalidFormat`; a valid
  time outside the supported range returns `OutOfRange`.

```quazi
// tutorial: fragment
const stamp = time.SystemTime.parse_rfc3339("2000-02-29T14:34:56.5+02:00").unwrap();
const text: String = stamp.format_rfc3339(); // "2000-02-29T12:34:56.500000000Z"
```

## `DateTime`

`DateTime.new(year, month, day, hour, minute, second, nanosecond)` validates
every field (including leap years and month lengths) and returns
`Option[DateTime]`. Accessors return each field; `weekday()` returns the ISO
weekday (1 = Monday ... 7 = Sunday); `to_system_time()` and
`format_rfc3339()` convert back. Values are always UTC.

## Errors

- `ClockUnavailable` means allocation or the platform clock call failed.
- `ClockWentBackwards` reports an invalid ordering supplied to
  `duration_since`.
- `InvalidFormat` reports text that is not a supported RFC 3339 timestamp.
- `OutOfRange` reports a valid time outside years 0001-9999.

`TimeError.message()` provides display text; applications should generally
propagate the structured variant instead of matching error strings.
