# Ownership implementation boundary audit — 2026-09-15

Audience: maintainers continuing the local production-readiness plan.

## Conclusion

The production-readiness plan remains incomplete. The compiler now has a
conservative partial-move safety boundary, but this is not structural
destruction or D-014 whole-program ownership analysis.

## Current evidence

- Compiler commit `7dd6275` rejects consuming moves of move-only fields,
  built-in indexed elements, and values reached through safe dereferences.
  It retains valid scalar, `f16`, unconstrained-generic, and overloaded-index
  result paths. The pinned offline compiler suite passes 603 tests.
- The existing lexical borrow checker implements local shared/exclusive loans
  and resolved direct inherent consuming receivers. It does not solve effects
  across calls, recursion, indirect dispatch, or QZI-only dependencies.
- QZI v9 and QZC v7 contain no compiler-verifiable ownership summaries.
  They cannot prove a safe borrowed-call boundary for a source-unavailable
  dependency.

## Structural-destruction readiness

Phase 3 of the generic-storage design cannot safely be reduced to adding
compiler-generated recursive field cleanup alone. The current code generator
recognizes a named type's `free(self)` hook as its entire local cleanup action,
while existing standard-library hooks already dispose owned fields manually.
For example, `std.ini.IniDocument.free` frees `content` and replaces it with an
empty `String`. Calling that hook and then mechanically destroying fields would
release `content` twice.

The required implementation unit is therefore coordinated:

1. define compiler-generated hook and field-drop ordering;
2. migrate existing hooks so each resource is released exactly once;
3. add place-level move state and enum-payload/husk rules;
4. record destructor specializations and implement owned container-element
   cleanup; and
5. verify exact destruction on normal scope exit, replacement, return,
   branches, arrays, and enums.

The partial-move guard is intentionally retained until that unit is complete.

## Plan status

| Plan area | Status | Evidence needed before closure |
| --- | --- | --- |
| D-014 effects and loans | Incomplete | Flow-sensitive interprocedural effects, recursive/indirect-call handling, and QZI summary verification. |
| D-003 structural destruction | Incomplete | Coordinated compiler/std migration and exact-once destruction regressions. |
| Platform/policy foundations | Incomplete | Decisions D-004–D-009 and D-013 where they gate public behavior. |
| Editor runtime validation | Incomplete | Reproducible Zed and Helix runtime tests. |
| Final readiness review | Incomplete | Cross-project, target-scoped verification after the preceding work. |

No open row is closed by the 603-test compiler result alone.
