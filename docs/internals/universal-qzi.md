# Universal QZI: one compilation for every target

Audience: compiler developers.

`qz build -i` (project or explicit source files) writes one format-16 QZI
for Linux, Windows and NetBSD from a single compilation. There is no
per-target frontend run, no staging artifact and no merge of compiled
programs. The pipeline lives in `src/universal.rs`:

1. **Load** (`loader::load_program_for_all_targets`). Import discovery runs
   for each target, because `@cfg` imports decide which files exist. The
   files are parsed once, in shared source coordinates, and returned as one
   program together with the mask of targets that load each file.
2. **Target views** (`semantic::target_views`). The program, still carrying
   `@cfg`, becomes one analyzable program in which every declaration is
   valid for a known target mask.
3. **Analysis and code generation** run once on that program.
4. **Assembly** (`bytecode::unified::assemble_unified_qzi`) assigns each
   chunk its targets and writes the v16 layout directly.

## Target views

A declaration is available on `file mask & @cfg mask`. Declarations are
grouped by module and name (methods by impl and name):

- Identical conditional declarations (same text without spans and `@cfg`)
  become one declaration available on the union of their targets.
- A function or constant whose body differs between targets is copied once
  per *target class*. Classes refine its availability by the masks of its
  `@cfg` statements and of every target-dependent name it mentions. The
  first copy keeps the source name; the others are named `name@t<mask>`.
  Each copy has its `@cfg` statements resolved and carries the internal
  attribute `@__targets(mask)` when it is not available everywhere.
- Callers keep calling the source name. The QZI resolves calls by name per
  target, so a shared caller reaches the body of its target.
- A type alias defined differently per target (`std.ffi.c_long`) must be a
  non-generic alias of built-in types. Each copy that mentions it gets the
  definition of its class substituted.
- A function or constant whose *signature* differs between targets (directly,
  or through such an alias) is target-dependent: callers are copied per class
  as well, and each caller copy names the copy of its class. Imports of the
  name also import the copies.
- Target-specific structs, enums, traits and non-built-in aliases, and
  methods whose signature differs between targets, are rejected (S14).
  A method body may still differ: extra copies of trait-impl methods are
  placed in an inherent impl of the same type.

Name references are found syntactically. Finding too many only makes copies
coarser; rewriting is limited to names that resolve to a target-dependent
value and are not shadowed by a local of the item.

### Source coordinates

Analysis and lowering key facts by span, so two copies of one item cannot
share coordinates. `SpanLayout` gives every file `replicas` consecutive
images of its text: copy `r` of an item is relocated into image `r`. Each
`SourceFile` covers all of its images (and its type-import evidence is
replicated into each), so a copy still belongs to its file, module and
imports. `SpanLayout::original` maps a relocated span back to the source;
the driver renders diagnostics through it and reports a finding shared by
several copies once.

## Analysis cooperation

- `link_target_variants` adds a call edge from the first copy to each other
  copy: copies stay reachable and whole-program call facts include them.
- `merge_target_variant_retention` unions their result-retention slots into
  the first copy, which is what callers consult.
- Copies are never inline candidates; inlining would bind a shared caller to
  one target's body.
- W03 (unused function) is not reported for copies.

## Assembly

`Codegen::declared_targets` records the mask of every chunk compiled from a
declaration (monomorphizations inherit their template's). A chunk's
*inherent* targets are its declared mask, else the mask of a `@t<mask>`
marker in its name (closures and helpers of a copy), else every target.

For one target, a reference (call, `FnAddr`, vtable method) resolves to the
chunk of that exact name if it is available there, else to the single chunk
with the same source name available there. Using this rule:

- every chunk compiled from a declaration must resolve all its references on
  each of its declared targets, as a per-target compilation would require;
- each target keeps what it reaches from `main` and exports (libraries: from
  every declared chunk);
- names lose their markers, identical bodies merge, and every call points at
  a body sharing a target with its caller.

Dependency QZIs are linked into the same set with their own masks, mapped by
target name. A dependency whose public interface differs between targets is
seen through one merged interface (`bytecode::interface::merge_target_interfaces`)
in which differing declarations carry `@cfg(target_os = ...)`.

The output keeps the v16 encoding: one module of distinct bodies, one mask
per body and interface overrides for targets whose public interface differs.
`scripts/check_universal_qzi.py` verifies one analysis and one bytecode stage
per build, the layout, natives for every target (Windows under Wine),
diagnostics for missing implementations, and dependency linking.
