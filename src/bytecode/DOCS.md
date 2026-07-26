# Bytecode (`src/bytecode/`)

## VBC Format

Platform-independent, AOT-only. **6 bytes/instruction**: `[opcode u8][operands 4B][flags u8]`.

Operand layouts: **RRR** (dst/src1/src2), **RI16** (dst/imm16 LE), **MEM** (val/base/offset16 LE signed).

### Opcode Groups

| Range | Group |
|-------|-------|
| `0x00–0x0F` | Data movement: `Nop`, `Mov`, `MovI`, `MovConst` |
| `0x10–0x1F` | Arithmetic/logic: `Add`–`Sar` |
| `0x20–0x2F` | Memory: `Load`, `Store`, `Lea`, `Move`, `Drop`, `Dup` |
| `0x30–0x3F` | Control: `Cmp`, `Jmp`, `Je`–`Jnz`, `CallIdx`, `CallReg`, `Ret` |
| `0x40–0x4F` | Structs: `New`, `NewObj`, `FieldLoad`, `FieldStore`, `VtblLoad` |
| `0x50–0x5F` | Foreign: `AtomicAdd`, `AtomicCas`, `MemFence`, `Spawn`, `CallExt=0x5D`, `Syscall=0x5E` |
| `0x60–0x6F` | Strings: `StrLen=0x60`, `StrConcat=0x61`, `StrToInt=0x62`, `StrToFloat=0x63`, `PrimToStr=0x64`, `StrAsStr=0x65` |

Key constants: `ENUM_DISCRIM_OFFSET=0`, `ENUM_PAYLOAD_OFFSET=8`, `enum_variant_alloc_size(n)=((n+1)*8).max(16)`.

`Chunk` = fn code + const pool + name + param_count + reg_count. Return value always in `r0`.

### VBC File Layout

- Magic: `\x00VBC`
- Version: `0x02`
- chunk_count: u32 LE
- Per chunk: name, param_count, reg_count, consts, instrs

### Const Pool Tags

| Tag | Type |
|-----|------|
| `0` | Int(i64) |
| `1` | Float(f64) |
| `2` | Str(u16_len + bytes) |
| `3` | FnAddr(u16_len + bytes) |
| `4` | VtableAddr(type u16 + bytes, trait u16 + bytes) |

---

## Codegen (`codegen.rs`)

- Pass 1: assign fn-table indices.
- Pass 2: compile via `FnCompiler` (virtual reg allocator).
- Post-pass: inline expansion with jump-target fixup.
- Then: `elim_dead_regs` + `linear_scan_alloc` per chunk.

### Key Codegen Behaviours

- `@syscall` → `Syscall+Ret`. `@api` → `CallExt+Ret`.
- Const-fold: `ConstValue` in `const_map` → `MovI`/`MovConst` directly.
- `&&`/`||`: short-circuit via `Jz`/`Jnz`.
- Variadics: call-site packs coerced args into consecutive slots, emits `Lea` (ptr) + `MovI` (len), passes as two registers to callee.
- Enum constructors: `New` + discriminant `FieldStore` at `ENUM_DISCRIM_OFFSET`, payloads at `ENUM_PAYLOAD_OFFSET+i*8`.
- `?` operator: reads discriminant, uses `.expect()` for tag lookup (no silent fallback).
- Struct: `New(size)` + `FieldStore` per field in declaration order. Field access → `FieldLoad(dst, ptr, offset)`.
- Fn-name as value: `MovConst(FnAddr(name))`. Variable callee: `CallArg*+CallReg`.
- Closure: `__quazi_closure_N` chunk; captures detected via `capture_ident_names`; env struct heap-allocated; fn ptr at `ENUM_DISCRIM_OFFSET`, captures at `ENUM_PAYLOAD_OFFSET+i*8`; hidden env ptr in r0 on call.

### Monomorphization

- Top-level fn `id[T]` call → `id<i32>` mangled chunk.
- Impl method `Box[i32].get` → mangled `Box.get<i32>` chunk.
- Pre-pass searches both `ItemKind::Fn` and `ItemKind::Impl`.
- `struct_generic_params` in `SemanticReport` maps struct name → param names for subst.
- Mangled format: `name<type1,type2>`.
- Struct mono MethodCall: receiver type `Named { type_args }` → mangle call target; falls back to unmangled if mangled not in `fn_index`.

### Intrinsic Dispatch

- `INTRINSIC_MAP` HashMap; array ops via `INTRINSIC_OPCODE_MAP` HashMap.
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

- `coerce_to_dyn(obj_reg, type_name, trait_name)` allocates 16-byte fat pointer — `fat[0]=concrete_ptr`, `fat[8]=vtable_ptr` (via `MovConst(VtableAddr)`).
- MethodCall on `Dyn` receiver: `FieldLoad fat[8]→vtbl_ptr`, `VtblLoad vtbl_ptr[slot]→fn_ptr`, `FieldLoad fat[0]→data_ptr`, `CallArg*+CallReg`.

### Non-Slice Iterator (`ForLoop::Each` on non-slice)

- `Array[T]` is special-cased to compile as an index loop (`len()` + `get()`). The typechecker records `Array.len`/`Array.get` monomorphizations so the specialized chunks exist in `fn_index`; codegen falls back to the unmangled name if the mangled one is missing.
- `for i : &arr` strips the `Ref` and iterates the inner value directly.
- Other `Named` types use `has_next()` / `next()` protocol with `Option[T]` unwrapping.
- `Dyn` trait objects use vtable dispatch.

### For-Loop Move Semantics

- `for x : collection` moves/consumes the collection.
- Codegen calls `mark_consumed_expr` on the iterable and registers a hidden `__for_iter` drop-local so the value is cleaned up when the enclosing scope exits (early returns included).
- `for x : &collection` borrows and leaves the original variable untouched.

### Index Assignment

- `arr[i] = val` is supported for `Array[T]` (dispatches to `Array.set`), slices (direct stack store), and fixed-size arrays (register move for literal index, computed address store for dynamic index).

### Local RAII Cleanup

- Codegen tracks local variables whose resolved `Named` type has a `free(self)` method and emits destructor calls at block exits and before returns.
- Destructor roots are forced reachable so tree-shaking keeps `Type.free` and its dependencies.
- Values moved into returns/call args are deactivated to avoid double-free; method receivers remain non-consuming to match existing `self: Array[T]` APIs.
- `Array[T]` and `String` locals are auto-cleaned at scope exit.
- This is source-level cleanup, not `Drop` bytecode yet.

---

## Regalloc (`regalloc.rs`)

Two passes run on each chunk after inline expansion:

1. **`elim_dead_regs`**: iterative dead-def removal → Nop → `strip_nops_fix_jumps` → `compact_regs` (params pinned to identity; rest sequential).
2. **`linear_scan_alloc`**: builds live intervals, finds **pinned** regs (params; Intrinsic/Syscall consecutive arg groups when `flags>1`; Lea(offset=0) adjacent MovI base groups), then linear scans non-pinned with slot reuse. Back-edge extension: for each backward jump at `j` targeting `t`, any reg with `start < t && end >= t` gets `end = j` — prevents slot recycling across loop iterations.

Key: `Ret` has `ops[0]` = return-value register (remapped by `remap_instr_regs`; encoder uses `slot(ops[0])` not hardcoded `slot(0)`).

## Missing / Planned

- **`break` / `continue`**: No codegen support. Will need a loop-context stack in `FnCompiler` tracking `(loop_start_label, loop_end_label)` so `break` emits `Jmp` to `loop_end` and `continue` emits `Jmp` to `loop_start`.
- **`else if` chains**: Currently compiled as nested `If` inside `else_block` (two jumps per branch: `Jz` over then-block + `Jmp` over else-block). A flattened AST would allow chained `Jz` jumps with a single final `Jmp` to the end, saving one jump per `else if` branch. See P1 roadmap in `DOCS.md`.

## Recent Cleanups

- **`as` casts**: Previously a lazy no-op (`..` pattern). Now explicit with a comment: VBC uses 64-bit slots for all values, so integer/float size changes are no-ops at the bytecode level; the typechecker has already validated.
- **`Array.from` hardcoding removed**: Previously codegen had an inline `new() + push()` special case for `Array.from([...])`. Now `Array.from(...items: T)` is a real variadic method in `prelude/src/array.qz` that iterates the variadic slice and pushes each element. Generic inference for variadic static methods was fixed in `typecheck.rs` (`infer_type_subst` now handles `Slice` types).
- **`import_aliases` removed**: Unnecessary `HashMap` added by a teammate that did not change behavior; cleaned up from `Codegen` and `FnCompiler`.
