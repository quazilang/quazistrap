# Process contract status correction

The readiness audit and process decision index now match D-011's approved
2026-09-10 first-release child-process contract.

## Why

Older status text correctly described the original decision but became stale
after D-011 approved exact executable paths, borrowed argument slices,
exit-status representation, forced termination, and repeat-safe close
behavior. It incorrectly continued to list contract approval as unfinished.

## Compatibility

This is a documentation correction only. `std.process` and its compiler
runtime intrinsics are still unimplemented; applications must not rely on
undocumented process creation or construct shell command strings.

## Remaining work

Linux and Windows runtime implementations, the public standard-library API,
and executable-path, argument, failure, termination, and cleanup regressions
remain required before process support can be claimed.
