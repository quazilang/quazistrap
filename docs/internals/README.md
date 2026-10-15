# Compiler and runtime internals

Audience: language and tooling developers.

This section will document compiler phases, bootstrap constraints, QZI/QZC,
runtime ABI, ownership invariants, testing, and contribution workflows.

## Pages

- [Source result retention](source-result-retention.md)
- [Portable dynamic vtable layouts](dynamic-vtable-layout.md)

- [Checked type-alias resolution](type-alias-resolution.md)

- [Concrete source ownership call graph](ownership-call-graph.md)
  — canonical specialization identities and the boundary before D-014 effects.
- [Generic storage layout and recursive destruction — implementation design](generic-storage-layout.md)
  — the approved D-001/D-002/D-003 route for sound generic containers.
