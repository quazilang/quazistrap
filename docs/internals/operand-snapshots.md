# Eager values and computed places

Audience: compiler developers.

`FnCompiler::compile_value` captures binding-backed value slots before evaluating
later operands. `compile_expr` remains the storage-preserving entry for places,
explicit address-of and receiver adaptation. Never add an unconditional copy to
`compile_expr`: it would redirect references and mutating methods to temporaries.

Physical widths come from resolved runtime layouts. Constructor paths lacking
semantic result annotations have explicit structural evidence: enum handles,
single-slot indexed loads/getters, equal-width match arms, and recorded binding
initializer widths. Register identities are monotonic; lexical register-map
restoration selects the correct binding evidence. Missing layout evidence is an
emission error. Copies and captures check allocated register ranges and fail
closed at the QZI register budget.

Eager binary operands, comparison branches, call arguments and indirect callees,
format inputs, aggregate fields/payloads, and range bounds use stable values.
Named calls and struct fields retain declaration-slot evaluation order. Match
results reserve/copy their complete block. Slice aliases copy pointer plus length;
length observations and indexing use the same captured descriptor. Taking a
reference pins its complete block. Borrowed receivers adapt original storage
before their argument value is captured.

`compute_lvalue_addr` evaluates a destination once before its RHS. It freezes
heap handles, reference/raw pointers and indexes; fixed arrays retain their
actual frame storage. Bounds checks precede RHS evaluation. Compound assignment
also captures the old loaded scalar before evaluating its RHS. Field and flexible
C-array places retain width, signedness and f32 conversion through load/store.
A raw C aggregate dereference retains its inline storage address; ordinary
Quazi aggregate pointers still load their value handle. A value snapshot does not deep-copy pointees or turn a place into independent
storage.

Bit copies never create another cleanup owner. Pending full-expression owner
slots retain transferred arguments, fields and consuming receivers until the
call/store commits, including early `?` failure. Capture updates the pending
slot's direct-register identity. Receiver/field consumption occurs when that
operand is evaluated, before siblings can replace its former binding. Consuming
an owned direct assignment result consumes its newly initialized destination;
function-valued and owned projected assignment transfers retain their gates.
Consuming receiver fallback without an early resolved target fails closed.

`MoveEnv::pending_storage` retains allocation roots across pending operations.
It ends with the synthetic operation scope, merges by declaration identity, and
rejects releasing/moving/replacing required owner storage. Plain scalar writes
through a pinned aggregate remain allowed. View observations retain ordinary
shared loans, since mutation can invalidate the view's data as well as its
allocation. Indirect function callees, legacy receivers, computed places/indexes,
binary views and guarded match traversals retain the roots they still need.
Borrow checking uses the same declared argument/field schedule as emission.
Observation scopes do not shorten explicit reference loans at opaque calls;
reference creation skips these guard-only regions when choosing its lifetime.

These are conservative source rules. They do not supply normalized projection
aliases, opaque-callee effects, partial-place move/initialization state, complete
recursive destruction or QZI ownership certificates. QZI13 remains Unverified.
QZC23 invalidates previous exact/reusable source chunks; rebuild older QZI bodies
from source. See the [migration guide](../migrations/operand-snapshots.md).
