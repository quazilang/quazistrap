# Compiler checkpoint resume

Audience: the next assistant continuing the current `dev` worktree.

## Checkpoint (2026-10-20, source statement control flow)

Added src/semantic/control_flow.rs and fourteen focused tests as the next D-014
foundation. OwnershipCallable.control_flow shares a canonical source graph
across concrete specializations, including leaf bodies. It records structured
branches/loops, C-style continue/update, return/break/continue termination,
lexical scope exits and separate deferred closure graphs. Possible ? failure
returns are conservative; whole expression regions and iteration protocols stay
opaque. No typed effects, alias/escape/move/drop proof or loan eligibility is
inferred. QZI13 stays Unverified; QZC17 invalidates older exact/reusable caches.

The construction is iterative/cancellable and globally budgeted. Metadata and
work contexts count toward the limit; invalid/overflowing construction publishes
no partial map. Structural validation checks root entry, blocks, scope chains
and transfers. Opaque expression depth is unbounded by this pass (node budget
still applies), avoiding an unnecessary compatibility regression on long sums.
Scope/deferred nesting is bounded at 64. Canonical-name helpers may allocate
before their results are charged; this is not a total frontend peak-heap bound.

Verification: full offline compiler suite 849/849; fourteen focused graph tests
and generic Arc-sharing integration pass. The 28 ownership-graph tests also
pass after adding module/method body-presence assertions. Build and direct Clippy-driver check
pass with existing warnings. Named-call and result-retention integrations pass
native/QZI/Windows COFF paths. Read-only independent reviewers challenged CFG
sequencing, nested-loop targets, scope exits, closure separation, validators and
resource bounds; root reconciled findings and corrected three new Clippy lints.
See docs/internals/source-control-flow.md and
docs/changes/2026-10-20-source-statement-flow.md for exact scope and commands.
Dynamic-vtable cold/exact/partial cache tests pass. Quazifetch source/run and
Windows COFF pass; repeated ordinary builds hit a verified v17 cache. Six
isolated STD memory tests pass with the matching compiler. Windows emission
retains four existing warnings; no Windows-host runtime is claimed.
Graphify query and AST update used. Its local AST index excludes Quazi source
semantics and is not an ownership proof. Changed Markdown link targets and
formatting/diff checks pass. Logs are /tmp/quazi-cfg-*.log. Existing
Rust ELF-loader wrappers remain necessary.

Next real dependency: normalize expression evaluation with lowering, attach typed
reads/writes/call effects to occurrence identities, then compose provenance and
recursive effects before changing ownership permissions or artifact formats.
Structural destruction, D-015 and coherent full STD migration remain unfinished.
The broader ../goal.md objective is still active; no full-readiness claim.
Preserve compiler .codex/ and example35 build outputs, STD CLI changes and editor
artifacts. No STD changes, push, PR, publication or external operations here.

## Checkpoint (2026-10-20, named calls and quazifetch RAM)

Named call emission now orders supplied operands using semantic declaration
slots. Bare method names previously missed qualified declarations or collided
with unrelated functions, silently swapping accepted named arguments. Source
spans retain one consistent mapping across concrete specializations; missing or
conflicting maps fail explicitly. Receiver operands stay separate and repeated
variadic slots preserve supplied order. This does not add a source evaluation-
order guarantee, ownership permissions or artifact-body verification. QZI13 is
unchanged; QZC16 invalidates older exact and reusable chunks.

Full compiler suite: 834/834 (831 existing plus three focused regressions).
Native/QZI-only/Windows COFF and aliased scalar QZI library imports with the
defining source removed pass scripts/check_named_arguments.py. The preceding
compiler fails the fixture at native exit 2. Independent read-only review found
no blocking issue. Named calls through erased function values still lack
parameter-name metadata and fail explicitly; a semantic diagnostic is follow-up.

User-reported quazifetch S10 errors no longer reproduce after the preceding
raw-retention fix: ordinary build/run succeed. A separate STD commit d3e3a0b
fixes Linux memory_available to use MemAvailable including reclaimable cache,
with native free-RAM fallback on unreadable/invalid procfs. Six deterministic
parser fixtures pass scripts/check_os_memory.py. Quazifetch source and QZI native
runs, repeated incremental builds, and Windows object emission pass. Live used
RAM follows installed fastfetch within sampling fluctuation (about 2.7 GiB).
Windows-host runtime and full STD checks are not claimed.

Rust checks use /tmp/quazi-named-rust-bin wrappers around the existing stable
binaries: their original Nix ELF loader disappeared after the environment
update. Build, full unit tests and cache-version regression pass with offline
Cargo and bfd. See docs/changes/2026-10-20-named-call-slots.md for commands and
lint details. Graphify query and AST update used; the local index is a navigation
aid and excludes .qz semantics. Preserve STD CLI work, compiler .codex/ and
example35 build artifacts. No push, PR, external publication or editor changes.

The broad ../goal.md readiness objective remains unfinished: D-014 CFG/effects,
structural destruction and artifact verification, D-015, and coherent full STD
migration remain outstanding. These bug fixes are a bounded checkpoint.

## Checkpoint (2026-10-18, raw-storage result retention)

The unfinished raw-memory refinement from the preceding checkpoint is now
integrated and verified. Pointer identity is separate from stored contents;
lexical, direct-allocation and unmodeled expression cells have tagged identities.
Explicit and implicit field/index/iteration loads recover stored pointer targets.
Unknown reads/writes share conservative weak facts through branch/loop merges.
Source pointer/aggregate helper results never become proven-empty storage just
because their retained parameter slots are empty. Intrinsic copy/fill and
string-pointer facts follow resolved declarations and named slots; byte-view
conversions preserve known underlying storage and uncertainty.

Valid UTF-8 range copies now remain independent after releasing their source
owner. Copied view handles keep their borrowed roots, including helper allocation,
nested pointer, temporary result and unknown-alias paths. No pointer/integer
casts, projected address-of syntax, borrowed element APIs or reference escapes
were enabled. Exact effects/projections, external/global memory, destruction
and artifact verification remain D-014 work. QZI13 stays Unverified; QZC15
invalidates v14 exact and reusable caches so source checks rerun.

Correction to the prior checkpoint: its saved (&view) as *u8 regression is
INVALID source (S06). It proved a metadata discrepancy on rejected input, not
a runtime vulnerability. New valid tests use allocated source slots or supported
direct-address argument conversion. Five of the initial eight valid memory
tests failed with the inherited dirty refinement. The previous committed
compiler also rejects the new valid owned-range copy fixture; current source
and QZI native execution pass after freeing the original owner. Both old and new
committed semantics reject the two negative CLI fixtures; do not claim new old-
compiler unsafe acceptance from them.

Verification with pinned stable Rust/bfd: full cargo test --offline -q passes
831/831, focused semantic::retention 39/39 (23 memory regressions), cache-version
guard, build and clippy pass with existing warnings. Expanded result-retention
runner passes 12 rejected / 7 valid cases, with source/QZI native and Windows
COFF. Dynamic-vtable/cache integration passes. Isolated real STD receiver suite
passes 10 tests, three S10 rejection probes, QZI execution and COFF. Full STD,
Windows host runtime, complete effect analysis and workspace readiness are not
claimed. See docs/changes/2026-10-18-raw-storage-retention.md and
/tmp/quazi-memory-retention-*.log. Independent reviewers verified range-copy
independence and the retained-owner rejection fixtures in /tmp.

Graphify query and AST update used; current diagnostic has 63 dangling edges
and 43 undirected endpoint collisions. It excludes .qz semantics. The raw
refinement files now belong to this completed local checkpoint; no unfinished
raw-memory patch from the previous checkpoint is intentionally left unstaged.
Preserve compiler .codex/, example35 build artifacts, STD CLI work and JetBrains
.gradle/. No STD/editor sources were changed. Local commits only; no push,
publication, PR, external service changes or documentation website.

Next: D-014 typed reads/writes and CFG/provenance/effect composition, structural
destruction and artifact-body verification, then D-015 and coherent Headers /
HTTP / JSON migration with full STD checks. The original ../goal.md readiness
objective remains active and substantially larger than these checkpoints.

## Checkpoint (2026-10-18, ownership call argument facts)

Resumed the prior dirty D-014 argument-place pass using Graphify and two
independent read-only reviewers. The concrete ownership graph now retains
lexically ordered actual operands, declared parameter slots, declaration-backed
storage roots, field/dereference/builtin-index paths, and deferred closure spans.
Snapshot types specialize under concrete caller substitutions. Global storage
aliases retain their declaration context through grouping/dereference; projected
temporaries and overloaded indexing remain unknown. Resource exhaustion is S14;
new work polls cancellation. Result retention shares the declared-slot mapper.

These facts do not grant loan permissions or implement a CFG, effects, place
moves, structural destruction, D-015 or artifact-body verification. QZI13 stays
Unverified; QZC14 is unchanged. See docs/internals/ownership-call-arguments.md
and docs/changes/2026-10-18-ownership-call-arguments.md.

IMPORTANT: preserve the unfinished raw-memory retention refinement in
src/semantic/retention.rs and its three Analyzer state lines in mod.rs. It was
already dirty at resume and is deliberately UNSTAGED, outside this checkpoint.
Review confirmed it loses a parameter dependency when a source helper returns
malloc storage, memcpy stores a view through it, and a wrapper returns the loaded
view. The same regression passes with HEAD retention and fails with that dirty
refinement. Evidence: /tmp/quazi-retention-review-current.log (failure),
/tmp/quazi-retention-review-head.log (pass), and the raw_memory_review_tests
module in /tmp/quazi-retention-review/retention-dirty.rs. Do not enable these
refinements without conservative source-call allocation/pointee flow and tests.

The completed staged candidate was verified separately in detached worktree
/tmp/quazi-argument-facts-verify using pinned stable Rust/bfd and target dir
/tmp/quazi-argument-facts-target: full tests 808/808, focused places 18/18,
build and clippy pass with existing warnings. Result-retention integration
passes 10 rejected / 6 valid cases with source/QZI execution and Windows COFF;
dynamic-vtable/cache integration passes. Logs: /tmp/quazi-argument-facts-*.log.
The inherited deep-parser test aborted with stack overflow; the replacement
isolates this pass via a synthetic deep operand after normal source analysis.
The full worktree with raw-memory changes is not certified by these results.

Graphify AST update is local and excludes .qz semantics. Its integrity diagnostic
reported 63 dangling endpoint edges and 40 undirected endpoint collisions.
Preserve compiler .codex/, example35 build artifacts, STD CLI work and JetBrains
.gradle/. No STD or editor sources were changed. Local commits are authorized;
no push, PR, publication, external changes or documentation website.

Next: complete D-014 typed reads/writes, alias/provenance and control-flow facts,
recursive effect composition, structural destruction and artifact verification;
then D-015 and coherent Headers/HTTP/JSON migration with full STD verification.
The original production-readiness objective in ../goal.md remains active and
much larger than this checkpoint; do not mark it complete.

## Checkpoint (2026-10-15, source aggregate result retention)

`semantic/retention.rs` derives source result parameter dependencies before
lexical borrow checking. Returned structs, nested/generic wrappers, enums,
arrays, forwarding calls, and outward assignments retain caller roots instead
of losing them at helper boundaries. Actual return dependencies compose through
recursive source calls, with named/variadic slots and legacy receivers handled
in declaration order. Static-only results and unused argument owners remain
independent. Source facts are exposed in SemanticReport::source_result_retention.

The evaluator conservatively models lexical assignments, branches, loops,
closure captures, pointer lineage and possible writes. Callee capabilities use
callee generic binders. Body-overriding intrinsic/foreign attributes cannot
prove source independence; the compiler's declared quazi.str_concat allocation
semantics preserve owned copies. Declaration-only call identity is retained
separately from complete generic targets; unresolved concrete graph edges stay
opaque. A span index avoids repeated whole-program annotation scans and is
cleared on reset/cancellation.

Lexical checking extends aggregate loans, preserves untyped qualified enum
payload roots, distinguishes moved enum payload ownership from borrowed storage,
and retains the existing narrow structural receiver-field transfer. Opaque
aggregate results containing views require source result facts. Own destructible
storage remains a local root for by-value parameter projections. No API-name or
String type-name special case was introduced.

QZC14 invalidates old exact hits and reusable units. QZI stays13, ownership
artifacts stay Unverified, and source result dependencies are not certificates.
This remains conservative parameter-slot flow: exact projection/effect/escape
facts, concrete control-flow analysis, indirect targets, destruction effects,
and artifact-body verification remain D-014 work. D-015 and full Headers/HTTP/JSON
migration remain open; do not enable borrowed element access from these facts.
See docs/internals/source-result-retention.md and
docs/changes/2026-10-15-result-retention.md.

Verification: pinned Rust with RUSTFLAGS='-C link-arg=-fuse-ld=bfd', full
`cargo test --offline -q` passes 790/790, focused result-retention 16/16.
`python3 scripts/check_result_retention.py target/debug/qz` covers 10 source
rejections during checking/building/QZI emission and 6 valid cases with source
and QZI native execution plus QZI->Windows COFF. Initial 8 cases were accepted
and crashed before the fix; the regression runner never executes rejected cases.
The old baseline evidence is /tmp/quazi-d014-review/cases/results.json;
full logs /tmp/quazi-retention-full-final.log and
/tmp/quazi-retention-integration-final.log. Build and clippy pass with existing
warnings. Dynamic-vtable/cache integration still passes, and the isolated STD
receiver script passes 10 tests, 3 rejection cases, QZI execution and Windows COFF
emission (/tmp/quazi-retention-std-net-final.log). Its first invocation used a
relative compiler path through the STD symlink and failed before launching qz;
the corrected absolute-path invocation passed. No full STD success is claimed.
Independent review findings about
legacy slots, generic aliases, overridden bodies, and ownership transfers were
reconciled with focused regressions. Graphify query and code AST update used;
its graph does not index Quazi source semantics or certify ownership.

The broader local workspace goal stays active. Preserve user STD CLI work,
compiler .codex/ and example35 build output, and JetBrains .gradle/. No push,
publication, PR, website, or unrelated repository changes are authorized.

## Checkpoint (2026-10-15, portable dynamic vtables)

Resolved the QZI-only dynamic layout gap from the preceding checkpoint.
QZI13 constant tag8 retains target-neutral type/trait identities and exact
implementation symbols in trait-slot order. Source/QZI native emission shares
that descriptor path. WPO retains descriptor method targets; final linking and
native emission reject conflicts and missing targets. Table symbols encode
identity without punctuation collisions; relocations use actual native names.
Historical tag4 addresses still decode but require a source rebuild to link.
Original ownership ABI generations and Unverified envelopes remain intact.
QZC13 invalidates older exact hits and reusable units, and preserves descriptors
through disk cache restoration and WPO.

The source-removed aliased-library regression exposed private impl declarations
leaking into the public interface without their defining nominal declarations.
Interface projection now uses proven same-source private nominal identities:
internal-only impl declarations are omitted, explicit public methods on public
targets survive as inherent declarations for private traits, and public methods
on private targets produce an actionable export error. Excluded/other-source
names, aliases, and nested generic argument spellings do not prove privacy.
General alias/composite/cross-source interface visibility closure remains open.

Two independent reviewers examined descriptor design, bytecode/cache binding,
native emission and optimization, and challenged the interface projection.
Their alias/binder/public-method findings were reconciled against the code.
Graphify query and AST update were used; the Rust/Python graph does not index
Quazi source semantics. See docs/internals/dynamic-vtable-layout.md,
docs/changes/2026-10-15-portable-vtables.md and docs/migrations/qzi-v13-vtables.md.

Verification with pinned Rust and RUSTFLAGS='-C link-arg=-fuse-ld=bfd':
`cargo test --offline -q` passes 770/770 (13 focused vtable and 13 interface
tests); build and clippy pass with existing warnings. Source and QZI-only
Linux execution plus QZI->Windows COFF pass for original and multi-method
fixtures. `python3 scripts/check_dynamic_vtables.py target/debug/qz` passes
imported trait/type dispatch with mangling=false, cold/exact/partial/updated
exact caches (1 restored | 1 compiled after implementation-only edit), and an
aliased scalar QZI library using private dyn after removing defining source.
Baseline old QZI12 failure is /tmp/quazi-vtables-before.log. Final logs are
/tmp/quazi-vtables-full-final.log, /tmp/quazi-vtables-integration-final.log,
/tmp/quazi-vtables-clippy-final.log. No Windows host execution is claimed.

Next: D-014 exact provenance/effect/body verification and D-015 element loans,
then coherent Headers/HTTP/JSON migration and full STD verification. Existing
alias/const dynamic-local coercion, dynamic cleanup, and unused-method warning
precision remain separate gaps. The full workspace objective stays active;
this checkpoint proves retained table layouts, not ownership or readiness.
Preserve unrelated compiler .codex/ and example35 build output, user STD CLI
work, and JetBrains .gradle/. Local commits authorized; no push/site/external work.

## Checkpoint (2026-10-15, resumed receiver ABI verification)

Resumed the uncommitted canonical-receiver checkpoint below and reviewed it
with two independent read-only agents. Graphify query traced the Rust receiver,
chunk, serialization, linker, and native-emission paths; its AST index was
updated after the fixes. Quazi sources remain outside that index.

Review found/fixed actual field-slot borrowing (scalar mutation and nested
handle replacement previously changed a temporary) and fixed-array reference
indexing (previously wrote the reference register). Checked array addresses
are fixed before assignment RHS evaluation, including RHS index mutation.
Exclusive narrow C fields, bitfields, and inline C aggregates are rejected with
S14; ordinary qword scalar C fields retain direct mutation, including packed
fields. See the expanded receiver change record and migration.

Verification: pinned Rust with RUSTFLAGS='-C link-arg=-fuse-ld=bfd':
`cargo test --offline -q` passes 752/752; build and clippy pass with existing
warnings. All three receiver fixtures run natively and emit Windows COFF;
static/field/array fixtures execute through QZI. Isolated real STD receiver
suite passes 10 tests, three negative lifetime checks, QZI execution and COFF.
The new native storage regression failed before its fix. Logs:
/tmp/quazi-resume-compiler-final.log, /tmp/quazi-resume-places-before.log,
/tmp/quazi-resume-places-final-focused.log, /tmp/quazi-resume-net-receivers-final.log.
The historical v11 fixture is intentionally included despite the *.qzi ignore.

Next artifact priority remains QZI-only dynamic vtables: persist ordered
implementation symbols for each (type, trait), bind them into the artifact,
preserve namespace rewrites and linker validation, and share descriptor-driven
native emission between source and QZI. Do not reconstruct layouts from public
interfaces or infer ownership certification. Alias/const dynamic-local coercion
also remains an existing source-analysis/lowering gap found during review.
D-014, D-015, full STD migration/tests, and the broader workspace objective
remain active. Preserve unrelated .codex/, example35 build output, user STD CLI
work, and JetBrains .gradle/. No push, website, or external changes.

## Checkpoint (2026-10-14, canonical receiver slots)

Fixed the confirmed native receiver corruption from the prior checkpoint.
Explicit borrowed receivers and ordinary safe-reference parameters now share a
slot-reference ABI. Owned receiver calls borrow their storage; existing refs
forward unchanged; field/dense metadata access loads the aggregate handle.
All direct/indexed/compound/format/parse/generated iterator paths adapt receivers.
Shared str/&str retains its value representation. Borrowed named Index types
now instantiate concrete element signatures and setter specializations. Checked
parse resolves its actual declared receiver capability, preserving custom parse.

Dynamic source dispatch uses the concrete fat-value slot and retains target
methods/dependencies. The test runner supplies the source report for vtables.
Discovery uses existing Dyn annotations, avoiding unscoped generic resolution.
QZI12/QZC12 reject unproved old receiver contracts and invalidate cached chunks.
Original ABI generation survives namespace rewriting. Real v11 writer fixture
v11-shared-receiver.qzi establishes historical raw-handle behavior and current
source-rebuild rejection, including direct CLI, project dependency, and module
link routes. Empty encoded interfaces cannot substitute for callable evidence.

Native static fixture covers forwarded/aliased refs, nonzero-offset fields,
replacement/readback, generic containers, indexed compound updates, scalar
methods, shared text, formatting, and checked parse. Linux native/QZI execution
and Windows COFF emission pass. Dynamic source native/COFF passes; Windows host
execution is unavailable. The old binary SIGSEGV reproduction now exits zero.
STD's isolated real receiver suite passes 10 tests plus negative lifetime, QZI,
and COFF checks. Full STD remains unverified; no std source was edited.

IMPORTANT remaining artifact gap: QZI-only dynamic vtables lack retained layout
metadata. Emitting the dynamic fixture succeeds, but compiling its QZI fails on
__vtable_DynamicCounter_Readable. Envelopes remain Unverified. D-014 exact
provenance/effects/aggregate retention/body verification, structural cleanup,
and D-015 loans remain required. Additional source-analysis generic alias/binder
and method-local generic substitution gaps are recorded in the change entry.
Do not mistake this representation fix for full ownership or readiness proof.

See docs/changes/2026-10-14-reference-receiver-abi.md for commands and exact
bounds. Logs and old/current native artifacts are /tmp/quazi-slot-ref-*.
Graphify was used to trace receiver/field/codegen connections. User authorized
local commits only. Preserve compiler .codex/ and example35 artifacts, unrelated
std README/RESUME/src/mod.qz/CLI work, and other editor outputs. Full workspace
goal stays active; no website, push, deployment, or external changes are wanted.

## Checkpoint (2026-10-14, local view provenance)

Resumed the dirty borrow.rs checkpoint and used Graphify query plus incremental
Rust/Cargo extraction and clustering (4,459 nodes / 11,147 edges). Independent
read-only reviewers identified/reconciled match-arm origin loss and capability
transfer rebasing. All work stays local; commits authorized, pushes are not.

Local view roots now include declaration identity; branch merges match roots
and retain loan region depth. Parameters retain slot provenance. Views derived
from exclusive parameters freeze mutation until scope exit; moved exclusive
capabilities rebase their active root. Scoped match environments preserve enum
payload view origins through return, aggregate storage, and temporary binding.
Untyped qualified enum constructors still contribute temporary storage roots.
The I/O tutorial fixture now prints its owner inside the match arm. QZC v11
invalidates older exact hits and reusable units; QZI remains v11/unverified.

Do not infer D-014 completion: parameter-derived aggregate retention remains the
existing unverified boundary. A blanket constructor ban broke synchronous
PanicInfo use and copying helpers, so it was removed without API/type exceptions.
Exact source result summaries and complete aggregate-origin propagation remain
required alongside the effect solver and QZI body verification. The initial
resumed patch failed an allocation-constructor regression; intermediate blanket
retention checks failed 17 compiler tests. Those approaches were reconciled;
see docs/changes/2026-10-14-view-root-provenance.md for final support limits.

Verification: compiler full suite passes 738/738; focused view suite has 19
tests. New match and exclusive-parameter negative regressions fail against HEAD's
previous implementation (baseline log /tmp/quazi-provenance-before-fix.log).
Build and clippy succeed with existing warnings; owned Rust files are formatted.
The local-owner probe passes Linux native/QZI execution and Windows COFF
emission. Match escape source receives S10 before QZI emission.

Native verification also CONFIRMED AN UNRESOLVED ABI BUG: general &Owner!
arguments hold slot pointers, but struct methods expect raw aggregate handles.
Forwarding through a free function overwrites the caller's owner slot. Direct,
one-alias and two-alias probes crash; the larger transferred-view probe exits
one. QZI/COFF emission is not proof of runtime correctness. The exact evidence,
required coherent slot-reference/receiver migration, and regression matrix are
in docs/audits/2026-10-14-reference-receiver-abi.md. Treat this as the next
runtime priority before D-015 or general borrowed-resource API claims.

Artifacts/logs: /tmp/quazi-provenance-*. Preserve unrelated compiler .codex/ and
example 35 build output, std README/RESUME/src/mod.qz/CLI work, and JetBrains
.gradle/. Std and editor repositories were inspected and left untouched. The
full workspace goal remains active; no website is wanted.

## Checkpoint (2026-10-13, HTTP receivers and dense read safety)

Resumed the full workspace goal and used the local Graphify query/index. All
work remains local; commits are explicitly authorized, pushing is not. Compiler
branch is `dev`; std is the workspace symlink to `~/.quazi/std` on `main`.
The prebuilt qz referenced an old CARGO_MANIFEST_DIR and could not load the
prelude; rebuilding with the pinned Rust/bfd setup corrected that environment.

STD now uses shared datagram/HTTP value observations and exclusive HTTP socket
operations. `serve_once` keeps a mutable client; request `send` captures its
response limit before consuming `encode`. Connection field destruction remains
implicit; server cleanup retains its consuming source hook. Exact declarations
are exercised by std/scripts/check_net_receivers.py and tests/net_receivers.qz.
The script deliberately excludes unfinished Headers/codecs and the user CLI.

Compiler ownership guards now reject owning `[]` reads, including observation,
projected roots, aliases/references, and exclusive Index implementations. Index
annotations retain the resolved method. Signature instantiation in the guard is
only a rejection proof; it does not enable new plain borrowed-reference Index
ABI behavior. Indexed replacement and exclusive remove remain accepted.
Generic helpers cannot bypass the guard: deferred source-read facts preserve
binder context and are checked against proven concrete mono identities after
closure. Closure propagates concrete provenance and revisits earlier symbolic
registrations when proof arrives. Checked resolver errors, cancellation, and
analyzer reset remain explicit. QZI remains v11 and unverified. QZC v10 invalidates v9 exact/reusable caches
so old accepted bytecode cannot bypass the strengthened analysis/lowering.

Native Array[Token] replacement/removal exposed premature cleanup at inferred
generic storage calls and discarded assignment results. Typed source-call
transfer now requires consuming declared fixed parameters plus actual caller
ownership (callee binders protected, unresolved actual binders rejected,
fixed-array ownership recursive). Concrete layouts remain authoritative.
Variadic packing and scalar/reference arguments cannot clear owner guards.
Assignment results alias destination storage and are retained on discard.

Verification: pinned/bfd full compiler suite 733/733, focused contiguous 23/23;
build and clippy succeed with existing warnings. The generic storage-call
regression fails with previous lowering; actual prelude owned-element-replacement
passes Linux native/QZI execution and Windows COFF emission after the fixes. Indexed-read regression failed
before the fix; an `extract[Item](Array[Item])` native-source fixture emitted QZI
before the generic guard and now receives S10 before codegen. The isolated STD
suite passes 10 native tests (7 new + 3 existing), 3 specific negative lifetime
checks, QZI execution of the 7 new tests, and Windows COFF emission. The Linux
socketpair test reads twice and verifies EOF after implicit connection cleanup.
Previous STD source fails the same receiver regression suite. No Windows host
execution or full HTTP roundtrip is claimed. See
`docs/changes/2026-10-13-http-receiver-ownership.md` for exact commands/bounds.

The full std qz test still fails analysis: 30 net diagnostics, one json.qz
Array[String] owning read, one prelude Array.index specialization
rejection, and two unrelated CLI errors. Current logs/probes are
/tmp/quazi-owned-reads-*, /tmp/quazi-net-receivers-*.log,
/tmp/quazi-http-full-final.log, and /tmp/quazi-generic-owned-index*. The old
unsafe generic .qzi probe was emitted only to establish the rejection regression.

Next: complete the accepted D-014 provenance/effect and artifact-summary
foundation needed by D-015, then implement checked dense-element access and
coherent Headers/HTTP/JSON migration. D-015's first slice admits local/parameter
roots, not self.items field projections or arbitrary view escapes. Do not fake
this with unsafe casts, shallow owners, or alternative storage shadows. Full
std tests and broader workspace audits remain required. Generic/scalar/enum
shell cleanup remains incomplete. The full production-readiness goal is active;
no website is wanted.

Preserve unrelated std README.md, RESUME.md, src/mod.qz, src/cli/, tests/cli.qz,
and compiler .codex/ plus examples/35-multi-slot-arrays/build/. Other repositories
were inspected and not changed; jetbrains-quazi has an untracked .gradle/.

## Checkpoint (2026-10-10, owned formatting migration)

D-013 is implemented for primitive/text conversions: Display.to_string returns
String; numeric to_str remains a borrowed static scratch view. Canonical core
text helpers return owners; codec/os/net and local user-CLI immediate-copy
callers were migrated. Formatting returns owned UTF-8, borrows String arguments,
and handles literal/dynamic templates, specs, and escaped braces. Direct format
packs its real variadic ABI; output wrappers borrow an owned formatter result.
Module call annotations no longer lose their resolved target to a duplicate.

Full-expression Named-owner scopes preallocate zero guarded slots, retain
borrowed receivers/arguments, transfer at actual call/struct/enum commits, and
clean prior untransferred arguments on question-mark failure. Root found/fixed
an enum payload dangling owner (Ok(number.to_string())). Source free(self)
hooks return void; supported hooks/structural cleanup release parent storage.
Entry stubs heap-allocate the Array[str] header on both platforms. Index now
has a shared receiver and generic indexing unwraps its declared capability.
This prevents Array/String reads from destroying the receiver. File.close now
uses &File!; old by-value close recursed via automatic File.free until stack
exhaustion. Owned-args, File-read/repeated-close, and quazifetch native probes
now succeed. Windows startup is emitted/checked, not executed on a host.

Borrow checking tracks local String-view owner provenance and conservatively
rejects local/temporary returns, retained temporary views, alias mutation,
aggregate/closure escapes, and sibling moves. This is not D-014 effects or
QZI certification. Unknown aggregate/function return captures remain unproved.
QZI11 preserves original ownership ABI origin through namespaces/serialization;
legacy owning/ambiguous contracts require source rebuilding. QZC9 invalidates
old exact hits and partial chunks. Envelopes remain Unverified.

Verification: full 728/728; final QZI focused suite 40/40 includes legacy
intrinsic 14/24/realloc rejection. Build and clippy succeed with existing
warnings. Native standalone fixtures live in prelude/tests/
(owned-display/format/output/args/file). Formatting has 78 mmap/78 munmap;
scoped number has 2/2. Injected mmap failure1/2 traps SIGILL. Independent
no-prelude native counter proved 13 drops. Codec i64.to_json and quazifetch
execute successfully. QZI roundtrip and Windows COFF pass for formatting/output.
Args/File COFF also passes after the shared Index changes. Artifacts/logs are in
/tmp/quazi-owned-*. Final reviewers hit usage limits; root reconciled their
work and found enum/startup/index/File.close gaps through native verification.

Whole canonical std qz test still fails at analysis: 39 HTTP ownership errors
in net.qz plus 2 in user-owned unfinished CLI. Do not weaken ownership to hide
these. The next task is coherent HTTP receiver/collection/view migration under
D-014/D-015 support constraints, then full std tests and broader audits. General
scalar-only/generic/enum shell destruction is still incomplete. The full goal
remains active; no website is wanted, and no push/publish is authorized.

Preserve std README.md, RESUME.md, src/mod.qz, untracked src/cli/ and tests/cli.qz.
Local surgical CLI conversions to_str have backups in /tmp/quazi-format-std-
backup-hby1er85. They must not be staged as whole user files. Clean tracked std
migration files are core.qz/codec.qz/fs.qz/net.qz/os.qz. Local migration checkpoint commits are authorized; stage only the explicitly
scoped compiler/std files. Inspect git log for their current hashes.

## Checkpoint (2026-10-09, generic binding context)

Fixed same-name generic capture across graph construction, emission closure,
borrow checking, and lowering. Source graph identities plus registration-time
symbolic/concrete provenance keep actual global type names distinct from lexical
binders, including implicit cleanup roots. Proven concrete mono arguments are
alias-normalized while registered emitted names are preserved. Transitive new
specializations use normalized types. Enum generic metadata is retained and
used for nominal arity. Borrow checking has callable binder scopes; impl scopes
begin before trait conformance. Symbolic helper emission remains necessary.

The maintainer explicitly accepted the rule that target formal parameter names
introduce local impl binders even when global aliases share the spelling.
Concrete target aliases use another spelling. Existing concrete `Holder[Token]`
and renamed/reordered binder regressions remain passing.

Verification: full suite 698/698; build and clippy succeed with existing warnings
using the pinned Rust/bfd setup.
Eight new regressions cover concrete same-name source calls, array aliases,
impl formals, symbolic helper origin, enum signatures, template copy ownership,
implicit cleanup dependencies, and analyzer reuse. `/tmp/qz-shadowed-alias-review-gdehiq79`
originally built but exited one without std; it now exits zero. Default-prelude
fixture `/tmp/quazi-formal-prelude-s5_e5ur4` also builds/runs zero, emits QZI,
and emits Windows COFF. Reviewers found and root reconciled the enum, borrow,
and injected-cleanup context gaps.

Next required work: maintainer selected D-013 option 1, `Display.to_string()`
returns owned `String`. The decision page records this, but implementation and
coordinated prelude/compiler/std/examples/QZI migration are still pending.
Current primitive numeric to_string allocates raw 80-byte buffers via intrinsic
IDs 15/16 and returns ownerless `str`; bool/text cases return borrowed data.
Do not merely change return annotations: construct real owners, handle allocation
failure, keep temporary ownership and exact destruction, and migrate consumers.
The canonical std has unrelated CLI edits; preserve them. No website is wanted.

## Checkpoint (2026-10-09, checked transparent aliases)

Verified current branch is `dev`; historical checkpoints below used `feat/test`.
Local commits: `2bce4e5` (compiler graph/aliases) and `caa5c0f` (Graphify setup).
User now explicitly authorized committing completed local changes. Preserve
unrelated changes and keep the full workspace production-readiness goal active.
The no-website clarification in `../RESUME.md` remains authoritative.

Added `src/type_resolution.rs`, a shared bounded resolver used by semantic
analysis, codegen, native ABI type lowering, and raw C-header validation.
It performs substitution lazily with lexical environments, normalizes actual
arguments before activating an alias, and rejects cycles/arity/growth before
unchecked tree copying. Protected binders cannot be renamed by nominal
canonicalization. Alias bodies do not capture caller substitutions.

Analysis preflights target-active declaration types before initial ownership and
serialization consumers. Generic binder scopes now cover function signatures,
struct/enum declarations, trait method generics, and alias definitions. Deferred
resolver errors are mandatory S14 diagnostics and reset between analyses.
Codegen shares an error sink with anonymous function compilers and rejects
malformed aliases in raw reports. Header spelling preserves callback typedefs
and target-dependent C names. This does not complete D-014 or verify QZI effects.

Verification: the pinned/bfd `cargo test --offline -q` passes 690/690; a final
trait-binder regression rerun passes after adding a global reference alias.
`cargo build --offline -q` and `cargo clippy --offline -q` succeed, with existing
warnings. Earlier full runs encountered one transient `Text file busy` test
runner failure; its isolated rerun and subsequent full suite both passed.
Resolver tests cover 14 cases; standalone header tests pass 9/9.
Native probes in `/tmp/quazi-alias-native-wwc5opt6` pass Linux execution, portable
QZI emission, Windows COFF emission, and Linux/Windows C-header generation.
Unused pointer-cycle aliases fail with S14 before codegen.

Independent agents reviewed resolver/header and semantic binder integration;
their findings were reconciled, including trait-method binder protection.
Their usage limit interrupted final review of codegen; root checked its changes
and regression coverage. Do not claim a final cross-workspace readiness review.
See `docs/changes/2026-10-09-type-alias-resolution.md` and
`docs/internals/type-alias-resolution.md` for the exact operational boundary.

## Checkpoint (2026-10-09, ownership specialization evidence)

The current workspace is `/DISK/QUAZI-PROJECTS/quazilang`, not the historical
home-directory paths below. Read `../RESUME.md`, `../goal.md`, and
`../AGENTS.md`; this is still the full production-readiness objective, with
the later clarification excluding a documentation website. Do not mark that
objective complete on the basis of this compiler checkpoint.

Resumed the prior dirty `OwnershipCallEffect` / `CallableOwnershipBodyEffects`
work without discarding it. Added a separate concrete source call graph in
`src/semantic/ownership.rs`: qualified structural identities, substituted
capabilities, source spans, explicit opaque/unavailable/unresolved boundaries,
recursive source closure, cancellation, and independent graph bounds. Source
declarations seed it; symbolic mono roots do not. Actual impl declarations now
record enclosing generics instead of guessing from dotted module prefixes.

Central emission checks reject mangled-name collisions and specialization
growth beyond 64 levels / 4,096 type nodes with S14. The graph and emission
registry have separate 16-bit callable budgets. Explicit wrong generic arity
is S08. Recorded source target structure supplements transitive generic
dependency expansion. Implicit dense-container release records its layout and
uses the same identity checks.

Current codegen still relies on some symbolic helper chunks; their names are
covered by the collision diagnostic too. Removing all template/helper emission
requires coordinated changes to generic intrinsic/mutation lowering. The source
graph neither relaxes lexical loans nor verifies a QZI envelope. Place/effect
IR, CFG/provenance, structural destruction, D-015 element loans, and artifact
verification remain required. Alias resolution also still needs independent
cycle/resource hardening; these specialization limits are not that resolver.

Verification and command details are recorded in
`docs/changes/2026-10-09-ownership-specialization-graph.md` and
`docs/internals/ownership-call-graph.md`. Twenty focused graph regressions
pass. Linux native scalar/multi-slot, Windows COFF, QZI, and negative CLI probes
pass. The complete compiler suite passes 666/666, including 20 graph tests.
Impl binder renaming/reordering is now shared with codegen, and a multi-slot
renamed receiver regression checks its hidden-sret ABI. Use the pinned Rust toolchain with
`RUSTFLAGS='-C link-arg=-fuse-ld=bfd'`: its bundled lld wrapper currently points
to a missing Nix path. Give repeated full test runs a fresh TMPDIR because the
existing LSP workspace fixture can otherwise collide with a stale PID directory.

Preserve the prior Graphify setup changes and untracked example 35 build
artifacts. Canonical std has unrelated CLI work; editor and Tree-sitter
repositories remain separate. Nothing was committed or published.

## Read first

- `/home/amapekibert/quazilang/RESUME.md`
- `/home/amapekibert/quazilang/AGENTS.md`
- `/home/amapekibert/quazilang/quazistrap/AGENTS.md`
- `/home/amapekibert/quazilang/quazistrap/src/semantic/AGENTS.md`
- the full active objective linked from the workspace resume

Commit only the completed task's owned files when the user explicitly requests
it. Preserve unrelated/prior dirty changes.

Note: the rustup proxies were unreliable during this checkpoint. Use
`$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin` directly for
cargo/rustc (1.98.0).

## Implemented in this checkpoint (2026-08-30, fourth entry)

Example-build regressions are fixed and every example now compiles:

- Semantic analysis closes generic template call dependencies for each concrete
  caller specialization and records the resulting callee layout. This prevents
  valid calls such as `Array.get[str]` from being absent during bytecode
  emission.
- Codegen resolves type aliases before matching monomorphizations, so aliases
  such as `Rune` and their concrete `u32` representation select the same
  generic chunk.
- The built-in ELF linker skips allocated GNU property notes and `.eh_frame`
  metadata from native C objects while continuing to reject unsupported loaded
  sections such as TLS.
- The standard library compares `String` values through `.as_str()` in HTTP
  parsing paths, keeping comparison operand types compatible.

Verification: `cargo test --offline -q` passes 481 tests. Examples `01` through
`35` all build using the canonical `~/.quazi/std`, including the Git-backed
example `28` after its dependency fetch.

## Implemented in this checkpoint (2026-08-30, third entry)

Phase 2 of the generic-storage design is implemented and verified:

- Codegen consumes canonical layout records through a derived mangled-name
  lookup. Multi-slot parameters reserve contiguous registers; multi-slot
  results use a hidden sret pointer and fixed `r1..rN` result slots.
- `ArrayLoad`/`ArrayStore` encode element slot count for stride and block copy.
  Array and Box wrappers lower concrete multi-slot storage directly rather
  than invoking the one-slot generic template.
- The allocator preserves multi-slot store sources, sret load effects, and
  return slots through DCE and register remapping. Missing specializations are
  code-generation errors, not template fallbacks.
- The contained LSP gained semantic identifier completion and UTF-16 position
  conversion; `docs/tooling/lsp.md` documents capabilities and limits.

Verification: `cargo test --offline` passes 465 tests, the native
`examples/35-multi-slot-arrays` project prints all expected triples, and a
`Box[[i32; 3]]` round trip exits zero. Targeted rustfmt checks and
`git diff --check` pass.

The still-open Phase 2 representation issue is values wider than the 255-slot
register-block limit. It requires an explicit indirect-value design; do not
relax the current compile error without that representation and tests.

## Prior checkpoint (2026-08-30, second entry)

Phase 1 of the generic-storage design (`docs/internals/generic-storage-layout.md`):

- `src/runtime_layout.rs`: `MoveKind` (Plain/Owned), `LayoutInfo`,
  `FnValueLayout`, `byte_size`/`align` queries, with unit tests.
- `SemanticReport::fn_value_layouts`: every concrete function and generic
  specialization records resolved parameter/variadic-element/result layouts
  during analysis (`record_fn_value_layout` in `typecheck.rs`). Keys are the
  internal name plus canonical resolved type arguments (`identity<i32>`),
  never the lossy mangled name. The `Array.set` index-assignment site now
  resolves the real symbol signature instead of a synthetic element-only one;
  concrete functions are recorded after `fn_name` resolution, excluding the
  erased `@format` pseudo-parameter.
- New `S14` gates (all guarded to skip the `Error` recovery type): enum
  payload declarations, struct field declarations (exempting `@repr(C)`,
  which has a real layout solver), fixed-array literals with multi-slot
  elements, generic struct instantiations via `StructInit`, contextual sum
  constructors, and direct `Some`/`Ok`/`Err` calls.
- Qualified enum constructor calls (`Option.Some(x)`, `Shape.Circle(5.0)`)
  are now validated in the `MethodCall` arm: unknown variant/associated
  function → `S04`, wrong arity → `S08`, unstorable payload → `S14`.
  Previously they fell through every resolution path into an untyped value
  and could reach internal codegen errors (`Option.Nope(5)`). Valid
  constructors keep the historical untyped result; the intercept preserves
  `reject_nested_owned_function_expression` and associated-function
  resolution on enum types.
- Change record `docs/changes/2026-08-30-value-layout-recording.md`; audit,
  design doc status, and AGENTS.md notes updated.

## Current verified state

- `cargo test --offline`: 451/451 (439 baseline + layout unit tests + gate
  and recorder tests).
- All 34 example projects build; example 03 runs; 32-testing 6/6.
- End-to-end probes: `Option[[i32; 3]]` rejected on bare/contextual/
  qualified paths; `Option.Nope(5)` fails in analysis; declaration gates and
  the `@repr(C)` exemption verified; nested array literals rejected.
- `git diff --check` passes; owned files are rustfmt-clean (whole-repo fmt
  remains red only on pre-existing unrelated sections).

## Review status

The golden-fixture/v2-reader review and the design-doc adversarial review
were reconciled earlier today. The phase-1 implementation diff is assigned
to a fresh read-only review; reconcile before phase 2.

## Next up

1. Phase 2: consume `fn_value_layouts` — register-block parameters/results
   in call sites and callee bodies, indirect fallback beyond the per-value
   block cap, mangled-only dispatch with hard errors (incl. the two
   Index-read fallthroughs), `size_of`/`align_of` intrinsics, stride-correct
   prelude storage. QZI v8 and QZC v6 land here.
2. Documented follow-up: qualified enum constructors are validated but
   untyped; semantic constructor resolution would type them properly.

## Current verified state

- `cargo test --offline`: 439/439.
- `git diff --check` passes.
- `cargo fmt -- --check` remains red on pre-existing unrelated formatting
  differences (`backend/target.rs`, `lexer/mod.rs`, `loader.rs`,
  `parser/format.rs`, and prior dirty sections). Do not reformat unrelated
  files; format only owned lines.

## Review status

Both checkpoint reviews completed and were reconciled:

- Reader/fixture review: verified the v2 fix byte-exact against the v2 writer,
  hand-checked every fixture assertion, and confirmed no downstream safety
  hole from the v2 flags=0 default. Fixed its three findings: a duplicated v4
  assertion (now asserts the real export-adapter chunk name), vacuous
  v6-lib relocation wording (v6 executables now assert non-empty relocations;
  docs corrected), and a missing cross-era link-conflict caveat in the README.
- Design review: found five HIGH and five MEDIUM issues in the first design
  draft (no size mechanism for prelude allocation; borrowed-`get` incoherent
  with function-long borrows and reallocation; place-level move double-destroy;
  QZI/QZC bumps misplaced at phase 4; unexpressible Clone-conditional APIs;
  plus the enum-payload storage subsystem, `take` holes, template-elimination
  breadth, the fake `Array.set` recorder signature, and doc-drift nits). The
  design doc was revised to address all of them: `size_of`/`align_of`
  intrinsics, an itemized six-piece borrowed-access machinery list with a
  container-freeze rule, place-level move-suppression rules, QZI v8/QZC v6 at
  phase 2, borrow-only `get`, shift-down `remove`, enum payload layout, and an
  expanded std migration inventory.

## Next up

1. Phase 1 of the generic-storage design: extend `src/runtime_layout.rs` into
   a size/alignment/move/drop model; turn `validate_specialized_internal_abi`
   (`src/semantic/typecheck.rs`) into a per-specialization layout recorder
   keyed by canonical substituted types (concrete declarations included; the
   `Array.set` call site must resolve the real signature; use the `_variadic`
   hook); gate multi-slot enum payloads, struct fields, and nested fixed-array
   literals with `S14`. No ABI change in phase 1.
2. Phase 2 lands QZI v8 and QZC v6 together with register-block
   parameters/results, mangled-only dispatch with hard errors (including the
   two Index-read fallthroughs), and `size_of`/`align_of` intrinsics.
3. Known design hazards are mapped in the design doc's Risks section; the
   dormant `for x : array` path (type checker rejects named iterables) is now
   documented in `src/bytecode/AGENTS.md`.
