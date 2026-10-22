# Source initialization and checked match execution

Audience: Quazi users.

Reads must be initialized on every normal path reaching them. An assignment in
one branch cannot initialize a sibling branch or the implicit else:

```quazi
var result: i32;
if flag { result = 1; } else { result = 2; }
ret result;
```

A branch that returns does not need to initialize a later read. A loop that may
run zero times does not establish its body's assignments; initialize before the
loop or assign on every reachable break. C updates run after the body and cannot
initialize its first iteration. Skipped && and || operands and deferred closure
bodies do not initialize later reads.

Known scalar matches that produce values need exhaustive coverage or a catch-all
arm. A runtime unmatched value traps; Void matches may fall through. Nested
literal/variant payload patterns require actual payload types. If a qualified
constructor leaves the value untyped, annotate the binding:

```quazi
var value: Option[i32] = Option.Some(1);
ret match value { Some(1) => 7, _ => 0 };
```

When different enums share a variant spelling, annotate the scrutinee or qualify
the pattern; lowering uses the checked discriminant. Duplicate struct initializer
fields are errors. Struct fields and named call arguments are evaluated in
declaration-slot order. Indirect callees and assignment target operands precede
arguments and assignment values respectively; range bounds see outer bindings.

Boolean literal patterns test zero/nonzero truthiness, including compatible
integer values: true matches 2, false matches 0. Integer literal patterns retain
exact numeric equality. This defines matching; general Boolean storage,
equality and bitwise normalization remain separate work. Float matching uses
IEEE equality: -0.0 matches 0.0 and NaN does not match a numeric literal.
String matching currently ends at the first NUL, like runtime string views.

Absorbing expressions retain executed effects: (value = 7) * 0 must assign
value, even though the result is zero. Unknown floating values multiplied by
zero retain the operation for NaN, infinity and signed-zero behavior.

QZC22 forces older source caches to rebuild. QZI13 format and calling convention
are unchanged, but old compiled bodies must be rebuilt from source to receive
these corrections. The artifact envelope remains Unverified.
