// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD
//
// VBC → x86-64 AT&T-syntax assembly emitter.
//
// Calling convention (VBC functions):
//   - Arguments: SysV AMD64 (rdi, rsi, rdx, rcx, r8, r9)
//   - Return value: rax → VBC r0 slot
//   - VBC register N → [rbp - (N+1)*8]
//   - CallArg instructions before CallIdx specify which VBC regs hold args

use std::collections::HashSet;
use std::fmt::Write;

use crate::bytecode::{Chunk, ConstPoolEntry, Opcode};

const ARG_REGS: &[&str] = &["%rdi", "%rsi", "%rdx", "%rcx", "%r8", "%r9"];

pub struct X86Emitter<'a> {
    chunks: &'a [Chunk],
    fn_table: Vec<String>,
}

impl<'a> X86Emitter<'a> {
    pub fn new(chunks: &'a [Chunk]) -> Self {
        let fn_table = chunks.iter().map(|c| c.name.clone()).collect();
        Self { chunks, fn_table }
    }

    pub fn emit_asm(&self) -> String {
        let mut out = String::new();

        // string constants → .rodata
        writeln!(out, "\t.section .rodata").unwrap();
        for chunk in self.chunks {
            let lbl = safe_label(&chunk.name);
            for (i, entry) in chunk.constants.iter().enumerate() {
                if let ConstPoolEntry::Str(s) = entry {
                    writeln!(out, ".{}_str{}:", lbl, i).unwrap();
                    writeln!(out, "\t.string \"{}\"", escape_str(s)).unwrap();
                }
            }
        }

        writeln!(out, "\n\t.text\n").unwrap();

        for chunk in self.chunks {
            self.emit_fn(chunk, &mut out);
            writeln!(out).unwrap();
        }

        out
    }

    fn emit_fn(&self, chunk: &Chunk, out: &mut String) {
        let lbl = safe_label(&chunk.name);
        let num_regs = max_reg_used(chunk) + 1;
        let frame_size = round_to_16(num_regs * 8);
        let targets = jump_targets(chunk);

        writeln!(out, "\t.globl {}", lbl).unwrap();
        writeln!(out, "{}:", lbl).unwrap();

        // prologue
        writeln!(out, "\tpushq %rbp").unwrap();
        writeln!(out, "\tmovq %rsp, %rbp").unwrap();
        if frame_size > 0 {
            writeln!(out, "\tsubq ${}, %rsp", frame_size).unwrap();
        }

        // load SysV arg regs into VBC param slots
        for i in 0..chunk.param_count.min(6) {
            writeln!(out, "\tmovq {}, {}", ARG_REGS[i], slot(i as u8)).unwrap();
        }

        // emit instructions
        let mut pending_args: Vec<u8> = Vec::new();
        for (idx, instr) in chunk.code.iter().enumerate() {
            if targets.contains(&(idx as u16)) {
                writeln!(out, ".{}_L{}:", lbl, idx).unwrap();
            }
            match Opcode::from_u8(instr.opcode) {
                Some(Opcode::CallArg) => {
                    pending_args.push(instr.ops[0]);
                }
                Some(Opcode::CallIdx) => {
                    self.emit_call(instr, &pending_args, &lbl, out);
                    pending_args.clear();
                }
                _ => self.emit_instr(instr, chunk, &lbl, out),
            }
        }

        // implicit return label (jumped to by if-without-else at function end)
        if targets.contains(&(chunk.code.len() as u16)) {
            writeln!(out, ".{}_L{}:", lbl, chunk.code.len()).unwrap();
            writeln!(out, "\txorq %rax, %rax").unwrap();
            writeln!(out, "\tmovq %rbp, %rsp").unwrap();
            writeln!(out, "\tpopq %rbp").unwrap();
            writeln!(out, "\tretq").unwrap();
        }
    }

    fn emit_instr(&self, instr: &crate::bytecode::Instruction, chunk: &Chunk, lbl: &str, out: &mut String) {
        match Opcode::from_u8(instr.opcode) {
            Some(Opcode::Nop) => {
                writeln!(out, "\tnop").unwrap();
            }

            Some(Opcode::Mov) => {
                let (dst, src, _) = instr.rrr();
                writeln!(out, "\tmovq {}, %rax", slot(src)).unwrap();
                writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
            }

            Some(Opcode::MovI) => {
                let (dst, imm) = instr.ri16();
                writeln!(out, "\tmovq ${}, {}", imm, slot(dst)).unwrap();
            }

            Some(Opcode::MovConst) => {
                let (dst, idx) = instr.ri16();
                let fn_lbl = safe_label(&chunk.name);
                match chunk.constants.get(idx as usize) {
                    Some(ConstPoolEntry::Int(n)) => {
                        writeln!(out, "\tmovq ${}, %rax", n).unwrap();
                        writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
                    }
                    Some(ConstPoolEntry::Str(_)) => {
                        writeln!(out, "\tleaq .{}_str{}(%rip), %rax", fn_lbl, idx).unwrap();
                        writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
                    }
                    Some(ConstPoolEntry::Float(f)) => {
                        let bits = f.to_bits() as i64;
                        writeln!(out, "\tmovq ${}, %rax", bits).unwrap();
                        writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
                    }
                    None => {
                        writeln!(out, "\t# MovConst: missing pool entry {}", idx).unwrap();
                    }
                }
            }

            Some(Opcode::Add) => {
                let (dst, s1, s2) = instr.rrr();
                writeln!(out, "\tmovq {}, %rax", slot(s1)).unwrap();
                writeln!(out, "\taddq {}, %rax", slot(s2)).unwrap();
                writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
            }

            Some(Opcode::Sub) => {
                let (dst, s1, s2) = instr.rrr();
                writeln!(out, "\tmovq {}, %rax", slot(s1)).unwrap();
                writeln!(out, "\tsubq {}, %rax", slot(s2)).unwrap();
                writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
            }

            Some(Opcode::Mul) => {
                let (dst, s1, s2) = instr.rrr();
                writeln!(out, "\tmovq {}, %rax", slot(s1)).unwrap();
                writeln!(out, "\timulq {}, %rax", slot(s2)).unwrap();
                writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
            }

            Some(Opcode::Div) => {
                let (dst, s1, s2) = instr.rrr();
                writeln!(out, "\tmovq {}, %rax", slot(s1)).unwrap();
                writeln!(out, "\tcqto").unwrap();
                writeln!(out, "\tidivq {}", slot(s2)).unwrap();
                writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
            }

            Some(Opcode::Mod) => {
                let (dst, s1, s2) = instr.rrr();
                writeln!(out, "\tmovq {}, %rax", slot(s1)).unwrap();
                writeln!(out, "\tcqto").unwrap();
                writeln!(out, "\tidivq {}", slot(s2)).unwrap();
                writeln!(out, "\tmovq %rdx, {}", slot(dst)).unwrap();
            }

            Some(Opcode::Neg) => {
                let (dst, src, _) = instr.rrr();
                writeln!(out, "\tmovq {}, %rax", slot(src)).unwrap();
                writeln!(out, "\tnegq %rax").unwrap();
                writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
            }

            Some(Opcode::Not) => {
                let (dst, src, _) = instr.rrr();
                writeln!(out, "\tmovq {}, %rax", slot(src)).unwrap();
                writeln!(out, "\tnotq %rax").unwrap();
                writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
            }

            Some(Opcode::Inc) => {
                let (dst, _, _) = instr.rrr();
                writeln!(out, "\tincq {}", slot(dst)).unwrap();
            }

            Some(Opcode::Dec) => {
                let (dst, _, _) = instr.rrr();
                writeln!(out, "\tdecq {}", slot(dst)).unwrap();
            }

            Some(Opcode::And) => {
                let (dst, s1, s2) = instr.rrr();
                writeln!(out, "\tmovq {}, %rax", slot(s1)).unwrap();
                writeln!(out, "\tandq {}, %rax", slot(s2)).unwrap();
                writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
            }

            Some(Opcode::Or) => {
                let (dst, s1, s2) = instr.rrr();
                writeln!(out, "\tmovq {}, %rax", slot(s1)).unwrap();
                writeln!(out, "\torq {}, %rax", slot(s2)).unwrap();
                writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
            }

            Some(Opcode::Xor) => {
                let (dst, s1, s2) = instr.rrr();
                writeln!(out, "\tmovq {}, %rax", slot(s1)).unwrap();
                writeln!(out, "\txorq {}, %rax", slot(s2)).unwrap();
                writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
            }

            Some(Opcode::Shl) => {
                let (dst, s1, s2) = instr.rrr();
                writeln!(out, "\tmovq {}, %rax", slot(s1)).unwrap();
                writeln!(out, "\tmovq {}, %rcx", slot(s2)).unwrap();
                writeln!(out, "\tshlq %cl, %rax").unwrap();
                writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
            }

            Some(Opcode::Shr) => {
                let (dst, s1, s2) = instr.rrr();
                writeln!(out, "\tmovq {}, %rax", slot(s1)).unwrap();
                writeln!(out, "\tmovq {}, %rcx", slot(s2)).unwrap();
                writeln!(out, "\tshrq %cl, %rax").unwrap();
                writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
            }

            Some(Opcode::Sar) => {
                let (dst, s1, s2) = instr.rrr();
                writeln!(out, "\tmovq {}, %rax", slot(s1)).unwrap();
                writeln!(out, "\tmovq {}, %rcx", slot(s2)).unwrap();
                writeln!(out, "\tsarq %cl, %rax").unwrap();
                writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
            }

            Some(Opcode::Cmp) => {
                // rrr(Cmp, 0, s1, s2) — sets flags for s1 vs s2
                let (_, s1, s2) = instr.rrr();
                writeln!(out, "\tmovq {}, %rax", slot(s1)).unwrap();
                // AT&T cmpq B, A means A-B; so cmpq s2_slot, %rax = rax - s2 = s1 - s2
                writeln!(out, "\tcmpq {}, %rax", slot(s2)).unwrap();
            }

            Some(Opcode::Jmp) => {
                let (_, target) = instr.ri16();
                writeln!(out, "\tjmp .{}_L{}", lbl, target).unwrap();
            }
            Some(Opcode::Je)  => { let (_, t) = instr.ri16(); writeln!(out, "\tje  .{}_L{}", lbl, t).unwrap(); }
            Some(Opcode::Jne) => { let (_, t) = instr.ri16(); writeln!(out, "\tjne .{}_L{}", lbl, t).unwrap(); }
            Some(Opcode::Jg)  => { let (_, t) = instr.ri16(); writeln!(out, "\tjg  .{}_L{}", lbl, t).unwrap(); }
            Some(Opcode::Jge) => { let (_, t) = instr.ri16(); writeln!(out, "\tjge .{}_L{}", lbl, t).unwrap(); }
            Some(Opcode::Jl)  => { let (_, t) = instr.ri16(); writeln!(out, "\tjl  .{}_L{}", lbl, t).unwrap(); }
            Some(Opcode::Jle) => { let (_, t) = instr.ri16(); writeln!(out, "\tjle .{}_L{}", lbl, t).unwrap(); }
            Some(Opcode::Ja)  => { let (_, t) = instr.ri16(); writeln!(out, "\tja  .{}_L{}", lbl, t).unwrap(); }
            Some(Opcode::Jb)  => { let (_, t) = instr.ri16(); writeln!(out, "\tjb  .{}_L{}", lbl, t).unwrap(); }

            Some(Opcode::Jz) => {
                let (reg, target) = instr.ri16();
                writeln!(out, "\tcmpq $0, {}", slot(reg)).unwrap();
                writeln!(out, "\tje  .{}_L{}", lbl, target).unwrap();
            }
            Some(Opcode::Jnz) => {
                let (reg, target) = instr.ri16();
                writeln!(out, "\tcmpq $0, {}", slot(reg)).unwrap();
                writeln!(out, "\tjne .{}_L{}", lbl, target).unwrap();
            }

            Some(Opcode::Ret) => {
                writeln!(out, "\tmovq {}, %rax", slot(0)).unwrap();
                writeln!(out, "\tmovq %rbp, %rsp").unwrap();
                writeln!(out, "\tpopq %rbp").unwrap();
                writeln!(out, "\tretq").unwrap();
            }

            Some(Opcode::Lea) => {
                let (dst, base, offset) = instr.mem();
                writeln!(out, "\tleaq {}, %rax", slot(base)).unwrap();
                if offset != 0 {
                    writeln!(out, "\taddq ${}, %rax", offset).unwrap();
                }
                writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
            }

            Some(Opcode::Load) => {
                let (dst, base, offset) = instr.mem();
                writeln!(out, "\tmovq {}, %rax", slot(base)).unwrap();
                if offset != 0 {
                    writeln!(out, "\taddq ${}, %rax", offset).unwrap();
                }
                writeln!(out, "\tmovq (%rax), %rcx").unwrap();
                writeln!(out, "\tmovq %rcx, {}", slot(dst)).unwrap();
            }

            Some(Opcode::Store) => {
                let (src, base, offset) = instr.mem();
                writeln!(out, "\tmovq {}, %rax", slot(base)).unwrap();
                if offset != 0 {
                    writeln!(out, "\taddq ${}, %rax", offset).unwrap();
                }
                writeln!(out, "\tmovq {}, %rcx", slot(src)).unwrap();
                writeln!(out, "\tmovq %rcx, (%rax)").unwrap();
            }

            Some(Opcode::VtblLoad) => {
                let (dst, _, _) = instr.rrr();
                writeln!(out, "\t# VtblLoad: stdlib dispatch not yet implemented").unwrap();
                writeln!(out, "\txorq %rax, %rax").unwrap();
                writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
            }

            Some(Opcode::CallReg) => {
                writeln!(out, "\t# CallReg: vtable/stdlib call not yet implemented").unwrap();
                writeln!(out, "\txorq %rax, %rax").unwrap();
                writeln!(out, "\tmovq %rax, {}", slot(0)).unwrap();
            }

            _ => {
                writeln!(out, "\t# unimplemented opcode 0x{:02X}", instr.opcode).unwrap();
            }
        }
    }

    fn emit_call(&self, instr: &crate::bytecode::Instruction, arg_regs: &[u8], _caller_lbl: &str, out: &mut String) {
        let (dst, fn_idx) = instr.ri16();
        let fn_name = self.fn_table
            .get(fn_idx as usize)
            .map(String::as_str)
            .unwrap_or("__unknown__");

        for (i, &vreg) in arg_regs.iter().enumerate().take(6) {
            writeln!(out, "\tmovq {}, {}", slot(vreg), ARG_REGS[i]).unwrap();
        }
        writeln!(out, "\tcallq {}", safe_label(fn_name)).unwrap();
        writeln!(out, "\tmovq %rax, {}", slot(dst)).unwrap();
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// VBC register N → stack slot string.
fn slot(reg: u8) -> String {
    format!("-{}(%rbp)", (reg as usize + 1) * 8)
}

/// Round up to nearest multiple of 16.
fn round_to_16(n: usize) -> usize {
    (n + 15) & !15
}

/// Max VBC register index referenced in a chunk (scanning per-opcode format).
fn max_reg_used(chunk: &Chunk) -> usize {
    let mut max = chunk.param_count.saturating_sub(1);
    for instr in &chunk.code {
        let (r0, r1, r2) = (instr.ops[0] as usize, instr.ops[1] as usize, instr.ops[2] as usize);
        match Opcode::from_u8(instr.opcode) {
            // RRR: dst=r0, src1=r1, src2=r2
            Some(
                Opcode::Mov | Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div |
                Opcode::Mod | Opcode::Neg | Opcode::Not | Opcode::Inc | Opcode::Dec |
                Opcode::And | Opcode::Or  | Opcode::Xor | Opcode::Shl | Opcode::Shr |
                Opcode::Sar | Opcode::VtblLoad | Opcode::FieldLoad | Opcode::FieldStore |
                Opcode::CallReg
            ) => { max = max.max(r0).max(r1).max(r2); }
            // Cmp: dst=0 (scratch), src1=r1, src2=r2
            Some(Opcode::Cmp) => { max = max.max(r1).max(r2); }
            // MEM: ops[0]=value/dst, ops[1]=base
            Some(Opcode::Load | Opcode::Store | Opcode::Lea) => {
                max = max.max(r0).max(r1);
            }
            // RI16 where ops[0] is a real register
            Some(Opcode::MovI | Opcode::MovConst | Opcode::Jz | Opcode::Jnz | Opcode::CallArg | Opcode::CallIdx) => {
                max = max.max(r0);
            }
            // Pure jumps / Ret / Nop: ops[0] is 0, not a reg
            _ => {}
        }
    }
    max
}

/// Collect instruction indices that are targets of any jump.
fn jump_targets(chunk: &Chunk) -> HashSet<u16> {
    let mut set = HashSet::new();
    for instr in &chunk.code {
        match Opcode::from_u8(instr.opcode) {
            Some(
                Opcode::Jmp | Opcode::Je  | Opcode::Jne | Opcode::Jg  | Opcode::Jge |
                Opcode::Jl  | Opcode::Jle | Opcode::Ja  | Opcode::Jb  |
                Opcode::Jz  | Opcode::Jnz
            ) => {
                let (_, target) = instr.ri16();
                set.insert(target);
            }
            _ => {}
        }
    }
    set
}

fn safe_label(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

fn escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"'  => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c    => out.push(c),
        }
    }
    out
}
