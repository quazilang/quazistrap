# Source definite initialization

Audience: compiler developers.

`semantic/initialization.rs` tracks initialized local and parameter storage by
lexical scope depth, name and declaration span. Shadows cannot contribute facts
to another declaration. Snapshots restore only initialization bits; diagnostics,
used bindings, imports, type inference and constant invalidation remain intact.
This is a source read check, separate from runtime owner cleanup flags.

Normal successors intersect their initialized sets. A missing successor means
no normal path, not an empty initialized set. Return, break and continue do not
supply ordinary fallthrough facts. Unsafe and active cfg blocks preserve terminal
status. SemanticReport terminal statement spans tell codegen where analysis
stopped, including conditions with a proven truth value but executed effects.

Conditions retain distinct truthy and falsy states through groups, negation,
Boolean casts, assignment, && and ||. Skipped operands contribute no effects.
Loop initializers and first conditions run once before the first body; C updates
follow body fallthrough and continue paths. A possibly empty loop does not prove
its body ran. Break states are projected into the loop entry declaration domain.
Deferred closure bodies cannot initialize the enclosing live scope.

Match arms begin with pending paths that can reach their pattern. Only a matching
pattern evaluates its guard; false guards pass their effects to later matching
paths. Boolean and unknown integer scrutinees use zero/nonzero truth classes.
Boolean literal patterns test those classes; nonzero integer literals cover only
part of the nonzero class. Enum cases use declared variant names. Refutable
payload patterns conservatively cover part of a variant. String constant coverage
uses the runtime first-NUL boundary; float patterns use IEEE equality, including
signed zero and unordered mismatch. Variant states share the entry snapshot.
Case/fact work has a checked 1,000,000-step budget; exhaustion is S14, never a
partial initialization proof.

Checked enum discriminants are published by exact pattern span and consumed by
lowering. Codegen must not search unrelated enums by a variant's spelling.
Nested refutable patterns require resolved, substituted actual payload types;
unknown constructor results require an explicit scrutinee annotation. Builtin
Option/Result payload actuals are already substituted and must not capture a
caller type named T or E. Direct payload binding slots include wildcards.

Checking follows emitted operand schedules: named arguments and struct fields
use declaration slots; indirect callees and method receivers precede arguments;
assignment place operands precede the RHS. Place validation consumes existing
annotations rather than rechecking effects. Range bounds run before introducing
the loop binder. These checks do not establish argument value snapshots: a live
register can still alias a later-mutated operand, which belongs to the normalized
place/call schedule work.

Constant result knowledge does not authorize eliding execution. Only expressions
whose evaluation can be discarded publish foldable constant annotations; calls,
mutation, allocations, projections and trapping arithmetic retain evaluation.
Floating multiplication by zero is not an absorbing identity for unknown values.
This also protects constant if/else/loop consumers, not just expression emission.

Borrowed scalar match scrutinees load their value slot; shared &str retains its
view representation, while exclusive &str! loads its slot. A value-bearing match
that reaches no arm traps instead of exposing an unwritten destination. Known
non-exhaustive scalar value matches are S09; Void matches keep fallthrough.

QZC22 invalidates older exact and reusable source chunks. QZI13 remains unchanged
and Unverified; rebuild old bodies from source. This checker does not establish
move state, aliases, interprocedural effects, partial-place initialization,
canonical Boolean storage/equality, recursive destruction or artifact ownership
certificates. See [D-014](../decisions/whole-program-ownership.md) and
[the migration guide](../migrations/source-initialization.md).

Constant propagation treats every slot in a Lea-addressed block as runtime
memory in both dataflow and rewrite. Aliased stores, calls and intrinsics
cannot leave a stale local constant. This is conservative alias handling;
non-addressable register values can still fold normally.
