// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Parameter provenance and effects recomputed from bytecode.
//!
//! This is stage 1 of the D-014 certificate verifier design
//! (`docs/internals/ownership-certificates.md`): a flow-sensitive dataflow
//! over a chunk's control flow tracks, per register, the parameter slots its
//! value may derive from. Loads, stores, returns and calls then yield
//! conservative effects on parameter slots. It is validated differentially
//! against the source `parameter_effects`; nothing consumes it as proof yet.

use super::chunk::Chunk;
use super::opcode::Opcode;
use super::regalloc::{instr_def, instr_uses, jump_target};

/// Parameter slots a register may derive from, as a bit set over the first
/// 64 slots. Higher slots collapse into bit 63.
type Slots = u64;

fn slot_bit(slot: usize) -> Slots {
    1 << slot.min(63)
}

/// Effects of one chunk on its parameter register slots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChunkEffects {
    /// Slots whose value, or memory reachable from it, is loaded.
    pub reads: Slots,
    /// Slots through whose value (an address) memory is stored to.
    pub writes_through: Slots,
    /// Slots the returned value may derive from.
    pub returned: Slots,
    /// Slots whose derived values are stored into memory.
    pub stored: Slots,
    /// Direct calls: function-table index and, per argument register in
    /// `CallArg` order, the slots it may derive from.
    pub calls: Vec<(u16, Vec<Slots>)>,
    /// Slots passed to calls with unknown targets (indirect, foreign,
    /// syscall, intrinsic).
    pub unknown: Slots,
}

/// Recompute a chunk's parameter effects. Registers `0..param_count` start
/// holding their own slot.
pub fn chunk_effects(chunk: &Chunk) -> ChunkEffects {
    let registers = 256;
    let length = chunk.code.len();
    let mut entry_state = vec![0 as Slots; registers];
    for slot in 0..chunk.param_count.min(registers) {
        entry_state[slot] = slot_bit(slot);
    }
    // In-state per instruction; `None` until reached.
    let mut states: Vec<Option<Vec<Slots>>> = vec![None; length];
    let mut effects = ChunkEffects::default();
    if length == 0 {
        return effects;
    }
    states[0] = Some(entry_state);
    let mut worklist = vec![0usize];
    // Per call site, the `CallArg` masks seen on paths reaching it.
    let mut call_masks: std::collections::BTreeMap<usize, Vec<Slots>> = Default::default();
    while let Some(index) = worklist.pop() {
        let Some(mut state) = states[index].clone() else {
            continue;
        };
        let instruction = &chunk.code[index];
        let Some(opcode) = Opcode::from_u8(instruction.opcode) else {
            continue;
        };
        let mask = |state: &[Slots], register: u8| state[register as usize];
        let union = |state: &[Slots], uses: &[u8]| {
            uses.iter()
                .fold(0, |all, &register| all | state[register as usize])
        };
        // Pending `CallArg` registers are recovered by scanning backwards from
        // a call to the preceding non-`CallArg` instruction (lowering emits
        // them contiguously before each call).
        let pending_args = |state: &[Slots]| {
            let mut masks = Vec::new();
            let mut at = index;
            while at > 0 && chunk.code[at - 1].opcode == Opcode::CallArg as u8 {
                at -= 1;
                masks.push(state[chunk.code[at].ops[0] as usize]);
            }
            masks.reverse();
            masks
        };
        let uses = instr_uses(instruction);
        let mut fall_through = true;
        match opcode {
            Opcode::Load | Opcode::FieldLoad | Opcode::ArrayLoad => {
                let base = mask(&state, instruction.ops[1]);
                effects.reads |= base;
                if let Some(destination) = instr_def(instruction) {
                    state[destination as usize] = base;
                }
                if opcode == Opcode::ArrayLoad && instruction.flags > 1 {
                    // A multi-slot element is copied through the sret pointer.
                    effects.writes_through |= mask(&state, instruction.ops[0]);
                }
            }
            Opcode::Store | Opcode::FieldStore | Opcode::ArrayStore => {
                // ops[0] is the stored value (a slot block for a multi-slot
                // array element); ops[1] is the address base.
                effects.writes_through |= mask(&state, instruction.ops[1]);
                let slots = instruction.flags.max(1);
                let values: Vec<u8> = if opcode == Opcode::ArrayStore {
                    (0..slots)
                        .map(|offset| instruction.ops[0].wrapping_add(offset))
                        .collect()
                } else {
                    vec![instruction.ops[0]]
                };
                effects.stored |= union(&state, &values);
            }
            Opcode::Lea => {
                // The address of this frame's own registers: writes through
                // it change local storage, not the caller's.
                state[instruction.ops[0] as usize] = 0;
            }
            Opcode::MovI | Opcode::MovConst | Opcode::New | Opcode::NewObj => {
                state[instruction.ops[0] as usize] = 0;
            }
            Opcode::CallIdx => {
                let args = pending_args(&state);
                let merged = call_masks
                    .entry(index)
                    .or_insert_with(|| vec![0; args.len()]);
                if merged.len() < args.len() {
                    merged.resize(args.len(), 0);
                }
                for (slot, arg) in merged.iter_mut().zip(&args) {
                    *slot |= arg;
                }
                // The result may derive from any argument.
                state[instruction.ops[0] as usize] = args.iter().fold(0, |all, arg| all | arg);
            }
            Opcode::CallReg | Opcode::CallCReg | Opcode::Spawn => {
                let args = pending_args(&state);
                let all = args
                    .iter()
                    .fold(mask(&state, instruction.ops[1]), |all, arg| all | arg);
                effects.unknown |= all;
                state[instruction.ops[0] as usize] = all;
            }
            Opcode::CallExt | Opcode::Syscall | Opcode::Intrinsic => {
                let all = union(&state, &uses)
                    | pending_args(&state).iter().fold(0, |all, arg| all | arg);
                effects.unknown |= all;
                if let Some(destination) = instr_def(instruction) {
                    state[destination as usize] = all;
                }
            }
            Opcode::Ret => {
                // A multi-slot result lives in r1.. behind the sret pointer r0.
                let values: Vec<u8> = if instruction.flags > 1 {
                    (1..=instruction.flags).collect()
                } else {
                    vec![instruction.ops[0]]
                };
                effects.returned |= union(&state, &values);
                fall_through = false;
            }
            Opcode::Trap => fall_through = false,
            Opcode::Jmp => fall_through = false,
            _ => {
                if let Some(destination) = instr_def(instruction) {
                    state[destination as usize] = union(&state, &uses);
                }
            }
        }
        let mut successors = Vec::with_capacity(2);
        if let Some(target) = jump_target(instruction) {
            successors.push(target);
        }
        if fall_through {
            successors.push(index + 1);
        }
        for successor in successors {
            if successor >= length {
                continue;
            }
            let changed = match &mut states[successor] {
                Some(existing) => {
                    let mut changed = false;
                    for (old, new) in existing.iter_mut().zip(&state) {
                        let joined = *old | new;
                        changed |= joined != *old;
                        *old = joined;
                    }
                    changed
                }
                slot @ None => {
                    *slot = Some(state.clone());
                    true
                }
            };
            if changed {
                worklist.push(successor);
            }
        }
    }
    for (index, masks) in call_masks {
        let (_, function) = chunk.code[index].ri16();
        effects.calls.push((function, masks));
    }
    effects
}

/// Compose callee writes through parameter slots into callers to a fixed
/// point, over chunks indexed by function-table position.
pub fn compose_writes_through(effects: &mut [ChunkEffects]) {
    loop {
        let mut changed = false;
        for caller in 0..effects.len() {
            let mut inherited: Slots = 0;
            for (function, arguments) in &effects[caller].calls {
                let Some(callee) = effects.get(*function as usize) else {
                    continue;
                };
                for (slot, argument) in arguments.iter().enumerate() {
                    if callee.writes_through & slot_bit(slot) != 0 {
                        inherited |= argument;
                    }
                }
            }
            let joined = effects[caller].writes_through | inherited;
            if joined != effects[caller].writes_through {
                effects[caller].writes_through = joined;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::codegen::Codegen;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::semantic::{Analyzer, SemanticReport};

    fn build(source: &str) -> (SemanticReport, Vec<Chunk>) {
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        let mut report = Analyzer::new().analyze_program(&program);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        // Keep every callable as its own chunk so each can be compared.
        report.inline_candidates.clear();
        let chunks = Codegen::new(&report)
            .compile_program(&program, &[])
            .expect("code generation");
        (report, chunks)
    }

    /// The source parameter index of each register slot of `name`, skipping a
    /// hidden sret pointer.
    fn slot_parameters(report: &SemanticReport, name: &str) -> Vec<Option<usize>> {
        let layout = &report.fn_value_layouts[name];
        let mut slots = Vec::new();
        if layout.result.layout.slot_count().unwrap_or(1) > 1 {
            slots.push(None);
        }
        for (index, parameter) in layout.params.iter().enumerate() {
            for _ in 0..parameter.layout.slot_count().unwrap_or(1) {
                slots.push(Some(index));
            }
        }
        slots
    }

    fn parameters(
        report: &SemanticReport,
        name: &str,
        mask: Slots,
    ) -> std::collections::BTreeSet<usize> {
        slot_parameters(report, name)
            .into_iter()
            .enumerate()
            .filter(|(slot, _)| mask & slot_bit(*slot) != 0)
            .filter_map(|(_, parameter)| parameter)
            .collect()
    }

    /// Bytecode effects must cover the source facts for every callable.
    fn assert_covers_source(source: &str) -> usize {
        let mut compared = 0;
        let (report, chunks) = build(source);
        let mut effects: Vec<ChunkEffects> = chunks.iter().map(chunk_effects).collect();
        compose_writes_through(&mut effects);
        for callable in &report.ownership_call_graph {
            let Some(source_effects) = &callable.parameter_effects else {
                continue;
            };
            let name = &callable.id.callable;
            let Some(index) = chunks.iter().position(|chunk| &chunk.name == name) else {
                continue;
            };
            if !report.fn_value_layouts.contains_key(name) {
                continue;
            }
            let written = parameters(&report, name, effects[index].writes_through);
            assert!(
                source_effects.written_through_reference.is_subset(&written),
                "{name}: source writes {:?}, bytecode {written:?}",
                source_effects.written_through_reference
            );
            let returned = parameters(&report, name, effects[index].returned);
            assert!(
                source_effects.returned_borrow.is_subset(&returned),
                "{name}: source returns {:?}, bytecode {returned:?}",
                source_effects.returned_borrow
            );
            compared += usize::from(
                !source_effects.written_through_reference.is_empty()
                    || !source_effects.returned_borrow.is_empty(),
            );
        }
        compared
    }

    #[test]
    fn bytecode_effects_cover_source_writes_and_returned_borrows() {
        let compared = assert_covers_source(
            r#"
struct Pair { left: i32, right: i32 }
fn set(pair: &Pair!) void { pair.left = 1; }
fn relay(pair: &Pair!) void { set(pair); }
fn outer(count: i32, pair: &Pair!) i32 { relay(pair); ret count; }
fn first(pair: &Pair) &i32 { ret &pair.left; }
fn pass(pair: &Pair) &i32 { ret first(pair); }
fn main() i32 {
    var pair = Pair { left: 0, right: 0 };
    outer(1, &pair!);
    ret *pass(&pair);
}
"#,
        );
        assert!(
            compared >= 4,
            "{compared} callables with source effects compared"
        );
    }

    #[test]
    fn control_flow_and_loops_keep_source_effects_covered() {
        let compared = assert_covers_source(
            r#"
struct Counter { total: i32, steps: i32 }
fn tick(counter: &Counter!, amount: i32) void {
    if amount > 0 {
        counter.total += amount;
    } else {
        counter.steps = 0;
    }
}
fn repeat(counter: &Counter!, times: i32) void {
    for var index = 0; index < times; index++ {
        tick(counter, index);
    }
}
fn pick(left: &Counter, right: &Counter, first: bool) &i32 {
    if first { ret &left.total; }
    ret &right.total;
}
fn main() i32 {
    var a = Counter { total: 0, steps: 0 };
    var b = Counter { total: 0, steps: 0 };
    repeat(&a!, 3);
    ret *pick(&a, &b, true);
}
"#,
        );
        assert!(compared >= 3, "{compared}");
    }

    #[test]
    fn call_indices_name_their_callee_chunks() {
        let (_, chunks) = build(
            "fn leaf(value: i32) i32 { ret value; } fn root() i32 { ret leaf(2); } fn main() i32 { ret root(); }",
        );
        let root = chunks
            .iter()
            .position(|chunk| chunk.name == "root")
            .unwrap();
        let effects = chunk_effects(&chunks[root]);
        assert!(
            effects
                .calls
                .iter()
                .any(|(function, _)| chunks[*function as usize].name == "leaf"),
            "{:?}",
            effects.calls
        );
    }

    #[test]
    fn local_register_addresses_are_not_writes_through_parameters() {
        let (report, chunks) = build(
            r#"
fn bump(value: i32) i32 {
    var copy = value;
    copy += 1;
    ret copy;
}
fn main() i32 { ret bump(1); }
"#,
        );
        let index = chunks
            .iter()
            .position(|chunk| chunk.name == "bump")
            .unwrap();
        let effects = chunk_effects(&chunks[index]);
        assert!(parameters(&report, "bump", effects.writes_through).is_empty());
        assert!(parameters(&report, "bump", effects.returned).contains(&0));
    }
}
