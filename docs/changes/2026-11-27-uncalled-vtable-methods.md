# Trait objects of types whose methods are never called directly

Date: 2026-11-27

Audience: Quazi users of `dyn Trait`.

Converting a concrete value to `dyn Trait` (a `dyn` local or argument,
`Array[dyn Trait]`, `Box[dyn Trait]`) failed with "code generation failed:
missing vtable implementation `C.show`" unless some code also called
`C.show` directly or through a `dyn` method call. The conversion now keeps
every trait method of the concrete type reachable, as the borrowed
(`&dyn`) and generic (`T: Trait`) conversions already did.

No source changes are needed. Regression:
`scripts/check_dynamic_vtables.py` (vtable methods never called directly).
