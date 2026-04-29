// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use super::instruction::Instruction;
use super::opcode::Opcode;
use super::instruction::{rrr, ri16};

/// Constant pool value — lives alongside bytecode, referenced by MovConst.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstPoolEntry {
    Int(i64),
    Float(f64),
    Str(String),
}

/// A single function's bytecode + its constant pool.
#[derive(Debug, Default)]
pub struct Chunk {
    pub code: Vec<Instruction>,
    pub constants: Vec<ConstPoolEntry>,
    pub name: String,
}

impl Chunk {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Default::default() }
    }

    pub fn emit(&mut self, instr: Instruction) -> usize {
        let idx = self.code.len();
        self.code.push(instr);
        idx
    }

    pub fn emit_rrr(&mut self, op: Opcode, dst: u8, src1: u8, src2: u8) -> usize {
        self.emit(rrr(op, dst, src1, src2))
    }

    pub fn emit_ri16(&mut self, op: Opcode, dst: u8, imm: u16) -> usize {
        self.emit(ri16(op, dst, imm))
    }

    pub fn add_constant(&mut self, val: ConstPoolEntry) -> u16 {
        let idx = self.constants.len() as u16;
        self.constants.push(val);
        idx
    }

    /// Patch a jump target after emitting the jump.
    pub fn patch_jump(&mut self, instr_idx: usize, target: u16) {
        let [lo, hi] = target.to_le_bytes();
        self.code[instr_idx].ops[1] = lo;
        self.code[instr_idx].ops[2] = hi;
    }

    pub fn len(&self) -> usize {
        self.code.len()
    }

    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }

    /// Serialise to flat byte stream: 6 bytes per instruction.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.code.iter().flat_map(|i| i.to_bytes()).collect()
    }
}

impl std::fmt::Display for Chunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "fn {} ({} instructions):", self.name, self.code.len())?;
        for (i, instr) in self.code.iter().enumerate() {
            let op = instr.opcode().map(|o| format!("{}", o)).unwrap_or_else(|| format!("0x{:02X}", instr.opcode));
            writeln!(f, "  {:04}  {} {:?}", i, op, instr.ops)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_and_patch_jump() {
        let mut chunk = Chunk::new("test");
        let jump_idx = chunk.emit_ri16(Opcode::Jmp, 0, 0xFFFF);
        let _ = chunk.emit_rrr(Opcode::Add, 0, 1, 2);
        let target = chunk.len() as u16;
        chunk.patch_jump(jump_idx, target);
        let (_, imm) = chunk.code[jump_idx].ri16();
        assert_eq!(imm, target);
    }

    #[test]
    fn constant_pool_index() {
        let mut chunk = Chunk::new("test");
        let idx = chunk.add_constant(ConstPoolEntry::Int(42));
        assert_eq!(idx, 0);
        assert_eq!(chunk.constants[0], ConstPoolEntry::Int(42));
    }

    #[test]
    fn to_bytes_length() {
        let mut chunk = Chunk::new("test");
        chunk.emit_rrr(Opcode::Add, 0, 1, 2);
        chunk.emit_rrr(Opcode::Ret, 0, 0, 0);
        assert_eq!(chunk.to_bytes().len(), 12); // 2 * 6
    }
}
