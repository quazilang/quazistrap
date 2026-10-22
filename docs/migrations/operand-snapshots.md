# Stable operands and destination evaluation

Audience: Quazi users.

Earlier operands preserve the value read before later expressions mutate a
binding. For example, with x initially 1, `first(x, x = 2)` receives 1 as its
first argument, and `x + (x = 2)` produces 3. Compound assignment reads its old
value before the RHS: `x += (x = 2)` also stores 3. Named arguments and struct
fields continue to run in declaration-slot order.

Assignment destinations are fixed before the assigned expression runs. An
assignment to `values[index]` keeps that destination if its RHS changes index.
Fixed-array places still refer to the original storage; snapshots do not copy
the memory behind references, arrays or heap handles. Slice values retain their
pointer and length together.

A later operand cannot release or replace owner storage still needed by an
earlier view, call target, computed destination or match guard. Such cases now
report S10. Finish the observation before replacing the owner, or keep a distinct
owner alive for the operation. Scalar field updates remain allowed when they do
not release required storage. Existing reference/view escape limits remain.

When an owned direct assignment result is passed into another owner, the newly
assigned binding is moved too. Reinitialize it before using it again. Owned
projected and function-valued assignment transfers remain restricted.

QZC23 rebuilds older source caches. QZI13 format and public ABI are unchanged;
rebuild old QZI bodies from source to receive these corrections. No Windows-host
execution or general ownership-artifact certification is claimed.
