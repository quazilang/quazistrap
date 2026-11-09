# Strength reduction for powers of two

Audience: compiler developers.

Integer multiplication, division and remainder by a constant always used
`imul`, `div` or `idiv` (tens of cycles for division). Constant propagation now
records, per instruction, that the second operand of an integer `Mul`, `Div`
or `Mod` is a proven `2^k`, in the otherwise unused `ops[3]` byte of the RRR
form (`k + 1`; `instruction::power_of_two_operand`). A constant left operand
of `Mul` is swapped right first. The x86-64 backend lowers hinted
instructions to:

| Operation | Lowering |
| --- | --- |
| `x * 2^k` | `shl` |
| unsigned `x / 2^k`, `x % 2^k` | `shr`, `and` with `2^k - 1` |
| signed `x / 2^k` | bias by `2^k - 1` when negative, then `sar` (truncates toward zero like `idiv`) |
| signed `x % 2^k` | `x - ((biased x >> k) << k)` |

The operand register still holds the constant, so a backend or artifact
reader that ignores the hint computes the same value; QZI stays compatible
(the byte was already serialized and is not a register operand). Floating
operations are never hinted.

Verification: `scripts/check_strength_reduction.py` compares 16 signed and
unsigned cases (including `i64::MIN`, negative dividends, division by 1 and
unsigned values above `i64::MAX`) with Python, natively and from
source-removed QZI; unit tests check the marking and that hinted instructions
contain no `imul`/`div`/`idiv`; the compiled example function keeps only the
`div` of its inlined decimal formatting.
