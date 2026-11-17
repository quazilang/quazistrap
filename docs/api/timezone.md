# `std.timezone`

Audience: language users.

`TimeZone` maps an instant to the UTC offset, abbreviation and daylight flag
of a named IANA zone. The full contract, including the database search path
and Windows limits, is in [`std.time`](time.md#time-zones-stdtimezone).

| API | Behavior |
| --- | --- |
| `TimeZone.load(name)` | Decode `name` (e.g. `Europe/Kyiv`) from the system TZif database. |
| `TimeZone.local()` | `TZ`, then `/etc/localtime`, then UTC. |
| `TimeZone.utc()` | UTC. |
| `TimeZone.from_posix(rule)` | A zone from a POSIX TZ rule. |
| `TimeZone.from_tzif(name, &bytes)` | Decode TZif bytes. |
| `name()` | The name the zone was loaded under. |
| `offset_at(&instant)` / `abbreviation_at(&instant)` / `is_dst_at(&instant)` | The local time type at an instant. |
| `local_time(&instant)` / `format_rfc3339(&instant)` | Wall-clock fields or RFC 3339 text in the zone. |

The data source follows D-009 as resolved by [D-025](../decisions/maintainer-answers-2026-11-18.md).
