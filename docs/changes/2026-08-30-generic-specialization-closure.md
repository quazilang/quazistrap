# Generic Specialization Closure

Audience: Quazi users and compiler maintainers.

## Change

The compiler now closes generic method dependencies over concrete call sites.
When a specialized generic method calls another generic method, the callee's
matching concrete chunk is emitted as well. This fixes valid programs such as
`Array[str]` indexing, where `Array.index[str]` calls `Array.get[str]`.

Specialization lookup also compares resolved type aliases. A source alias such
as `Rune` and its concrete representation `u32` now select the same emitted
generic chunk instead of producing a false missing-specialization error.

## Compatibility and Migration

This is a bug fix with no source migration. Programs that previously failed
during code generation for a required specialization now build normally.

## Verification

- Compiler regressions cover transitive generic impl calls and alias-resolved
  specializations.
- `examples/22-system-information` and `examples/33-ini-library` build with
  the local compiler and standard-library checkout.
