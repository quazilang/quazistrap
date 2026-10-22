# Bytecode (`src/bytecode/`)

## QZI Format

Platform-independent bytecode. **6 bytes/instruction**: `[opcode u8][operands 4B][flags u8]`.
Emitting QZI is target-neutral and does not compile the current project's `[cc]`
sources or require a host linker. Native inputs are used only for object/binary
outputs.

Operand layouts: **RRR** (dst/src1/src2), **RRRR** (dst/storage-base/index/length),
**RI16** (dst/unsigned imm16 LE), **MEM** (val/base/offset16 LE signed).
Negative and larger integer values use `MovConst` with an `Int(u64)` constant;
signed consumers interpret the stored two's-complement bits at their selected
width.

`FieldLoad` and `FieldStore` encode their unsigned 16-bit byte offset across
operand bytes 2 and 3. Register, constant, function, size, tag, and offset
conversions are checked before serialization; code generation returns an error
instead of wrapping a value into a smaller QZI field.

For `Load`/`Store`, flags bits 1–2 encode `MemWidth` (`00=qword`,
`01=byte`, `10=word`, `11=dword`) and bit 3 marks a signed sub-word
`Load`. Bit 0 remains `FLOAT_FLAG`. A zero flags byte therefore preserves the
legacy qword access. Explicit raw-pointer dereferences use the pointee width;
VM slots and safe-reference accesses remain qword. C fields and flexible tails
use their declared C widths; dword FLOAT_FLAG loads/stores convert native f32
and internal f64 slot values. Raw *f32 shares these C conversion rules.
For integer `Div`, `Mod`, and relational jumps, bit 2 is `UNSIGNED_FLAG`.
Zero retains signed behavior for historical bytecode. Signed `>>` lowers to
`Sar`; unsigned `>>` lowers to `Shr`.

### Opcode Groups

| Range | Group |
|-------|-------|
| `0x00–0x0F` | Data movement: `Nop`, `Mov`, `MovI`, `MovConst` |
| `0x10–0x1F` | Arithmetic/logic: `Add`–`Sar` |
| `0x20–0x2F` | Memory: `Load`, `Store`, `Lea`, `Move`, `Drop`, `Dup` |
| `0x30–0x3F` | Control: `Cmp`, `Jmp`, `Je`–`Jnz`, `CallIdx`, `CallReg`, `Ret` |
| `0x40–0x4F` | Structs: `New`, `NewObj`, `FieldLoad`, `FieldStore`, `VtblLoad` |
| `0x50–0x5F` | Runtime/foreign: `AtomicAdd`, `AtomicCas`, `MemFence`, `Spawn`, `CallCReg=0x54`, `Trap=0x55`, `CallExt=0x5D`, `Syscall=0x5E` |
| `0x60–0x6F` | Strings: `StrLen=0x60`, `StrConcat=0x61`, `StrToInt=0x62`, `StrToFloat=0x63`, `PrimToStr=0x64`, `StrAsStr=0x65` |

Key constants: `ENUM_DISCRIM_OFFSET=0`, `ENUM_PAYLOAD_OFFSET=8`, `enum_variant_alloc_size(n)=((n+1)*8).max(16)`.

`ContiguousElementAddr` (`0x28`) is a compiler-private checked dense-storage
address primitive. It takes a destination, storage base, index, initialized
length, and non-zero slot stride in `flags`; its native lowering traps before
address arithmetic when the index is out of bounds. It is not yet emitted by
source codegen: D-015 must first add provenance-validated accessors and the
matching artifact ownership boundary.

`Chunk` = fn code + const pool + name + param_count + reg_count. Return value always in `r0`.

### QZI File Layout

- Magic: `\x00QZI`
- Version: `0x0D` (v13). Readers retain compatible v2-v12 bytecode, but
  decoding does not prove current link compatibility. Pre-v12 safe-reference
  receiver/aggregate contracts and opaque outbound calls require a source
  rebuild; proven closed scalar/raw-pointer/shared-string artifacts retain
  compatibility. V1 omitted frame metadata; parameterized v6 trait interfaces
  also require rebuilding. Original ownership ABI generation survives aliases
  and reserialization. QZC v24 invalidates earlier exact/reusable caches for raw f32 conversion and floating updates; v23 introduced eager operand captures; v22 introduced source initialization and checked execution; v21 introduced runtime local-owner guards and selected-path transfers; v20 introduced loop cleanup and terminal-aware constant propagation.

Immutable golden fixtures from real historical writers (v2-v6) live in
`src/bytecode/fixtures/qzi/` with provenance in their README; `golden_*` tests
in `chunk.rs` lock the legacy reading paths against them. Era caveats: v2
chunk headers have no flags byte (intrinsic/variadic/export marks default
off), and v3 `@api` metadata is a plain string constant with `@export` names
never persisted.
- section directory: metadata, public interface, symbolic call relocations, bytecode
- metadata identifies package name/version, executable/library kind, and entry signature
- bytecode embeds a validated v5 chunk stream: names, parameters, registers,
  flags, optional export ABI metadata, constants, and instructions
- named relocations let independently compiled libraries resolve dotted symbols
- library builds retain public API roots and their whole-program dependency
  closure; the QZI linker deduplicates equivalent shared chunks and rejects
  conflicting definitions

### Const Pool Tags

| Tag | Type |
|-----|------|
| `0` | Int(i64) |
| `1` | Float(f64) |
| `2` | Str(u16_len + bytes) |
| `3` | FnAddr(u16_len + bytes) |
| `4` | Historical VtableAddr(type u16 + bytes, trait u16 + bytes); decoding only, rebuild before linking |
| `5` | ForeignSymbol(symbol + target-neutral `AbiSignature`) |
| `6` | Bytes(u32_len + exact bytes) |
| `7` | ForeignGlobal(symbol + target-neutral `AbiType`) |
| `8` | VtableLayout(type/trait u16 strings, u16 method count, ordered u16 method-symbol strings); QZI v13 |

### Runtime intrinsic IDs

IDs `37` through `41` are the private process-runtime ABI: spawn, wait,
try-wait, terminate, and close. They take only slot arguments and scalar
out-pointers; QZI v9 first admitted them, and QZI v10 carries the current
content-bound ownership-artifact envelope.

---

## Codegen (`codegen.rs`)

- Pass 1: assign fn-table indices.
- Pass 2: compile via `FnCompiler` (virtual reg allocator).
- Post-pass: inline expansion with jump-target fixup.
- Then: `elim_dead_regs` + `linear_scan_alloc` per chunk.
- Finally: validate register operands, branch targets, constant/function
  indices, consecutive intrinsic arguments, ABI metadata, and encoding limits.

### Key Codegen Behaviours

- `@syscall` → `Syscall+Ret`. `@api` → `CallArg*+CallExt+Ret`, with a
  `ForeignSymbol` constant carrying its portable ABI signature.
- `@api` wrapper chunks use a private backend symbol so an import whose local
  name equals its C symbol cannot resolve recursively to itself.
- `@export` functions remain ordinary internal-ABI chunks and are kept alive as
  roots. Codegen appends a synthetic C adapter chunk whose embedded export
  metadata names the stable dynamic symbol.
- Raw C callbacks use `CallCReg(dst, pointer_reg, signature_const)`. The
  signature is stored in an existing `ForeignSymbol` constant whose placeholder
  symbol is ignored for indirect calls. Converting an `@export` function emits
  the address of its synthetic C adapter, never its internal closure value.
- A foreign-global expression emits `MovConst(ForeignGlobal)` for the external
  data address followed by a width-aware `Load` or `Store`. QZI carries the ABI
  type so native backends do not infer C widths from host state.
- Byte constants are emitted into read-only data as `[u64 length][payload]`.
  `.len()` loads the prefix, checked indexing reads an unsigned byte, and
  `.as_ptr()` skips the prefix so C receives the first payload byte.
- Safe fixed-array, slice, and bytes indexing emits an unsigned `index < length`
  guard. This rejects negative signed indices too. Failure calls the language
  panic path when present and then executes `Trap`; `std = false` executes the
  deterministic trap directly. Unsafe C flexible-array indexing stays unchecked.
- `Lea.flags > 0` records the exact length of a virtual-register block that must
  remain contiguous and address-stable. Current codegen always emits explicit
  metadata, including `1` for scalar `&local`. QZI serialization/reading rejects
  zero because older artifacts could recycle an address-taken slot; legacy
  dependencies with implicit `Lea` metadata must be rebuilt from source.
- `FieldLoad`/`FieldStore` reuse memory-width flags for `@repr(C)` byte, word,
  dword, and qword fields, including sign extension on signed loads. `FLOAT_FLAG`
  on a dword field marks C `f32` conversion to/from internal f64 slots.
- Explicit shared/exclusive receivers use the same pointer-to-value-slot ABI as
  ordinary safe-reference parameters. `prepare_method_receiver` borrows an owned
  value block or forwards an existing reference. Field, bitfield, flexible-tail,
  and dense-metadata access load a referenced aggregate's handle before memory
  access. All Index/set/compound-update and compiler-generated call paths use
  the same adaptation. Shared `str` / `&str` is representation-identical;
  taking a shared string reference does not introduce an extra slot pointer.
- `compile_receiver_object` evaluates projected receivers once and retains the
  actual field slot for borrowing; mutating a loaded temporary is incorrect.
  Module fields remain ordinary expression resolution. Exclusive C-field
  receivers require scalar qword slots, not narrow fields or bitfields.
  `borrowed_fixed_array_address` indexes the existing descending slot pointer,
  checks bounds, and fixes the address before evaluating an assignment RHS.
- Dynamic source receivers use word zero of their fat value as the concrete
  borrowed slot. Reachability conservatively retains methods of the observed
  trait target set and their call dependencies. Do not resolve arbitrary
  generic expression annotations outside their lexical binder context. Source
  and QZI-only native backends use retained ordered descriptors. Descriptor
  targets remain address roots through dead-inline elimination. This is not
  a D-014 effect certificate.
- Named C bitfields lower to storage-unit loads, masks, shifts, and stores.
  Flexible-array field expressions produce the tail address and indexing uses
  the element's C width; packed fields remain valid unaligned x86-64 accesses.
- Const-fold: `ConstValue` in `const_map` → `MovI`/`MovConst` directly.
- `&&`/`||`: short-circuit via `Jz`/`Jnz`.
- Variadics: call-site packs coerced args into consecutive slots, emits `Lea`
  with the exact block length plus `MovI` (len), and passes both registers.
- Enum constructors: `New` + discriminant `FieldStore` at `ENUM_DISCRIM_OFFSET`, payloads at `ENUM_PAYLOAD_OFFSET+i*8`.
- `?` operator: reads discriminant, uses `.expect()` for tag lookup (no silent fallback).
- Struct: `New(size)` + `FieldStore` per field in declaration order. Field access → `FieldLoad(dst, ptr, offset)`.
- Fn-name as value: `MovConst(FnAddr(name))`. A Quazi variable callee uses
  `CallArg*+CallReg`; a `@repr(C)` callback variable uses
  `CallArg*+CallCReg` and target-specific C ABI lowering.
- Closure: `__quazi_closure_N` chunk; captures detected lexically via
  `capture_ident_names`; env struct heap-allocated; fn ptr at
  `ENUM_DISCRIM_OFFSET`, captures at `ENUM_PAYLOAD_OFFSET+i*8`; hidden env ptr
  is r0 and user parameters follow it. Reserve that entire ABI prefix before
  allocating capture temporaries. `fn` owns the environment: moves deactivate
  the source cleanup, replacement/scope/discard cleanup emits free intrinsic 4,
  and calls borrow the callee. Nested closure IDs are globally unique within a
  compilation and named-function forwarders are emitted once.
  Parent chunks with closure or forwarder `FnAddr` constants must not be reused
  independently by partial QZC restoration. QZI v7 is the affine ownership
  boundary; QZI v8 is the phase-2 layout-intrinsic boundary. Reject older public
  callable contracts and legacy synthetic chunks.

### Monomorphization

- Top-level fn `id[T]` call → `id<i32>` mangled chunk.
- Impl method `Box[i32].get` → mangled `Box.get<i32>` chunk.
- Pre-pass searches both `ItemKind::Fn` and `ItemKind::Impl`.
- `struct_generic_params` in `SemanticReport` maps struct name → param names for subst.
- Mangled format: `name<type1,type2>`.
- Generic dependency closure emits concrete callees required by a concrete
  generic method body. Specialization lookup compares resolved aliases, so
  source aliases and their concrete runtime types select the same chunk.
- Struct mono MethodCall: receiver type `Named { type_args }` → mangle call target; falls back to unmangled if mangled not in `fn_index`.

### Intrinsic Dispatch

- `INTRINSIC_MAP` HashMap; array ops via `INTRINSIC_OPCODE_MAP` HashMap.
- `quazi.size_of[T]()` and `quazi.align_of[T]()` are generic layout intrinsics.
  Codegen resolves their single concrete type argument during monomorphization
  and emits a constant, so native backends should not see their intrinsic IDs in
  executable chunks.
- Intrinsic 33 compares UTF-8 string contents lexicographically; intrinsic 34
  counts Unicode scalar values for `str.len()`. Both are dependency-free native
  loops. `bytes_len()` continues to use byte-oriented `StrLen`.
- `compile_intrinsic_fn` for `ArrayStore`/`ArrayLoad` must emit `rrr(op, param_reg, ...)` matching the callee param registers, not hardcoded `rrr(op, 0, 0, 0)`. The inline pass remaps registers via `base + r`, so hardcoded zeros become `base+0` for all operands — wrong.
- **str_variadic module call**: `ExprKind::Field` path (e.g. `io.println("{}", x)`) must pack coerced args into `(ptr, len)` before calling `format`, just like the `ExprKind::Ident` path does.

### Pattern Matching

- `PatternKind` = `Wildcard | Bind(String) | Literal(LiteralValue) | Variant { enum_name, variant, sub_patterns }`.
- `compile_pattern_match` recurses into sub_patterns.
- String literal match: `StrLen` fast-path + byte loop via intrinsic ID 23 (consecutive regs required: r_a, r_a+1).

### Format Specs

- `extract_format_specs(template)` parses `{:spec}` at compile time → `Vec<String>`.
- `strip_format_specs` rewrites `{:spec}` → `{}` for runtime `format()`.
- `coerce_with_spec(reg, spec, span)` applies int/float specs → extended `PrimToStr` tags.
- str_variadic fns use spec-aware coercion per arg slot.

### `dyn Trait`

- `coerce_to_dyn(obj_reg, type_name, trait_name)` allocates 16-byte fat pointer — `fat[0]=concrete_ptr`, `fat[8]=vtable_ptr` (via `MovConst(Vtable)` with retained method symbols).
- MethodCall on `Dyn` receiver: `FieldLoad fat[8]→vtbl_ptr`, `VtblLoad vtbl_ptr[slot]→fn_ptr`, `FieldLoad fat[0]→data_ptr`, `CallArg*+CallReg`.

### Non-Slice Iterator (`ForLoop::Each` on non-slice)

- `Array[T]` is special-cased to compile as an index loop (`len()` + `get()`). The typechecker records `Array.len`/`Array.get` monomorphizations so the specialized chunks exist in `fn_index`; codegen falls back to the unmangled name if the mangled one is missing. Note: the type checker currently accepts only fixed arrays and slices as iterables and rejects named `Array[T]` with `S01`, so this path is dormant until named iterables are re-admitted deliberately.
- `for i : &arr` strips the `Ref` and iterates the inner value directly.
- Other `Named` types use `has_next()` / `next()` protocol with `Option[T]` unwrapping.
- `Dyn` trait objects use vtable dispatch.

### For-Loop Move Semantics

- `for x : collection` moves/consumes the collection.
- Codegen calls `mark_consumed_expr` on the iterable and registers a hidden `__for_iter` drop-local so the value is cleaned up at loop exit (early returns included).
- `for x : &collection` borrows and leaves the original variable untouched.

### Index Assignment

- `arr[i] = val` is supported for `Array[T]` (dispatches to `Array.set`), slices (direct stack store), and fixed-size arrays (register move for literal index, computed address store for dynamic index).

### Local RAII Cleanup

- Codegen tracks local variables with a consuming `free(self)` hook and emits destructor calls at block exits, before returns, and on break/continue for exited body scopes. It also has a constrained structural fallback for acyclic, non-generic, non-C-layout structs without a source hook: eligible fields are cleaned in reverse declaration order. Arrays, enums, recursive/generic layouts, remain deferred; supported source-hook and structural owners release parent storage after resource/field teardown.
- Destructor roots are forced reachable so tree-shaking keeps `Type.free` and its dependencies.
- Each supported local owner has a runtime Boolean cleanup flag, reset at declaration, set after initialization/replacement and cleared on executed moves/destruction. Cleanup tests and clears it before reading/destroying the value. Parameter flags are allocated after the entire ABI prefix. Explicit by-value receivers consume; shared and exclusive receivers borrow. Consuming `free(self)` hooks return void. Full-expression named-owner temporaries retain pending argument ownership through groups/casts/match arms until the call commits, including later `?` failures.
- Block and loop exit restore lexical register/type bindings so destroyed shadows cannot replace outer owners. Cleanup lookup checks the current register as well as the name. Consumption is applied after evaluation on the selected path; post-evaluation transfers compare guarded candidate handles. A zero-argument call claims its pending span before compiling its callee. Cleanup no longer mutates compile-time active flags or needs a return snapshot. This does not prove ordinary reads definitely initialized.
- `Array[T]` and `String` locals are auto-cleaned at scope exit.
- Named source calls without a concrete layout transfer full-expression owners
  only when the callee's fixed declared capability consumes and the actual
  caller argument owns. Protect callee binders independently of caller
  substitutions; borrowed/scalar arguments and variadic pointer/count packing
  are not ownership transfers. Fixed arrays need element ownership proof.
- Assignment results alias destination storage; discarding an indexed or field
  assignment must preserve the owner deposited there.
- This is source-level cleanup, not `Drop` bytecode yet.

---

## Regalloc (`regalloc.rs`)

Two passes run on each chunk after inline expansion:

1. **`elim_dead_regs`**: iterative dead-def removal → Nop → `strip_nops_fix_jumps` → `compact_regs` (params and explicit contiguous blocks pinned to identity; other slots compacted).
2. **`linear_scan_alloc`**: builds live intervals, finds **pinned** regs (params; Intrinsic/Syscall consecutive arg groups when `flags>1`; explicit `Lea.flags` blocks; legacy Lea/adjacent-MovI groups), then linear scans non-pinned with slot reuse. Back-edge extension: for each backward jump at `j` targeting `t`, any reg with `start < t && end >= t` gets `end = j` — prevents slot recycling across loop iterations.

Key: `Ret` has `ops[0]` = return-value register (remapped by `remap_instr_regs`; encoder uses `slot(ops[0])` not hardcoded `slot(0)`).

Register operand discovery is shared by validation, allocation, and native frame
sizing so less-common opcodes cannot access slots outside the allocated frame.

## Structured control

- `FnCompiler.loop_stack` records body cleanup depth and patches break/continue jumps for every loop path. Abrupt edges read cleanup state without deactivating owners on alternate paths. C-style continue cleans the body, retains the initializer and targets the update. Break/condition exit reaches a common loop-scope cleanup epilogue.
- Constant propagation ends basic blocks at `Ret`/`Trap`, analyzes and rewrites only statically reachable blocks, and ignores unreachable predecessors. Callable entry registers are overdefined even when block zero has a backedge; analysis and rewriting share that input calculation. Unknown runtime values are not lattice Top.
- Source graph scope-exit markers alone do not certify emitted cleanup or complete destruction effects.
- `compile_else_if_chain` consumes the AST's ordered cases, shares final join
  patches and handles constant conditions without rewriting the AST.

## Recent Cleanups

- **`as` casts**: Previously a lazy no-op (`..` pattern). Now explicit with a comment. The current typechecker permits integer-family and same-family float casts; integer/float cross-family conversion needs dedicated lowering and remains rejected.
- **`Array.from` hardcoding removed**: Previously codegen had an inline `new() + push()` special case for `Array.from([...])`. Now `Array.from(...items: T)` is a real variadic method in `prelude/src/array.qz` that iterates the variadic slice and pushes each element. Generic inference for variadic static methods was fixed in `typecheck.rs` (`infer_type_subst` now handles `Slice` types).
- **`import_aliases` removed**: Unnecessary `HashMap` added by a teammate that did not change behavior; cleaned up from `Codegen` and `FnCompiler`.

## Portable vtable layouts

`vtable.rs` validates bounded tag-8 descriptors, deduplicates matching layouts,
rejects conflicts and historical layout-free addresses, and resolves complete
method bundles at linking/native emission. Serialization and raw cache decoding
validate shape without requiring companion chunks. Namespace aliases rewrite
matching type/trait/method prefixes; root qualification follows the existing
function-only map. See [the full contract](../../docs/internals/dynamic-vtable-layout.md).

The QZI source interface excludes internal-only impl declarations for proven
same-source private nominal targets/traits before public-template restrictions.
Explicit public methods on public targets survive as inherent declarations if
the trait is private; public methods on private targets get an export error.
Other-source names, aliases, and nested generic spellings never prove privacy.
Implementation bytecode and private vtable metadata remain internal dependencies.

## Source initialization checkpoint (2026-10-22)

`semantic/initialization.rs` snapshots declaration-backed initialization bits and
joins normal paths, with separate truth/falsy states, loop exits and pending
match cases. Deferred closures restore outer state. Match case/fact work is
bounded at 1,000,000 steps; exhaustion is S14. Terminal statement spans align
analysis and emission. Checking follows declared named-argument/field slots;
indirect callees and place operands precede arguments/RHS, and range bounds
precede loop binders. Place validation consumes checked annotations once.

Checked pattern discriminants are published by pattern span and used directly
by codegen. Nested refutable patterns require substituted actual payload types.
Match captures a stable scrutinee snapshot; borrowed scalar slots load, shared
&str preserves its view ABI and &str! loads. Boolean literal patterns use
zero/nonzero truthiness, while integer patterns use exact equality. Float
comparison is IEEE, string comparison uses first-NUL views. Unmatched values
trap; Void matches retain fallthrough. Constant result knowledge cannot erase
executed effects; only elidable expressions publish foldable annotations, and
unknown floating multiplication by zero is not absorbing.

The initialization checkpoint introduced **QZC22**, invalidating previous exact
and reusable
chunks for these checks and corrected execution. QZI13 remains Unverified and
unchanged; rebuild old bodies from source. Stable operand values are covered
by the subsequent QZC23 checkpoint below. Canonical Boolean storage/equality,
partial-place initialization, typed effects,
recursive destruction and verified artifacts remain open. See
[the initialization contract](../../docs/internals/source-initialization.md).

Constant propagation treats every slot in a Lea-addressed block as runtime
memory in both dataflow and rewrite. Aliased stores, calls and intrinsics
cannot leave a stale local constant. This is conservative alias handling;
non-addressable register values can still fold normally.

## Stable operand checkpoint (2026-10-22)

QZC23 rebuilds previous exact/reusable source chunks for eager value captures,
computed places and pending allocation storage. QZI13 remains Unverified.
`compile_value` freezes complete value blocks; `compile_expr` preserves actual
place storage. Adapt borrowed receivers before capture. Pending owner transfers
begin at operand evaluation and commit only at call/store. Slice pointer/length
travel together; match results retain full width. Preserve C typed access flags
when sharing place paths. Borrow checking follows declared named-argument/field
slots and scopes allocation pins separately from shared view loans. Scalar field
writes do not release a pinned aggregate. No normalized alias/effect/artifact
proof follows. See [the contract](../../docs/internals/operand-snapshots.md).

## Scalar memory checkpoint (2026-10-22)

Current source cache is **QZC24**; QZI13 remains unchanged and Unverified.
Raw f32 accesses share the C scalar descriptor and native conversion flag; safe
float references retain qword VM slots. Float ++/-- uses floating 1.0 plus
flagged Add/Sub, with a captured old value for postfix. Integer local fast paths
remain. General narrow-result normalization, f16 memory and numeric type
obligations remain separate work. See [the contract](../../docs/internals/scalar-memory.md).

## Numeric mutation checkpoint (2026-10-22)

Current source cache is **QZC25**; QZI13 remains Unverified. Numeric mutations
retain declaration-backed generic obligations and validate every proven concrete
identity after closure. Missing types/budget exhaustion fail closed. Borrowed
builtin indexing preserves element types and loads complete descriptors; Bytes
remains immutable. Known numeric reference RHS operands load even with generic
targets. General arithmetic/coercion/effect proof is still incomplete. See
[the contract](../../docs/internals/numeric-mutations.md).
