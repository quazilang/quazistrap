// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD
//
// VBC → x86-64 binary encoding via iced-x86 CodeAssembler.
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

use crate::backend::{BackendError, TargetSpec, target::Abi};
use crate::bytecode::{Chunk, ConstPoolEntry, Opcode};

use super::relocations::{PendingReloc, RelocKind};

// ── Calling-convention register tables ──────────────────────────────────────

const SYSV_REGS: [AsmRegister64; 6] = [rdi, rsi, rdx, rcx, r8, r9];
const SYSCALL_REGS: [AsmRegister64; 6] = [rdi, rsi, rdx, r10, r8, r9];
// Win64: first 4 integer args in rcx/rdx/r8/r9; args 5-6 on stack.
const WIN64_REGS: [AsmRegister64; 4] = [rcx, rdx, r8, r9];

// ── helpers ──────────────────────────────────────────────────────────────────

/// VBC register N → `qword ptr [rbp - (N+1)*8]`.
fn slot(reg: u8) -> AsmMemoryOperand {
    qword_ptr(rbp + (-((reg as i32 + 1) * 8)))
}

fn round_to_16(n: usize) -> usize {
    (n + 15) & !15
}

fn max_reg_used(chunk: &Chunk) -> usize {
    let mut max = chunk.param_count.saturating_sub(1);
    for instr in &chunk.code {
        let (r0, r1, r2) = (
            instr.ops[0] as usize,
            instr.ops[1] as usize,
            instr.ops[2] as usize,
        );
        match Opcode::from_u8(instr.opcode) {
            Some(
                Opcode::Mov
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
                | Opcode::VtblLoad
                | Opcode::FieldLoad
                | Opcode::FieldStore
                | Opcode::CallReg
                | Opcode::StrLen
                | Opcode::StrConcat
                | Opcode::StrToInt
                | Opcode::StrToFloat
                | Opcode::PrimToStr
                | Opcode::StrAsStr
                | Opcode::Pow
                | Opcode::ArrayStore
                | Opcode::ArrayLoad,
            ) => {
                max = max.max(r0).max(r1).max(r2);
            }
            Some(Opcode::Cmp) => {
                max = max.max(r1).max(r2);
            }
            Some(Opcode::Load | Opcode::Store | Opcode::Lea) => {
                max = max.max(r0).max(r1);
            }
            Some(
                Opcode::MovI
                | Opcode::MovConst
                | Opcode::Jz
                | Opcode::Jnz
                | Opcode::CallArg
                | Opcode::CallIdx
                | Opcode::CallExt
                | Opcode::Syscall
                | Opcode::New,
            ) => {
                max = max.max(r0);
            }
            Some(Opcode::Intrinsic) => {
                // Intrinsic accesses dst through dst+flags-1 (flags = arg_count).
                let last = r0 + instr.flags.saturating_sub(1) as usize;
                max = max.max(last);
            }
            _ => {}
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
        // Win64: reserve 32-byte shadow space + 16 bytes for up to 2 stack args
        let frame_size = if is_win64 {
            round_to_16(num_regs * 8 + 48) as i32
        } else {
            round_to_16(num_regs * 8) as i32
        };

        let err = |e: IcedError| BackendError(e.to_string());

        let mut asm = CodeAssembler::new(64).map_err(err)?;

        // fn_start label at byte 0 of this function — used as dummy placeholder
        // for all external references that will become relocations.
        let mut fn_start = asm.create_label();
        asm.set_label(&mut fn_start).map_err(err)?;

        // Build label map for VBC jump targets (instr index → CodeLabel).
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
        if is_win64 {
            for i in 0..chunk.param_count.min(4) {
                emit!(asm.mov(slot(i as u8), WIN64_REGS[i]));
            }
            if chunk.param_count > 4 {
                // arg5 lives at [rbp+48]: above saved_rbp(8) + ret_addr(8) + 4 shadow slots(32)
                emit!(asm.mov(rax, qword_ptr(rbp + 48i32)));
                emit!(asm.mov(slot(4), rax));
            }
            if chunk.param_count > 5 {
                emit!(asm.mov(rax, qword_ptr(rbp + 56i32)));
                emit!(asm.mov(slot(5), rax));
            }
        } else {
            for i in 0..chunk.param_count.min(6) {
                emit!(asm.mov(slot(i as u8), SYSV_REGS[i]));
            }
        }

        // ── Instruction loop ─────────────────────────────────────────────────
        let mut pending_args: Vec<u8> = vec![];

        for (vbc_idx, instr) in chunk.code.iter().enumerate() {
            // Set label if this VBC instruction index is a jump target.
            if let Some(lbl) = label_map.get_mut(&vbc_idx) {
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
                        .unwrap_or_else(|| "__void_unknown".into());

                    if is_win64 {
                        for (i, &vreg) in pending_args.iter().enumerate().take(4) {
                            emit!(asm.mov(WIN64_REGS[i], slot(vreg)));
                        }
                        if pending_args.len() > 4 {
                            emit!(asm.mov(rax, slot(pending_args[4])));
                            emit!(asm.mov(qword_ptr(rsp + 32i32), rax));
                        }
                        if pending_args.len() > 5 {
                            emit!(asm.mov(rax, slot(pending_args[5])));
                            emit!(asm.mov(qword_ptr(rsp + 40i32), rax));
                        }
                    } else {
                        for (i, &vreg) in pending_args.iter().enumerate().take(6) {
                            emit!(asm.mov(SYSV_REGS[i], slot(vreg)));
                        }
                    }
                    let call_idx = asm.instructions().len();
                    emit!(asm.call(fn_start));
                    pending.push((call_idx, 1, RelocKind::Plt32, fn_name, -4));
                    emit!(asm.mov(slot(dst), rax));
                    pending_args.clear();
                }

                // ── CallReg: indirect call through a function pointer in a register ──
                Some(Opcode::CallReg) => {
                    let (dst, fn_reg, _) = instr.rrr();
                    if is_win64 {
                        for (i, &vreg) in pending_args.iter().enumerate().take(4) {
                            emit!(asm.mov(WIN64_REGS[i], slot(vreg)));
                        }
                        if pending_args.len() > 4 {
                            emit!(asm.mov(rax, slot(pending_args[4])));
                            emit!(asm.mov(qword_ptr(rsp + 32i32), rax));
                        }
                        if pending_args.len() > 5 {
                            emit!(asm.mov(rax, slot(pending_args[5])));
                            emit!(asm.mov(qword_ptr(rsp + 40i32), rax));
                        }
                    } else {
                        for (i, &vreg) in pending_args.iter().enumerate().take(6) {
                            emit!(asm.mov(SYSV_REGS[i], slot(vreg)));
                        }
                    }
                    emit!(asm.mov(rax, slot(fn_reg)));
                    emit!(asm.call(rax));
                    emit!(asm.mov(slot(dst), rax));
                    pending_args.clear();
                }

                // ── All other opcodes ─────────────────────────────────────────
                op => {
                    self.emit_instr(
                        &mut asm,
                        instr,
                        chunk,
                        vbc_idx,
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
    fn emit_instr(
        &self,
        asm: &mut CodeAssembler,
        instr: &crate::bytecode::Instruction,
        chunk: &Chunk,
        vbc_idx: usize,
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
                    Some(ConstPoolEntry::Str(_)) => {
                        if let Some(sym) = self.str_syms.get(idx as usize).and_then(|s| s.as_ref())
                        {
                            lea_rip!(rax, sym.clone());
                            emit!(asm.mov(slot(dst), rax));
                        }
                    }
                    Some(ConstPoolEntry::FnAddr(name)) => {
                        // Look up the function symbol name. FnAddr stores the raw function name;
                        // the symbol may have __void_intr_ prefix or safe-label mangling.
                        let sym = self
                            .fn_table
                            .iter()
                            .find(|s| {
                                s == &name
                                    || s.trim_start_matches("__void_intr_") == name
                                    || s.trim_start_matches("__void_intr_") == safe_fn_label(name)
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
                emit!(asm.mov(rcx, qword_ptr(rax)));
                emit!(asm.mov(slot(dst), rcx));
            }

            Some(Opcode::Store) => {
                let (src, base, offset) = instr.mem();
                emit!(asm.mov(rax, slot(base)));
                if offset != 0 {
                    emit!(asm.add(rax, offset as i32));
                }
                emit!(asm.mov(rcx, slot(src)));
                emit!(asm.mov(qword_ptr(rax), rcx));
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
                    // Raw syscalls are not safe on Windows; @syscall is Linux-only.
                    emit!(asm.xor(rax, rax));
                    emit!(asm.mov(slot(dst), rax));
                } else {
                    let syscall_num = match chunk.constants.get(idx as usize) {
                        Some(ConstPoolEntry::Int(n)) => *n as u64,
                        Some(ConstPoolEntry::Str(s)) => resolve_x86_64_syscall(s),
                        _ => 0,
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
                        // void.write(fd, buf, len) → isize
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
                        // void.read(fd, buf, len) → isize
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
                        // void.exit(code) → !
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
                        // void.malloc(size) → ptr
                        if is_win64 {
                            emit!(asm.mov(rcx, slot(dst)));
                        } else {
                            emit!(asm.mov(rdi, slot(dst)));
                        }
                        call_ext!("malloc".into(), RelocKind::Plt32);
                        emit!(asm.mov(slot(dst), rax));
                    }
                    4 => {
                        // void.free(ptr) → void
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
                        // void.realloc(ptr, size) → usize
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
                        // void.memcpy(dst_ptr, src, n) → usize
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
                        // void.memset(ptr, val, n) → usize
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
                        // void.memmove(dst_ptr, src, n) → usize
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
                        // void.memcmp(a, b, n) → i32
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
                        // void.strlen(s) -> usize. Inline it so Linux stays libc-free.
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
                        // void.stderr_write(buf, len) → isize
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
                        // void.sleep_ms(ms) → void
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
                        // void.getenv(name) → usize (char*)
                        if is_win64 {
                            emit!(asm.mov(rcx, slot(dst)));
                        } else {
                            emit!(asm.mov(rdi, slot(dst)));
                        }
                        call_ext!("getenv".into(), RelocKind::Plt32);
                        emit!(asm.mov(slot(dst), rax));
                    }
                    14 => {
                        // void.str_concat(s1, s2) → str (heap-allocated, null-terminated)
                        // slot(dst)=s1, slot(dst+1)=s2
                        // malloc(strlen(s1)+strlen(s2)+1), strcpy(buf,s1), strcat(buf,s2)
                        // Uses callee-saved rbx, r12, r13 to survive calls.
                        emit!(asm.push(rbx));
                        emit!(asm.push(r12));
                        emit!(asm.push(r13));
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
                        emit!(asm.pop(r13));
                        emit!(asm.pop(r12));
                        emit!(asm.pop(rbx));
                    }
                    15 => {
                        // void.int_to_str(n: i64) → str (heap-allocated, null-terminated)
                        // slot(dst) = n; malloc(32), sprintf(buf, "%ld", n), return buf
                        // Push rbx + rax (2*8=16 bytes) to keep rsp 16-byte aligned.
                        emit!(asm.push(rbx));
                        emit!(asm.push(rax));
                        if is_win64 {
                            emit!(asm.mov(rcx, 32i64));
                        } else {
                            emit!(asm.mov(rdi, 32i64));
                        }
                        call_ext!("malloc".into(), RelocKind::Plt32);
                        emit!(asm.mov(rbx, rax));
                        if is_win64 {
                            emit!(asm.mov(rcx, rbx));
                            lea_rip!(rdx, "__void_fmt_ld".into());
                            emit!(asm.mov(r8, slot(dst)));
                        } else {
                            emit!(asm.mov(rdi, rbx));
                            lea_rip!(rsi, "__void_fmt_ld".into());
                            emit!(asm.mov(rdx, slot(dst)));
                        }
                        call_ext!("sprintf".into(), RelocKind::Plt32);
                        emit!(asm.mov(slot(dst), rbx));
                        emit!(asm.pop(rax));
                        emit!(asm.pop(rbx));
                    }
                    16 => {
                        // void.float_to_str(f: f64) → str (heap-allocated, null-terminated)
                        // slot(dst) = f (64-bit IEEE754); malloc(32), sprintf(buf, "%g", f), return buf
                        // Push rbx + rax (2*8=16 bytes) to keep rsp 16-byte aligned.
                        emit!(asm.push(rbx));
                        emit!(asm.push(rax));
                        if is_win64 {
                            emit!(asm.mov(rcx, 32i64));
                        } else {
                            emit!(asm.mov(rdi, 32i64));
                        }
                        call_ext!("malloc".into(), RelocKind::Plt32);
                        emit!(asm.mov(rbx, rax));
                        emit!(asm.mov(rax, slot(dst)));
                        if is_win64 {
                            emit!(asm.mov(rcx, rbx));
                            lea_rip!(rdx, "__void_fmt_g".into());
                            emit!(asm.movq(xmm2, rax));
                            emit!(asm.mov(r8, rax));
                        } else {
                            emit!(asm.mov(rdi, rbx));
                            lea_rip!(rsi, "__void_fmt_g".into());
                            emit!(asm.movq(xmm0, rax));
                            emit!(asm.mov(eax, 1i32));
                        }
                        call_ext!("sprintf".into(), RelocKind::Plt32);
                        emit!(asm.mov(slot(dst), rbx));
                        emit!(asm.pop(rax));
                        emit!(asm.pop(rbx));
                    }
                    18 => {
                        // void.thread.spawn(f: any) → usize (thread handle)
                        // slot(dst) = function pointer (address)
                        emit!(asm.push(rbx));
                        emit!(asm.push(rax)); // align rsp to 16
                        if is_win64 {
                            // CreateThread(NULL, 0, fn_ptr, NULL, 0, NULL)
                            emit!(asm.xor(rcx, rcx));
                            emit!(asm.xor(rdx, rdx));
                            emit!(asm.mov(r8, slot(dst)));
                            emit!(asm.xor(r9, r9));
                            emit!(asm.mov(qword_ptr(rsp + 32i32), 0i32)); // flags
                            emit!(asm.mov(qword_ptr(rsp + 40i32), 0i32)); // thread_id
                            call_ext!("CreateThread".into(), RelocKind::Plt32);
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
                        // void.thread.join(handle: usize) → void
                        // slot(dst) = thread handle
                        emit!(asm.push(rbx));
                        emit!(asm.push(rax)); // align
                        if is_win64 {
                            emit!(asm.mov(rcx, slot(dst)));
                            emit!(asm.mov(edx, u32::MAX as i32)); // INFINITE
                            call_ext!("WaitForSingleObject".into(), RelocKind::Plt32);
                            emit!(asm.mov(rcx, slot(dst)));
                            call_ext!("CloseHandle".into(), RelocKind::Plt32);
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
                        // void.net.bind_tcp(sockfd: i32, port: i32) → i32
                        // Builds sockaddr_in on stack — void has no byte-level memory writes
                        emit!(asm.push(rbx));
                        emit!(asm.push(rax)); // keep rsp 16-byte aligned
                        emit!(asm.sub(rsp, 32i32)); // 16 bytes sockaddr_in + 16 shadow (Win64)
                        emit!(asm.xor(rax, rax));
                        emit!(asm.mov(qword_ptr(rsp), rax)); // zero first 8 bytes
                        emit!(asm.mov(qword_ptr(rsp + 8i32), rax)); // zero last 8 bytes
                        emit!(asm.mov(word_ptr(rsp), 2i32)); // sa_family = AF_INET
                        // port: host→big-endian byte swap
                        emit!(asm.mov(rax, slot(dst + 1)));
                        emit!(asm.rol(ax, 8u32));
                        emit!(asm.mov(word_ptr(rsp + 2i32), ax)); // sin_port (big-endian)
                        // sin_addr = INADDR_ANY = 0 (already zeroed)
                        if is_win64 {
                            emit!(asm.mov(rcx, slot(dst))); // sockfd
                            emit!(asm.lea(rdx, qword_ptr(rsp))); // &sockaddr_in
                            emit!(asm.mov(r8d, 16i32)); // addrlen
                            call_ext!("bind".into(), RelocKind::Plt32);
                        } else {
                            emit!(asm.mov(rdi, slot(dst))); // sockfd
                            emit!(asm.lea(rsi, qword_ptr(rsp))); // &sockaddr_in
                            emit!(asm.mov(edx, 16i32)); // addrlen
                            emit!(asm.mov(rax, 49i64)); // bind syscall
                            emit!(asm.syscall());
                        }
                        emit!(asm.add(rsp, 32i32));
                        emit!(asm.mov(slot(dst), rax));
                        emit!(asm.pop(rax));
                        emit!(asm.pop(rbx));
                    }
                    21 => {
                        // void.net.connect_tcp(sockfd: i32, ip: str, port: i32) → i32
                        // Uses inet_pton to parse dotted-decimal IP, builds sockaddr_in
                        emit!(asm.push(rbx));
                        emit!(asm.push(r12));
                        emit!(asm.push(rax));
                        emit!(asm.push(rax)); // 4 pushes = 32 bytes, rsp stays 16-aligned
                        emit!(asm.sub(rsp, 32i32)); // 16 sockaddr_in + 16 shadow
                        emit!(asm.xor(rax, rax));
                        emit!(asm.mov(qword_ptr(rsp), rax));
                        emit!(asm.mov(qword_ptr(rsp + 8i32), rax));
                        emit!(asm.mov(word_ptr(rsp), 2i32)); // sa_family = AF_INET
                        emit!(asm.mov(rax, slot(dst + 2)));
                        emit!(asm.rol(ax, 8u32));
                        emit!(asm.mov(word_ptr(rsp + 2i32), ax)); // sin_port (big-endian)
                        // inet_pton(AF_INET, ip_str, &sin_addr) — sin_addr at rsp+4
                        emit!(asm.lea(r12, qword_ptr(rsp + 4i32)));
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
                            emit!(asm.lea(rdx, qword_ptr(rsp)));
                            emit!(asm.mov(r8d, 16i32));
                            call_ext!("connect".into(), RelocKind::Plt32);
                        } else {
                            emit!(asm.mov(rdi, slot(dst)));
                            emit!(asm.lea(rsi, qword_ptr(rsp)));
                            emit!(asm.mov(edx, 16i32));
                            emit!(asm.mov(rax, 42i64)); // connect syscall
                            emit!(asm.syscall());
                        }
                        emit!(asm.add(rsp, 32i32));
                        emit!(asm.mov(slot(dst), rax));
                        emit!(asm.pop(rax));
                        emit!(asm.pop(rax));
                        emit!(asm.pop(r12));
                        emit!(asm.pop(rbx));
                    }
                    23 => {
                        // void.str.byte_at(s: str, i: usize) u8
                        // s is a char* pointer (str register = ptr portion).
                        // Load byte at [s + i] with zero-extension.
                        emit!(asm.mov(rax, slot(dst))); // s (ptr)
                        emit!(asm.mov(rcx, slot(dst + 1))); // i
                        emit!(asm.movzx(rax, byte_ptr(rax + rcx)));
                        emit!(asm.mov(slot(dst), rax));
                    }
                    24 => {
                        // void.str.from_byte(b: u8) str
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
                        // void.print_backtrace() → void
                        call_ext!("__void_print_backtrace".into(), RelocKind::Plt32);
                        emit!(asm.xor(rax, rax));
                        emit!(asm.mov(slot(dst), rax));
                    }
                    _ => {
                        emit!(asm.xor(rax, rax));
                        emit!(asm.mov(slot(dst), rax));
                    }
                }
            }

            Some(Opcode::CallExt) => {
                let (dst, idx) = instr.ri16();
                let arg_count = instr.flags as usize;
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
                    .get(vbc_idx)
                    .and_then(|s| s.as_ref())
                    .cloned()
                    .unwrap_or_else(|| format!("__void_itoa_missing_{}", vbc_idx));

                match type_tag {
                    1 => {
                        // float: load 64-bit bits → XMM, sprintf with "%g"
                        emit!(asm.mov(rax, slot(src)));
                        if is_win64 {
                            lea_rip!(rcx, buf_sym.clone());
                            lea_rip!(rdx, "__void_fmt_g".into());
                            emit!(asm.movq(xmm2, rax));
                            emit!(asm.mov(r8, rax));
                        } else {
                            lea_rip!(rdi, buf_sym.clone());
                            lea_rip!(rsi, "__void_fmt_g".into());
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
                            3 => "__void_fmt_llx",
                            4 => "__void_fmt_llX",
                            _ => "__void_fmt_llo",
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
                        let fmt_sym = format!("__void_fmt_prec_{}", prec);
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
                            lea_rip!(rdx, "__void_fmt_ld".into());
                            emit!(asm.mov(r8, slot(src)));
                        } else {
                            lea_rip!(rdi, buf_sym.clone());
                            lea_rip!(rsi, "__void_fmt_ld".into());
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
                let (dst, obj, byte_off) = instr.rrr();
                emit!(asm.mov(rax, slot(obj)));
                emit!(asm.mov(rcx, qword_ptr(rax + byte_off as i64)));
                emit!(asm.mov(slot(dst), rcx));
            }

            Some(Opcode::FieldStore) => {
                let (val, obj, byte_off) = instr.rrr();
                emit!(asm.mov(rax, slot(obj)));
                emit!(asm.mov(rcx, slot(val)));
                emit!(asm.mov(qword_ptr(rax + byte_off as i64), rcx));
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
                panic!(
                    "encoder: unimplemented opcode {:?}",
                    Opcode::from_u8(instr.opcode)
                );
            }
        }

        Ok(())
    }
}

fn resolve_x86_64_syscall(name: &str) -> u64 {
    match name {
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
        _ => 0,
    }
}
