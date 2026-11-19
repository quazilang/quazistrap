// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Parameter provenance and effects recomputed from bytecode.
//!
//! This is the analysis behind D-014 QZI ownership certificates
//! (`docs/internals/ownership-certificates.md`): a flow-sensitive dataflow
//! over a chunk's basic blocks tracks, per register, the parameter slots its
//! value may derive from. Loads, stores, returns and calls then yield
//! conservative effects on parameter slots, and [`compose`] closes them over
//! the call graph. It is validated differentially against the source
//! `parameter_effects`.

use super::chunk::Chunk;
use super::opcode::Opcode;
use super::regalloc::{instr_def, instr_uses, jump_target};

/// Parameter slots a register may derive from, as a bit set over the first
/// 64 slots. Higher slots collapse into bit 63.
pub type Slots = u64;

pub fn slot_bit(slot: usize) -> Slots {
    1 << slot.min(63)
}

/// A direct call recorded by [`chunk_effects`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    /// Index of the `CallIdx` instruction in its chunk.
    pub instruction: usize,
    /// The function-table index the instruction names.
    pub function: u16,
    /// Per argument register in `CallArg` order, the slots it may derive from.
    pub arguments: Vec<Slots>,
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
    /// Direct calls in instruction order.
    pub calls: Vec<CallSite>,
    /// Slots passed to calls with unknown targets (indirect, foreign,
    /// syscall, intrinsic).
    pub unknown: Slots,
}

impl ChunkEffects {
    /// True when no parameter-derived value reaches code the analysis cannot
    /// see, so the other sets are a complete summary.
    pub fn is_complete(&self) -> bool {
        self.unknown == 0
    }
}

/// Number of registers a chunk names (an upper bound on its register file).
fn register_count(chunk: &Chunk) -> usize {
    let mut highest = chunk.param_count.min(256);
    for instruction in &chunk.code {
        for register in instr_uses(instruction)
            .into_iter()
            .chain(instr_def(instruction))
        {
            highest = highest.max(register as usize + 1);
        }
        // Lea, CallArg-adjacent and multi-slot forms address ops[0..2]
        // directly; cover them as well.
        for register in [instruction.ops[0], instruction.ops[1]] {
            highest = highest.max(register as usize + 1);
        }
        if instruction.flags > 1 {
            highest = highest
                .max((instruction.ops[0] as usize + instruction.flags as usize + 1).min(256));
        }
    }
    highest.max(1)
}

fn falls_through(instruction: &crate::bytecode::instruction::Instruction) -> bool {
    !matches!(
        Opcode::from_u8(instruction.opcode),
        Some(Opcode::Ret | Opcode::Trap | Opcode::Jmp)
    )
}

/// Recompute a chunk's parameter effects. Registers `0..param_count` start
/// holding their own slot.
pub fn chunk_effects(chunk: &Chunk) -> ChunkEffects {
    let length = chunk.code.len();
    let mut effects = ChunkEffects::default();
    if length == 0 {
        return effects;
    }
    let registers = register_count(chunk);
    // Basic-block leaders: the entry, jump targets and instructions after a
    // jump or a block end.
    let mut leader = vec![false; length];
    leader[0] = true;
    for (index, instruction) in chunk.code.iter().enumerate() {
        if let Some(target) = jump_target(instruction)
            && target < length
        {
            leader[target] = true;
        }
        if (jump_target(instruction).is_some() || !falls_through(instruction)) && index + 1 < length
        {
            leader[index + 1] = true;
        }
    }
    let mut entry_state = vec![0 as Slots; registers];
    for (slot, state) in entry_state.iter_mut().take(chunk.param_count).enumerate() {
        *state = slot_bit(slot);
    }
    // In-state per block leader; `None` until reached.
    let mut states: Vec<Option<Vec<Slots>>> = vec![None; length];
    states[0] = Some(entry_state);
    let mut worklist = vec![0usize];
    // Per call site, the `CallArg` masks seen on paths reaching it.
    let mut call_masks: std::collections::BTreeMap<usize, Vec<Slots>> = Default::default();
    let mut successors = Vec::with_capacity(2);
    while let Some(start) = worklist.pop() {
        let Some(mut state) = states[start].clone() else {
            continue;
        };
        successors.clear();
        let mut index = start;
        loop {
            let instruction = &chunk.code[index];
            let fall_through = step(chunk, index, &mut state, &mut effects, &mut call_masks);
            if let Some(target) = jump_target(instruction) {
                successors.push(target);
            }
            let next = index + 1;
            if !fall_through || next >= length {
                break;
            }
            if leader[next] {
                successors.push(next);
                break;
            }
            index = next;
        }
        for &successor in &successors {
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
    for (instruction, arguments) in call_masks {
        let (_, function) = chunk.code[instruction].ri16();
        effects.calls.push(CallSite {
            instruction,
            function,
            arguments,
        });
    }
    effects
}

/// Transfer function of one instruction; returns whether control may fall
/// through to the next instruction.
fn step(
    chunk: &Chunk,
    index: usize,
    state: &mut [Slots],
    effects: &mut ChunkEffects,
    call_masks: &mut std::collections::BTreeMap<usize, Vec<Slots>>,
) -> bool {
    let instruction = &chunk.code[index];
    let Some(opcode) = Opcode::from_u8(instruction.opcode) else {
        return true;
    };
    let at = |state: &[Slots], register: u8| state.get(register as usize).copied().unwrap_or(0);
    let union = |state: &[Slots], uses: &[u8]| {
        uses.iter()
            .fold(0, |all, &register| all | at(state, register))
    };
    let define = |state: &mut [Slots], register: u8, value: Slots| {
        if let Some(slot) = state.get_mut(register as usize) {
            *slot = value;
        }
    };
    // Pending `CallArg` registers are recovered by scanning backwards from a
    // call to the preceding non-`CallArg` instruction (lowering emits them
    // contiguously before each call).
    let pending_args = |state: &[Slots]| {
        let mut masks = Vec::new();
        let mut cursor = index;
        while cursor > 0 && chunk.code[cursor - 1].opcode == Opcode::CallArg as u8 {
            cursor -= 1;
            masks.push(at(state, chunk.code[cursor].ops[0]));
        }
        masks.reverse();
        masks
    };
    match opcode {
        Opcode::Load | Opcode::FieldLoad | Opcode::ArrayLoad => {
            let base = at(state, instruction.ops[1]);
            effects.reads |= base;
            if let Some(destination) = instr_def(instruction) {
                define(state, destination, base);
            }
            if opcode == Opcode::ArrayLoad && instruction.flags > 1 {
                // A multi-slot element is copied through the sret pointer.
                effects.writes_through |= at(state, instruction.ops[0]);
            }
        }
        Opcode::Store | Opcode::FieldStore | Opcode::ArrayStore => {
            // ops[0] is the stored value (a slot block for a multi-slot array
            // element); ops[1] is the address base.
            effects.writes_through |= at(state, instruction.ops[1]);
            let slots = instruction.flags.max(1);
            let values: Vec<u8> = if opcode == Opcode::ArrayStore {
                (0..slots)
                    .map(|offset| instruction.ops[0].wrapping_add(offset))
                    .collect()
            } else {
                vec![instruction.ops[0]]
            };
            effects.stored |= union(state, &values);
        }
        Opcode::Lea => {
            // The address of this frame's own registers: writes through it
            // change local storage, not the caller's.
            define(state, instruction.ops[0], 0);
        }
        Opcode::MovI | Opcode::MovConst | Opcode::New | Opcode::NewObj => {
            define(state, instruction.ops[0], 0);
        }
        Opcode::CallIdx => {
            let args = pending_args(state);
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
            define(
                state,
                instruction.ops[0],
                args.iter().fold(0, |all, arg| all | arg),
            );
        }
        Opcode::CallReg | Opcode::CallCReg | Opcode::Spawn => {
            let args = pending_args(state);
            let all = args
                .iter()
                .fold(at(state, instruction.ops[1]), |all, arg| all | arg);
            effects.unknown |= all;
            define(state, instruction.ops[0], all);
        }
        Opcode::CallExt | Opcode::Syscall | Opcode::Intrinsic => {
            let all = union(state, &instr_uses(instruction))
                | pending_args(state).iter().fold(0, |all, arg| all | arg);
            effects.unknown |= all;
            if let Some(destination) = instr_def(instruction) {
                define(state, destination, all);
            }
        }
        Opcode::Ret => {
            // A multi-slot result lives in r1.. behind the sret pointer r0.
            let values: Vec<u8> = if instruction.flags > 1 {
                (1..=instruction.flags).collect()
            } else {
                vec![instruction.ops[0]]
            };
            effects.returned |= union(state, &values);
            return false;
        }
        Opcode::Trap | Opcode::Jmp => return false,
        _ => {
            if let Some(destination) = instr_def(instruction) {
                let value = union(state, &instr_uses(instruction));
                define(state, destination, value);
            }
        }
    }
    true
}

/// Close chunk effects over direct calls to a fixed point. `resolve` names
/// the chunks a call site may reach (several for target-specific bodies);
/// `None` means the target is unknown, so every argument becomes unknown.
///
/// A callee's reads, writes through, stores and unknown uses of a parameter
/// slot become the caller's effects on the slots that argument derives from.
/// Results already derive from every argument (see [`chunk_effects`]).
pub fn compose(
    effects: &mut [ChunkEffects],
    resolve: impl Fn(usize, &CallSite) -> Option<std::rc::Rc<[usize]>>,
) {
    let targets: Vec<Vec<Option<std::rc::Rc<[usize]>>>> = effects
        .iter()
        .enumerate()
        .map(|(caller, chunk)| {
            chunk
                .calls
                .iter()
                .map(|call| resolve(caller, call))
                .collect()
        })
        .collect();
    loop {
        let mut changed = false;
        for caller in 0..effects.len() {
            let mut inherited = [0 as Slots; 4];
            for (call, callees) in effects[caller].calls.iter().zip(&targets[caller]) {
                let Some(callees) = callees else {
                    inherited[3] |= call
                        .arguments
                        .iter()
                        .fold(0, |all, argument| all | argument);
                    continue;
                };
                for &callee in callees.iter() {
                    let Some(callee) = effects.get(callee) else {
                        inherited[3] |= call
                            .arguments
                            .iter()
                            .fold(0, |all, argument| all | argument);
                        continue;
                    };
                    let sets = [
                        callee.reads,
                        callee.writes_through,
                        callee.stored,
                        callee.unknown,
                    ];
                    for (slot, argument) in call.arguments.iter().enumerate() {
                        for (inherit, set) in inherited.iter_mut().zip(sets) {
                            if set & slot_bit(slot) != 0 {
                                *inherit |= argument;
                            }
                        }
                    }
                }
            }
            let chunk = &mut effects[caller];
            for (field, inherit) in [
                &mut chunk.reads,
                &mut chunk.writes_through,
                &mut chunk.stored,
                &mut chunk.unknown,
            ]
            .into_iter()
            .zip(inherited)
            {
                let joined = *field | inherit;
                changed |= joined != *field;
                *field = joined;
            }
        }
        if !changed {
            break;
        }
    }
}

/// [`compose`] over chunks indexed by function-table position.
pub fn compose_by_function_index(effects: &mut [ChunkEffects]) {
    compose(effects, |_, call| {
        Some(std::rc::Rc::from([call.function as usize]))
    });
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
        compose_by_function_index(&mut effects);
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

    /// Compare bytecode and source effects for every callable of a loaded
    /// program; returns how many callables had source effects to cover.
    fn covered_callables(report: &SemanticReport, chunks: &[Chunk]) -> usize {
        let mut effects: Vec<ChunkEffects> = chunks.iter().map(chunk_effects).collect();
        compose_by_function_index(&mut effects);
        let mut compared = 0;
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
            let written = parameters(report, name, effects[index].writes_through);
            let returned = parameters(report, name, effects[index].returned);
            assert!(
                source_effects.written_through_reference.is_subset(&written),
                "{name}: source writes {:?}, bytecode {written:?}",
                source_effects.written_through_reference
            );
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
    fn tutorial_programs_with_std_keep_source_effects_covered() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/tutorial/fixtures");
        let mut fixtures: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|extension| extension == "qz"))
            .collect();
        fixtures.sort();
        assert!(!fixtures.is_empty());
        let mut compared = 0;
        for fixture in fixtures {
            let loaded = crate::loader::load_programs_configured(
                std::slice::from_ref(&fixture),
                None,
                true,
                &[],
            )
            .unwrap();
            assert!(loaded.parse_error.is_none(), "{}", fixture.display());
            let program = crate::semantic::strip_cfg(&loaded.program);
            let namespaced = loaded
                .namespaced_paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            let mut report = crate::analysis::analyze_program_with_source_files(
                &loaded.merged_source,
                &program,
                loaded.library_fn_names,
                loaded.library_char_ranges,
                loaded.source_files.clone(),
                namespaced,
            );
            assert!(
                report.errors.is_empty(),
                "{}: {:?}",
                fixture.display(),
                report.errors
            );
            report.inline_candidates.clear();
            let chunks = Codegen::new(&report)
                .compile_program(&program, &loaded.source_files)
                .unwrap();
            compared += covered_callables(&report, &chunks);
        }
        assert!(
            compared > 0,
            "the tutorial programs have source effects to compare"
        );
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
                .any(|call| chunks[call.function as usize].name == "leaf"),
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
