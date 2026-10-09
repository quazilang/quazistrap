# Checked type-alias resolution

Audience: compiler and tooling developers.

`src/type_resolution.rs` resolves transparent aliases and concrete substitutions
without changing parsed AST nodes. Semantic analysis, native ABI lowering,
function code generation, and standalone C-header generation share this engine.
Named aggregates remain nominal; their fields are not expanded recursively.

## Lexical environments

Explicit source generic parameters shadow global aliases. Actual alias arguments
are normalized in the caller environment before entering the new alias frame,
so repeated applications such as `Id[Id[i32]]` do not form a false cycle.
An alias body sees only its own normalized parameters and global declarations;
free names in that body cannot capture a caller's substitution or binder.
A direct identity substitution `T -> T` represents a symbolic template atom.
Indirect substitution cycles are errors.

Qualified alias lookup retains the compiler's existing exact-name then leaf-name
fallback. Optional nominal canonicalization cannot rename protected parameters.
This mechanism does not change module visibility or define a new namespace model.

## Bounds and errors

Every query independently enforces these operational limits:

| Resource | Limit |
|---|---:|
| Expanded constructor depth | 64 |
| Expanded type nodes | 4,096 |
| Active aliases | 64 |
| Recursive resolver visits | 128 levels |
| Traversal and normalized-subtree copy work | 32,768 units |
| Expanded type-name bytes | 1 MiB |
| Copied type-name bytes | 16 MiB |

The engine charges known subtree costs before cloning and checks parent growth
while collecting children. It never eagerly substitutes an unchecked alias body.
Cycles, alias arity mismatches, applied type parameters, and exhausted bounds
return structured errors. Analysis reports S14 and retains `Error` only for
recovery; lowering must return a compilation error if resolution fails.

## Consumers

Analysis checks target-active declaration types before serialization layouts or
ownership summaries, even for unused declarations. Its deferred diagnostic sink
allows read-only type helpers to retain failures, is cleared on reset, and is
flushed before returning the report. Generic scopes cover signature validation,
aggregate declarations, alias definitions, and function bodies.

Header generation independently validates raw parsed programs before walking
aliases for C spelling. It preserves callback typedef names and target-dependent
C integer names. The shared resolver does not certify QZI ownership metadata or
solve whole-program loan effects. Existing semantic cancellation polls remain;
a bounded individual resolver query is an atomic operation.
