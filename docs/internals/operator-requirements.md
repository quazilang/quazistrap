# Bitwise, shift and ordering requirements

Audience: compiler developers.

`semantic/arithmetic.rs` extends its bounded declaration-slot facts to bitwise
operators, shifts and ordering. The pair categories are operation-specific:

| Operation | Supported operands | Result |
| --- | --- | --- |
| & / pipe / ^ | Integer pair or Bool pair | Left value type |
| << / >> | Integer pair | Left integer type |
| < / <= / > / >= | Integer pair, float pair, or recognized string pair | Bool |

Bitwise Boolean support is established by the existing semantic regression and
Boolean constant evaluator. Float bitwise and Bool shifts are rejected. This
checkpoint does not define normalization for noncanonical Bool storage or new
shift-count/narrow-width rules. Right shifts retain signed/unsigned selection.
Ordering does not reuse arithmetic's directional numeric compatibility: mixed
float/integer pairs are rejected. Different declaration parameters may defer
ordering category checks because its result shape is always Bool; general
arithmetic inference for distinct T/U is unchanged.

Known numeric, Boolean, pointer and string-view reference operands mark physical
loads independently of symbolic peers. A generic actual reference does not
retroactively supply a missing template load. Shared string-view actuals retain
their representation exception; unsupported exclusive chains fail with S06.
See [reference representation](arithmetic.md).

Parameter identity precedes nominal classification. A binder named String is a
declaration parameter, not the standard owner. Source nominal-string markers
require Named types, matching concrete resolution. Every proven concrete
identity must satisfy its recorded pair categories after specialization closure.
Missing operand types fail S14. Bounds/reset/cancellation remain shared with
[the mutation facts](numeric-mutations.md); no effect/artifact proof is added.

Recognized owned String comparisons preserve existing content comparison and
Bool results, including mixed owned/view operands. String recognition still
uses the compiler's existing nominal-name convention. A counterfeit String
layout remains a confirmed separate gap: these category checks do not prove
String ABI identity or storage validity. See [remaining findings](../audits/2026-10-22-operator-followups.md).

QZC27 invalidates older source chunks, including missing Boolean reference loads
and generic category checks. QZI13 is unchanged and Unverified; rebuild old
bodies from source. Pow lowering remains unrepaired; its
[approved contract](../decisions/power-operator.md) requires a separate implementation. Mixed numeric coercion, numeric narrowing,
String ABI validation and complete ownership/effect proof also remain open.
See [verification](../changes/2026-10-22-operator-requirements.md).
