// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

// QZI → x86-64 binary encoding via iced-x86 CodeAssembler.
//
// Relocation strategy:
//   - Cross-function calls (CallIdx, CallExt, string ops) emit `call fn_start` as
//     a dummy near-relative call (E8 rel32). After assembly, we zero the 4-byte
//     displacement and record a PLT32 relocation pointing to the real target.
//   - RIP-relative data loads (MovConst/Str, PrimToStr) emit `lea reg, [rip+fn_start]`
//     as dummy. After assembly, we zero the 4-byte displacement and record a PC32
//     relocation pointing to the real data symbol.
//   - Local jumps within the function use iced-x86 labels directly.

use std::collections::HashMap;

use iced_x86::code_asm::*;

use crate::abi::{AbiSignature, AbiType, ForeignSymbol};
use crate::backend::{BackendError, TargetSpec, target::Abi};
use crate::bytecode::{Chunk, ConstPoolEntry, Opcode, instruction::MemWidth};

use super::relocations::{PendingReloc, RelocKind};
use super::sysv_abi::{EightbyteClass, TypeClass, classify};

// ── Calling-convention register tables ──────────────────────────────────────

const SYSV_REGS: [AsmRegister64; 6] = [rdi, rsi, rdx, rcx, r8, r9];
const SYSV_XMM_REGS: [AsmRegisterXmm; 8] = [xmm0, xmm1, xmm2, xmm3, xmm4, xmm5, xmm6, xmm7];
const SYSCALL_REGS: [AsmRegister64; 6] = [rdi, rsi, rdx, r10, r8, r9];
// Win64: first 4 integer args in rcx/rdx/r8/r9; args 5-6 on stack.
const WIN64_REGS: [AsmRegister64; 4] = [rcx, rdx, r8, r9];
const WIN64_XMM_REGS: [AsmRegisterXmm; 4] = [xmm0, xmm1, xmm2, xmm3];

// ── helpers ──────────────────────────────────────────────────────────────────

/// QZI register N → `qword ptr [rbp - (N+1)*8]`.
fn slot(reg: u8) -> AsmMemoryOperand {
    qword_ptr(rbp + (-((reg as i32 + 1) * 8)))
}

fn round_to_16(n: usize) -> usize {
    (n + 15) & !15
}

fn round_to_8(n: usize) -> usize {
    (n + 7) & !7
}

#[derive(Debug, Clone)]
enum SysvPiece {
    Gp(usize),
    Sse(usize),
}

#[derive(Debug, Clone)]
enum SysvArgLocation {
    Registers(Vec<SysvPiece>),
    Stack { offset: usize },
}

#[derive(Debug)]
struct SysvCallPlan {
    args: Vec<SysvArgLocation>,
    stack_size: usize,
    sse_used: usize,
}

fn plan_sysv_call(signature: &AbiSignature) -> SysvCallPlan {
    let return_in_memory = matches!(classify(&signature.return_type), TypeClass::Memory);
    let mut gp = usize::from(return_in_memory);
    let mut sse = 0usize;
    let mut stack_size = 0usize;
    let mut args = Vec::with_capacity(signature.params.len());

    for ty in &signature.params {
        let classification = classify(ty);
        let TypeClass::Registers(classes) = &classification else {
            let offset = stack_size;
            stack_size += round_to_8(ty.size());
            args.push(SysvArgLocation::Stack { offset });
            continue;
        };
        let needed_gp = classes
            .iter()
            .filter(|class| **class == EightbyteClass::Integer)
            .count();
        let needed_sse = classes.len() - needed_gp;
        if gp + needed_gp > SYSV_REGS.len() || sse + needed_sse > SYSV_XMM_REGS.len() {
            let offset = stack_size;
            stack_size += round_to_8(ty.size());
            args.push(SysvArgLocation::Stack { offset });
            continue;
        }
        let mut pieces = Vec::with_capacity(classes.len());
        for class in classes {
            match class {
                EightbyteClass::Integer => {
                    pieces.push(SysvPiece::Gp(gp));
                    gp += 1;
                }
                EightbyteClass::Sse => {
                    pieces.push(SysvPiece::Sse(sse));
                    sse += 1;
                }
            }
        }
        args.push(SysvArgLocation::Registers(pieces));
    }

    SysvCallPlan {
        args,
        stack_size: round_to_16(stack_size),
        sse_used: sse,
    }
}

fn emit_mem_copy(
    asm: &mut CodeAssembler,
    dst_base: AsmRegister64,
    dst_disp: i32,
    src_base: AsmRegister64,
    src_disp: i32,
    size: usize,
) -> Result<(), BackendError> {
    let err = |error: IcedError| BackendError(error.to_string());
    let mut offset = 0usize;
    while offset + 8 <= size {
        asm.mov(rax, qword_ptr(src_base + (src_disp + offset as i32)))
            .map_err(err)?;
        asm.mov(qword_ptr(dst_base + (dst_disp + offset as i32)), rax)
            .map_err(err)?;
        offset += 8;
    }
    if offset + 4 <= size {
        asm.mov(eax, dword_ptr(src_base + (src_disp + offset as i32)))
            .map_err(err)?;
        asm.mov(dword_ptr(dst_base + (dst_disp + offset as i32)), eax)
            .map_err(err)?;
        offset += 4;
    }
    if offset + 2 <= size {
        asm.mov(ax, word_ptr(src_base + (src_disp + offset as i32)))
            .map_err(err)?;
        asm.mov(word_ptr(dst_base + (dst_disp + offset as i32)), ax)
            .map_err(err)?;
        offset += 2;
    }
    if offset < size {
        asm.mov(al, byte_ptr(src_base + (src_disp + offset as i32)))
            .map_err(err)?;
        asm.mov(byte_ptr(dst_base + (dst_disp + offset as i32)), al)
            .map_err(err)?;
    }
    Ok(())
}

#[derive(Debug)]
struct AbiFrameLayout {
    frame_size: i32,
    export_aggregate_disps: Vec<Option<i32>>,
    export_sret_disp: Option<i32>,
    foreign_result_disp: Option<i32>,
}

fn abi_frame_layout(chunk: &Chunk, num_regs: usize, is_win64: bool) -> AbiFrameLayout {
    let mut used = num_regs * 8;
    let mut export_aggregate_disps = vec![None; chunk.param_count];
    let mut export_sret_disp = None;
    if let Some(export) = &chunk.export {
        for (index, ty) in export.signature.params.iter().enumerate() {
            if matches!(ty, AbiType::Aggregate { .. }) {
                used += round_to_8(ty.size()).max(8);
                if let Some(disp) = export_aggregate_disps.get_mut(index) {
                    *disp = Some(-(used as i32));
                }
            }
        }
        let return_in_memory = if is_win64 {
            win64_return_in_memory(&export.signature.return_type)
        } else {
            matches!(classify(&export.signature.return_type), TypeClass::Memory)
        };
        if return_in_memory {
            used += 8;
            export_sret_disp = Some(-(used as i32));
        }
    }

    let needs_foreign_result = chunk.constants.iter().any(|constant| {
        matches!(
            constant,
            ConstPoolEntry::ForeignSymbol(symbol)
                if matches!(symbol.signature.return_type, AbiType::Aggregate { .. })
        )
    });
    let foreign_result_disp = if needs_foreign_result {
        used += 8;
        Some(-(used as i32))
    } else {
        None
    };
    let outgoing = if is_win64 { 48 } else { 0 };
    AbiFrameLayout {
        frame_size: round_to_16(used + outgoing) as i32,
        export_aggregate_disps,
        export_sret_disp,
        foreign_result_disp,
    }
}

fn win64_return_in_memory(ty: &AbiType) -> bool {
    matches!(ty, AbiType::Aggregate { size, .. } if !matches!(*size, 1 | 2 | 4 | 8))
}

fn win64_pass_indirect(ty: &AbiType) -> bool {
    matches!(ty, AbiType::Aggregate { size, .. } if !matches!(*size, 1 | 2 | 4 | 8))
}

fn emit_store_aggregate_bits(
    asm: &mut CodeAssembler,
    dst_base: AsmRegister64,
    dst_disp: i32,
    size: usize,
) -> Result<(), BackendError> {
    let err = |error: IcedError| BackendError(error.to_string());
    match size {
        1 => asm.mov(byte_ptr(dst_base + dst_disp), al).map_err(err)?,
        2 => asm.mov(word_ptr(dst_base + dst_disp), ax).map_err(err)?,
        4 => asm.mov(dword_ptr(dst_base + dst_disp), eax).map_err(err)?,
        8 => asm.mov(qword_ptr(dst_base + dst_disp), rax).map_err(err)?,
        _ => {
            return Err(BackendError(format!(
                "invalid direct Win64 aggregate size {size}"
            )));
        }
    }
    Ok(())
}

fn emit_load_aggregate_bits(
    asm: &mut CodeAssembler,
    src_base: AsmRegister64,
    src_disp: i32,
    size: usize,
) -> Result<(), BackendError> {
    let err = |error: IcedError| BackendError(error.to_string());
    match size {
        1 => asm.movzx(rax, byte_ptr(src_base + src_disp)).map_err(err)?,
        2 => asm.movzx(rax, word_ptr(src_base + src_disp)).map_err(err)?,
        4 => asm.mov(eax, dword_ptr(src_base + src_disp)).map_err(err)?,
        8 => asm.mov(rax, qword_ptr(src_base + src_disp)).map_err(err)?,
        _ => {
            return Err(BackendError(format!(
                "invalid direct Win64 aggregate size {size}"
            )));
        }
    }
    Ok(())
}

fn emit_store_sse_piece(
    asm: &mut CodeAssembler,
    dst_base: AsmRegister64,
    dst_disp: i32,
    src: AsmRegisterXmm,
    size: usize,
) -> Result<(), BackendError> {
    let err = |error: IcedError| BackendError(error.to_string());
    match size {
        4 => asm.movd(dword_ptr(dst_base + dst_disp), src).map_err(err)?,
        8 => asm.movq(qword_ptr(dst_base + dst_disp), src).map_err(err)?,
        _ => return Err(BackendError(format!("invalid SysV SSE piece size {size}"))),
    }
    Ok(())
}

fn emit_load_sse_piece(
    asm: &mut CodeAssembler,
    dst: AsmRegisterXmm,
    src_base: AsmRegister64,
    src_disp: i32,
    size: usize,
) -> Result<(), BackendError> {
    let err = |error: IcedError| BackendError(error.to_string());
    match size {
        4 => asm.movd(dst, dword_ptr(src_base + src_disp)).map_err(err)?,
        8 => asm.movq(dst, qword_ptr(src_base + src_disp)).map_err(err)?,
        _ => return Err(BackendError(format!("invalid SysV SSE piece size {size}"))),
    }
    Ok(())
}

fn emit_sysv_export_prologue(
    asm: &mut CodeAssembler,
    export: &ForeignSymbol,
    layout: &AbiFrameLayout,
) -> Result<(), BackendError> {
    let err = |error: IcedError| BackendError(error.to_string());
    let plan = plan_sysv_call(&export.signature);
    if matches!(classify(&export.signature.return_type), TypeClass::Memory) {
        let disp = layout
            .export_sret_disp
            .expect("SysV sret export must reserve a frame slot");
        asm.mov(qword_ptr(rbp + disp), rdi).map_err(err)?;
    }

    for (index, (ty, location)) in export
        .signature
        .params
        .iter()
        .zip(plan.args.iter())
        .enumerate()
    {
        match (ty, location) {
            (AbiType::Aggregate { .. }, SysvArgLocation::Stack { offset }) => {
                let dst_disp = layout.export_aggregate_disps[index]
                    .expect("aggregate export parameter must reserve local storage");
                emit_mem_copy(asm, rbp, dst_disp, rbp, 16 + *offset as i32, ty.size())?;
                asm.lea(rax, qword_ptr(rbp + dst_disp)).map_err(err)?;
                asm.mov(slot(index as u8), rax).map_err(err)?;
            }
            (AbiType::Aggregate { .. }, SysvArgLocation::Registers(pieces)) => {
                let dst_disp = layout.export_aggregate_disps[index]
                    .expect("aggregate export parameter must reserve local storage");
                for (piece_index, piece) in pieces.iter().enumerate() {
                    let disp = dst_disp + (piece_index * 8) as i32;
                    let piece_size = ty.size().saturating_sub(piece_index * 8).min(8);
                    match piece {
                        SysvPiece::Gp(reg) => {
                            asm.mov(rax, SYSV_REGS[*reg]).map_err(err)?;
                            emit_store_aggregate_bits(asm, rbp, disp, piece_size)?;
                        }
                        SysvPiece::Sse(reg) => {
                            emit_store_sse_piece(asm, rbp, disp, SYSV_XMM_REGS[*reg], piece_size)?
                        }
                    }
                }
                asm.lea(rax, qword_ptr(rbp + dst_disp)).map_err(err)?;
                asm.mov(slot(index as u8), rax).map_err(err)?;
            }
            (scalar, SysvArgLocation::Registers(pieces)) => match (scalar, pieces.as_slice()) {
                (AbiType::Float64, [SysvPiece::Sse(reg)]) => {
                    asm.movsd_2(slot(index as u8), SYSV_XMM_REGS[*reg])
                        .map_err(err)?;
                }
                (AbiType::Float32, [SysvPiece::Sse(reg)]) => {
                    asm.cvtss2sd(xmm15, SYSV_XMM_REGS[*reg]).map_err(err)?;
                    asm.movsd_2(slot(index as u8), xmm15).map_err(err)?;
                }
                (AbiType::Integer { .. } | AbiType::Pointer, [SysvPiece::Gp(reg)]) => {
                    asm.mov(slot(index as u8), SYSV_REGS[*reg]).map_err(err)?;
                }
                _ => {
                    return Err(BackendError(format!(
                        "unsupported SysV export register parameter {scalar:?} at index {index}"
                    )));
                }
            },
            (AbiType::Float64, SysvArgLocation::Stack { offset }) => {
                asm.mov(rax, qword_ptr(rbp + (16 + *offset as i32)))
                    .map_err(err)?;
                asm.mov(slot(index as u8), rax).map_err(err)?;
            }
            (AbiType::Float32, SysvArgLocation::Stack { offset }) => {
                asm.movss(xmm15, dword_ptr(rbp + (16 + *offset as i32)))
                    .map_err(err)?;
                asm.cvtss2sd(xmm15, xmm15).map_err(err)?;
                asm.movsd_2(slot(index as u8), xmm15).map_err(err)?;
            }
            (AbiType::Integer { .. } | AbiType::Pointer, SysvArgLocation::Stack { offset }) => {
                asm.mov(rax, qword_ptr(rbp + (16 + *offset as i32)))
                    .map_err(err)?;
                asm.mov(slot(index as u8), rax).map_err(err)?;
            }
            _ => {
                return Err(BackendError(format!(
                    "unsupported SysV export parameter {ty:?} at index {index}"
                )));
            }
        }
    }
    Ok(())
}

fn emit_win64_export_prologue(
    asm: &mut CodeAssembler,
    export: &ForeignSymbol,
    layout: &AbiFrameLayout,
) -> Result<(), BackendError> {
    let err = |error: IcedError| BackendError(error.to_string());
    let has_sret = win64_return_in_memory(&export.signature.return_type);
    if has_sret {
        let disp = layout
            .export_sret_disp
            .expect("Win64 sret export must reserve a frame slot");
        asm.mov(qword_ptr(rbp + disp), rcx).map_err(err)?;
    }

    for (index, ty) in export.signature.params.iter().enumerate() {
        let position = index + usize::from(has_sret);
        let stack_disp = 48 + (position.saturating_sub(4) * 8) as i32;
        match ty {
            AbiType::Aggregate { size, .. } => {
                let dst_disp = layout.export_aggregate_disps[index]
                    .expect("aggregate export parameter must reserve local storage");
                if win64_pass_indirect(ty) {
                    if position < 4 {
                        asm.mov(r10, WIN64_REGS[position]).map_err(err)?;
                    } else {
                        asm.mov(r10, qword_ptr(rbp + stack_disp)).map_err(err)?;
                    }
                    emit_mem_copy(asm, rbp, dst_disp, r10, 0, usize::from(*size))?;
                } else {
                    if position < 4 {
                        asm.mov(rax, WIN64_REGS[position]).map_err(err)?;
                    } else {
                        asm.mov(rax, qword_ptr(rbp + stack_disp)).map_err(err)?;
                    }
                    emit_store_aggregate_bits(asm, rbp, dst_disp, usize::from(*size))?;
                }
                asm.lea(rax, qword_ptr(rbp + dst_disp)).map_err(err)?;
                asm.mov(slot(index as u8), rax).map_err(err)?;
            }
            AbiType::Float64 => {
                if position < 4 {
                    asm.movsd_2(slot(index as u8), WIN64_XMM_REGS[position])
                        .map_err(err)?;
                } else {
                    asm.mov(rax, qword_ptr(rbp + stack_disp)).map_err(err)?;
                    asm.mov(slot(index as u8), rax).map_err(err)?;
                }
            }
            AbiType::Float32 => {
                if position < 4 {
                    asm.cvtss2sd(xmm15, WIN64_XMM_REGS[position]).map_err(err)?;
                } else {
                    asm.movss(xmm15, dword_ptr(rbp + stack_disp)).map_err(err)?;
                    asm.cvtss2sd(xmm15, xmm15).map_err(err)?;
                }
                asm.movsd_2(slot(index as u8), xmm15).map_err(err)?;
            }
            AbiType::Integer { .. } | AbiType::Pointer => {
                if position < 4 {
                    asm.mov(slot(index as u8), WIN64_REGS[position])
                        .map_err(err)?;
                } else {
                    asm.mov(rax, qword_ptr(rbp + stack_disp)).map_err(err)?;
                    asm.mov(slot(index as u8), rax).map_err(err)?;
                }
            }
            AbiType::Void => {
                return Err(BackendError(
                    "void cannot be an export parameter".to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn max_reg_used(chunk: &Chunk) -> usize {
    let mut max = chunk.param_count.saturating_sub(1);
    for instr in &chunk.code {
        for register in crate::bytecode::regalloc::instruction_registers(instr) {
            max = max.max(register as usize);
        }
    }
    max
}

fn jump_targets(chunk: &Chunk) -> std::collections::HashSet<u16> {
    let mut set = std::collections::HashSet::new();
    for instr in &chunk.code {
        if let Some(
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
            | Opcode::Jnz,
        ) = Opcode::from_u8(instr.opcode)
        {
            let (_, target) = instr.ri16();
            set.insert(target);
        }
    }
    set
}

fn safe_fn_label(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ── FnEncoder ────────────────────────────────────────────────────────────────

pub struct FnEncoder<'a> {
    pub chunk: &'a Chunk,
    pub fn_table: &'a [String],
    /// Byte offset of this function from the start of the .text section.
    pub fn_offset: usize,
    /// str_syms[i] = Some(symbol_name) if constants[i] is Str.
    pub str_syms: &'a [Option<String>],
    /// bss_syms[i] = Some(symbol_name) if code[i] is PrimToStr.
    pub bss_syms: &'a [Option<String>],
    pub target: &'a TargetSpec,
}

impl<'a> FnEncoder<'a> {
    pub fn encode(&self) -> Result<(Vec<u8>, Vec<PendingReloc>), BackendError> {
        let chunk = self.chunk;
        let is_win64 = self.target.abi == Abi::Win64;
        let num_regs = max_reg_used(chunk) + 1;
        let abi_layout = abi_frame_layout(chunk, num_regs, is_win64);
        let frame_size = abi_layout.frame_size;

        let err = |e: IcedError| BackendError(e.to_string());

        let mut asm = CodeAssembler::new(64).map_err(err)?;

        // fn_start label at byte 0 of this function — used as dummy placeholder
        // for all external references that will become relocations.
        let mut fn_start = asm.create_label();
        asm.set_label(&mut fn_start).map_err(err)?;

        // Build label map for QZI jump targets (instr index → CodeLabel).
        let targets = jump_targets(chunk);
        let mut label_map: HashMap<usize, CodeLabel> = targets
            .iter()
            .map(|&t| (t as usize, asm.create_label()))
            .collect();
        // Implicit-return label at chunk.code.len() (jumped to by conditionals at fn end).
        let needs_implicit_ret = targets.contains(&(chunk.code.len() as u16));
        if needs_implicit_ret {
            label_map
                .entry(chunk.code.len())
                .or_insert_with(|| asm.create_label());
        }

        // Pending relocations: (asm_instr_idx, disp_field_offset_in_instr, kind, symbol, addend)
        let mut pending: Vec<(usize, usize, RelocKind, String, i64)> = vec![];

        macro_rules! emit {
            ($e:expr) => {
                $e.map_err(err)?
            };
        }

        // ── Prologue ─────────────────────────────────────────────────────────
        emit!(asm.push(rbp));
        emit!(asm.mov(rbp, rsp));
        if frame_size > 0 {
            emit!(asm.sub(rsp, frame_size));
        }
        if let Some(export) = &chunk.export {
            if is_win64 {
                emit_win64_export_prologue(&mut asm, export, &abi_layout)?;
            } else {
                emit_sysv_export_prologue(&mut asm, export, &abi_layout)?;
            }
        } else if is_win64 {
            for (i, register) in WIN64_REGS.iter().take(chunk.param_count).enumerate() {
                emit!(asm.mov(slot(i as u8), *register));
            }
            for i in 4..chunk.param_count {
                // Above saved rbp + return address + the 32-byte shadow space.
                emit!(asm.mov(rax, qword_ptr(rbp + 48 + ((i - 4) * 8) as i32)));
                emit!(asm.mov(slot(i as u8), rax));
            }
        } else {
            for (i, register) in SYSV_REGS.iter().take(chunk.param_count).enumerate() {
                emit!(asm.mov(slot(i as u8), *register));
            }
            for i in 6..chunk.param_count {
                emit!(asm.mov(rax, qword_ptr(rbp + 16 + ((i - 6) * 8) as i32)));
                emit!(asm.mov(slot(i as u8), rax));
            }
        }

        // ── Instruction loop ─────────────────────────────────────────────────
        let mut pending_args: Vec<u8> = vec![];

        for (qzi_idx, instr) in chunk.code.iter().enumerate() {
            // Set label if this QZI instruction index is a jump target.
            if let Some(lbl) = label_map.get_mut(&qzi_idx) {
                emit!(asm.set_label(lbl));
            }

            match Opcode::from_u8(instr.opcode) {
                // ── CallArg / CallIdx ─────────────────────────────────────────
                Some(Opcode::CallArg) => {
                    pending_args.push(instr.ops[0]);
                }

                Some(Opcode::CallIdx) => {
                    let (dst, fn_idx) = instr.ri16();
                    let fn_name = self
                        .fn_table
                        .get(fn_idx as usize)
                        .map(|s| safe_fn_label(s))
                        .unwrap_or_else(|| "__quazi_unknown".into());

                    if is_win64 {
                        let call_frame =
                            round_to_16(32 + pending_args.len().saturating_sub(4) * 8).max(32);
                        emit!(asm.sub(rsp, call_frame as i32));
                        for (i, &vreg) in pending_args.iter().enumerate().skip(4) {
                            emit!(asm.mov(rax, slot(vreg)));
                            emit!(asm.mov(qword_ptr(rsp + 32 + ((i - 4) * 8) as i32), rax));
                        }
                        for (i, &vreg) in pending_args.iter().enumerate().take(4) {
                            emit!(asm.mov(WIN64_REGS[i], slot(vreg)));
                        }
                        let call_idx = asm.instructions().len();
                        emit!(asm.call(fn_start));
                        pending.push((call_idx, 1, RelocKind::Plt32, fn_name, -4));
                        emit!(asm.add(rsp, call_frame as i32));
                    } else {
                        let stack_size = round_to_16(pending_args.len().saturating_sub(6) * 8);
                        if stack_size > 0 {
                            emit!(asm.sub(rsp, stack_size as i32));
                        }
                        for (i, &vreg) in pending_args.iter().enumerate().skip(6) {
                            emit!(asm.mov(rax, slot(vreg)));
                            emit!(asm.mov(qword_ptr(rsp + ((i - 6) * 8) as i32), rax));
                        }
                        for (i, &vreg) in pending_args.iter().enumerate().take(6) {
                            emit!(asm.mov(SYSV_REGS[i], slot(vreg)));
                        }
                        let call_idx = asm.instructions().len();
                        emit!(asm.call(fn_start));
                        pending.push((call_idx, 1, RelocKind::Plt32, fn_name, -4));
                        if stack_size > 0 {
                            emit!(asm.add(rsp, stack_size as i32));
                        }
                    }
                    emit!(asm.mov(slot(dst), rax));
                    pending_args.clear();
                }

                // ── CallReg: indirect call through a function pointer in a register ──
                Some(Opcode::CallReg) => {
                    let (dst, fn_reg, _) = instr.rrr();
                    if is_win64 {
                        let call_frame =
                            round_to_16(32 + pending_args.len().saturating_sub(4) * 8).max(32);
                        emit!(asm.sub(rsp, call_frame as i32));
                        for (i, &vreg) in pending_args.iter().enumerate().skip(4) {
                            emit!(asm.mov(rax, slot(vreg)));
                            emit!(asm.mov(qword_ptr(rsp + 32 + ((i - 4) * 8) as i32), rax));
                        }
                        for (i, &vreg) in pending_args.iter().enumerate().take(4) {
                            emit!(asm.mov(WIN64_REGS[i], slot(vreg)));
                        }
                        emit!(asm.mov(rax, slot(fn_reg)));
                        emit!(asm.call(rax));
                        emit!(asm.add(rsp, call_frame as i32));
                    } else {
                        let stack_size = round_to_16(pending_args.len().saturating_sub(6) * 8);
                        if stack_size > 0 {
                            emit!(asm.sub(rsp, stack_size as i32));
                        }
                        for (i, &vreg) in pending_args.iter().enumerate().skip(6) {
                            emit!(asm.mov(rax, slot(vreg)));
                            emit!(asm.mov(qword_ptr(rsp + ((i - 6) * 8) as i32), rax));
                        }
                        for (i, &vreg) in pending_args.iter().enumerate().take(6) {
                            emit!(asm.mov(SYSV_REGS[i], slot(vreg)));
                        }
                        emit!(asm.mov(rax, slot(fn_reg)));
                        emit!(asm.call(rax));
                        if stack_size > 0 {
                            emit!(asm.add(rsp, stack_size as i32));
                        }
                    }
                    emit!(asm.mov(slot(dst), rax));
                    pending_args.clear();
                }

                // ── All other opcodes ─────────────────────────────────────────
                Some(Opcode::CallExt) => {
                    let (dst, constant_index) = instr.ri16();
                    if let Some(ConstPoolEntry::ForeignSymbol(foreign)) =
                        chunk.constants.get(constant_index as usize)
                    {
                        if is_win64 {
                            self.emit_win64_foreign_call(
                                &mut asm,
                                dst,
                                foreign,
                                &pending_args,
                                abi_layout.foreign_result_disp,
                                &mut pending,
                                fn_start,
                                None,
                            )?;
                        } else {
                            self.emit_sysv_foreign_call(
                                &mut asm,
                                dst,
                                foreign,
                                &pending_args,
                                abi_layout.foreign_result_disp,
                                &mut pending,
                                fn_start,
                                None,
                            )?;
                        }
                        pending_args.clear();
                    } else {
                        self.emit_instr(
                            &mut asm,
                            instr,
                            chunk,
                            qzi_idx,
                            Some(Opcode::CallExt),
                            &mut pending,
                            &label_map,
                            fn_start,
                        )?;
                    }
                }

                Some(Opcode::CallCReg) => {
                    let (dst, function, constant_index) = instr.call_c_reg_parts();
                    let Some(ConstPoolEntry::ForeignSymbol(foreign)) =
                        chunk.constants.get(constant_index as usize)
                    else {
                        return Err(BackendError(
                            "C function-pointer call is missing ABI signature metadata".to_string(),
                        ));
                    };
                    if is_win64 {
                        self.emit_win64_foreign_call(
                            &mut asm,
                            dst,
                            foreign,
                            &pending_args,
                            abi_layout.foreign_result_disp,
                            &mut pending,
                            fn_start,
                            Some(function),
                        )?;
                    } else {
                        self.emit_sysv_foreign_call(
                            &mut asm,
                            dst,
                            foreign,
                            &pending_args,
                            abi_layout.foreign_result_disp,
                            &mut pending,
                            fn_start,
                            Some(function),
                        )?;
                    }
                    pending_args.clear();
                }

                Some(Opcode::Ret) if chunk.export.is_some() && !is_win64 => {
                    self.emit_sysv_export_return(
                        &mut asm,
                        instr.ops[0],
                        chunk.export.as_ref().unwrap(),
                        &abi_layout,
                    )?;
                }

                Some(Opcode::Ret) if chunk.export.is_some() && is_win64 => {
                    self.emit_win64_export_return(
                        &mut asm,
                        instr.ops[0],
                        chunk.export.as_ref().unwrap(),
                        &abi_layout,
                    )?;
                }

                op => {
                    self.emit_instr(
                        &mut asm,
                        instr,
                        chunk,
                        qzi_idx,
                        op,
                        &mut pending,
                        &label_map,
                        fn_start,
                    )?;
                }
            }
        }

        // ── Implicit return at chunk.code.len() ──────────────────────────────
        if needs_implicit_ret {
            if let Some(lbl) = label_map.get_mut(&chunk.code.len()) {
                emit!(asm.set_label(lbl));
            }
            emit!(asm.xor(rax, rax));
            emit!(asm.mov(rsp, rbp));
            emit!(asm.pop(rbp));
            emit!(asm.ret());
        }

        // ── Assemble ─────────────────────────────────────────────────────────
        let mut bytes = asm.assemble(self.fn_offset as u64).map_err(err)?;

        // Compute per-instruction byte offsets by decoding the assembled output.
        // Labels have no bytes, so decoder and asm.instructions() stay in sync.
        let offsets: Vec<usize> = {
            let mut out = Vec::with_capacity(asm.instructions().len());
            let mut dec = iced_x86::Decoder::with_ip(
                64,
                &bytes,
                self.fn_offset as u64,
                iced_x86::DecoderOptions::NONE,
            );
            let mut tmp = iced_x86::Instruction::default();
            while dec.can_decode() {
                out.push((dec.ip() - self.fn_offset as u64) as usize);
                dec.decode_out(&mut tmp);
            }
            out
        };

        // Build relocation list and zero placeholder displacements.
        let mut relocs = Vec::with_capacity(pending.len());
        for (asm_idx, disp_off, kind, sym, addend) in pending {
            let byte_off = *offsets.get(asm_idx).unwrap_or(&0);
            let field = byte_off + disp_off;
            bytes[field..field + 4].fill(0);
            relocs.push(PendingReloc {
                offset_in_text: self.fn_offset + field,
                kind,
                symbol: sym,
                addend,
            });
        }

        Ok((bytes, relocs))
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_win64_foreign_call(
        &self,
        asm: &mut CodeAssembler,
        dst: u8,
        foreign: &ForeignSymbol,
        arg_regs: &[u8],
        result_disp: Option<i32>,
        pending: &mut Vec<(usize, usize, RelocKind, String, i64)>,
        fn_start: CodeLabel,
        indirect_function: Option<u8>,
    ) -> Result<(), BackendError> {
        let err = |error: IcedError| BackendError(error.to_string());
        if foreign.signature.params.len() != arg_regs.len() {
            return Err(BackendError(format!(
                "foreign call `{}` expected {} ABI arguments, got {}",
                foreign.symbol,
                foreign.signature.params.len(),
                arg_regs.len()
            )));
        }

        let return_in_memory = win64_return_in_memory(&foreign.signature.return_type);
        let aggregate_return = matches!(foreign.signature.return_type, AbiType::Aggregate { .. });
        if aggregate_return {
            let scratch = result_disp.expect("aggregate foreign result needs scratch storage");
            let allocation_size = round_to_8(foreign.signature.return_type.size()).max(8);
            asm.mov(rcx, allocation_size as i64).map_err(err)?;
            let call_index = asm.instructions().len();
            asm.call(fn_start).map_err(err)?;
            pending.push((call_index, 1, RelocKind::Plt32, "malloc".to_string(), -4));
            asm.mov(qword_ptr(rbp + scratch), rax).map_err(err)?;
        }

        let first_position = usize::from(return_in_memory);
        let position_count = first_position + foreign.signature.params.len();
        let stack_slots = position_count.saturating_sub(4);
        let stack_area = 32 + stack_slots * 8;
        let mut temp_cursor = round_to_16(stack_area);
        let mut temp_offsets = Vec::with_capacity(foreign.signature.params.len());
        for ty in &foreign.signature.params {
            if win64_pass_indirect(ty) {
                temp_offsets.push(Some(temp_cursor));
                temp_cursor += round_to_16(ty.size()).max(16);
            } else {
                temp_offsets.push(None);
            }
        }
        let call_frame_size = round_to_16(temp_cursor).max(32);
        asm.sub(rsp, call_frame_size as i32).map_err(err)?;

        for (index, (ty, vreg)) in foreign
            .signature
            .params
            .iter()
            .zip(arg_regs.iter())
            .enumerate()
        {
            let position = first_position + index;
            let stack_disp = 32 + (position.saturating_sub(4) * 8) as i32;
            match ty {
                AbiType::Aggregate { size, .. } => {
                    asm.mov(r10, slot(*vreg)).map_err(err)?;
                    if let Some(temp_offset) = temp_offsets[index] {
                        emit_mem_copy(asm, rsp, temp_offset as i32, r10, 0, usize::from(*size))?;
                        asm.lea(rax, qword_ptr(rsp + temp_offset as i32))
                            .map_err(err)?;
                    } else {
                        emit_load_aggregate_bits(asm, r10, 0, usize::from(*size))?;
                    }
                    if position < 4 {
                        asm.mov(WIN64_REGS[position], rax).map_err(err)?;
                    } else {
                        asm.mov(qword_ptr(rsp + stack_disp), rax).map_err(err)?;
                    }
                }
                AbiType::Float64 => {
                    asm.movq(xmm15, slot(*vreg)).map_err(err)?;
                    if position < 4 {
                        asm.movq(WIN64_XMM_REGS[position], xmm15).map_err(err)?;
                        if foreign.signature.variadic {
                            asm.movq(rax, xmm15).map_err(err)?;
                            asm.mov(WIN64_REGS[position], rax).map_err(err)?;
                        }
                    } else {
                        asm.movq(qword_ptr(rsp + stack_disp), xmm15).map_err(err)?;
                    }
                }
                AbiType::Float32 => {
                    asm.movq(xmm15, slot(*vreg)).map_err(err)?;
                    asm.cvtsd2ss(xmm15, xmm15).map_err(err)?;
                    if position < 4 {
                        asm.movss(WIN64_XMM_REGS[position], xmm15).map_err(err)?;
                        if foreign.signature.variadic {
                            asm.movd(eax, xmm15).map_err(err)?;
                            asm.mov(WIN64_REGS[position], rax).map_err(err)?;
                        }
                    } else {
                        asm.movss(dword_ptr(rsp + stack_disp), xmm15).map_err(err)?;
                    }
                }
                AbiType::Integer { .. } | AbiType::Pointer => {
                    asm.mov(rax, slot(*vreg)).map_err(err)?;
                    if position < 4 {
                        asm.mov(WIN64_REGS[position], rax).map_err(err)?;
                    } else {
                        asm.mov(qword_ptr(rsp + stack_disp), rax).map_err(err)?;
                    }
                }
                AbiType::Void => {
                    return Err(BackendError(
                        "void cannot be a foreign argument".to_string(),
                    ));
                }
            }
        }
        if return_in_memory {
            let scratch = result_disp.expect("Win64 sret foreign result needs scratch storage");
            asm.mov(rcx, qword_ptr(rbp + scratch)).map_err(err)?;
        }

        if let Some(function) = indirect_function {
            asm.mov(r11, slot(function)).map_err(err)?;
            asm.call(r11).map_err(err)?;
        } else {
            let call_index = asm.instructions().len();
            asm.call(fn_start).map_err(err)?;
            pending.push((call_index, 1, RelocKind::Plt32, foreign.symbol.clone(), -4));
        }
        asm.add(rsp, call_frame_size as i32).map_err(err)?;

        match &foreign.signature.return_type {
            AbiType::Void => {
                asm.xor(rax, rax).map_err(err)?;
                asm.mov(slot(dst), rax).map_err(err)?;
            }
            AbiType::Integer { bytes, signed } => {
                match (*bytes, *signed) {
                    (1, true) => asm.movsx(rax, al).map_err(err)?,
                    (2, true) => asm.movsx(rax, ax).map_err(err)?,
                    (4, true) => asm.movsxd(rax, eax).map_err(err)?,
                    (1, false) => asm.movzx(rax, al).map_err(err)?,
                    (2, false) => asm.movzx(rax, ax).map_err(err)?,
                    (4, false) => asm.mov(eax, eax).map_err(err)?,
                    _ => {}
                }
                asm.mov(slot(dst), rax).map_err(err)?;
            }
            AbiType::Pointer => asm.mov(slot(dst), rax).map_err(err)?,
            AbiType::Float64 => asm.movsd_2(slot(dst), xmm0).map_err(err)?,
            AbiType::Float32 => {
                asm.cvtss2sd(xmm15, xmm0).map_err(err)?;
                asm.movsd_2(slot(dst), xmm15).map_err(err)?;
            }
            AbiType::Aggregate { size, .. } if return_in_memory => {
                let scratch = result_disp.expect("aggregate result needs scratch storage");
                asm.mov(rax, qword_ptr(rbp + scratch)).map_err(err)?;
                asm.mov(slot(dst), rax).map_err(err)?;
            }
            AbiType::Aggregate { size, .. } => {
                let scratch = result_disp.expect("aggregate result needs scratch storage");
                asm.mov(r10, qword_ptr(rbp + scratch)).map_err(err)?;
                emit_store_aggregate_bits(asm, r10, 0, usize::from(*size))?;
                asm.mov(slot(dst), r10).map_err(err)?;
            }
        }
        Ok(())
    }

    fn emit_win64_export_return(
        &self,
        asm: &mut CodeAssembler,
        return_reg: u8,
        export: &ForeignSymbol,
        layout: &AbiFrameLayout,
    ) -> Result<(), BackendError> {
        let err = |error: IcedError| BackendError(error.to_string());
        match &export.signature.return_type {
            AbiType::Void => asm.xor(rax, rax).map_err(err)?,
            AbiType::Integer { .. } | AbiType::Pointer => {
                asm.mov(rax, slot(return_reg)).map_err(err)?;
            }
            AbiType::Float64 => asm.movq(xmm0, slot(return_reg)).map_err(err)?,
            AbiType::Float32 => {
                asm.movq(xmm15, slot(return_reg)).map_err(err)?;
                asm.cvtsd2ss(xmm0, xmm15).map_err(err)?;
            }
            AbiType::Aggregate { size, .. }
                if win64_return_in_memory(&export.signature.return_type) =>
            {
                let sret = layout
                    .export_sret_disp
                    .expect("Win64 aggregate export return needs sret storage");
                asm.mov(r10, qword_ptr(rbp + sret)).map_err(err)?;
                asm.mov(r11, slot(return_reg)).map_err(err)?;
                emit_mem_copy(asm, r10, 0, r11, 0, usize::from(*size))?;
                asm.mov(rax, r10).map_err(err)?;
            }
            AbiType::Aggregate { size, .. } => {
                asm.mov(r10, slot(return_reg)).map_err(err)?;
                emit_load_aggregate_bits(asm, r10, 0, usize::from(*size))?;
            }
        }
        asm.mov(rsp, rbp).map_err(err)?;
        asm.pop(rbp).map_err(err)?;
        asm.ret().map_err(err)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_sysv_foreign_call(
        &self,
        asm: &mut CodeAssembler,
        dst: u8,
        foreign: &ForeignSymbol,
        arg_regs: &[u8],
        result_disp: Option<i32>,
        pending: &mut Vec<(usize, usize, RelocKind, String, i64)>,
        fn_start: CodeLabel,
        indirect_function: Option<u8>,
    ) -> Result<(), BackendError> {
        let err = |error: IcedError| BackendError(error.to_string());
        if foreign.signature.params.len() != arg_regs.len() {
            return Err(BackendError(format!(
                "foreign call `{}` expected {} ABI arguments, got {}",
                foreign.symbol,
                foreign.signature.params.len(),
                arg_regs.len()
            )));
        }
        let plan = plan_sysv_call(&foreign.signature);
        let return_class = classify(&foreign.signature.return_type);
        let aggregate_return = matches!(foreign.signature.return_type, AbiType::Aggregate { .. });

        if aggregate_return {
            let scratch = result_disp.expect("aggregate foreign result needs scratch storage");
            let allocation_size = round_to_8(foreign.signature.return_type.size()).max(8);
            asm.mov(rdi, allocation_size as i64).map_err(err)?;
            let call_index = asm.instructions().len();
            asm.call(fn_start).map_err(err)?;
            pending.push((call_index, 1, RelocKind::Plt32, "malloc".to_string(), -4));
            asm.mov(qword_ptr(rbp + scratch), rax).map_err(err)?;
        }

        if plan.stack_size > 0 {
            asm.sub(rsp, plan.stack_size as i32).map_err(err)?;
        }

        // Materialize stack arguments before register arguments so scratch use
        // cannot overwrite an already assigned ABI register.
        for ((ty, location), vreg) in foreign
            .signature
            .params
            .iter()
            .zip(plan.args.iter())
            .zip(arg_regs.iter())
        {
            let SysvArgLocation::Stack { offset } = location else {
                continue;
            };
            match ty {
                AbiType::Aggregate { .. } => {
                    asm.mov(r10, slot(*vreg)).map_err(err)?;
                    emit_mem_copy(asm, rsp, *offset as i32, r10, 0, ty.size())?;
                }
                AbiType::Float32 => {
                    asm.movq(xmm15, slot(*vreg)).map_err(err)?;
                    asm.cvtsd2ss(xmm15, xmm15).map_err(err)?;
                    asm.movss(dword_ptr(rsp + *offset as i32), xmm15)
                        .map_err(err)?;
                }
                AbiType::Float64 | AbiType::Integer { .. } | AbiType::Pointer => {
                    asm.mov(rax, slot(*vreg)).map_err(err)?;
                    asm.mov(qword_ptr(rsp + *offset as i32), rax).map_err(err)?;
                }
                AbiType::Void => {
                    return Err(BackendError(
                        "void cannot be a foreign argument".to_string(),
                    ));
                }
            }
        }

        for ((ty, location), vreg) in foreign
            .signature
            .params
            .iter()
            .zip(plan.args.iter())
            .zip(arg_regs.iter())
        {
            let SysvArgLocation::Registers(pieces) = location else {
                continue;
            };
            match ty {
                AbiType::Aggregate { .. } => {
                    asm.mov(r10, slot(*vreg)).map_err(err)?;
                    for (piece_index, piece) in pieces.iter().enumerate() {
                        let offset = (piece_index * 8) as i32;
                        let piece_size = ty.size().saturating_sub(piece_index * 8).min(8);
                        match piece {
                            SysvPiece::Gp(reg) => {
                                emit_load_aggregate_bits(asm, r10, offset, piece_size)?;
                                asm.mov(SYSV_REGS[*reg], rax).map_err(err)?;
                            }
                            SysvPiece::Sse(reg) => emit_load_sse_piece(
                                asm,
                                SYSV_XMM_REGS[*reg],
                                r10,
                                offset,
                                piece_size,
                            )?,
                        }
                    }
                }
                AbiType::Float64 => {
                    let [SysvPiece::Sse(reg)] = pieces.as_slice() else {
                        return Err(BackendError("invalid SysV f64 classification".to_string()));
                    };
                    asm.movq(SYSV_XMM_REGS[*reg], slot(*vreg)).map_err(err)?;
                }
                AbiType::Float32 => {
                    let [SysvPiece::Sse(reg)] = pieces.as_slice() else {
                        return Err(BackendError("invalid SysV f32 classification".to_string()));
                    };
                    asm.movq(xmm15, slot(*vreg)).map_err(err)?;
                    asm.cvtsd2ss(SYSV_XMM_REGS[*reg], xmm15).map_err(err)?;
                }
                AbiType::Integer { .. } | AbiType::Pointer => {
                    let [SysvPiece::Gp(reg)] = pieces.as_slice() else {
                        return Err(BackendError(
                            "invalid SysV integer classification".to_string(),
                        ));
                    };
                    asm.mov(SYSV_REGS[*reg], slot(*vreg)).map_err(err)?;
                }
                AbiType::Void => {
                    return Err(BackendError(
                        "void cannot be a foreign argument".to_string(),
                    ));
                }
            }
        }

        if matches!(return_class, TypeClass::Memory) {
            let scratch = result_disp.expect("sret foreign result needs scratch storage");
            asm.mov(rdi, qword_ptr(rbp + scratch)).map_err(err)?;
        }
        if foreign.signature.variadic {
            asm.mov(eax, plan.sse_used as i32).map_err(err)?;
        }

        if let Some(function) = indirect_function {
            asm.mov(r11, slot(function)).map_err(err)?;
            asm.call(r11).map_err(err)?;
        } else {
            let call_index = asm.instructions().len();
            asm.call(fn_start).map_err(err)?;
            pending.push((call_index, 1, RelocKind::Plt32, foreign.symbol.clone(), -4));
        }
        if plan.stack_size > 0 {
            asm.add(rsp, plan.stack_size as i32).map_err(err)?;
        }

        match (&foreign.signature.return_type, return_class) {
            (AbiType::Void, _) => {
                asm.xor(rax, rax).map_err(err)?;
                asm.mov(slot(dst), rax).map_err(err)?;
            }
            (AbiType::Integer { bytes, signed }, _) => {
                match (*bytes, *signed) {
                    (1, true) => asm.movsx(rax, al).map_err(err)?,
                    (2, true) => asm.movsx(rax, ax).map_err(err)?,
                    (4, true) => asm.movsxd(rax, eax).map_err(err)?,
                    (1, false) => asm.movzx(rax, al).map_err(err)?,
                    (2, false) => asm.movzx(rax, ax).map_err(err)?,
                    (4, false) => asm.mov(eax, eax).map_err(err)?,
                    _ => {}
                }
                asm.mov(slot(dst), rax).map_err(err)?;
            }
            (AbiType::Pointer, _) => {
                asm.mov(slot(dst), rax).map_err(err)?;
            }
            (AbiType::Float64, _) => {
                asm.movsd_2(slot(dst), xmm0).map_err(err)?;
            }
            (AbiType::Float32, _) => {
                asm.cvtss2sd(xmm15, xmm0).map_err(err)?;
                asm.movsd_2(slot(dst), xmm15).map_err(err)?;
            }
            (AbiType::Aggregate { .. }, TypeClass::Memory) => {
                let scratch = result_disp.expect("aggregate result needs scratch storage");
                asm.mov(rax, qword_ptr(rbp + scratch)).map_err(err)?;
                asm.mov(slot(dst), rax).map_err(err)?;
            }
            (AbiType::Aggregate { .. }, TypeClass::Registers(classes)) => {
                let scratch = result_disp.expect("aggregate result needs scratch storage");
                asm.mov(r10, qword_ptr(rbp + scratch)).map_err(err)?;
                asm.mov(r11, rax).map_err(err)?;
                for (piece_index, class) in classes.iter().enumerate().rev() {
                    let offset = (piece_index * 8) as i32;
                    let piece_size = foreign
                        .signature
                        .return_type
                        .size()
                        .saturating_sub(piece_index * 8)
                        .min(8);
                    match class {
                        EightbyteClass::Integer => {
                            let gp = classes[..=piece_index]
                                .iter()
                                .filter(|class| **class == EightbyteClass::Integer)
                                .count()
                                - 1;
                            let reg = [r11, rdx][gp];
                            asm.mov(rax, reg).map_err(err)?;
                            emit_store_aggregate_bits(asm, r10, offset, piece_size)?;
                        }
                        EightbyteClass::Sse => {
                            let sse = classes[..=piece_index]
                                .iter()
                                .filter(|class| **class == EightbyteClass::Sse)
                                .count()
                                - 1;
                            let reg = [xmm0, xmm1][sse];
                            emit_store_sse_piece(asm, r10, offset, reg, piece_size)?;
                        }
                    }
                }
                asm.mov(slot(dst), r10).map_err(err)?;
            }
        }
        Ok(())
    }

    fn emit_sysv_export_return(
        &self,
        asm: &mut CodeAssembler,
        return_reg: u8,
        export: &ForeignSymbol,
        layout: &AbiFrameLayout,
    ) -> Result<(), BackendError> {
        let err = |error: IcedError| BackendError(error.to_string());
        match (
            &export.signature.return_type,
            classify(&export.signature.return_type),
        ) {
            (AbiType::Void, _) => asm.xor(rax, rax).map_err(err)?,
            (AbiType::Integer { .. } | AbiType::Pointer, _) => {
                asm.mov(rax, slot(return_reg)).map_err(err)?;
            }
            (AbiType::Float64, _) => {
                asm.movq(xmm0, slot(return_reg)).map_err(err)?;
            }
            (AbiType::Float32, _) => {
                asm.movq(xmm15, slot(return_reg)).map_err(err)?;
                asm.cvtsd2ss(xmm0, xmm15).map_err(err)?;
            }
            (AbiType::Aggregate { .. }, TypeClass::Memory) => {
                let sret = layout
                    .export_sret_disp
                    .expect("SysV aggregate export return needs sret storage");
                asm.mov(r10, qword_ptr(rbp + sret)).map_err(err)?;
                asm.mov(r11, slot(return_reg)).map_err(err)?;
                emit_mem_copy(asm, r10, 0, r11, 0, export.signature.return_type.size())?;
                asm.mov(rax, r10).map_err(err)?;
            }
            (AbiType::Aggregate { .. }, TypeClass::Registers(classes)) => {
                asm.mov(r10, slot(return_reg)).map_err(err)?;
                for (piece_index, class) in classes.iter().enumerate().rev() {
                    let offset = (piece_index * 8) as i32;
                    let piece_size = export
                        .signature
                        .return_type
                        .size()
                        .saturating_sub(piece_index * 8)
                        .min(8);
                    match class {
                        EightbyteClass::Integer => {
                            let gp = classes[..=piece_index]
                                .iter()
                                .filter(|class| **class == EightbyteClass::Integer)
                                .count()
                                - 1;
                            let reg = [rax, rdx][gp];
                            emit_load_aggregate_bits(asm, r10, offset, piece_size)?;
                            if reg != rax {
                                asm.mov(reg, rax).map_err(err)?;
                            }
                        }
                        EightbyteClass::Sse => {
                            let sse = classes[..=piece_index]
                                .iter()
                                .filter(|class| **class == EightbyteClass::Sse)
                                .count()
                                - 1;
                            let reg = [xmm0, xmm1][sse];
                            emit_load_sse_piece(asm, reg, r10, offset, piece_size)?;
                        }
                    }
                }
            }
        }
        asm.mov(rsp, rbp).map_err(err)?;
        asm.pop(rbp).map_err(err)?;
        asm.ret().map_err(err)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_instr(
        &self,
        asm: &mut CodeAssembler,
        instr: &crate::bytecode::Instruction,
        chunk: &Chunk,
        qzi_idx: usize,
        op: Option<Opcode>,
        pending: &mut Vec<(usize, usize, RelocKind, String, i64)>,
        label_map: &HashMap<usize, CodeLabel>,
        fn_start: CodeLabel,
    ) -> Result<(), BackendError> {
        let is_win64 = self.target.abi == Abi::Win64;
        let err = |e: IcedError| BackendError(e.to_string());

        macro_rules! emit {
            ($e:expr) => {
                $e.map_err(err)?
            };
        }

        // Record asm instruction index, add reloc entry, and return the used label.
        macro_rules! call_ext {
            ($sym:expr, $kind:expr) => {{
                let idx = asm.instructions().len();
                emit!(asm.call(fn_start));
                pending.push((idx, 1, $kind, $sym, -4));
            }};
        }

        macro_rules! lea_rip {
            ($reg:expr, $sym:expr) => {{
                let idx = asm.instructions().len();
                emit!(asm.lea($reg, qword_ptr(fn_start)));
                pending.push((idx, 3, RelocKind::Pc32, $sym, -4));
            }};
        }

        match op {
            Some(Opcode::Nop) => {
                emit!(asm.nop());
            }

            Some(Opcode::Mov) => {
                let (dst, src, _) = instr.rrr();
                emit!(asm.mov(rax, slot(src)));
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::MovI) => {
                let (dst, imm) = instr.ri16();
                emit!(asm.mov(rax, imm as i64));
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::MovConst) => {
                let (dst, idx) = instr.ri16();
                match chunk.constants.get(idx as usize) {
                    Some(ConstPoolEntry::Int(n)) => {
                        emit!(asm.mov(rax, *n));
                        emit!(asm.mov(slot(dst), rax));
                    }
                    Some(ConstPoolEntry::Float(f)) => {
                        let bits = f.to_bits() as i64;
                        emit!(asm.mov(rax, bits));
                        emit!(asm.mov(slot(dst), rax));
                    }
                    Some(ConstPoolEntry::Str(_) | ConstPoolEntry::Bytes(_)) => {
                        if let Some(sym) = self.str_syms.get(idx as usize).and_then(|s| s.as_ref())
                        {
                            lea_rip!(rax, sym.clone());
                            emit!(asm.mov(slot(dst), rax));
                        }
                    }
                    Some(ConstPoolEntry::FnAddr(name)) => {
                        // Look up the function symbol name. FnAddr stores the raw function name;
                        // the symbol may have __quazi_intr_ prefix or safe-label mangling.
                        let sym = self
                            .fn_table
                            .iter()
                            .find(|s| {
                                s == &name
                                    || s.trim_start_matches("__quazi_intr_") == name
                                    || s.trim_start_matches("__quazi_intr_") == safe_fn_label(name)
                            })
                            .cloned()
                            .unwrap_or_else(|| safe_fn_label(name));
                        lea_rip!(rax, sym);
                        emit!(asm.mov(slot(dst), rax));
                    }
                    Some(ConstPoolEntry::VtableAddr(type_name, trait_name)) => {
                        let sym = format!(
                            "__vtable_{}_{}",
                            safe_fn_label(type_name),
                            safe_fn_label(trait_name)
                        );
                        lea_rip!(rax, sym);
                        emit!(asm.mov(slot(dst), rax));
                    }
                    Some(ConstPoolEntry::ForeignGlobal(global)) => {
                        lea_rip!(rax, global.symbol.clone());
                        emit!(asm.mov(slot(dst), rax));
                    }
                    Some(ConstPoolEntry::ForeignSymbol(_)) => {
                        return Err(BackendError(
                            "encoder: foreign symbol used as a runtime constant".to_string(),
                        ));
                    }
                    None => {
                        emit!(asm.xor(rax, rax));
                        emit!(asm.mov(slot(dst), rax));
                    }
                }
            }

            Some(Opcode::Add) => {
                let (dst, s1, s2) = instr.rrr();
                if instr.flags & crate::bytecode::instruction::FLOAT_FLAG != 0 {
                    emit!(asm.movq(xmm0, slot(s1)));
                    emit!(asm.movq(xmm1, slot(s2)));
                    emit!(asm.addsd(xmm0, xmm1));
                    emit!(asm.movq(slot(dst), xmm0));
                } else {
                    emit!(asm.mov(rax, slot(s1)));
                    emit!(asm.add(rax, slot(s2)));
                    emit!(asm.mov(slot(dst), rax));
                }
            }

            Some(Opcode::Sub) => {
                let (dst, s1, s2) = instr.rrr();
                if instr.flags & crate::bytecode::instruction::FLOAT_FLAG != 0 {
                    emit!(asm.movq(xmm0, slot(s1)));
                    emit!(asm.movq(xmm1, slot(s2)));
                    emit!(asm.subsd(xmm0, xmm1));
                    emit!(asm.movq(slot(dst), xmm0));
                } else {
                    emit!(asm.mov(rax, slot(s1)));
                    emit!(asm.sub(rax, slot(s2)));
                    emit!(asm.mov(slot(dst), rax));
                }
            }

            Some(Opcode::Mul) => {
                let (dst, s1, s2) = instr.rrr();
                if instr.flags & crate::bytecode::instruction::FLOAT_FLAG != 0 {
                    emit!(asm.movq(xmm0, slot(s1)));
                    emit!(asm.movq(xmm1, slot(s2)));
                    emit!(asm.mulsd(xmm0, xmm1));
                    emit!(asm.movq(slot(dst), xmm0));
                } else {
                    emit!(asm.mov(rax, slot(s1)));
                    emit!(asm.imul_2(rax, slot(s2)));
                    emit!(asm.mov(slot(dst), rax));
                }
            }

            Some(Opcode::Div) => {
                let (dst, s1, s2) = instr.rrr();
                if instr.flags & crate::bytecode::instruction::FLOAT_FLAG != 0 {
                    emit!(asm.movq(xmm0, slot(s1)));
                    emit!(asm.movq(xmm1, slot(s2)));
                    emit!(asm.divsd(xmm0, xmm1));
                    emit!(asm.movq(slot(dst), xmm0));
                } else {
                    emit!(asm.mov(rax, slot(s1)));
                    emit!(asm.cqo());
                    emit!(asm.idiv(slot(s2)));
                    emit!(asm.mov(slot(dst), rax));
                }
            }

            Some(Opcode::Mod) => {
                let (dst, s1, s2) = instr.rrr();
                emit!(asm.mov(rax, slot(s1)));
                emit!(asm.cqo());
                emit!(asm.idiv(slot(s2)));
                emit!(asm.mov(slot(dst), rdx));
            }

            Some(Opcode::Neg) => {
                let (dst, src, _) = instr.rrr();
                emit!(asm.mov(rax, slot(src)));
                emit!(asm.neg(rax));
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::Not) => {
                let (dst, src, _) = instr.rrr();
                emit!(asm.mov(rax, slot(src)));
                emit!(asm.not(rax));
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::Inc) => {
                let (dst, _, _) = instr.rrr();
                emit!(asm.inc(slot(dst)));
            }

            Some(Opcode::Dec) => {
                let (dst, _, _) = instr.rrr();
                emit!(asm.dec(slot(dst)));
            }

            Some(Opcode::And) => {
                let (dst, s1, s2) = instr.rrr();
                emit!(asm.mov(rax, slot(s1)));
                emit!(asm.and(rax, slot(s2)));
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::Or) => {
                let (dst, s1, s2) = instr.rrr();
                emit!(asm.mov(rax, slot(s1)));
                emit!(asm.or(rax, slot(s2)));
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::Xor) => {
                let (dst, s1, s2) = instr.rrr();
                emit!(asm.mov(rax, slot(s1)));
                emit!(asm.xor(rax, slot(s2)));
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::Shl) => {
                let (dst, s1, s2) = instr.rrr();
                emit!(asm.mov(rax, slot(s1)));
                emit!(asm.mov(rcx, slot(s2)));
                emit!(asm.shl(rax, cl));
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::Shr) => {
                let (dst, s1, s2) = instr.rrr();
                emit!(asm.mov(rax, slot(s1)));
                emit!(asm.mov(rcx, slot(s2)));
                emit!(asm.shr(rax, cl));
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::Sar) => {
                let (dst, s1, s2) = instr.rrr();
                emit!(asm.mov(rax, slot(s1)));
                emit!(asm.mov(rcx, slot(s2)));
                emit!(asm.sar(rax, cl));
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::Pow) => {
                let (dst, s1, s2) = instr.rrr();
                // Both SysV and Win64 pass floating-point args in xmm0/xmm1.
                emit!(asm.cvtsi2sd(xmm0, slot(s1)));
                emit!(asm.cvtsi2sd(xmm1, slot(s2)));
                call_ext!("pow".into(), RelocKind::Plt32);
                emit!(asm.cvttsd2si(rax, xmm0));
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::Cmp) => {
                let (_, s1, s2) = instr.rrr();
                emit!(asm.mov(rax, slot(s1)));
                emit!(asm.cmp(rax, slot(s2)));
            }

            Some(Opcode::Jmp) => {
                let (_, target) = instr.ri16();
                let lbl = *label_map
                    .get(&(target as usize))
                    .expect("Jmp target missing label");
                emit!(asm.jmp(lbl));
            }
            Some(Opcode::Je) => {
                let (_, t) = instr.ri16();
                let lbl = *label_map.get(&(t as usize)).expect("Je target");
                emit!(asm.je(lbl));
            }
            Some(Opcode::Jne) => {
                let (_, t) = instr.ri16();
                let lbl = *label_map.get(&(t as usize)).expect("Jne target");
                emit!(asm.jne(lbl));
            }
            Some(Opcode::Jg) => {
                let (_, t) = instr.ri16();
                let lbl = *label_map.get(&(t as usize)).expect("Jg target");
                emit!(asm.jg(lbl));
            }
            Some(Opcode::Jge) => {
                let (_, t) = instr.ri16();
                let lbl = *label_map.get(&(t as usize)).expect("Jge target");
                emit!(asm.jge(lbl));
            }
            Some(Opcode::Jl) => {
                let (_, t) = instr.ri16();
                let lbl = *label_map.get(&(t as usize)).expect("Jl target");
                emit!(asm.jl(lbl));
            }
            Some(Opcode::Jle) => {
                let (_, t) = instr.ri16();
                let lbl = *label_map.get(&(t as usize)).expect("Jle target");
                emit!(asm.jle(lbl));
            }
            Some(Opcode::Ja) => {
                let (_, t) = instr.ri16();
                let lbl = *label_map.get(&(t as usize)).expect("Ja target");
                emit!(asm.ja(lbl));
            }
            Some(Opcode::Jb) => {
                let (_, t) = instr.ri16();
                let lbl = *label_map.get(&(t as usize)).expect("Jb target");
                emit!(asm.jb(lbl));
            }

            Some(Opcode::Jz) => {
                let (reg, target) = instr.ri16();
                let lbl = *label_map.get(&(target as usize)).expect("Jz target");
                emit!(asm.cmp(slot(reg), 0i32));
                emit!(asm.je(lbl));
            }

            Some(Opcode::Jnz) => {
                let (reg, target) = instr.ri16();
                let lbl = *label_map.get(&(target as usize)).expect("Jnz target");
                emit!(asm.cmp(slot(reg), 0i32));
                emit!(asm.jne(lbl));
            }

            Some(Opcode::Ret) => {
                emit!(asm.mov(rax, slot(instr.ops[0])));
                emit!(asm.mov(rsp, rbp));
                emit!(asm.pop(rbp));
                emit!(asm.ret());
            }

            Some(Opcode::Lea) => {
                let (dst, base, offset) = instr.mem();
                // Load address of stack slot (base reg), then add field offset.
                emit!(asm.lea(rax, slot(base)));
                if offset != 0 {
                    emit!(asm.add(rax, offset as i32));
                }
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::Load) => {
                let (dst, base, offset) = instr.mem();
                emit!(asm.mov(rax, slot(base)));
                if offset != 0 {
                    emit!(asm.add(rax, offset as i32));
                }
                if instr.flags & crate::bytecode::instruction::FLOAT_FLAG != 0
                    && instr.mem_width() == MemWidth::Dword
                {
                    emit!(asm.movss(xmm0, dword_ptr(rax)));
                    emit!(asm.cvtss2sd(xmm0, xmm0));
                    emit!(asm.movsd_2(slot(dst), xmm0));
                    return Ok(());
                }
                match (instr.mem_width(), instr.mem_signed()) {
                    (MemWidth::Byte, false) => emit!(asm.movzx(rcx, byte_ptr(rax))),
                    (MemWidth::Byte, true) => emit!(asm.movsx(rcx, byte_ptr(rax))),
                    (MemWidth::Word, false) => emit!(asm.movzx(rcx, word_ptr(rax))),
                    (MemWidth::Word, true) => emit!(asm.movsx(rcx, word_ptr(rax))),
                    (MemWidth::Dword, false) => emit!(asm.mov(ecx, dword_ptr(rax))),
                    (MemWidth::Dword, true) => emit!(asm.movsxd(rcx, dword_ptr(rax))),
                    (MemWidth::Qword, _) => emit!(asm.mov(rcx, qword_ptr(rax))),
                }
                emit!(asm.mov(slot(dst), rcx));
            }

            Some(Opcode::Store) => {
                let (src, base, offset) = instr.mem();
                emit!(asm.mov(rax, slot(base)));
                if offset != 0 {
                    emit!(asm.add(rax, offset as i32));
                }
                if instr.flags & crate::bytecode::instruction::FLOAT_FLAG != 0
                    && instr.mem_width() == MemWidth::Dword
                {
                    emit!(asm.movq(xmm0, slot(src)));
                    emit!(asm.cvtsd2ss(xmm0, xmm0));
                    emit!(asm.movss(dword_ptr(rax), xmm0));
                    return Ok(());
                }
                emit!(asm.mov(rcx, slot(src)));
                match instr.mem_width() {
                    MemWidth::Byte => emit!(asm.mov(byte_ptr(rax), cl)),
                    MemWidth::Word => emit!(asm.mov(word_ptr(rax), cx)),
                    MemWidth::Dword => emit!(asm.mov(dword_ptr(rax), ecx)),
                    MemWidth::Qword => emit!(asm.mov(qword_ptr(rax), rcx)),
                }
            }

            Some(Opcode::ArrayStore) => {
                // RRR: ops[0]=val, ops[1]=base_ptr, ops[2]=idx — base[idx*8] = val
                let (val, base, idx) = instr.rrr();
                emit!(asm.mov(rax, slot(base)));
                emit!(asm.mov(rcx, slot(idx)));
                emit!(asm.mov(rdx, 8i64));
                emit!(asm.imul_2(rcx, rdx));
                emit!(asm.add(rax, rcx));
                emit!(asm.mov(rcx, slot(val)));
                emit!(asm.mov(qword_ptr(rax), rcx));
            }

            Some(Opcode::ArrayLoad) => {
                // RRR: ops[0]=dst, ops[1]=base_ptr, ops[2]=idx — dst = base[idx*8]
                let (dst, base, idx) = instr.rrr();
                emit!(asm.mov(rax, slot(base)));
                emit!(asm.mov(rcx, slot(idx)));
                emit!(asm.mov(rdx, 8i64));
                emit!(asm.imul_2(rcx, rdx));
                emit!(asm.add(rax, rcx));
                emit!(asm.mov(rax, qword_ptr(rax)));
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::Syscall) => {
                let (dst, idx) = instr.ri16();
                if is_win64 {
                    return Err(BackendError(
                        "@syscall is unsupported by the Win64 backend".to_string(),
                    ));
                } else {
                    let syscall_num = match chunk.constants.get(idx as usize) {
                        Some(ConstPoolEntry::Int(n)) if *n >= 0 => *n as u64,
                        Some(ConstPoolEntry::Str(s)) => resolve_x86_64_syscall(s).ok_or_else(|| {
                            BackendError(format!("unknown x86-64 Linux syscall `{s}`"))
                        })?,
                        _ => {
                            return Err(BackendError(
                                "syscall instruction is missing valid number/name metadata"
                                    .to_string(),
                            ));
                        }
                    };
                    let arg_count = instr.flags as usize;
                    for (i, &reg) in SYSCALL_REGS.iter().enumerate().take(arg_count) {
                        emit!(asm.mov(reg, slot(dst + i as u8)));
                    }
                    emit!(asm.mov(rax, syscall_num as i64));
                    emit!(asm.syscall());
                    emit!(asm.mov(slot(dst), rax));
                }
            }

            Some(Opcode::Intrinsic) => {
                let (dst, id) = instr.ri16();
                let arg_count = instr.flags as usize;
                match id {
                    0 => {
                        // quazi.write(fd, buf, len) → isize
                        if is_win64 {
                            // GetStdHandle: fd 0→STD_INPUT(0xFFFFFFF6), 1→STD_OUTPUT(0xFFFFFFF5), 2→STD_ERROR(0xFFFFFFF4)
                            // Formula: handle_const = -10 - fd  (DWORD, lower 32 bits = Win32 constant)
                            // push -10; pop eax = 3 bytes vs mov rax,-10 = 10 bytes
                            emit!(asm.push(-10i32));
                            emit!(asm.pop(rax));
                            emit!(asm.sub(eax, dword_ptr(rbp + (-((dst as i32 + 1) * 8)))));
                            emit!(asm.mov(ecx, eax)); // GetStdHandle reads DWORD from ecx
                            call_ext!("GetStdHandle".into(), RelocKind::Plt32);
                            // WriteFile(handle, buf, count, &n_written, null)
                            emit!(asm.mov(rcx, rax));
                            if arg_count > 1 {
                                emit!(asm.mov(rdx, slot(dst + 1)));
                            }
                            if arg_count > 2 {
                                emit!(asm.mov(r8, slot(dst + 2)));
                            }
                            // r9 = &n_written (at [rsp], in our shadow area); 5th arg (overlapped) = null at [rsp+32]
                            emit!(asm.lea(r9, qword_ptr(rsp)));
                            emit!(asm.mov(qword_ptr(rsp + 32i32), 0i32));
                            call_ext!("WriteFile".into(), RelocKind::Plt32);
                            // WriteFile returns BOOL in rax; actual bytes written are at [rsp] (via r9 ptr)
                            emit!(asm.mov(eax, dword_ptr(rsp)));
                        } else {
                            emit!(asm.mov(rdi, slot(dst)));
                            if arg_count > 1 {
                                emit!(asm.mov(rsi, slot(dst + 1)));
                            }
                            if arg_count > 2 {
                                emit!(asm.mov(rdx, slot(dst + 2)));
                            }
                            emit!(asm.mov(rax, 1i64));
                            emit!(asm.syscall());
                        }
                        emit!(asm.mov(slot(dst), rax));
                    }
                    1 => {
                        // quazi.read(fd, buf, len) → isize
                        if is_win64 {
                            // GetStdHandle for the fd, then ReadFile
                            emit!(asm.push(-10i32));
                            emit!(asm.pop(rax));
                            emit!(asm.sub(eax, dword_ptr(rbp + (-((dst as i32 + 1) * 8)))));
                            emit!(asm.mov(ecx, eax));
                            call_ext!("GetStdHandle".into(), RelocKind::Plt32);
                            // ReadFile(handle, buf, count, &n_read, null)
                            emit!(asm.mov(rcx, rax));
                            if arg_count > 1 {
                                emit!(asm.mov(rdx, slot(dst + 1)));
                            }
                            if arg_count > 2 {
                                emit!(asm.mov(r8, slot(dst + 2)));
                            }
                            // r9 = &n_read (at [rsp], in our shadow area); 5th arg (overlapped) = null at [rsp+32]
                            emit!(asm.lea(r9, qword_ptr(rsp)));
                            emit!(asm.mov(qword_ptr(rsp + 32i32), 0i32));
                            call_ext!("ReadFile".into(), RelocKind::Plt32);
                            // ReadFile returns BOOL in rax; actual bytes read are at [rsp] (via r9 ptr)
                            emit!(asm.mov(eax, dword_ptr(rsp)));
                        } else {
                            emit!(asm.mov(rdi, slot(dst)));
                            if arg_count > 1 {
                                emit!(asm.mov(rsi, slot(dst + 1)));
                            }
                            if arg_count > 2 {
                                emit!(asm.mov(rdx, slot(dst + 2)));
                            }
                            emit!(asm.mov(rax, 0i64));
                            emit!(asm.syscall());
                        }
                        emit!(asm.mov(slot(dst), rax));
                    }
                    2 => {
                        // quazi.exit(code) → !
                        if is_win64 {
                            if arg_count > 0 {
                                emit!(asm.mov(rcx, slot(dst)));
                            }
                            call_ext!("ExitProcess".into(), RelocKind::Plt32);
                        } else {
                            if arg_count > 0 {
                                emit!(asm.mov(rdi, slot(dst)));
                            }
                            emit!(asm.mov(rax, 60i64));
                            emit!(asm.syscall());
                        }
                        emit!(asm.xor(rax, rax));
                        emit!(asm.mov(slot(dst), rax));
                    }
                    3 => {
                        // quazi.malloc(size) → ptr
                        if is_win64 {
                            emit!(asm.mov(rcx, slot(dst)));
                        } else {
                            emit!(asm.mov(rdi, slot(dst)));
                        }
                        call_ext!("malloc".into(), RelocKind::Plt32);
                        emit!(asm.mov(slot(dst), rax));
                    }
                    4 => {
                        // quazi.free(ptr) → void
                        if is_win64 {
                            emit!(asm.mov(rcx, slot(dst)));
                        } else {
                            emit!(asm.mov(rdi, slot(dst)));
                        }
                        call_ext!("free".into(), RelocKind::Plt32);
                        emit!(asm.xor(rax, rax));
                        emit!(asm.mov(slot(dst), rax));
                    }
                    5 => {
                        // quazi.realloc(ptr, size) → usize
                        if is_win64 {
                            emit!(asm.mov(rcx, slot(dst)));
                            emit!(asm.mov(rdx, slot(dst + 1)));
                        } else {
                            emit!(asm.mov(rdi, slot(dst)));
                            emit!(asm.mov(rsi, slot(dst + 1)));
                        }
                        call_ext!("realloc".into(), RelocKind::Plt32);
                        emit!(asm.mov(slot(dst), rax));
                    }
                    6 => {
                        // quazi.memcpy(dst_ptr, src, n) → usize
                        if is_win64 {
                            emit!(asm.mov(rcx, slot(dst)));
                            emit!(asm.mov(rdx, slot(dst + 1)));
                            emit!(asm.mov(r8, slot(dst + 2)));
                        } else {
                            emit!(asm.mov(rdi, slot(dst)));
                            emit!(asm.mov(rsi, slot(dst + 1)));
                            emit!(asm.mov(rdx, slot(dst + 2)));
                        }
                        call_ext!("memcpy".into(), RelocKind::Plt32);
                        emit!(asm.mov(slot(dst), rax));
                    }
                    7 => {
                        // quazi.memset(ptr, val, n) → usize
                        if is_win64 {
                            emit!(asm.mov(rcx, slot(dst)));
                            emit!(asm.mov(rdx, slot(dst + 1)));
                            emit!(asm.mov(r8, slot(dst + 2)));
                        } else {
                            emit!(asm.mov(rdi, slot(dst)));
                            emit!(asm.mov(rsi, slot(dst + 1)));
                            emit!(asm.mov(rdx, slot(dst + 2)));
                        }
                        call_ext!("memset".into(), RelocKind::Plt32);
                        emit!(asm.mov(slot(dst), rax));
                    }
                    8 => {
                        // quazi.memmove(dst_ptr, src, n) → usize
                        if is_win64 {
                            emit!(asm.mov(rcx, slot(dst)));
                            emit!(asm.mov(rdx, slot(dst + 1)));
                            emit!(asm.mov(r8, slot(dst + 2)));
                        } else {
                            emit!(asm.mov(rdi, slot(dst)));
                            emit!(asm.mov(rsi, slot(dst + 1)));
                            emit!(asm.mov(rdx, slot(dst + 2)));
                        }
                        call_ext!("memmove".into(), RelocKind::Plt32);
                        emit!(asm.mov(slot(dst), rax));
                    }
                    9 => {
                        // quazi.memcmp(a, b, n) → i32
                        if is_win64 {
                            emit!(asm.mov(rcx, slot(dst)));
                            emit!(asm.mov(rdx, slot(dst + 1)));
                            emit!(asm.mov(r8, slot(dst + 2)));
                        } else {
                            emit!(asm.mov(rdi, slot(dst)));
                            emit!(asm.mov(rsi, slot(dst + 1)));
                            emit!(asm.mov(rdx, slot(dst + 2)));
                        }
                        call_ext!("memcmp".into(), RelocKind::Plt32);
                        emit!(asm.mov(slot(dst), rax));
                    }
                    10 => {
                        // quazi.strlen(s) -> usize. Inline it so Linux stays libc-free.
                        let mut loop_lbl = asm.create_label();
                        let mut done_lbl = asm.create_label();
                        emit!(asm.mov(rax, slot(dst)));
                        emit!(asm.xor(rcx, rcx));
                        emit!(asm.set_label(&mut loop_lbl));
                        emit!(asm.movzx(rdx, byte_ptr(rax)));
                        emit!(asm.test(rdx, rdx));
                        emit!(asm.je(done_lbl));
                        emit!(asm.inc(rax));
                        emit!(asm.inc(rcx));
                        emit!(asm.jmp(loop_lbl));
                        emit!(asm.set_label(&mut done_lbl));
                        emit!(asm.mov(slot(dst), rcx));
                    }
                    11 => {
                        // quazi.stderr_write(buf, len) → isize
                        if is_win64 {
                            // GetStdHandle(STD_ERROR_HANDLE = -12 = 0xFFFFFFF4)
                            emit!(asm.push(-12i32));
                            emit!(asm.pop(rcx)); // GetStdHandle reads DWORD from ecx
                            call_ext!("GetStdHandle".into(), RelocKind::Plt32);
                            // WriteFile(handle, buf, count, &n_written, null)
                            emit!(asm.mov(rcx, rax));
                            emit!(asm.mov(rdx, slot(dst)));
                            if arg_count > 1 {
                                emit!(asm.mov(r8, slot(dst + 1)));
                            }
                            emit!(asm.lea(r9, qword_ptr(rsp)));
                            emit!(asm.mov(qword_ptr(rsp + 32i32), 0i32));
                            call_ext!("WriteFile".into(), RelocKind::Plt32);
                            emit!(asm.mov(slot(dst), rax));
                        } else {
                            emit!(asm.mov(rdi, 2i64));
                            emit!(asm.mov(rsi, slot(dst)));
                            emit!(asm.mov(rdx, slot(dst + 1)));
                            emit!(asm.mov(rax, 1i64));
                            emit!(asm.syscall());
                            emit!(asm.mov(slot(dst), rax));
                        }
                    }
                    12 => {
                        // quazi.sleep_ms(ms) → void
                        if is_win64 {
                            emit!(asm.mov(rcx, slot(dst)));
                            call_ext!("Sleep".into(), RelocKind::Plt32);
                        } else {
                            // usleep takes microseconds; multiply ms by 1000
                            emit!(asm.mov(rax, slot(dst)));
                            emit!(asm.mov(rcx, 1000i64));
                            emit!(asm.imul_2(rax, rcx));
                            emit!(asm.mov(rdi, rax));
                            call_ext!("usleep".into(), RelocKind::Plt32);
                        }
                        emit!(asm.xor(rax, rax));
                        emit!(asm.mov(slot(dst), rax));
                    }
                    13 => {
                        // quazi.getenv(name) → usize (char*)
                        if is_win64 {
                            emit!(asm.mov(rcx, slot(dst)));
                        } else {
                            emit!(asm.mov(rdi, slot(dst)));
                        }
                        call_ext!("getenv".into(), RelocKind::Plt32);
                        emit!(asm.mov(slot(dst), rax));
                    }
                    14 => {
                        // quazi.str_concat(s1, s2) → str (heap-allocated, null-terminated)
                        // slot(dst)=s1, slot(dst+1)=s2
                        // malloc(strlen(s1)+strlen(s2)+1), strcpy(buf,s1), strcat(buf,s2)
                        // Uses callee-saved rbx, r12, r13 to survive calls.
                        emit!(asm.push(rbx));
                        emit!(asm.push(r12));
                        emit!(asm.push(r13));
                        if is_win64 {
                            // 32-byte home space plus 8 bytes to restore 16-byte alignment.
                            emit!(asm.sub(rsp, 40i32));
                        } else {
                            emit!(asm.sub(rsp, 8i32));
                        }
                        emit!(asm.mov(r12, slot(dst))); // r12 = s1
                        emit!(asm.mov(r13, slot(dst + 1))); // r13 = s2
                        let mut len1_loop = asm.create_label();
                        let mut len1_done = asm.create_label();
                        emit!(asm.mov(rax, r12));
                        emit!(asm.xor(rbx, rbx));
                        emit!(asm.set_label(&mut len1_loop));
                        emit!(asm.movzx(rcx, byte_ptr(rax)));
                        emit!(asm.test(rcx, rcx));
                        emit!(asm.je(len1_done));
                        emit!(asm.inc(rax));
                        emit!(asm.inc(rbx));
                        emit!(asm.jmp(len1_loop));
                        emit!(asm.set_label(&mut len1_done));

                        let mut len2_loop = asm.create_label();
                        let mut len2_done = asm.create_label();
                        emit!(asm.mov(rax, r13));
                        emit!(asm.xor(rcx, rcx));
                        emit!(asm.set_label(&mut len2_loop));
                        emit!(asm.movzx(rdx, byte_ptr(rax)));
                        emit!(asm.test(rdx, rdx));
                        emit!(asm.je(len2_done));
                        emit!(asm.inc(rax));
                        emit!(asm.inc(rcx));
                        emit!(asm.jmp(len2_loop));
                        emit!(asm.set_label(&mut len2_done));
                        emit!(asm.mov(rax, rcx));
                        emit!(asm.add(rax, rbx));
                        emit!(asm.inc(rax));
                        if is_win64 {
                            emit!(asm.mov(rcx, rax));
                        } else {
                            emit!(asm.mov(rdi, rax));
                        }
                        call_ext!("malloc".into(), RelocKind::Plt32); // rax = buf
                        emit!(asm.mov(rbx, rax)); // rbx = buf
                        if is_win64 {
                            emit!(asm.mov(rcx, rbx));
                            emit!(asm.mov(rdx, r12));
                        } else {
                            emit!(asm.mov(rdi, rbx));
                            emit!(asm.mov(rsi, r12));
                        }
                        call_ext!("strcpy".into(), RelocKind::Plt32);
                        if is_win64 {
                            emit!(asm.mov(rcx, rbx));
                            emit!(asm.mov(rdx, r13));
                        } else {
                            emit!(asm.mov(rdi, rbx));
                            emit!(asm.mov(rsi, r13));
                        }
                        call_ext!("strcat".into(), RelocKind::Plt32);
                        emit!(asm.mov(slot(dst), rbx));
                        if is_win64 {
                            emit!(asm.add(rsp, 40i32));
                        } else {
                            emit!(asm.add(rsp, 8i32));
                        }
                        emit!(asm.pop(r13));
                        emit!(asm.pop(r12));
                        emit!(asm.pop(rbx));
                    }
                    15 => {
                        // quazi.int_to_str(n: i64) → str (heap-allocated, null-terminated)
                        // slot(dst) = n; malloc(32), sprintf(buf, "%ld", n), return buf
                        // Push rbx + rax (2*8=16 bytes) to keep rsp 16-byte aligned.
                        emit!(asm.push(rbx));
                        emit!(asm.push(rax));
                        if is_win64 {
                            emit!(asm.sub(rsp, 32i32));
                            emit!(asm.mov(rcx, 32i64));
                        } else {
                            emit!(asm.mov(rdi, 32i64));
                        }
                        call_ext!("malloc".into(), RelocKind::Plt32);
                        emit!(asm.mov(rbx, rax));
                        if is_win64 {
                            emit!(asm.mov(rcx, rbx));
                            lea_rip!(rdx, "__quazi_fmt_ld".into());
                            emit!(asm.mov(r8, slot(dst)));
                        } else {
                            emit!(asm.mov(rdi, rbx));
                            lea_rip!(rsi, "__quazi_fmt_ld".into());
                            emit!(asm.mov(rdx, slot(dst)));
                        }
                        call_ext!("sprintf".into(), RelocKind::Plt32);
                        emit!(asm.mov(slot(dst), rbx));
                        if is_win64 {
                            emit!(asm.add(rsp, 32i32));
                        }
                        emit!(asm.pop(rax));
                        emit!(asm.pop(rbx));
                    }
                    16 => {
                        // quazi.float_to_str(f: f64) → str (heap-allocated, null-terminated)
                        // slot(dst) = f (64-bit IEEE754); malloc(32), sprintf(buf, "%g", f), return buf
                        // Push rbx + rax (2*8=16 bytes) to keep rsp 16-byte aligned.
                        emit!(asm.push(rbx));
                        emit!(asm.push(rax));
                        if is_win64 {
                            emit!(asm.sub(rsp, 32i32));
                            emit!(asm.mov(rcx, 32i64));
                        } else {
                            emit!(asm.mov(rdi, 32i64));
                        }
                        call_ext!("malloc".into(), RelocKind::Plt32);
                        emit!(asm.mov(rbx, rax));
                        emit!(asm.mov(rax, slot(dst)));
                        if is_win64 {
                            emit!(asm.mov(rcx, rbx));
                            lea_rip!(rdx, "__quazi_fmt_g".into());
                            emit!(asm.movq(xmm2, rax));
                            emit!(asm.mov(r8, rax));
                        } else {
                            emit!(asm.mov(rdi, rbx));
                            lea_rip!(rsi, "__quazi_fmt_g".into());
                            emit!(asm.movq(xmm0, rax));
                            emit!(asm.mov(eax, 1i32));
                        }
                        call_ext!("sprintf".into(), RelocKind::Plt32);
                        emit!(asm.mov(slot(dst), rbx));
                        if is_win64 {
                            emit!(asm.add(rsp, 32i32));
                        }
                        emit!(asm.pop(rax));
                        emit!(asm.pop(rbx));
                    }
                    18 => {
                        // quazi.thread.spawn(f: any) → usize (thread handle)
                        // slot(dst) = function pointer (address)
                        emit!(asm.push(rbx));
                        emit!(asm.push(rax)); // align rsp to 16
                        if is_win64 {
                            emit!(asm.sub(rsp, 48i32));
                            // CreateThread(NULL, 0, fn_ptr, NULL, 0, NULL)
                            emit!(asm.xor(rcx, rcx));
                            emit!(asm.xor(rdx, rdx));
                            emit!(asm.mov(r8, slot(dst)));
                            emit!(asm.xor(r9, r9));
                            emit!(asm.mov(qword_ptr(rsp + 32i32), 0i32)); // flags
                            emit!(asm.mov(qword_ptr(rsp + 40i32), 0i32)); // thread_id
                            call_ext!("CreateThread".into(), RelocKind::Plt32);
                            emit!(asm.add(rsp, 48i32));
                        } else {
                            // malloc(8) for pthread_t storage
                            emit!(asm.mov(rdi, 8i64));
                            call_ext!("malloc".into(), RelocKind::Plt32);
                            emit!(asm.mov(rbx, rax)); // rbx = thread_storage_ptr
                            // pthread_create(storage, NULL, fn_ptr, NULL)
                            emit!(asm.mov(rdi, rbx));
                            emit!(asm.xor(rsi, rsi));
                            emit!(asm.mov(rdx, slot(dst)));
                            emit!(asm.xor(rcx, rcx));
                            call_ext!("pthread_create".into(), RelocKind::Plt32);
                            emit!(asm.mov(rax, rbx));
                        }
                        emit!(asm.mov(slot(dst), rax));
                        emit!(asm.pop(rax));
                        emit!(asm.pop(rbx));
                    }
                    19 => {
                        // quazi.thread.join(handle: usize) → void
                        // slot(dst) = thread handle
                        emit!(asm.push(rbx));
                        emit!(asm.push(rax)); // align
                        if is_win64 {
                            emit!(asm.sub(rsp, 32i32));
                            emit!(asm.mov(rcx, slot(dst)));
                            emit!(asm.mov(edx, u32::MAX as i32)); // INFINITE
                            call_ext!("WaitForSingleObject".into(), RelocKind::Plt32);
                            emit!(asm.mov(rcx, slot(dst)));
                            call_ext!("CloseHandle".into(), RelocKind::Plt32);
                            emit!(asm.add(rsp, 32i32));
                        } else {
                            // pthread_join(*(pthread_t*)handle, NULL)
                            emit!(asm.mov(rbx, slot(dst))); // rbx = thread_storage_ptr
                            emit!(asm.mov(rdi, qword_ptr(rbx))); // *rbx = actual pthread_t
                            emit!(asm.xor(rsi, rsi));
                            call_ext!("pthread_join".into(), RelocKind::Plt32);
                            emit!(asm.mov(rdi, rbx));
                            call_ext!("free".into(), RelocKind::Plt32);
                        }
                        emit!(asm.xor(rax, rax));
                        emit!(asm.mov(slot(dst), rax));
                        emit!(asm.pop(rax));
                        emit!(asm.pop(rbx));
                    }
                    20 => {
                        // quazi.net.bind_tcp(sockfd: i32, port: i32) → i32
                        // Builds sockaddr_in on stack — quazi has no byte-level memory writes
                        emit!(asm.push(rbx));
                        emit!(asm.push(rax)); // keep rsp 16-byte aligned
                        if is_win64 {
                            emit!(asm.sub(rsp, 48i32));
                        } else {
                            emit!(asm.sub(rsp, 32i32));
                        }
                        emit!(asm.xor(rax, rax));
                        if is_win64 {
                            emit!(asm.mov(qword_ptr(rsp + 32i32), rax));
                            emit!(asm.mov(qword_ptr(rsp + 40i32), rax));
                            emit!(asm.mov(word_ptr(rsp + 32i32), 2i32));
                        } else {
                            emit!(asm.mov(qword_ptr(rsp), rax));
                            emit!(asm.mov(qword_ptr(rsp + 8i32), rax));
                            emit!(asm.mov(word_ptr(rsp), 2i32));
                        }
                        // port: host→big-endian byte swap
                        emit!(asm.mov(rax, slot(dst + 1)));
                        emit!(asm.rol(ax, 8u32));
                        if is_win64 {
                            emit!(asm.mov(word_ptr(rsp + 34i32), ax));
                        } else {
                            emit!(asm.mov(word_ptr(rsp + 2i32), ax));
                        }
                        // sin_addr = INADDR_ANY = 0 (already zeroed)
                        if is_win64 {
                            emit!(asm.mov(rcx, slot(dst))); // sockfd
                            emit!(asm.lea(rdx, qword_ptr(rsp + 32i32))); // &sockaddr_in
                            emit!(asm.mov(r8d, 16i32)); // addrlen
                            call_ext!("bind".into(), RelocKind::Plt32);
                        } else {
                            emit!(asm.mov(rdi, slot(dst))); // sockfd
                            emit!(asm.lea(rsi, qword_ptr(rsp))); // &sockaddr_in
                            emit!(asm.mov(edx, 16i32)); // addrlen
                            emit!(asm.mov(rax, 49i64)); // bind syscall
                            emit!(asm.syscall());
                        }
                        if is_win64 {
                            emit!(asm.add(rsp, 48i32));
                        } else {
                            emit!(asm.add(rsp, 32i32));
                        }
                        emit!(asm.mov(slot(dst), rax));
                        emit!(asm.pop(rax));
                        emit!(asm.pop(rbx));
                    }
                    21 => {
                        // quazi.net.connect_tcp(sockfd: i32, ip: str, port: i32) → i32
                        // Uses inet_pton to parse dotted-decimal IP, builds sockaddr_in
                        emit!(asm.push(rbx));
                        emit!(asm.push(r12));
                        emit!(asm.push(rax));
                        emit!(asm.push(rax)); // 4 pushes = 32 bytes, rsp stays 16-aligned
                        if is_win64 {
                            emit!(asm.sub(rsp, 48i32));
                        } else {
                            emit!(asm.sub(rsp, 32i32));
                        }
                        emit!(asm.xor(rax, rax));
                        if is_win64 {
                            emit!(asm.mov(qword_ptr(rsp + 32i32), rax));
                            emit!(asm.mov(qword_ptr(rsp + 40i32), rax));
                            emit!(asm.mov(word_ptr(rsp + 32i32), 2i32));
                        } else {
                            emit!(asm.mov(qword_ptr(rsp), rax));
                            emit!(asm.mov(qword_ptr(rsp + 8i32), rax));
                            emit!(asm.mov(word_ptr(rsp), 2i32));
                        }
                        emit!(asm.mov(rax, slot(dst + 2)));
                        emit!(asm.rol(ax, 8u32));
                        if is_win64 {
                            emit!(asm.mov(word_ptr(rsp + 34i32), ax));
                        } else {
                            emit!(asm.mov(word_ptr(rsp + 2i32), ax));
                        }
                        // inet_pton(AF_INET, ip_str, &sin_addr) — sin_addr at rsp+4
                        if is_win64 {
                            emit!(asm.lea(r12, qword_ptr(rsp + 36i32)));
                        } else {
                            emit!(asm.lea(r12, qword_ptr(rsp + 4i32)));
                        }
                        if is_win64 {
                            emit!(asm.mov(ecx, 2i32));
                            emit!(asm.mov(rdx, slot(dst + 1)));
                            emit!(asm.mov(r8, r12));
                            call_ext!("inet_pton".into(), RelocKind::Plt32);
                        } else {
                            emit!(asm.mov(edi, 2i32));
                            emit!(asm.mov(rsi, slot(dst + 1)));
                            emit!(asm.mov(rdx, r12));
                            call_ext!("inet_pton".into(), RelocKind::Plt32);
                        }
                        // connect(sockfd, &sockaddr_in, 16)
                        if is_win64 {
                            emit!(asm.mov(rcx, slot(dst)));
                            emit!(asm.lea(rdx, qword_ptr(rsp + 32i32)));
                            emit!(asm.mov(r8d, 16i32));
                            call_ext!("connect".into(), RelocKind::Plt32);
                        } else {
                            emit!(asm.mov(rdi, slot(dst)));
                            emit!(asm.lea(rsi, qword_ptr(rsp)));
                            emit!(asm.mov(edx, 16i32));
                            emit!(asm.mov(rax, 42i64)); // connect syscall
                            emit!(asm.syscall());
                        }
                        if is_win64 {
                            emit!(asm.add(rsp, 48i32));
                        } else {
                            emit!(asm.add(rsp, 32i32));
                        }
                        emit!(asm.mov(slot(dst), rax));
                        emit!(asm.pop(rax));
                        emit!(asm.pop(rax));
                        emit!(asm.pop(r12));
                        emit!(asm.pop(rbx));
                    }
                    23 => {
                        // quazi.str.byte_at(s: str, i: usize) u8
                        // s is a char* pointer (str register = ptr portion).
                        // Load byte at [s + i] with zero-extension.
                        emit!(asm.mov(rax, slot(dst))); // s (ptr)
                        emit!(asm.mov(rcx, slot(dst + 1))); // i
                        emit!(asm.movzx(rax, byte_ptr(rax + rcx)));
                        emit!(asm.mov(slot(dst), rax));
                    }
                    24 => {
                        // quazi.str.from_byte(b: u8) str
                        // Allocates a 2-byte buffer [b, '\0'] on the heap and returns ptr.
                        emit!(asm.push(rbx));
                        emit!(asm.push(rax));
                        if is_win64 {
                            emit!(asm.sub(rsp, 32i32));
                            emit!(asm.mov(rcx, 2i64));
                            call_ext!("malloc".into(), RelocKind::Plt32);
                            emit!(asm.add(rsp, 32i32));
                        } else {
                            emit!(asm.mov(rdi, 2i64));
                            call_ext!("malloc".into(), RelocKind::Plt32);
                        }
                        // rax = allocated ptr; write b at [rax] and '\0' at [rax+1]
                        emit!(asm.mov(rbx, slot(dst))); // b
                        emit!(asm.mov(byte_ptr(rax), bl));
                        emit!(asm.mov(byte_ptr(rax + 1i32), 0i32));
                        emit!(asm.mov(slot(dst), rax));
                        emit!(asm.pop(rax));
                        emit!(asm.pop(rbx));
                    }
                    25 => {
                        // quazi.print_backtrace() → void
                        call_ext!("__quazi_print_backtrace".into(), RelocKind::Plt32);
                        emit!(asm.xor(rax, rax));
                        emit!(asm.mov(slot(dst), rax));
                    }
                    _ => {
                        return Err(BackendError(format!("unknown intrinsic id {id}")));
                    }
                }
            }

            Some(Opcode::CallExt) => {
                let (dst, idx) = instr.ri16();
                let c_variadic = (instr.flags & 0x80) != 0;
                let arg_count = (instr.flags & 0x7F) as usize;
                match chunk.constants.get(idx as usize) {
                    Some(ConstPoolEntry::Str(sym)) => {
                        let sym = sym.clone();
                        if is_win64 {
                            for (i, &reg) in WIN64_REGS.iter().enumerate().take(arg_count.min(4)) {
                                emit!(asm.mov(reg, slot(i as u8)));
                            }
                            if arg_count > 4 {
                                emit!(asm.mov(rax, slot(4)));
                                emit!(asm.mov(qword_ptr(rsp + 32i32), rax));
                            }
                            if arg_count > 5 {
                                emit!(asm.mov(rax, slot(5)));
                                emit!(asm.mov(qword_ptr(rsp + 40i32), rax));
                            }
                        } else {
                            for (i, &reg) in SYSV_REGS.iter().enumerate().take(arg_count) {
                                emit!(asm.mov(reg, slot(i as u8)));
                            }
                            if c_variadic {
                                // SysV ABI requires AL to contain the number of vector registers
                                // used for variadic arguments. We currently don't pass floats
                                // to variadic functions, so we set AL (via RAX) to 0.
                                emit!(asm.xor(rax, rax));
                            }
                        }
                        call_ext!(sym, RelocKind::Plt32);
                        emit!(asm.mov(slot(dst), rax));
                    }
                    _ => {
                        emit!(asm.xor(rax, rax));
                        emit!(asm.mov(slot(dst), rax));
                    }
                }
            }

            Some(Opcode::StrLen) => {
                let (dst, src, _) = instr.rrr();
                // inline strlen: scan bytes until null
                let mut loop_lbl = asm.create_label();
                let mut done_lbl = asm.create_label();
                emit!(asm.mov(rax, slot(src))); // rax = pointer
                emit!(asm.xor(rcx, rcx)); // rcx = length = 0
                emit!(asm.set_label(&mut loop_lbl));
                emit!(asm.movzx(rdx, byte_ptr(rax))); // rdx = *rax
                emit!(asm.test(rdx, rdx)); // null?
                emit!(asm.je(done_lbl));
                emit!(asm.inc(rax));
                emit!(asm.inc(rcx));
                emit!(asm.jmp(loop_lbl));
                emit!(asm.set_label(&mut done_lbl));
                emit!(asm.mov(slot(dst), rcx));
            }

            Some(Opcode::StrToInt) => {
                let (dst, src, _) = instr.rrr();
                if is_win64 {
                    emit!(asm.mov(rcx, slot(src)));
                } else {
                    emit!(asm.mov(rdi, slot(src)));
                }
                call_ext!("atoll".into(), RelocKind::Plt32);
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::StrToFloat) => {
                let (dst, src, _) = instr.rrr();
                if is_win64 {
                    emit!(asm.mov(rcx, slot(src)));
                    emit!(asm.xor(rdx, rdx));
                } else {
                    emit!(asm.mov(rdi, slot(src)));
                    emit!(asm.xor(rsi, rsi));
                }
                call_ext!("strtod".into(), RelocKind::Plt32);
                emit!(asm.movsd_2(slot(dst), xmm0));
            }

            Some(Opcode::PrimToStr) => {
                let (dst, src, type_tag) = instr.rrr();
                let buf_sym = self
                    .bss_syms
                    .get(qzi_idx)
                    .and_then(|s| s.as_ref())
                    .cloned()
                    .unwrap_or_else(|| format!("__quazi_itoa_missing_{}", qzi_idx));

                match type_tag {
                    1 => {
                        // float: load 64-bit bits → XMM, sprintf with "%g"
                        emit!(asm.mov(rax, slot(src)));
                        if is_win64 {
                            lea_rip!(rcx, buf_sym.clone());
                            lea_rip!(rdx, "__quazi_fmt_g".into());
                            emit!(asm.movq(xmm2, rax));
                            emit!(asm.mov(r8, rax));
                        } else {
                            lea_rip!(rdi, buf_sym.clone());
                            lea_rip!(rsi, "__quazi_fmt_g".into());
                            emit!(asm.movq(xmm0, rax));
                            emit!(asm.mov(eax, 1i32));
                        }
                        call_ext!("sprintf".into(), RelocKind::Plt32);
                    }
                    2 => {
                        // bool: emit "true" or "false" directly into static buffer
                        // if (slot(src) != 0) strcpy(buf, "true") else strcpy(buf, "false")
                        let mut lbl_false = asm.create_label();
                        let mut lbl_end = asm.create_label();
                        emit!(asm.mov(rax, slot(src)));
                        emit!(asm.test(rax, rax));
                        emit!(asm.jz(lbl_false));
                        // true branch
                        if is_win64 {
                            lea_rip!(rcx, buf_sym.clone());
                        } else {
                            lea_rip!(rdi, buf_sym.clone());
                        }
                        // Write "true\0" — store dword "true" (LE: 0x65757274) + byte null
                        emit!(asm.mov(rax, 0x65757274u64 as i64));
                        if is_win64 {
                            emit!(asm.mov(dword_ptr(rcx), eax));
                            emit!(asm.mov(byte_ptr(rcx + 4i32), 0i32));
                        } else {
                            emit!(asm.mov(dword_ptr(rdi), eax));
                            emit!(asm.mov(byte_ptr(rdi + 4i32), 0i32));
                        }
                        emit!(asm.jmp(lbl_end));
                        emit!(asm.set_label(&mut lbl_false));
                        // false branch: "false\0" = 6 bytes
                        if is_win64 {
                            lea_rip!(rcx, buf_sym.clone());
                        } else {
                            lea_rip!(rdi, buf_sym.clone());
                        }
                        emit!(asm.mov(rax, 0x736c6166u64 as i64)); // "fals" LE
                        if is_win64 {
                            emit!(asm.mov(dword_ptr(rcx), eax));
                            emit!(asm.mov(word_ptr(rcx + 4i32), 0x65u32 as i32)); // "e\0"
                        } else {
                            emit!(asm.mov(dword_ptr(rdi), eax));
                            emit!(asm.mov(word_ptr(rdi + 4i32), 0x65u32 as i32)); // "e\0"
                        }
                        emit!(asm.set_label(&mut lbl_end));
                    }
                    // Integer hex/octal: sprintf(buf, fmt, val)
                    3..=5 => {
                        let fmt_sym = match type_tag {
                            3 => "__quazi_fmt_llx",
                            4 => "__quazi_fmt_llX",
                            _ => "__quazi_fmt_llo",
                        };
                        if is_win64 {
                            lea_rip!(rcx, buf_sym.clone());
                            lea_rip!(rdx, fmt_sym.into());
                            emit!(asm.mov(r8, slot(src)));
                        } else {
                            lea_rip!(rdi, buf_sym.clone());
                            lea_rip!(rsi, fmt_sym.into());
                            emit!(asm.mov(rdx, slot(src)));
                        }
                        call_ext!("sprintf".into(), RelocKind::Plt32);
                    }
                    // Binary: inline bit-loop using BSR, no libc needed.
                    // Output: strip leading zeros, write '0'/'1' chars, null-terminate.
                    6 => {
                        let mut lbl_nonzero = asm.create_label();
                        let mut lbl_loop = asm.create_label();
                        let mut lbl_last_bit = asm.create_label();
                        let mut lbl_done = asm.create_label();

                        emit!(asm.mov(rax, slot(src)));
                        lea_rip!(r11, buf_sym.clone());

                        // Zero shortcut: write "0\0"
                        emit!(asm.test(rax, rax));
                        emit!(asm.jnz(lbl_nonzero));
                        emit!(asm.mov(byte_ptr(r11), 48i32)); // '0'
                        emit!(asm.mov(byte_ptr(r11 + 1i32), 0i32)); // '\0'
                        emit!(asm.jmp(lbl_done));

                        emit!(asm.set_label(&mut lbl_nonzero));
                        emit!(asm.bsr(rcx, rax)); // rcx = index of highest set bit
                        emit!(asm.mov(r10, r11)); // r10 = write pointer

                        emit!(asm.set_label(&mut lbl_loop));
                        emit!(asm.mov(r9, rax));
                        emit!(asm.shr(r9, cl)); // r9 = rax >> rcx
                        emit!(asm.and(r9, 1i32));
                        emit!(asm.add(r9, 48i32)); // '0' or '1'
                        emit!(asm.mov(byte_ptr(r10), r9b));
                        emit!(asm.inc(r10));
                        // If rcx == 0, this was the last (least significant) bit
                        emit!(asm.test(rcx, rcx));
                        emit!(asm.jz(lbl_last_bit));
                        emit!(asm.dec(rcx));
                        emit!(asm.jmp(lbl_loop));

                        emit!(asm.set_label(&mut lbl_last_bit));
                        emit!(asm.mov(byte_ptr(r10), 0i32)); // null terminator

                        emit!(asm.set_label(&mut lbl_done));
                    }
                    // Float with precision: sprintf(buf, "%.Nf", val) where N = tag - 20
                    t @ 20..=29 => {
                        let prec = t - 20;
                        let fmt_sym = format!("__quazi_fmt_prec_{}", prec);
                        emit!(asm.mov(rax, slot(src)));
                        if is_win64 {
                            lea_rip!(rcx, buf_sym.clone());
                            lea_rip!(rdx, fmt_sym);
                            emit!(asm.movq(xmm2, rax));
                            emit!(asm.mov(r8, rax));
                        } else {
                            lea_rip!(rdi, buf_sym.clone());
                            lea_rip!(rsi, fmt_sym);
                            emit!(asm.movq(xmm0, rax));
                            emit!(asm.mov(eax, 1i32));
                        }
                        call_ext!("sprintf".into(), RelocKind::Plt32);
                    }
                    _ => {
                        // int (type_tag=0 or any other): existing "%ld" path
                        if is_win64 {
                            lea_rip!(rcx, buf_sym.clone());
                            lea_rip!(rdx, "__quazi_fmt_ld".into());
                            emit!(asm.mov(r8, slot(src)));
                        } else {
                            lea_rip!(rdi, buf_sym.clone());
                            lea_rip!(rsi, "__quazi_fmt_ld".into());
                            emit!(asm.mov(rdx, slot(src)));
                        }
                        call_ext!("sprintf".into(), RelocKind::Plt32);
                    }
                }
                lea_rip!(rax, buf_sym);
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::StrAsStr) => {
                let (dst, src, _) = instr.rrr();
                emit!(asm.mov(rax, slot(src)));
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::MemFence) => {
                emit!(asm.mfence());
            }

            Some(Opcode::FieldLoad) => {
                let (dst, obj, byte_off) = instr.field();
                emit!(asm.mov(rax, slot(obj)));
                if instr.flags & crate::bytecode::instruction::FLOAT_FLAG != 0
                    && instr.mem_width() == MemWidth::Dword
                {
                    emit!(asm.movss(xmm0, dword_ptr(rax + byte_off as i64)));
                    emit!(asm.cvtss2sd(xmm0, xmm0));
                    emit!(asm.movsd_2(slot(dst), xmm0));
                    return Ok(());
                }
                match instr.mem_width() {
                    MemWidth::Byte if instr.mem_signed() => {
                        emit!(asm.movsx(rcx, byte_ptr(rax + byte_off as i64)))
                    }
                    MemWidth::Byte => emit!(asm.movzx(rcx, byte_ptr(rax + byte_off as i64))),
                    MemWidth::Word if instr.mem_signed() => {
                        emit!(asm.movsx(rcx, word_ptr(rax + byte_off as i64)))
                    }
                    MemWidth::Word => emit!(asm.movzx(rcx, word_ptr(rax + byte_off as i64))),
                    MemWidth::Dword if instr.mem_signed() => {
                        emit!(asm.movsxd(rcx, dword_ptr(rax + byte_off as i64)))
                    }
                    MemWidth::Dword => emit!(asm.mov(ecx, dword_ptr(rax + byte_off as i64))),
                    MemWidth::Qword => emit!(asm.mov(rcx, qword_ptr(rax + byte_off as i64))),
                }
                emit!(asm.mov(slot(dst), rcx));
            }

            Some(Opcode::FieldStore) => {
                let (val, obj, byte_off) = instr.field();
                emit!(asm.mov(rax, slot(obj)));
                if instr.flags & crate::bytecode::instruction::FLOAT_FLAG != 0
                    && instr.mem_width() == MemWidth::Dword
                {
                    emit!(asm.movq(xmm0, slot(val)));
                    emit!(asm.cvtsd2ss(xmm0, xmm0));
                    emit!(asm.movss(dword_ptr(rax + byte_off as i64), xmm0));
                    return Ok(());
                }
                emit!(asm.mov(rcx, slot(val)));
                match instr.mem_width() {
                    MemWidth::Byte => emit!(asm.mov(byte_ptr(rax + byte_off as i64), cl)),
                    MemWidth::Word => emit!(asm.mov(word_ptr(rax + byte_off as i64), cx)),
                    MemWidth::Dword => emit!(asm.mov(dword_ptr(rax + byte_off as i64), ecx)),
                    MemWidth::Qword => emit!(asm.mov(qword_ptr(rax + byte_off as i64), rcx)),
                }
            }

            Some(Opcode::New) => {
                let (dst, size_bytes) = instr.ri16();
                // calloc(1, size_bytes) — zero-initializes the allocation
                if is_win64 {
                    emit!(asm.mov(rcx, 1i64));
                    emit!(asm.mov(rdx, size_bytes as i64));
                } else {
                    emit!(asm.mov(rdi, 1i64));
                    emit!(asm.mov(rsi, size_bytes as i64));
                }
                call_ext!("calloc".into(), RelocKind::Plt32);
                emit!(asm.mov(slot(dst), rax));
            }

            Some(Opcode::VtblLoad) => {
                // Load fn ptr from vtable. Operand is the vtable ptr itself (not the fat ptr).
                // vtable[slot * 8] = fn ptr.
                let (dst, vtbl_ptr, method_slot) = instr.rrr();
                emit!(asm.mov(rax, slot(vtbl_ptr)));
                emit!(asm.mov(rax, qword_ptr(rax + (method_slot as i64) * 8)));
                emit!(asm.mov(slot(dst), rax));
            }

            _ => {
                return Err(BackendError(format!(
                    "encoder does not implement opcode {:?}",
                    Opcode::from_u8(instr.opcode)
                )));
            }
        }

        Ok(())
    }
}

fn resolve_x86_64_syscall(name: &str) -> Option<u64> {
    Some(match name {
        // File I/O
        "read" => 0,
        "write" => 1,
        "open" => 2,
        "close" => 3,
        "stat" => 4,
        "fstat" => 5,
        "lstat" => 6,
        "poll" => 7,
        "lseek" => 8,
        "mmap" => 9,
        "mprotect" => 10,
        "munmap" => 11,
        "brk" => 12,
        "rt_sigaction" => 13,
        "rt_sigprocmask" => 14,
        "rt_sigreturn" => 15,
        "ioctl" => 16,
        "pread64" => 17,
        "pwrite64" => 18,
        "readv" => 19,
        "writev" => 20,
        "access" => 21,
        "pipe" => 22,
        "select" => 23,
        "sched_yield" => 24,
        "mremap" => 25,
        "msync" => 26,
        "mincore" => 27,
        "madvise" => 28,
        "shmget" => 29,
        "shmat" => 30,
        "shmctl" => 31,
        "dup" => 32,
        "dup2" => 33,
        "pause" => 34,
        "nanosleep" => 35,
        "getitimer" => 36,
        "alarm" => 37,
        "setitimer" => 38,
        "getpid" => 39,
        "sendfile" => 40,
        "socket" => 41,
        "connect" => 42,
        "accept" => 43,
        "sendto" => 44,
        "recvfrom" => 45,
        "sendmsg" => 46,
        "recvmsg" => 47,
        "shutdown" => 48,
        "bind" => 49,
        "listen" => 50,
        "getsockname" => 51,
        "getpeername" => 52,
        "socketpair" => 53,
        "setsockopt" => 54,
        "getsockopt" => 55,
        "clone" => 56,
        "fork" => 57,
        "vfork" => 58,
        "execve" => 59,
        "exit" => 60,
        "wait4" => 61,
        "kill" => 62,
        "uname" => 63,
        "semget" => 64,
        "semop" => 65,
        "semctl" => 66,
        "shmdt" => 67,
        "msgget" => 68,
        "msgsnd" => 69,
        "msgrcv" => 70,
        "msgctl" => 71,
        "fcntl" => 72,
        "flock" => 73,
        "fsync" => 74,
        "fdatasync" => 75,
        "truncate" => 76,
        "ftruncate" => 77,
        "getdents" => 78,
        "getcwd" => 79,
        "chdir" => 80,
        "fchdir" => 81,
        "rename" => 82,
        "mkdir" => 83,
        "rmdir" => 84,
        "creat" => 85,
        "link" => 86,
        "unlink" => 87,
        "symlink" => 88,
        "readlink" => 89,
        "chmod" => 90,
        "fchmod" => 91,
        "chown" => 92,
        "fchown" => 93,
        "lchown" => 94,
        "umask" => 95,
        "gettimeofday" => 96,
        "getrlimit" => 97,
        "getrusage" => 98,
        "sysinfo" => 99,
        "times" => 100,
        "ptrace" => 101,
        "getuid" => 102,
        "syslog" => 103,
        "getgid" => 104,
        "setuid" => 105,
        "setgid" => 106,
        "geteuid" => 107,
        "getegid" => 108,
        "setpgid" => 109,
        "getppid" => 110,
        "getpgrp" => 111,
        "setsid" => 112,
        "setreuid" => 113,
        "setregid" => 114,
        "getgroups" => 115,
        "setgroups" => 116,
        "setresuid" => 117,
        "getresuid" => 118,
        "setresgid" => 119,
        "getresgid" => 120,
        "getpgid" => 121,
        "setfsuid" => 122,
        "setfsgid" => 123,
        "getsid" => 124,
        "capget" => 125,
        "capset" => 126,
        "rt_sigpending" => 127,
        "rt_sigtimedwait" => 128,
        "rt_sigqueueinfo" => 129,
        "rt_sigsuspend" => 130,
        "sigaltstack" => 131,
        "utime" => 132,
        "mknod" => 133,
        "statfs" => 137,
        "fstatfs" => 138,
        "getpriority" => 140,
        "setpriority" => 141,
        "sched_setparam" => 142,
        "sched_getparam" => 143,
        "sched_setscheduler" => 144,
        "sched_getscheduler" => 145,
        "sched_get_priority_max" => 146,
        "sched_get_priority_min" => 147,
        "sched_rr_get_interval" => 148,
        "mlock" => 149,
        "munlock" => 150,
        "mlockall" => 151,
        "munlockall" => 152,
        "vhangup" => 153,
        "modify_ldt" => 154,
        "pivot_root" => 155,
        "prctl" => 157,
        "arch_prctl" => 158,
        "adjtimex" => 159,
        "setrlimit" => 160,
        "chroot" => 161,
        "sync" => 162,
        "acct" => 163,
        "settimeofday" => 164,
        "mount" => 165,
        "umount2" => 166,
        "swapon" => 167,
        "swapoff" => 168,
        "reboot" => 169,
        "sethostname" => 170,
        "setdomainname" => 171,
        "iopl" => 172,
        "ioperm" => 173,
        "gettid" => 186,
        "readahead" => 187,
        "setxattr" => 188,
        "lsetxattr" => 189,
        "fsetxattr" => 190,
        "getxattr" => 191,
        "lgetxattr" => 192,
        "fgetxattr" => 193,
        "listxattr" => 194,
        "llistxattr" => 195,
        "flistxattr" => 196,
        "removexattr" => 197,
        "lremovexattr" => 198,
        "fremovexattr" => 199,
        "tkill" => 200,
        "time" => 201,
        "futex" => 202,
        "sched_setaffinity" => 203,
        "sched_getaffinity" => 204,
        "io_setup" => 206,
        "io_destroy" => 207,
        "io_getevents" => 208,
        "io_submit" => 209,
        "io_cancel" => 210,
        "epoll_create" => 213,
        "getdents64" => 217,
        "set_tid_address" => 218,
        "fadvise64" => 221,
        "timer_create" => 222,
        "timer_settime" => 223,
        "timer_gettime" => 224,
        "timer_getoverrun" => 225,
        "timer_delete" => 226,
        "clock_settime" => 227,
        "clock_gettime" => 228,
        "clock_getres" => 229,
        "clock_nanosleep" => 230,
        "exit_group" => 231,
        "epoll_wait" => 232,
        "epoll_ctl" => 233,
        "tgkill" => 234,
        "utimes" => 235,
        "mq_open" => 240,
        "mq_unlink" => 241,
        "mq_timedsend" => 242,
        "mq_timedreceive" => 243,
        "mq_notify" => 244,
        "mq_getsetattr" => 245,
        "waitid" => 247,
        "inotify_init" => 253,
        "inotify_add_watch" => 254,
        "inotify_rm_watch" => 255,
        "openat" => 257,
        "mkdirat" => 258,
        "mknodat" => 259,
        "fchownat" => 260,
        "futimesat" => 261,
        "newfstatat" => 262,
        "unlinkat" => 263,
        "renameat" => 264,
        "linkat" => 265,
        "symlinkat" => 266,
        "readlinkat" => 267,
        "fchmodat" => 268,
        "faccessat" => 269,
        "pselect6" => 270,
        "ppoll" => 271,
        "unshare" => 272,
        "splice" => 275,
        "tee" => 276,
        "sync_file_range" => 277,
        "vmsplice" => 278,
        "move_pages" => 279,
        "utimensat" => 280,
        "epoll_pwait" => 281,
        "signalfd" => 282,
        "timerfd_create" => 283,
        "eventfd" => 284,
        "fallocate" => 285,
        "timerfd_settime" => 286,
        "timerfd_gettime" => 287,
        "accept4" => 288,
        "signalfd4" => 289,
        "eventfd2" => 290,
        "epoll_create1" => 291,
        "dup3" => 292,
        "pipe2" => 293,
        "inotify_init1" => 294,
        "preadv" => 295,
        "pwritev" => 296,
        "prlimit64" => 302,
        "fanotify_init" => 303,
        "fanotify_mark" => 304,
        "syncfs" => 306,
        "sendmmsg" => 307,
        "setns" => 308,
        "getcpu" => 309,
        "process_vm_readv" => 310,
        "process_vm_writev" => 311,
        "seccomp" => 317,
        "getrandom" => 318,
        "memfd_create" => 319,
        "bpf" => 321,
        "execveat" => 322,
        "membarrier" => 324,
        "mlock2" => 325,
        "copy_file_range" => 326,
        "preadv2" => 327,
        "pwritev2" => 328,
        "statx" => 332,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::{AbiField, ForeignGlobal};
    use crate::backend::target::{Arch, Os};
    use crate::bytecode::instruction::{call_c_reg, ri16, rrr};

    fn pair_type() -> AbiType {
        AbiType::Aggregate {
            size: 16,
            align: 8,
            fields: vec![
                AbiField {
                    offset: 0,
                    ty: AbiType::Float64,
                },
                AbiField {
                    offset: 8,
                    ty: AbiType::Float64,
                },
            ],
        }
    }

    fn target(abi: Abi) -> TargetSpec {
        TargetSpec {
            arch: Arch::X86_64,
            os: if abi == Abi::Win64 {
                Os::Windows
            } else {
                Os::Linux
            },
            abi,
            emit_start: false,
            no_crash: true,
        }
    }

    fn encode(chunk: &Chunk, abi: Abi) -> (Vec<u8>, Vec<PendingReloc>) {
        let fn_table = vec![chunk.name.clone()];
        let str_syms = vec![None; chunk.constants.len()];
        let bss_syms = vec![None; chunk.code.len()];
        FnEncoder {
            chunk,
            fn_table: &fn_table,
            fn_offset: 0,
            str_syms: &str_syms,
            bss_syms: &bss_syms,
            target: &target(abi),
        }
        .encode()
        .expect("ABI adapter should encode")
    }

    #[test]
    fn foreign_float_and_aggregate_call_encodes_for_sysv_and_win64() {
        let params = vec![
            pair_type(),
            AbiType::Float64,
            AbiType::Float32,
            AbiType::Integer {
                bytes: 4,
                signed: true,
            },
            AbiType::Integer {
                bytes: 8,
                signed: false,
            },
        ];
        let foreign = ForeignSymbol {
            symbol: "native_transform".to_string(),
            signature: AbiSignature {
                params: params.clone(),
                return_type: pair_type(),
                variadic: false,
            },
        };
        let mut chunk = Chunk::with_params("call_native", params.len());
        let constant = chunk.add_constant(ConstPoolEntry::ForeignSymbol(foreign));
        for index in 0..params.len() {
            chunk.emit(rrr(Opcode::CallArg, index as u8, 0, 0));
        }
        chunk.emit(ri16(Opcode::CallExt, params.len() as u8, constant));
        chunk.emit(rrr(Opcode::Ret, params.len() as u8, 0, 0));

        for abi in [Abi::SysV, Abi::Win64] {
            let (bytes, relocs) = encode(&chunk, abi);
            assert!(!bytes.is_empty());
            assert!(
                relocs
                    .iter()
                    .any(|reloc| reloc.symbol == "native_transform")
            );
            assert!(relocs.iter().any(|reloc| reloc.symbol == "malloc"));
        }
    }

    #[test]
    fn c_function_pointer_call_encodes_for_sysv_and_win64() {
        let signature = AbiSignature {
            params: vec![
                AbiType::Integer {
                    bytes: 4,
                    signed: true,
                },
                AbiType::Float64,
            ],
            return_type: AbiType::Float64,
            variadic: false,
        };
        let mut chunk = Chunk::with_params("call_callback", 3);
        let constant = chunk.add_constant(ConstPoolEntry::ForeignSymbol(ForeignSymbol {
            symbol: "<function-pointer>".to_string(),
            signature,
        }));
        chunk.emit(rrr(Opcode::CallArg, 1, 0, 0));
        chunk.emit(rrr(Opcode::CallArg, 2, 0, 0));
        chunk.emit(call_c_reg(3, 0, constant));
        chunk.emit(rrr(Opcode::Ret, 3, 0, 0));

        for abi in [Abi::SysV, Abi::Win64] {
            let (bytes, relocs) = encode(&chunk, abi);
            assert!(!bytes.is_empty());
            assert!(
                !relocs
                    .iter()
                    .any(|reloc| reloc.symbol == "<function-pointer>")
            );
        }
    }

    #[test]
    fn foreign_global_address_encodes_as_data_relocation() {
        let mut chunk = Chunk::new("read_global");
        let constant = chunk.add_constant(ConstPoolEntry::ForeignGlobal(ForeignGlobal {
            symbol: "native_counter".to_string(),
            ty: AbiType::Integer {
                bytes: 4,
                signed: true,
            },
        }));
        chunk.emit(ri16(Opcode::MovConst, 0, constant));
        chunk.emit(rrr(Opcode::Ret, 0, 0, 0));

        for abi in [Abi::SysV, Abi::Win64] {
            let (bytes, relocs) = encode(&chunk, abi);
            assert!(!bytes.is_empty());
            assert!(relocs.iter().any(|reloc| {
                reloc.symbol == "native_counter" && reloc.kind == RelocKind::Pc32
            }));
        }
    }

    #[test]
    fn exported_float_and_aggregate_adapter_encodes_for_sysv_and_win64() {
        let signature = AbiSignature {
            params: vec![pair_type(), AbiType::Float32, AbiType::Float64],
            return_type: pair_type(),
            variadic: false,
        };
        let mut chunk = Chunk::with_params("export_adapter", signature.params.len());
        chunk.export = Some(ForeignSymbol {
            symbol: "quazi_transform".to_string(),
            signature,
        });
        chunk.emit(rrr(Opcode::Ret, 0, 0, 0));

        for abi in [Abi::SysV, Abi::Win64] {
            let (bytes, relocs) = encode(&chunk, abi);
            assert!(!bytes.is_empty());
            assert!(relocs.is_empty());
        }
    }
}
