// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Cross-basic-block constant propagation and folding pass.
//!
//! This pass operates on a compiled [`Chunk`] (post-inline-expansion, pre-regalloc)
//! and performs a classic forward dataflow analysis to find registers holding
//! compile-time-known constants, then rewrites arithmetic instructions and
//! conditional branches that can be reduced at compile time.
//!
//! Algorithm:
//! 1. Build a control-flow graph (CFG) from the flat instruction array.
//! 2. Run a worklist-based forward dataflow to compute a `ConstLattice` per
//!    register at each program point.
//! 3. Rewrite instructions whose operands are all known constants.
//! 4. Eliminate branches whose condition is statically known.

use std::collections::{HashMap, HashSet, VecDeque};

use super::chunk::{Chunk, ConstPoolEntry};
use super::instruction::{ri16, Instruction};
use super::opcode::Opcode;

// ── Lattice ───────────────────────────────────────────────────────────────────

/// Lattice of known constant values for a single virtual register.
///
/// Ordering (height): `Top > Const(_) > Bottom`.
/// - `Top`    — no reaching fact yet, for example an unprocessed predecessor.
/// - `Const`  — exactly one known value on all reaching paths.
/// - `Bottom` — runtime-unknown input/load or conflicting values; not foldable.
#[derive(Debug, Clone, PartialEq)]
enum ConstLattice {
    Top,
    Const(ConstVal),
    Bottom,
}

/// A constant value that can be held by a register.
#[derive(Debug, Clone, PartialEq)]
enum ConstVal {
    Int(i64),
    /// Stored as raw IEEE-754 bits to avoid NaN != NaN issues.
    Float(u64),
}

impl ConstLattice {
    /// Lattice meet (greatest lower bound).
    fn meet(&self, other: &ConstLattice) -> ConstLattice {
        match (self, other) {
            (ConstLattice::Top, x) | (x, ConstLattice::Top) => x.clone(),
            (ConstLattice::Bottom, _) | (_, ConstLattice::Bottom) => ConstLattice::Bottom,
            (ConstLattice::Const(a), ConstLattice::Const(b)) => {
                if a == b {
                    ConstLattice::Const(a.clone())
                } else {
                    ConstLattice::Bottom
                }
            }
        }
    }

    fn as_int(&self) -> Option<i64> {
        if let ConstLattice::Const(ConstVal::Int(v)) = self {
            Some(*v)
        } else {
            None
        }
    }

    fn as_float_bits(&self) -> Option<u64> {
        if let ConstLattice::Const(ConstVal::Float(bits)) = self {
            Some(*bits)
        } else {
            None
        }
    }
}

// ── CFG ───────────────────────────────────────────────────────────────────────

/// A basic block: contiguous range of instructions with no interior branches.
#[derive(Debug, Default, Clone)]
struct BasicBlock {
    start: usize,
    end: usize,
    succs: Vec<usize>,
    preds: Vec<usize>,
}

fn is_unconditional_jump(op: u8) -> bool {
    op == Opcode::Jmp as u8
}

fn is_conditional_branch(op: u8) -> bool {
    op == Opcode::Je as u8
        || op == Opcode::Jne as u8
        || op == Opcode::Jg as u8
        || op == Opcode::Jge as u8
        || op == Opcode::Jl as u8
        || op == Opcode::Jle as u8
        || op == Opcode::Ja as u8
        || op == Opcode::Jb as u8
        || op == Opcode::Jz as u8
        || op == Opcode::Jnz as u8
}

fn is_branch(op: u8) -> bool {
    is_unconditional_jump(op) || is_conditional_branch(op)
}

fn is_terminal(op: u8) -> bool {
    op == Opcode::Ret as u8 || op == Opcode::Trap as u8
}

/// Build a CFG from a flat instruction array.
/// Returns `(blocks, instr_to_block)`.
fn build_cfg(code: &[Instruction]) -> (Vec<BasicBlock>, Vec<usize>) {
    if code.is_empty() {
        return (vec![], vec![]);
    }

    // Pass 1: identify leaders (BB starts).
    let mut leaders: HashSet<usize> = HashSet::new();
    leaders.insert(0);

    for (i, instr) in code.iter().enumerate() {
        let op = instr.opcode;
        if is_branch(op) {
            let (_, target_u16) = instr.ri16();
            let target = target_u16 as usize;
            if target < code.len() {
                leaders.insert(target);
            }
        }
        if (is_branch(op) || is_terminal(op)) && i + 1 < code.len() {
            leaders.insert(i + 1);
        }
    }

    let mut leader_vec: Vec<usize> = leaders.into_iter().collect();
    leader_vec.sort_unstable();

    let n = leader_vec.len();
    let mut blocks: Vec<BasicBlock> = Vec::with_capacity(n);
    for (bi, &start) in leader_vec.iter().enumerate() {
        let end = if bi + 1 < n {
            leader_vec[bi + 1] - 1
        } else {
            code.len() - 1
        };
        blocks.push(BasicBlock {
            start,
            end,
            succs: Vec::new(),
            preds: Vec::new(),
        });
    }

    let block_of: Vec<usize> = {
        let mut v = vec![0usize; code.len()];
        for (bi, b) in blocks.iter().enumerate() {
            for i in b.start..=b.end {
                v[i] = bi;
            }
        }
        v
    };

    for bi in 0..n {
        let last = blocks[bi].end;
        let last_op = code[last].opcode;

        if is_unconditional_jump(last_op) {
            let (_, target_u16) = code[last].ri16();
            let target = target_u16 as usize;
            if target < code.len() {
                let succ = block_of[target];
                blocks[bi].succs.push(succ);
                blocks[succ].preds.push(bi);
            }
        } else if is_conditional_branch(last_op) {
            let (_, target_u16) = code[last].ri16();
            let target = target_u16 as usize;
            if target < code.len() {
                let succ = block_of[target];
                blocks[bi].succs.push(succ);
                blocks[succ].preds.push(bi);
            }
            if last + 1 < code.len() {
                let succ = block_of[last + 1];
                blocks[bi].succs.push(succ);
                blocks[succ].preds.push(bi);
            }
        } else if !is_terminal(last_op) {
            if bi + 1 < n {
                let succ = bi + 1;
                blocks[bi].succs.push(succ);
                blocks[succ].preds.push(bi);
            }
        }
    }

    (blocks, block_of)
}

// ── Dataflow ──────────────────────────────────────────────────────────────────

type RegState = HashMap<u8, ConstLattice>;

fn meet_states(a: &RegState, b: &RegState) -> RegState {
    let mut out = a.clone();
    for (&r, lb) in b {
        let entry = out.entry(r).or_insert(ConstLattice::Top);
        *entry = entry.meet(lb);
    }
    out
}

fn record_result(state: &mut RegState, dst: u8, result: ConstLattice, address_taken: &HashSet<u8>) {
    // Addressable slots are memory, not immutable register values. Stores,
    // calls and intrinsics may change them through aliases without a VM def.
    state.insert(
        dst,
        if address_taken.contains(&dst) {
            ConstLattice::Bottom
        } else {
            result
        },
    );
}

fn transfer(
    instr: &Instruction,
    state: &mut RegState,
    constants: &[ConstPoolEntry],
    address_taken: &HashSet<u8>,
) {
    let op = instr.opcode;
    if let Some(result) = compute_result(instr, state, constants) {
        record_result(state, instr.ops[0], result, address_taken);
        return;
    }
    if has_dest(op) {
        state.insert(instr.ops[0], ConstLattice::Bottom);
    }
}

fn compute_result(
    instr: &Instruction,
    state: &RegState,
    constants: &[ConstPoolEntry],
) -> Option<ConstLattice> {
    let op = instr.opcode;
    let ops = &instr.ops;

    if op == Opcode::MovI as u8 {
        let (_, imm) = instr.ri16();
        return Some(ConstLattice::Const(ConstVal::Int(imm as i64)));
    }

    if op == Opcode::MovConst as u8 {
        let (_, idx) = instr.ri16();
        let entry = constants.get(idx as usize)?;
        return match entry {
            ConstPoolEntry::Int(v) => Some(ConstLattice::Const(ConstVal::Int(*v))),
            ConstPoolEntry::Float(v) => Some(ConstLattice::Const(ConstVal::Float(v.to_bits()))),
            _ => Some(ConstLattice::Bottom),
        };
    }

    if op == Opcode::Mov as u8 {
        let src = ops[1];
        return Some(state.get(&src).cloned().unwrap_or(ConstLattice::Bottom));
    }

    // Determine whether the instruction is a floating-point op.
    // Float arithmetic uses the same opcodes as integer arithmetic but carries
    // FLOAT_FLAG in `instr.flags`.  We dispatch float ops first so they don't
    // accidentally hit the integer binop path (which would return Bottom).
    let is_float_instr = (instr.flags & super::instruction::FLOAT_FLAG) != 0;

    // Float binary ops (only when FLOAT_FLAG is set and both operands are float constants).
    if is_float_instr {
        let float_binop: Option<fn(f64, f64) -> f64> = match Opcode::from_u8(op) {
            Some(Opcode::Add) => Some(|a, b| a + b),
            Some(Opcode::Sub) => Some(|a, b| a - b),
            Some(Opcode::Mul) => Some(|a, b| a * b),
            Some(Opcode::Div) => Some(|a, b| a / b),
            _ => None,
        };
        if let Some(f) = float_binop {
            let la = state.get(&ops[1]).cloned().unwrap_or(ConstLattice::Bottom);
            let lb = state.get(&ops[2]).cloned().unwrap_or(ConstLattice::Bottom);
            if let (Some(ab), Some(bb)) = (la.as_float_bits(), lb.as_float_bits()) {
                let result = f(f64::from_bits(ab), f64::from_bits(bb));
                return Some(ConstLattice::Const(ConstVal::Float(result.to_bits())));
            }
            return Some(ConstLattice::Bottom);
        }
    }

    // Integer binary ops (only when FLOAT_FLAG is not set).
    let is_unsigned_instr = (instr.flags & super::instruction::UNSIGNED_FLAG) != 0;
    let int_binop: Option<fn(i64, i64) -> Option<i64>> = if is_float_instr {
        None
    } else {
        match Opcode::from_u8(op) {
            Some(Opcode::Add) => Some(|a, b| Some(a.wrapping_add(b))),
            Some(Opcode::Sub) => Some(|a, b| Some(a.wrapping_sub(b))),
            Some(Opcode::Mul) => Some(|a, b| Some(a.wrapping_mul(b))),
            Some(Opcode::Div) if is_unsigned_instr => {
                Some(|a, b| (b != 0).then(|| ((a as u64) / (b as u64)) as i64))
            }
            Some(Opcode::Div) => Some(|a, b| (b != 0).then(|| a.wrapping_div(b))),
            Some(Opcode::Mod) if is_unsigned_instr => {
                Some(|a, b| (b != 0).then(|| ((a as u64) % (b as u64)) as i64))
            }
            Some(Opcode::Mod) => Some(|a, b| (b != 0).then(|| a.wrapping_rem(b))),
            Some(Opcode::And) => Some(|a, b| Some(a & b)),
            Some(Opcode::Or) => Some(|a, b| Some(a | b)),
            Some(Opcode::Xor) => Some(|a, b| Some(a ^ b)),
            Some(Opcode::Shl) => Some(|a, b| Some(a.wrapping_shl((b & 63) as u32))),
            Some(Opcode::Shr) => {
                Some(|a, b| Some(((a as u64).wrapping_shr((b & 63) as u32)) as i64))
            }
            Some(Opcode::Sar) => Some(|a, b| Some(a.wrapping_shr((b & 63) as u32))),
            _ => None,
        }
    };
    if let Some(f) = int_binop {
        let la = state.get(&ops[1]).cloned().unwrap_or(ConstLattice::Bottom);
        let lb = state.get(&ops[2]).cloned().unwrap_or(ConstLattice::Bottom);
        if let (Some(a), Some(b)) = (la.as_int(), lb.as_int()) {
            return Some(match f(a, b) {
                Some(v) => ConstLattice::Const(ConstVal::Int(v)),
                None => ConstLattice::Bottom,
            });
        }
        return Some(ConstLattice::Bottom);
    }

    if op == Opcode::Neg as u8 {
        let l = state.get(&ops[1]).cloned().unwrap_or(ConstLattice::Bottom);
        return Some(match l.as_int() {
            Some(v) => ConstLattice::Const(ConstVal::Int(v.wrapping_neg())),
            None => ConstLattice::Bottom,
        });
    }
    if op == Opcode::Not as u8 {
        let l = state.get(&ops[1]).cloned().unwrap_or(ConstLattice::Bottom);
        return Some(match l.as_int() {
            Some(v) => ConstLattice::Const(ConstVal::Int(!v)),
            None => ConstLattice::Bottom,
        });
    }
    if op == Opcode::Inc as u8 {
        let l = state.get(&ops[0]).cloned().unwrap_or(ConstLattice::Bottom);
        return Some(match l.as_int() {
            Some(v) => ConstLattice::Const(ConstVal::Int(v.wrapping_add(1))),
            None => ConstLattice::Bottom,
        });
    }
    if op == Opcode::Dec as u8 {
        let l = state.get(&ops[0]).cloned().unwrap_or(ConstLattice::Bottom);
        return Some(match l.as_int() {
            Some(v) => ConstLattice::Const(ConstVal::Int(v.wrapping_sub(1))),
            None => ConstLattice::Bottom,
        });
    }

    None
}

fn has_dest(op: u8) -> bool {
    matches!(
        Opcode::from_u8(op),
        Some(
            Opcode::Mov
                | Opcode::MovI
                | Opcode::MovConst
                | Opcode::Add
                | Opcode::Sub
                | Opcode::Mul
                | Opcode::Div
                | Opcode::Mod
                | Opcode::Neg
                | Opcode::Not
                | Opcode::Inc
                | Opcode::Dec
                | Opcode::And
                | Opcode::Or
                | Opcode::Xor
                | Opcode::Shl
                | Opcode::Shr
                | Opcode::Sar
                | Opcode::Pow
                | Opcode::Cmp
                | Opcode::Load
                | Opcode::Lea
                | Opcode::ContiguousElementAddr
                | Opcode::Dup
                | Opcode::ArrayLoad
                | Opcode::New
                | Opcode::NewObj
                | Opcode::FieldLoad
                | Opcode::VtblLoad
                | Opcode::StrLen
                | Opcode::StrConcat
                | Opcode::StrToInt
                | Opcode::StrToFloat
                | Opcode::PrimToStr
                | Opcode::StrAsStr
                | Opcode::IntAbs
                | Opcode::IntMin
                | Opcode::IntMax
                | Opcode::FloatAbs
                | Opcode::FloatSqrt
                | Opcode::FloatFloor
                | Opcode::FloatCeil
                | Opcode::FloatRound
                | Opcode::FloatMin
                | Opcode::FloatMax
                | Opcode::Intrinsic
                | Opcode::CallIdx
                | Opcode::CallReg
                | Opcode::CallCReg
                | Opcode::Syscall
                | Opcode::CallExt
        )
    )
}

fn reachable_blocks(blocks: &[BasicBlock]) -> Vec<bool> {
    let mut reachable = vec![false; blocks.len()];
    let mut pending = vec![0];
    while let Some(block) = pending.pop() {
        if std::mem::replace(&mut reachable[block], true) {
            continue;
        }
        pending.extend(blocks[block].succs.iter().copied());
    }
    reachable
}

/// Include the real callable entry even when block zero has a backedge. Missing
/// facts are lattice Top, so they cannot represent an unknown incoming argument.
fn block_input(
    block: usize,
    blocks: &[BasicBlock],
    outputs: &[RegState],
    reachable: &[bool],
    entry: &RegState,
) -> RegState {
    let mut state = if block == 0 {
        entry.clone()
    } else {
        RegState::new()
    };
    for &pred in &blocks[block].preds {
        if reachable[pred] {
            state = meet_states(&state, &outputs[pred]);
        }
    }
    state
}

fn run_dataflow(
    blocks: &[BasicBlock],
    code: &[Instruction],
    constants: &[ConstPoolEntry],
    reachable: &[bool],
    entry: &RegState,
    address_taken: &HashSet<u8>,
) -> Vec<RegState> {
    let n = blocks.len();
    let mut out_state: Vec<RegState> = vec![RegState::new(); n];
    let mut worklist: VecDeque<usize> = (0..n).filter(|&block| reachable[block]).collect();

    while let Some(bi) = worklist.pop_front() {
        let bb = &blocks[bi];

        let mut state = block_input(bi, blocks, &out_state, reachable, entry);
        for i in bb.start..=bb.end {
            transfer(&code[i], &mut state, constants, address_taken);
        }

        if state != out_state[bi] {
            out_state[bi] = state;
            for &succ in &bb.succs {
                if !worklist.contains(&succ) {
                    worklist.push_back(succ);
                }
            }
        }
    }

    out_state
}

// ── Rewrite ───────────────────────────────────────────────────────────────────

fn add_or_find_const(chunk: &mut Chunk, val: &ConstVal) -> Option<u16> {
    match val {
        ConstVal::Int(v) => {
            for (i, c) in chunk.constants.iter().enumerate() {
                if let ConstPoolEntry::Int(existing) = c {
                    if *existing == *v {
                        return u16::try_from(i).ok();
                    }
                }
            }
            if chunk.constants.len() >= u16::MAX as usize {
                None
            } else {
                Some(chunk.add_constant(ConstPoolEntry::Int(*v)))
            }
        }
        ConstVal::Float(bits) => {
            let f = f64::from_bits(*bits);
            for (i, c) in chunk.constants.iter().enumerate() {
                if let ConstPoolEntry::Float(existing) = c {
                    if existing.to_bits() == *bits {
                        return u16::try_from(i).ok();
                    }
                }
            }
            if chunk.constants.len() >= u16::MAX as usize {
                None
            } else {
                Some(chunk.add_constant(ConstPoolEntry::Float(f)))
            }
        }
    }
}

fn emit_const_instr(chunk: &mut Chunk, i: usize, dst: u8, val: &ConstVal) {
    match val {
        ConstVal::Int(v) => {
            if (0..=u16::MAX as i64).contains(v) {
                chunk.code[i] = ri16(Opcode::MovI, dst, *v as u16);
            } else {
                if let Some(idx) = add_or_find_const(chunk, val) {
                    chunk.code[i] = ri16(Opcode::MovConst, dst, idx);
                }
            }
        }
        ConstVal::Float(_) => {
            if let Some(idx) = add_or_find_const(chunk, val) {
                chunk.code[i] = ri16(Opcode::MovConst, dst, idx);
            }
        }
    }
}

fn eval_branch(op: u8, val: i64) -> Option<bool> {
    if op == Opcode::Jz as u8 {
        return Some(val == 0);
    }
    if op == Opcode::Jnz as u8 {
        return Some(val != 0);
    }
    if op == Opcode::Je as u8 {
        return Some(val == 0);
    }
    if op == Opcode::Jne as u8 {
        return Some(val != 0);
    }
    None
}

/// Run constant propagation and folding on a single chunk in-place.
pub fn const_prop_fold(chunk: &mut Chunk) {
    if chunk.code.is_empty() {
        return;
    }

    let (blocks, _block_of) = build_cfg(&chunk.code);
    if blocks.is_empty() {
        return;
    }

    let reachable = reachable_blocks(&blocks);
    // Every incoming register is unknown. Definitions inside the function can
    // refine it, but a backedge cannot invent a constant for the first entry.
    let entry: RegState = (0..chunk.reg_count)
        .map(|reg| (reg, ConstLattice::Bottom))
        .collect();
    let address_taken: HashSet<u8> = chunk
        .code
        .iter()
        .filter(|instruction| instruction.opcode == Opcode::Lea as u8)
        .flat_map(|instruction| {
            let start = usize::from(instruction.ops[1]);
            (start..start + usize::from(instruction.flags.max(1)))
                .filter_map(|reg| u8::try_from(reg).ok())
        })
        .collect();
    let constants = chunk.constants.clone();
    let out_states = run_dataflow(
        &blocks,
        &chunk.code,
        &constants,
        &reachable,
        &entry,
        &address_taken,
    );

    let n_instrs = chunk.code.len();
    for bi in 0..blocks.len() {
        if !reachable[bi] {
            continue;
        }
        let bb_start = blocks[bi].start;
        let bb_end = blocks[bi].end;
        let mut state = block_input(bi, &blocks, &out_states, &reachable, &entry);

        for i in bb_start..=bb_end {
            if i >= n_instrs {
                break;
            }
            let instr = chunk.code[i];
            let op = instr.opcode;

            if let Some(result) = compute_result(&instr, &state, &constants) {
                if let ConstLattice::Const(ref cv) = result {
                    let already_const = op == Opcode::MovI as u8 || op == Opcode::MovConst as u8;
                    if !already_const && has_dest(op) {
                        let dst = instr.ops[0];
                        emit_const_instr(chunk, i, dst, cv);
                    }
                }
                if has_dest(op) {
                    record_result(&mut state, instr.ops[0], result, &address_taken);
                }
                continue;
            }

            if is_conditional_branch(op) {
                let (cond_reg, target) = instr.ri16();
                let cond_state = state
                    .get(&cond_reg)
                    .cloned()
                    .unwrap_or(ConstLattice::Bottom);
                if let Some(v) = cond_state.as_int() {
                    if let Some(taken) = eval_branch(op, v) {
                        if taken {
                            chunk.code[i] = ri16(Opcode::Jmp, 0, target);
                        } else {
                            chunk.code[i] = Instruction::nop();
                        }
                        continue;
                    }
                }
            }

            transfer(&chunk.code[i], &mut state, &constants, &address_taken);
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::chunk::{Chunk, ConstPoolEntry};
    use crate::bytecode::instruction::{ri16, rrr, rrr_f};
    use crate::bytecode::opcode::Opcode;

    fn make_chunk(name: &str, instrs: Vec<Instruction>) -> Chunk {
        let mut c = Chunk::new(name);
        c.code = instrs;
        c
    }

    #[test]
    fn address_taken_slots_do_not_retain_constants_across_aliased_writes() {
        for clobber in [
            rrr(Opcode::Store, 2, 1, 0),
            ri16(Opcode::CallIdx, 4, 0),
            ri16(Opcode::Intrinsic, 4, 0),
        ] {
            let mut chunk = Chunk::new("aliased");
            chunk.reg_count = 5;
            chunk.code = vec![
                ri16(Opcode::MovI, 0, 0),
                ri16(Opcode::MovI, 2, 7),
                {
                    let mut address = rrr(Opcode::Lea, 1, 0, 0);
                    address.flags = 1;
                    address
                },
                clobber,
                rrr(Opcode::Mov, 3, 0, 0),
                rrr(Opcode::Ret, 3, 0, 0),
            ];
            const_prop_fold(&mut chunk);
            assert_eq!(
                chunk.code[4].opcode,
                Opcode::Mov as u8,
                "aliased read must remain a load from its VM slot"
            );
        }
    }

    #[test]
    fn address_taken_facts_stay_unknown_across_basic_blocks() {
        let mut chunk = Chunk::new("aliased_successor");
        chunk.reg_count = 4;
        let mut address = rrr(Opcode::Lea, 1, 0, 0);
        address.flags = 1;
        chunk.code = vec![
            ri16(Opcode::MovI, 0, 0),
            ri16(Opcode::MovI, 2, 7),
            address,
            rrr(Opcode::Store, 2, 1, 0),
            ri16(Opcode::Jmp, 0, 5),
            rrr(Opcode::Mov, 3, 0, 0),
            rrr(Opcode::Ret, 3, 0, 0),
        ];
        const_prop_fold(&mut chunk);
        assert_eq!(chunk.code[5].opcode, Opcode::Mov as u8);
    }

    #[test]
    fn every_slot_in_an_addressed_block_remains_runtime_unknown() {
        let mut chunk = Chunk::new("aliased_block");
        chunk.reg_count = 6;
        let mut address = rrr(Opcode::Lea, 4, 1, 0);
        address.flags = 3;
        chunk.code = vec![
            ri16(Opcode::MovI, 1, 1),
            ri16(Opcode::MovI, 2, 2),
            ri16(Opcode::MovI, 3, 3),
            address,
            rrr(Opcode::Mov, 0, 3, 0),
            rrr(Opcode::Ret, 0, 0, 0),
        ];
        const_prop_fold(&mut chunk);
        assert_eq!(chunk.code[4].opcode, Opcode::Mov as u8);
    }

    #[test]
    fn folds_add_of_two_constants() {
        let mut chunk = make_chunk(
            "test_add",
            vec![
                ri16(Opcode::MovI, 0, 5),
                ri16(Opcode::MovI, 1, 3),
                rrr(Opcode::Add, 2, 0, 1),
                rrr(Opcode::Ret, 2, 0, 0),
            ],
        );
        const_prop_fold(&mut chunk);
        let instr = chunk.code[2];
        assert_eq!(instr.opcode, Opcode::MovI as u8);
        let (dst, imm) = instr.ri16();
        assert_eq!(dst, 2);
        assert_eq!(imm as i16 as i64, 8);
    }

    #[test]
    fn terminal_instructions_end_cfg_blocks() {
        for terminal in [Opcode::Ret, Opcode::Trap] {
            let code = vec![rrr(terminal, 0, 0, 0), ri16(Opcode::Jmp, 0, 0)];
            let (blocks, owners) = build_cfg(&code);
            assert_ne!(owners[0], owners[1]);
            assert_eq!(blocks[owners[0]].end, 0);
            assert!(blocks[owners[0]].succs.is_empty());
        }
    }

    #[test]
    fn return_constant_cannot_flow_through_a_dead_backedge() {
        let mut chunk = make_chunk(
            "return_backedge",
            vec![
                rrr(Opcode::Mov, 1, 0, 0),
                ri16(Opcode::MovI, 0, 7),
                rrr(Opcode::Ret, 0, 0, 0),
                ri16(Opcode::MovI, 0, 9),
                ri16(Opcode::Jmp, 0, 0),
            ],
        );
        chunk.param_count = 1;
        chunk.reg_count = 2;
        const_prop_fold(&mut chunk);
        assert_eq!(
            chunk.code[0].opcode,
            Opcode::Mov as u8,
            "an unknown parameter cannot become the return constant"
        );
    }

    #[test]
    fn live_backedge_meets_the_unknown_callable_entry() {
        let mut chunk = make_chunk(
            "entry_backedge",
            vec![
                rrr(Opcode::Mov, 1, 0, 0),
                ri16(Opcode::MovI, 0, 7),
                ri16(Opcode::Jmp, 0, 0),
            ],
        );
        chunk.param_count = 1;
        chunk.reg_count = 2;
        const_prop_fold(&mut chunk);
        assert_eq!(
            chunk.code[0].opcode,
            Opcode::Mov as u8,
            "the first iteration observes the caller's unknown argument"
        );
    }

    #[test]
    fn unreachable_predecessors_do_not_pollute_reachable_constants() {
        let mut chunk = make_chunk(
            "dead_predecessor",
            vec![
                ri16(Opcode::MovI, 0, 3),
                ri16(Opcode::Jmp, 0, 4),
                ri16(Opcode::MovI, 0, 9),
                ri16(Opcode::Jmp, 0, 4),
                rrr(Opcode::Mov, 1, 0, 0),
                rrr(Opcode::Ret, 1, 0, 0),
            ],
        );
        chunk.reg_count = 2;
        const_prop_fold(&mut chunk);
        assert_eq!(chunk.code[4], ri16(Opcode::MovI, 1, 3));
    }

    #[test]
    fn folds_chained_ops() {
        let mut chunk = make_chunk(
            "test_chain",
            vec![
                ri16(Opcode::MovI, 0, 2),
                ri16(Opcode::MovI, 1, 3),
                rrr(Opcode::Mul, 2, 0, 1), // 2*3=6
                ri16(Opcode::MovI, 3, 1),
                rrr(Opcode::Add, 4, 2, 3), // 6+1=7
                rrr(Opcode::Ret, 4, 0, 0),
            ],
        );
        const_prop_fold(&mut chunk);
        let instr = chunk.code[4];
        assert_eq!(instr.opcode, Opcode::MovI as u8);
        let (_, imm) = instr.ri16();
        assert_eq!(imm as i16 as i64, 7);
    }

    #[test]
    fn checked_contiguous_element_address_invalidates_its_destination() {
        let mut chunk = make_chunk(
            "checked_address_invalidates_destination",
            vec![
                ri16(Opcode::MovI, 0, 7),
                Instruction::new(Opcode::ContiguousElementAddr, [0, 1, 2, 3], 1),
                ri16(Opcode::MovI, 4, 1),
                rrr(Opcode::Add, 5, 0, 4),
                rrr(Opcode::Ret, 5, 0, 0),
            ],
        );

        const_prop_fold(&mut chunk);

        assert_eq!(chunk.code[3].opcode, Opcode::Add as u8);
    }

    #[test]
    fn eliminates_always_not_taken_jz() {
        let mut chunk = make_chunk(
            "test_jz",
            vec![
                ri16(Opcode::MovI, 0, 42),
                ri16(Opcode::Jz, 0, 3),
                ri16(Opcode::MovI, 1, 99),
                rrr(Opcode::Ret, 1, 0, 0),
            ],
        );
        const_prop_fold(&mut chunk);
        assert_eq!(chunk.code[1].opcode, Opcode::Nop as u8);
    }

    #[test]
    fn eliminates_always_not_taken_jnz() {
        let mut chunk = make_chunk(
            "test_jnz",
            vec![
                ri16(Opcode::MovI, 0, 0),
                ri16(Opcode::Jnz, 0, 3),
                ri16(Opcode::MovI, 1, 1),
                rrr(Opcode::Ret, 1, 0, 0),
            ],
        );
        const_prop_fold(&mut chunk);
        assert_eq!(chunk.code[1].opcode, Opcode::Nop as u8);
    }

    #[test]
    fn promotes_always_taken_jz_to_jmp() {
        let mut chunk = make_chunk(
            "test_jz_taken",
            vec![
                ri16(Opcode::MovI, 0, 0),
                ri16(Opcode::Jz, 0, 3),
                ri16(Opcode::MovI, 1, 99),
                rrr(Opcode::Ret, 0, 0, 0),
            ],
        );
        const_prop_fold(&mut chunk);
        assert_eq!(chunk.code[1].opcode, Opcode::Jmp as u8);
    }

    #[test]
    fn does_not_fold_unknown_param() {
        let mut chunk = Chunk::with_params("test_param", 1);
        chunk.code = vec![
            ri16(Opcode::MovI, 1, 5),
            rrr(Opcode::Add, 2, 0, 1),
            rrr(Opcode::Ret, 2, 0, 0),
        ];
        const_prop_fold(&mut chunk);
        assert_ne!(chunk.code[1].opcode, Opcode::MovI as u8);
    }

    #[test]
    fn does_not_fold_div_by_zero() {
        let mut chunk = make_chunk(
            "test_divz",
            vec![
                ri16(Opcode::MovI, 0, 10),
                ri16(Opcode::MovI, 1, 0),
                rrr(Opcode::Div, 2, 0, 1),
                rrr(Opcode::Ret, 2, 0, 0),
            ],
        );
        const_prop_fold(&mut chunk);
        assert_eq!(chunk.code[2].opcode, Opcode::Div as u8);
    }

    #[test]
    fn join_of_different_values_prevents_fold() {
        // BB0: r0=1, Jnz → BB2
        // BB1: r0=2, Jmp → BB2
        // BB2: Add r1, r0, r2  — r0 is Bottom
        let mut chunk = make_chunk(
            "test_join",
            vec![
                ri16(Opcode::MovI, 0, 1),  // 0
                ri16(Opcode::Jnz, 0, 4),   // 1 → jump to 4
                ri16(Opcode::MovI, 0, 2),  // 2
                ri16(Opcode::Jmp, 0, 4),   // 3 → jump to 4
                ri16(Opcode::MovI, 2, 0),  // 4
                rrr(Opcode::Add, 1, 0, 2), // 5 — r0 is Bottom
                rrr(Opcode::Ret, 1, 0, 0), // 6
            ],
        );
        const_prop_fold(&mut chunk);
        assert_eq!(chunk.code[5].opcode, Opcode::Add as u8);
    }

    #[test]
    fn folds_float_add() {
        let mut chunk = make_chunk("test_float", vec![]);
        let idx0 = chunk.add_constant(ConstPoolEntry::Float(1.5)) as u16;
        let idx1 = chunk.add_constant(ConstPoolEntry::Float(2.5)) as u16;
        chunk.code = vec![
            ri16(Opcode::MovConst, 0, idx0),
            ri16(Opcode::MovConst, 1, idx1),
            // Float Add requires FLOAT_FLAG — use rrr_f.
            rrr_f(Opcode::Add, 2, 0, 1),
            rrr(Opcode::Ret, 2, 0, 0),
        ];
        const_prop_fold(&mut chunk);
        let instr = chunk.code[2];
        assert_eq!(instr.opcode, Opcode::MovConst as u8);
        let (_, idx) = instr.ri16();
        if let ConstPoolEntry::Float(v) = &chunk.constants[idx as usize] {
            assert!((v - 4.0).abs() < 1e-10);
        } else {
            panic!("expected Float in const pool");
        }
    }

    #[test]
    fn folds_unsigned_division_and_remainder_as_u64() {
        for (opcode, expected) in [(Opcode::Div, i64::MAX), (Opcode::Mod, 1)] {
            let mut operation = rrr(opcode, 2, 0, 1);
            operation.flags |= super::super::instruction::UNSIGNED_FLAG;
            let mut chunk = make_chunk(
                "unsigned",
                vec![
                    ri16(Opcode::MovI, 0, u16::MAX),
                    ri16(Opcode::MovI, 1, 2),
                    operation,
                    rrr(Opcode::Ret, 2, 0, 0),
                ],
            );
            chunk.constants.push(ConstPoolEntry::Int(-1));
            chunk.code[0] = ri16(Opcode::MovConst, 0, 0);

            const_prop_fold(&mut chunk);

            let folded = chunk.code[2];
            let (_, encoded) = folded.ri16();
            let actual = if folded.opcode == Opcode::MovI as u8 {
                encoded as i64
            } else {
                assert_eq!(folded.opcode, Opcode::MovConst as u8);
                match chunk.constants.get(encoded as usize) {
                    Some(ConstPoolEntry::Int(value)) => *value,
                    other => panic!("expected integer constant, got {other:?}"),
                }
            };
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn propagated_negative_constants_remain_in_the_constant_pool() {
        let mut chunk = make_chunk(
            "negative",
            vec![
                ri16(Opcode::MovConst, 0, 0),
                rrr(Opcode::Mov, 1, 0, 0),
                rrr(Opcode::Ret, 1, 0, 0),
            ],
        );
        chunk.constants.push(ConstPoolEntry::Int(-1));

        const_prop_fold(&mut chunk);

        assert_eq!(chunk.code[1].opcode, Opcode::MovConst as u8);
        let (_, index) = chunk.code[1].ri16();
        assert_eq!(chunk.constants[index as usize], ConstPoolEntry::Int(-1));
    }
}
