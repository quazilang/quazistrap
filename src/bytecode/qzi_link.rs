// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Assemble a format-16 QZI from one compilation for every target.
//!
//! Code generation lowers the target-view program once. Its chunks carry
//! target information in two ways: chunks compiled from a declaration copy
//! know that copy's mask (`declared`), and copies after the first are named
//! `name@t<mask>`. Everything else (closures, glue, specializations) takes the
//! targets that actually reach it. Names are then restored to their source
//! spelling, so each target selects exactly one body per name.

use std::collections::{HashMap, HashSet};

use super::chunk::{
    Chunk, ConstPoolEntry, QZI_MODULE_VERSION, QziCallRelocation, QziMetadata, QziModule, QziModuleKind,
    QziOwnershipArtifact, Qzi, equivalent_relocatable_chunks, infer_call_relocations,
    validate_qzi_chunks,
};
use super::opcode::Opcode;
use crate::semantic::target_views::{strip_variant_markers, variant_marker_mask};

pub struct QziLinkInput<'a> {
    pub targets: &'a [&'a str],
    pub chunks: Vec<Chunk>,
    /// Target mask of chunks compiled from source declarations.
    pub declared: HashMap<String, u8>,
    /// Calls from `chunks` to dependency symbols.
    pub external_relocations: Vec<QziCallRelocation>,
    /// A library's public API (its reachability roots); unused for executables.
    pub library_roots: Vec<String>,
    pub dependencies: Vec<Qzi>,
    pub metadata: QziMetadata,
    /// Public interface per target (empty for executables).
    pub interfaces: Vec<String>,
}

struct Node {
    chunk: Chunk,
    /// Targets this body may serve.
    inherent: u8,
    /// Compiled from a source declaration: its references must hold on every
    /// target it is declared for, as a per-target compilation would require.
    source: bool,
    /// (instruction index, callee symbol) of every call.
    calls: Vec<(usize, String)>,
}

impl Node {
    fn references(&self) -> impl Iterator<Item = &String> {
        self.calls
            .iter()
            .map(|(_, name)| name)
            .chain(self.chunk.constants.iter().flat_map(
                |constant| -> Box<dyn Iterator<Item = &String>> {
                    match constant {
                        ConstPoolEntry::FnAddr(name) => Box::new(std::iter::once(name)),
                        ConstPoolEntry::Vtable(layout) => Box::new(layout.methods.iter()),
                        _ => Box::new(std::iter::empty()),
                    }
                },
            ))
    }
}

fn call_names(chunk: &Chunk, names: &[String]) -> Result<Vec<(usize, String)>, String> {
    chunk
        .code
        .iter()
        .enumerate()
        .filter(|(_, instruction)| instruction.opcode == Opcode::CallIdx as u8)
        .map(|(index, instruction)| {
            let target = instruction.ri16().1 as usize;
            names
                .get(target)
                .map(|name| (index, name.clone()))
                .ok_or_else(|| format!("call in `{}` has no target", chunk.name))
        })
        .collect()
}

pub fn link_qzi(input: QziLinkInput<'_>) -> Result<Qzi, String> {
    let count = input.targets.len();
    if !(1..=8).contains(&count) {
        return Err("a QZI needs between one and eight targets".to_string());
    }
    let full = (u16::from(u8::MAX) >> (8 - count)) as u8;

    // Our chunks: calls by name, external calls through their relocations.
    let own_names: Vec<String> = input
        .chunks
        .iter()
        .map(|chunk| chunk.name.clone())
        .collect();
    let mut external: HashMap<(usize, usize), String> = HashMap::new();
    for relocation in &input.external_relocations {
        external.insert(
            (
                relocation.chunk_index as usize,
                relocation.instruction_index as usize,
            ),
            relocation.symbol.clone(),
        );
    }
    let mut nodes: Vec<Node> = Vec::with_capacity(input.chunks.len());
    for (chunk_index, chunk) in input.chunks.into_iter().enumerate() {
        let mut calls = Vec::new();
        for (index, instruction) in chunk.code.iter().enumerate() {
            if instruction.opcode != Opcode::CallIdx as u8 {
                continue;
            }
            let name = match external.get(&(chunk_index, index)) {
                Some(symbol) => symbol.clone(),
                None => own_names
                    .get(instruction.ri16().1 as usize)
                    .cloned()
                    .ok_or_else(|| format!("call in `{}` has no target", chunk.name))?,
            };
            calls.push((index, name));
        }
        let declared = input.declared.get(&chunk.name).copied();
        let inherent = declared
            .or_else(|| variant_marker_mask(&chunk.name))
            .unwrap_or(full)
            & full;
        nodes.push(Node {
            source: declared.is_some(),
            inherent,
            calls,
            chunk,
        });
    }

    // Dependency bodies keep their own masks, mapped onto our target order.
    for dependency in input.dependencies {
        let mut remap = Vec::with_capacity(dependency.targets.len());
        for target in &dependency.targets {
            remap.push(
                input
                    .targets
                    .iter()
                    .position(|known| known == target)
                    .map_or(0u8, |bit| 1 << bit),
            );
        }
        let names: Vec<String> = dependency
            .module
            .chunks
            .iter()
            .map(|chunk| chunk.name.clone())
            .collect();
        for (chunk, mask) in dependency.module.chunks.iter().zip(&dependency.masks) {
            let inherent = remap
                .iter()
                .enumerate()
                .filter(|(bit, _)| mask & (1 << bit) != 0)
                .fold(0, |acc, (_, ours)| acc | ours);
            if inherent == 0 {
                continue;
            }
            let calls = call_names(chunk, &names)?;
            // As in native linking from QZI, a dependency body equivalent
            // up to call targets is the one already present (a library
            // qualifies its root symbols, so callee names may differ).
            if let Some(existing) = nodes.iter_mut().find(|node| {
                strip_variant_markers(&node.chunk.name) == chunk.name
                    && node.inherent & inherent != 0
                    && equivalent_relocatable_chunks(&node.chunk, chunk)
            }) {
                existing.inherent |= inherent;
                continue;
            }
            if let Some(conflict) = nodes
                .iter()
                .find(|node| node.chunk.name == chunk.name && node.inherent & inherent != 0)
            {
                return Err(format!(
                    "conflicting QZI definitions for symbol `{}`",
                    conflict.chunk.name
                ));
            }
            nodes.push(Node {
                chunk: chunk.clone(),
                inherent,
                source: false,
                calls,
            });
        }
    }

    // Symbol resolution for one target: the exact symbol, else the one body
    // of its source name available there.
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_source_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, node) in nodes.iter().enumerate() {
        by_name
            .entry(node.chunk.name.clone())
            .or_default()
            .push(index);
        by_source_name
            .entry(strip_variant_markers(&node.chunk.name))
            .or_default()
            .push(index);
    }
    let resolve = |name: &str, bit: u8| -> Result<Option<usize>, String> {
        let available = |index: &&usize| nodes[**index].inherent & bit != 0;
        if let Some(index) = by_name
            .get(name)
            .and_then(|list| list.iter().find(available))
        {
            return Ok(Some(*index));
        }
        let candidates: Vec<usize> = by_source_name
            .get(&strip_variant_markers(name))
            .into_iter()
            .flatten()
            .filter(available)
            .copied()
            .collect();
        match candidates.as_slice() {
            [] => Ok(None),
            [one] => Ok(Some(*one)),
            _ => Err(format!(
                "`{}` has more than one body for one target",
                strip_variant_markers(name)
            )),
        }
    };
    let target_name = |bit: usize| input.targets[bit];

    // Bodies compiled from source keep their per-target obligations.
    let mut errors = Vec::new();
    for node in &nodes {
        if !node.source {
            continue;
        }
        for bit in 0..count {
            if node.inherent & (1 << bit) == 0 {
                continue;
            }
            for name in node.references() {
                if resolve(name, 1 << bit)?.is_none() {
                    errors.push(format!(
                        "`{}` uses `{}`, which is not available on {}",
                        strip_variant_markers(&node.chunk.name),
                        strip_variant_markers(name),
                        target_name(bit)
                    ));
                }
            }
        }
    }
    if !errors.is_empty() {
        errors.sort();
        errors.dedup();
        return Err(errors.join("\n"));
    }

    // Each target's reachable bodies.
    let mut masks = vec![0u8; nodes.len()];
    let library = input.metadata.kind == QziModuleKind::Library;
    for bit in 0..count {
        let target = 1u8 << bit;
        let mut queue: Vec<usize> = Vec::new();
        for (index, node) in nodes.iter().enumerate() {
            if node.inherent & target != 0 && node.chunk.export.is_some() {
                queue.push(index);
            }
        }
        if library {
            // Each public function serves the targets that have a body for it.
            for root in &input.library_roots {
                if let Some(index) = resolve(root, target)? {
                    queue.push(index);
                }
            }
        } else {
            match resolve("main", target)? {
                Some(main) => queue.push(main),
                None => {
                    return Err(format!(
                        "the program has no entry point for {}",
                        target_name(bit)
                    ));
                }
            }
        }
        while let Some(index) = queue.pop() {
            if masks[index] & target != 0 {
                continue;
            }
            masks[index] |= target;
            for name in nodes[index].references() {
                match resolve(name, target)? {
                    Some(callee) => queue.push(callee),
                    None => {
                        return Err(format!(
                            "`{}` uses `{}`, which is not available on {}",
                            strip_variant_markers(&nodes[index].chunk.name),
                            strip_variant_markers(name),
                            target_name(bit)
                        ));
                    }
                }
            }
        }
    }

    // Keep reached bodies under their source names; merge identical ones.
    let mut kept: Vec<(Node, u8)> = Vec::new();
    for (node, mask) in nodes.into_iter().zip(masks) {
        if mask == 0 {
            continue;
        }
        let mut node = node;
        node.chunk.name = strip_variant_markers(&node.chunk.name);
        for (_, name) in &mut node.calls {
            *name = strip_variant_markers(name);
        }
        for constant in &mut node.chunk.constants {
            match constant {
                ConstPoolEntry::FnAddr(name) => *name = strip_variant_markers(name),
                ConstPoolEntry::Vtable(layout) => {
                    for method in &mut layout.methods {
                        *method = strip_variant_markers(method);
                    }
                }
                _ => {}
            }
        }
        if let Some((_, existing)) = kept.iter_mut().find(|(known, _)| {
            known.chunk.name == node.chunk.name
                && known
                    .calls
                    .iter()
                    .map(|(_, n)| n)
                    .eq(node.calls.iter().map(|(_, n)| n))
                && equivalent_relocatable_chunks(&known.chunk, &node.chunk)
        }) {
            *existing |= mask;
            continue;
        }
        kept.push((node, mask));
    }

    // Point every call at a body of the callee sharing a target with the caller.
    let mut chunks = Vec::with_capacity(kept.len());
    for (node, mask) in &kept {
        let mut chunk = node.chunk.clone();
        for (instruction_index, name) in &node.calls {
            let target = kept
                .iter()
                .position(|(callee, callee_mask)| {
                    callee.chunk.name == *name && callee_mask & mask != 0
                })
                .ok_or_else(|| format!("`{}` calls missing `{name}`", chunk.name))?;
            let [lo, hi] = u16::try_from(target)
                .map_err(|_| "QZI has too many functions".to_string())?
                .to_le_bytes();
            let instruction = &mut chunk.code[*instruction_index];
            instruction.ops[1] = lo;
            instruction.ops[2] = hi;
        }
        chunks.push(chunk);
    }
    // Validate each target's selection as a standalone module.
    for bit in 0..count {
        let selected: Vec<&str> = kept
            .iter()
            .filter(|(_, mask)| mask & (1 << bit) != 0)
            .map(|(node, _)| node.chunk.name.as_str())
            .collect();
        let mut seen = HashSet::new();
        for name in selected {
            if !seen.insert(name) {
                return Err(format!(
                    "`{name}` has more than one body for {}",
                    target_name(bit)
                ));
            }
        }
    }
    validate_qzi_chunks(&chunks)?;

    let mut interface_overrides: Vec<(u8, String)> = Vec::new();
    let base = input.interfaces.first().cloned().unwrap_or_default();
    for (bit, interface) in input.interfaces.iter().enumerate().skip(1) {
        if *interface == base {
            continue;
        }
        match interface_overrides
            .iter_mut()
            .find(|(_, known)| known == interface)
        {
            Some((mask, _)) => *mask |= 1 << bit,
            None => interface_overrides.push((1 << bit, interface.clone())),
        }
    }
    let masks = kept.iter().map(|(_, mask)| *mask).collect();
    Ok(Qzi {
        targets: input
            .targets
            .iter()
            .map(|target| target.to_string())
            .collect(),
        module: QziModule {
            ownership_abi_version: QZI_MODULE_VERSION,
            metadata: input.metadata,
            interface: base,
            call_relocations: infer_call_relocations(&chunks)?,
            ownership: QziOwnershipArtifact::default(),
            chunks,
        },
        masks,
        interface_overrides,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::chunk::{decode_qzi, project_qzi_for_target};
    use crate::bytecode::instruction::{ri16, rrr};

    const TARGETS: [&str; 3] = ["linux", "windows", "netbsd"];

    fn chunk(name: &str, call: Option<u16>, value: u16) -> Chunk {
        let mut chunk = Chunk::with_params(name, 0);
        match call {
            Some(index) => chunk.emit(ri16(Opcode::CallIdx, 0, index)),
            None => chunk.emit(ri16(Opcode::MovI, 0, value)),
        };
        chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
        chunk.reg_count = 1;
        chunk
    }

    fn metadata() -> QziMetadata {
        QziMetadata {
            name: String::new(),
            version: None,
            kind: QziModuleKind::Executable,
            main_takes_args: false,
        }
    }

    /// main -> shared -> os, where `os` has a Linux/NetBSD copy and a
    /// Windows copy, as the target-view pass produces them.
    fn program() -> (Vec<Chunk>, HashMap<String, u8>) {
        let chunks = vec![
            chunk("main", Some(1), 0),
            chunk("shared", Some(2), 0),
            chunk("os", None, 1),
            chunk("os@t2", None, 2),
        ];
        let declared = HashMap::from([
            ("main".to_string(), u8::MAX),
            ("shared".to_string(), u8::MAX),
            ("os".to_string(), 0b101),
            ("os@t2".to_string(), 0b010),
        ]);
        (chunks, declared)
    }

    fn assemble(chunks: Vec<Chunk>, declared: HashMap<String, u8>) -> Result<Qzi, String> {
        link_qzi(QziLinkInput {
            targets: &TARGETS,
            chunks,
            declared,
            external_relocations: Vec::new(),
            library_roots: Vec::new(),
            dependencies: Vec::new(),
            metadata: metadata(),
            interfaces: vec![String::new(); 3],
        })
    }

    #[test]
    fn one_compilation_stores_shared_bodies_once_and_selects_each_target() {
        let (chunks, declared) = program();
        let bytes = assemble(chunks, declared).unwrap().encode().unwrap();
        let qzi = decode_qzi(&bytes).unwrap();
        let names: Vec<_> = qzi
            .module
            .chunks
            .iter()
            .map(|chunk| chunk.name.as_str())
            .collect();
        assert_eq!(names, ["main", "shared", "os", "os"]);
        assert_eq!(qzi.masks, [0b111, 0b111, 0b101, 0b010]);
        for (os, value) in [("linux", 1), ("windows", 2), ("netbsd", 1)] {
            let module = project_qzi_for_target(decode_qzi(&bytes).unwrap(), os).unwrap();
            assert_eq!(module.chunks.len(), 3);
            let callee = module.chunks[1].code[0].ri16().1 as usize;
            assert_eq!(module.chunks[callee].name, "os");
            assert_eq!(module.chunks[callee].code[0].ri16().1, value, "{os}");
        }
        assert!(
            project_qzi_for_target(decode_qzi(&bytes).unwrap(), "macos")
                .unwrap_err()
                .contains("no `macos` target")
        );
    }

    #[test]
    fn identical_target_copies_merge_and_tampering_is_rejected() {
        let (mut chunks, declared) = program();
        chunks[3] = chunk("os@t2", None, 1);
        let qzi = assemble(chunks, declared).unwrap();
        assert_eq!(qzi.masks, [0b111, 0b111, 0b111]);
        let mut bytes = qzi.encode().unwrap();
        let last = bytes.len() - 40;
        bytes[last] ^= 1;
        assert!(
            decode_qzi(&bytes)
                .unwrap_err()
                .contains("integrity")
        );
    }

    #[test]
    fn a_reference_missing_on_one_target_is_an_error_naming_it() {
        let (mut chunks, mut declared) = program();
        chunks.pop();
        declared.remove("os@t2");
        let error = assemble(chunks, declared).unwrap_err();
        assert!(
            error.contains("`shared` uses `os`, which is not available on windows"),
            "{error}"
        );
    }

    #[test]
    fn unreached_generated_bodies_take_the_targets_that_reach_them() {
        // A closure of the Windows copy is generated (not declared): it is
        // only kept for the target that reaches it.
        let (mut chunks, declared) = program();
        chunks[3] = chunk("os@t2", Some(4), 0);
        chunks.push(chunk("os@t2.closure0", None, 7));
        let qzi = assemble(chunks, declared).unwrap();
        let closure = qzi
            .module
            .chunks
            .iter()
            .position(|chunk| chunk.name == "os.closure0")
            .unwrap();
        assert_eq!(qzi.masks[closure], 0b010);
    }
}
