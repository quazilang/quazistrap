// quazi - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap, HashMap, HashSet};

use super::chunk::Chunk;
use super::instruction::Instruction;
use super::opcode::Opcode;

pub fn elim_dead_regs(chunk: &mut Chunk) {
    loop {
        let use_set = compute_use_set(&chunk.code);
        let dead = find_dead_defs(&chunk.code, &use_set);
        if dead.is_empty() {
            break;
        }
        for idx in dead {
            chunk.code[idx] = Instruction::nop();
        }
    }
    strip_nops_fix_jumps(chunk);
    compact_regs(chunk);
}

fn strip_nops_fix_jumps(chunk: &mut Chunk) {
    // Nops are deleted. Build old-index → new-index map.
    // If an instruction is a Nop, jumping to it is equivalent to jumping to
    // the next valid instruction. So we map Nop to the current `new_idx`.
    let mut new_idx = 0usize;
    let mut old_to_new: Vec<usize> = Vec::with_capacity(chunk.code.len());
    
    for ins in &chunk.code {
        old_to_new.push(new_idx);
        if ins.opcode != Opcode::Nop as u8 {
            new_idx += 1;
        }
    }
    // Also push a mapping for the end of the chunk (length).
    old_to_new.push(new_idx);

    if new_idx == chunk.code.len() {
        return; // No Nops to strip.
    }

    // Fix jump targets before stripping.
    for instr in &mut chunk.code {
        let op = instr.opcode;
        let is_jump = op == Opcode::Jmp as u8
            || op == Opcode::Je as u8
            || op == Opcode::Jne as u8
            || op == Opcode::Jg as u8
            || op == Opcode::Jge as u8
            || op == Opcode::Jl as u8
            || op == Opcode::Jle as u8
            || op == Opcode::Ja as u8
            || op == Opcode::Jb as u8
            || op == Opcode::Jz as u8
            || op == Opcode::Jnz as u8;
        if !is_jump {
            continue;
        }
        let old_target = u16::from_le_bytes([instr.ops[1], instr.ops[2]]) as usize;
        let new_target = old_to_new
            .get(old_target)
            .copied()
            .unwrap_or(old_target)
            .min(u16::MAX as usize) as u16;
        let [lo, hi] = new_target.to_le_bytes();
        instr.ops[1] = lo;
        instr.ops[2] = hi;
    }

    chunk.code.retain(|ins| ins.opcode != Opcode::Nop as u8);
}

fn compute_use_set(code: &[Instruction]) -> HashSet<u8> {
    let mut set = HashSet::new();
    for instr in code {
        for r in instr_uses(instr) {
            set.insert(r);
        }
    }
    // Lea(rdst, rbase, 0) creates a pointer to rbase's stack slot; rbase..rbase+N-1 are
    // accessed via pointer arithmetic without direct register reads.
    // Two patterns emitted by codegen:
    //   Variadic args:    Lea(ptr, base, 0)  followed by  MovI(len, N)  — N = arg count
    //   Static array iter: MovI(len, N)  followed by  Lea(base_addr, ptr, 0) — N = array length
    for i in 0..code.len().saturating_sub(1) {
        if code[i].opcode != Opcode::Lea as u8 {
            continue;
        }
        let offset = i16::from_le_bytes([code[i].ops[2], code[i].ops[3]]);
        if offset != 0 {
            continue;
        }
        let base = code[i].ops[1];
        let n_after = if code[i + 1].opcode == Opcode::MovI as u8 {
            u16::from_le_bytes([code[i + 1].ops[1], code[i + 1].ops[2]])
        } else {
            0
        };
        let n_before = if i > 0 && code[i - 1].opcode == Opcode::MovI as u8 {
            u16::from_le_bytes([code[i - 1].ops[1], code[i - 1].ops[2]])
        } else {
            0
        };
        let n = n_after.max(n_before);
        for j in 0..n {
            set.insert(base.wrapping_add(j as u8));
        }
    }
    set
}

fn find_dead_defs(code: &[Instruction], use_set: &HashSet<u8>) -> Vec<usize> {
    let mut dead = Vec::new();
    for (i, instr) in code.iter().enumerate() {
        let Some(op) = Opcode::from_u8(instr.opcode) else {
            continue;
        };
        let Some(dst) = instr_def(instr) else {
            continue;
        };
        if !use_set.contains(&dst) && is_side_effect_free(op) {
            dead.push(i);
        }
    }
    dead
}

fn compact_regs(chunk: &mut Chunk) {
    let mut all_regs: BTreeSet<u8> = BTreeSet::new();
    for instr in &chunk.code {
        if let Some(d) = instr_def(instr) {
            all_regs.insert(d);
        }
        for r in instr_uses(instr) {
            all_regs.insert(r);
        }
    }

    // Param regs 0..param_count are pinned by calling convention.
    let param_count = chunk.param_count as u8;
    let mut remap: HashMap<u8, u8> = HashMap::new();
    for p in 0..param_count {
        remap.insert(p, p);
    }
    let mut next = param_count;
    for r in all_regs {
        if remap.contains_key(&r) {
            continue;
        }
        remap.insert(r, next);
        next += 1;
    }

    for instr in &mut chunk.code {
        super::codegen::remap_instr_regs(instr, |r| *remap.get(&r).unwrap_or(&r));
    }
    chunk.reg_count = next;
}

pub fn linear_scan_alloc(chunk: &mut Chunk) {
    if chunk.code.is_empty() {
        return;
    }

    let pinned = compute_pinned(chunk);
    let intervals = compute_intervals(chunk);

    if intervals.is_empty() {
        return;
    }

    // Sort intervals by start position.
    let mut sorted: Vec<(usize, usize, u8)> =
        intervals.iter().map(|(&r, &(s, e))| (s, e, r)).collect();
    sorted.sort_unstable();

    let mut slot_map: HashMap<u8, u8> = HashMap::new();
    // Min-heap by interval end for active non-pinned intervals.
    let mut active: BinaryHeap<Reverse<(usize, u8)>> = BinaryHeap::new();
    // Min-heap of available slot numbers for reuse (smallest slot first).
    let mut free_slots: BinaryHeap<Reverse<u8>> = BinaryHeap::new();
    let mut next_fresh: u8 = 0;
    let mut max_slot: u8 = 0;

    for (start, end, reg) in sorted {
        // Expire intervals whose end is strictly before this interval's start.
        while let Some(&Reverse((ae, ar))) = active.peek() {
            if ae < start {
                active.pop();
                if let Some(&s) = slot_map.get(&ar) {
                    free_slots.push(Reverse(s));
                }
            } else {
                break;
            }
        }

        if pinned.contains(&reg) {
            slot_map.insert(reg, reg);
            if reg > max_slot {
                max_slot = reg;
            }
        } else {
            // Reuse a free slot (skipping any that coincide with a pinned number) or
            // allocate fresh, skipping pinned slot numbers.
            let slot = loop {
                if let Some(Reverse(s)) = free_slots.pop() {
                    if !pinned.contains(&s) {
                        if s > max_slot {
                            max_slot = s;
                        }
                        break s;
                    }
                } else {
                    while pinned.contains(&next_fresh) {
                        next_fresh = next_fresh.wrapping_add(1);
                    }
                    let s = next_fresh;
                    next_fresh = next_fresh.wrapping_add(1);
                    if s > max_slot {
                        max_slot = s;
                    }
                    break s;
                }
            };
            slot_map.insert(reg, slot);
            active.push(Reverse((end, reg)));
        }
    }

    for instr in &mut chunk.code {
        super::codegen::remap_instr_regs(instr, |r| *slot_map.get(&r).unwrap_or(&r));
    }

    chunk.reg_count = max_slot + 1;
}

// Registers that must keep their current slot numbers.
// Params obey the calling convention. Consecutive groups (Lea/Intrinsic/Syscall)
// must stay consecutive because the encoder infers adjacent slots by arithmetic.
fn compute_pinned(chunk: &Chunk) -> HashSet<u8> {
    let mut pinned = HashSet::new();

    // Params r0..r_{param_count-1} are fixed by the calling convention.
    for p in 0..chunk.param_count as u8 {
        pinned.insert(p);
    }

    // Intrinsic/Syscall with flags>1 access ops[0]..ops[0]+flags-1 consecutively.
    for instr in &chunk.code {
        let op = instr.opcode;
        if (op == Opcode::Intrinsic as u8 || op == Opcode::Syscall as u8) && instr.flags > 1 {
            for i in 0..instr.flags {
                pinned.insert(instr.ops[0].wrapping_add(i));
            }
        }
    }

    // Lea(dst, base, 0) adjacent to MovI(_, N): base..base+N-1 must stay consecutive
    // because Lea creates a pointer into that contiguous stack region.
    for i in 0..chunk.code.len() {
        let instr = &chunk.code[i];
        if instr.opcode != Opcode::Lea as u8 {
            continue;
        }
        let offset = i16::from_le_bytes([instr.ops[2], instr.ops[3]]);
        if offset != 0 {
            continue;
        }
        let base = instr.ops[1];
        let n_after = if i + 1 < chunk.code.len() && chunk.code[i + 1].opcode == Opcode::MovI as u8
        {
            u16::from_le_bytes([chunk.code[i + 1].ops[1], chunk.code[i + 1].ops[2]])
        } else {
            0
        };
        let n_before = if i > 0 && chunk.code[i - 1].opcode == Opcode::MovI as u8 {
            u16::from_le_bytes([chunk.code[i - 1].ops[1], chunk.code[i - 1].ops[2]])
        } else {
            0
        };
        let n = n_after.max(n_before);
        for j in 0..n {
            pinned.insert(base.wrapping_add(j as u8));
        }
    }

    pinned
}

fn compute_intervals(chunk: &Chunk) -> HashMap<u8, (usize, usize)> {
    let mut intervals: HashMap<u8, (usize, usize)> = HashMap::new();

    // Params are live from the function entry.
    for p in 0..chunk.param_count as u8 {
        intervals.insert(p, (0, 0));
    }

    for (i, instr) in chunk.code.iter().enumerate() {
        if let Some(dst) = instr_def(instr) {
            let e = intervals.entry(dst).or_insert((i, i));
            if i < e.0 {
                e.0 = i;
            }
            if i > e.1 {
                e.1 = i;
            }
        }
        for r in instr_uses(instr) {
            let e = intervals.entry(r).or_insert((i, i));
            if i < e.0 {
                e.0 = i;
            }
            if i > e.1 {
                e.1 = i;
            }
        }
    }

    // Extend intervals across loop back-edges.
    // A backward jump at position `j` targeting `t < j` forms a loop body [t, j].
    // Variables defined before the loop (start < t) but with a use inside it (end >= t)
    // must remain live until the back-edge — otherwise linear scan recycles their slot
    // during the loop body and corrupts values on the second iteration.
    for j in 0..chunk.code.len() {
        let instr = &chunk.code[j];
        let is_jump = matches!(
            Opcode::from_u8(instr.opcode),
            Some(
                Opcode::Jmp
                    | Opcode::Je
                    | Opcode::Jne
                    | Opcode::Jg
                    | Opcode::Jge
                    | Opcode::Jl
                    | Opcode::Jle
                    | Opcode::Ja
                    | Opcode::Jb
                    | Opcode::Jz
                    | Opcode::Jnz
            )
        );
        if !is_jump {
            continue;
        }
        let target = u16::from_le_bytes([instr.ops[1], instr.ops[2]]) as usize;
        if target >= j {
            continue; // forward jump — not a back-edge
        }
        for interval in intervals.values_mut() {
            if interval.0 < target && interval.1 >= target && interval.1 < j {
                interval.1 = j;
            }
        }
    }

    intervals
}

// ── Per-instruction def / use ─────────────────────────────────────────────────

fn instr_def(instr: &Instruction) -> Option<u8> {
    let Some(op) = Opcode::from_u8(instr.opcode) else {
        return None;
    };
    match op {
        Opcode::Nop
        | Opcode::Ret
        | Opcode::MemFence
        | Opcode::Jmp
        | Opcode::Je
        | Opcode::Jne
        | Opcode::Jg
        | Opcode::Jge
        | Opcode::Jl
        | Opcode::Jle
        | Opcode::Ja
        | Opcode::Jb
        | Opcode::Jz    // ops[0] is condition SOURCE
        | Opcode::Jnz
        | Opcode::Cmp
        | Opcode::CallArg
        | Opcode::Drop
        | Opcode::Store
        | Opcode::FieldStore
        | Opcode::ArrayStore => None,
        _ => Some(instr.ops[0]),
    }
}

fn instr_uses(instr: &Instruction) -> Vec<u8> {
    let Some(op) = Opcode::from_u8(instr.opcode) else {
        return vec![];
    };
    match op {
        Opcode::Ret => vec![instr.ops[0]],

        Opcode::Nop
        | Opcode::MemFence
        | Opcode::Jmp
        | Opcode::Je
        | Opcode::Jne
        | Opcode::Jg
        | Opcode::Jge
        | Opcode::Jl
        | Opcode::Jle
        | Opcode::Ja
        | Opcode::Jb
        // RI16: dst + immediate, no source regs.
        | Opcode::MovI
        | Opcode::MovConst
        | Opcode::New
        | Opcode::NewObj
        | Opcode::CallIdx => vec![],

        // After inlining, these use consecutive arg registers ops[0]..ops[0]+flags-1.
        Opcode::Intrinsic | Opcode::Syscall | Opcode::CallExt => {
            (0..instr.flags as usize).map(|i| instr.ops[0].wrapping_add(i as u8)).collect()
        }

        // Jz/Jnz: ops[0] is condition register when nonzero.
        Opcode::Jz | Opcode::Jnz => {
            if instr.ops[0] != 0 { vec![instr.ops[0]] } else { vec![] }
        }

        // Single source: ops[0] is the register.
        Opcode::CallArg | Opcode::Drop => vec![instr.ops[0]],

        // Single source: ops[0]=dst, ops[1]=src.
        Opcode::Mov
        | Opcode::Move
        | Opcode::Dup
        | Opcode::Neg
        | Opcode::Not
        | Opcode::Inc
        | Opcode::Dec
        | Opcode::IntAbs
        | Opcode::FloatAbs
        | Opcode::FloatSqrt
        | Opcode::FloatFloor
        | Opcode::FloatCeil
        | Opcode::FloatRound
        | Opcode::StrLen
        | Opcode::StrToInt
        | Opcode::StrToFloat
        | Opcode::StrAsStr
        | Opcode::VtblLoad   // ops[2]=slot (not reg)
        | Opcode::PrimToStr  // ops[2]=type tag (not reg)
        | Opcode::FieldLoad  // ops[2]=byte offset (not reg)
        | Opcode::Load       // ops[0]=dst, ops[1]=base
        | Opcode::Lea        // ops[0]=dst, ops[1]=base
        | Opcode::CallReg    // ops[0]=dst, ops[1]=fn_ptr
        | Opcode::Spawn      // ops[0]=dst, ops[1]=fn_ptr
        => vec![instr.ops[1]],

        // Store: ops[0]=val, ops[1]=base.
        Opcode::Store => vec![instr.ops[0], instr.ops[1]],

        // FieldStore: ops[0]=val, ops[1]=obj, ops[2]=byte_offset.
        Opcode::FieldStore => vec![instr.ops[0], instr.ops[1]],

        // Cmp: reads ops[1] and ops[2].
        Opcode::Cmp => vec![instr.ops[1], instr.ops[2]],

        // ArrayStore: ops[0]=val, ops[1]=base, ops[2]=idx — all sources.
        Opcode::ArrayStore => vec![instr.ops[0], instr.ops[1], instr.ops[2]],

        // ArrayLoad: ops[0]=dst, ops[1]=base, ops[2]=idx.
        Opcode::ArrayLoad => vec![instr.ops[1], instr.ops[2]],

        // Two-source RRR: ops[0]=dst, ops[1]=src1, ops[2]=src2.
        Opcode::Add
        | Opcode::Sub
        | Opcode::Mul
        | Opcode::Div
        | Opcode::Mod
        | Opcode::Pow
        | Opcode::And
        | Opcode::Or
        | Opcode::Xor
        | Opcode::Shl
        | Opcode::Shr
        | Opcode::Sar
        | Opcode::StrConcat
        | Opcode::AtomicAdd
        | Opcode::AtomicCas
        | Opcode::IntMin
        | Opcode::IntMax
        | Opcode::FloatMin
        | Opcode::FloatMax => vec![instr.ops[1], instr.ops[2]],
    }
}

fn is_side_effect_free(op: Opcode) -> bool {
    matches!(
        op,
        Opcode::Mov
            | Opcode::MovI
            | Opcode::MovConst
            | Opcode::Add
            | Opcode::Sub
            | Opcode::Mul
            | Opcode::Div
            | Opcode::Mod
            | Opcode::Pow
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
            | Opcode::Load
            | Opcode::Lea
            | Opcode::Move
            | Opcode::Dup
            | Opcode::New
            | Opcode::NewObj
            | Opcode::FieldLoad
            | Opcode::VtblLoad
            | Opcode::ArrayLoad
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
    )
}
