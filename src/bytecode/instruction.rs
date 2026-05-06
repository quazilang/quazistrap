// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use super::opcode::Opcode;

/// One VBC instruction: 6 bytes total.
///
/// Layout:
///   [byte 0]     opcode
///   [bytes 1–4]  operands (32 bits, layout varies by opcode group)
///   [byte 5]     flags / reserved
///
/// Operand layouts (32 bits = ops[0..4]):
///   RRR  : ops[0]=dst  ops[1]=src1  ops[2]=src2  ops[3]=flags
///   RRI  : ops[0]=dst  ops[1]=src   ops[2]=imm8
///   RI16 : ops[0]=dst  ops[1..2]=imm16 (LE)
///   MEM  : ops[0]=value_reg  ops[1]=base_reg  ops[2..3]=offset16 (LE, signed)
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction {
    pub opcode: u8,
    pub ops: [u8; 4],
    pub flags: u8,
}

impl Instruction {
    pub const SIZE: usize = 6;

    pub fn new(opcode: Opcode, ops: [u8; 4], flags: u8) -> Self {
        Self { opcode: opcode as u8, ops, flags }
    }

    pub fn nop() -> Self {
        Self::new(Opcode::Nop, [0; 4], 0)
    }

    // ── Operand decoders ──────────────────────────────────────────────────────

    /// RRR layout: (dst, src1, src2)
    pub fn rrr(&self) -> (u8, u8, u8) {
        (self.ops[0], self.ops[1], self.ops[2])
    }

    /// RRI layout: (dst, src, imm8)
    pub fn rri(&self) -> (u8, u8, u8) {
        (self.ops[0], self.ops[1], self.ops[2])
    }

    /// RI16 layout: (dst, imm16)
    pub fn ri16(&self) -> (u8, u16) {
        let imm = u16::from_le_bytes([self.ops[1], self.ops[2]]);
        (self.ops[0], imm)
    }

    /// MEM layout: (value_reg, base_reg, offset16_signed)
    pub fn mem(&self) -> (u8, u8, i16) {
        let offset = i16::from_le_bytes([self.ops[2], self.ops[3]]);
        (self.ops[0], self.ops[1], offset)
    }

    pub fn opcode(&self) -> Option<Opcode> {
        Opcode::from_u8(self.opcode)
    }

    pub fn disasm(&self, consts: &[crate::bytecode::chunk::ConstPoolEntry]) -> String {
        use crate::bytecode::opcode::Opcode;
        use crate::bytecode::chunk::ConstPoolEntry;

        let Some(op) = self.opcode() else {
            return format!("??? (0x{:02X})  {:?}", self.opcode, self.ops);
        };

        let name = format!("{:?}", op).to_lowercase();
        let pad = |s: &str| format!("{:<12}", s);

        match op {
            Opcode::Nop | Opcode::Ret | Opcode::MemFence => pad(&name),

            // RRR — dst, src1, src2
            Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Mod
            | Opcode::And | Opcode::Or | Opcode::Xor | Opcode::Shl | Opcode::Shr | Opcode::Sar
            | Opcode::StrConcat | Opcode::AtomicAdd | Opcode::AtomicCas => {
                let (d, s1, s2) = self.rrr();
                format!("{}r{}, r{}, r{}", pad(&name), d, s1, s2)
            }

            // RR — dst, src only
            Opcode::Mov | Opcode::Move | Opcode::Dup | Opcode::Neg | Opcode::Not
            | Opcode::Inc | Opcode::Dec | Opcode::StrLen | Opcode::VtblLoad | Opcode::FieldLoad
            | Opcode::StrToInt | Opcode::StrToFloat | Opcode::PrimToStr | Opcode::StrAsStr => {
                let (d, s, _) = self.rrr();
                format!("{}r{}, r{}", pad(&name), d, s)
            }

            // Cmp: no dst
            Opcode::Cmp => {
                let (_, s1, s2) = self.rrr();
                format!("{}r{}, r{}", pad("cmp"), s1, s2)
            }

            // FieldStore: [obj] = val
            Opcode::FieldStore => {
                let (val, obj, _) = self.rrr();
                format!("{}[r{}], r{}", pad("fieldstore"), obj, val)
            }

            // CallReg: dst, reg
            Opcode::CallReg => {
                let (d, s, _) = self.rrr();
                format!("{}r{}, r{}", pad(&name), d, s)
            }

            // MovI: dst, #imm
            Opcode::MovI => {
                let (d, imm) = self.ri16();
                format!("{}r{}, #{}", pad("movi"), d, imm)
            }

            // MovConst: dst, const[idx]=val
            Opcode::MovConst => {
                let (d, idx) = self.ri16();
                let val = consts.get(idx as usize).map(|c| match c {
                    ConstPoolEntry::Int(v) => format!("={}", v),
                    ConstPoolEntry::Float(v) => format!("={}", v),
                    ConstPoolEntry::Str(s) => format!("={:?}", s),
                }).unwrap_or_default();
                format!("{}r{}, const[{}]{}", pad("movconst"), d, idx, val)
            }

            // CallIdx: dst, fn[idx]
            Opcode::CallIdx => {
                let (d, idx) = self.ri16();
                format!("{}r{}, fn[{}]", pad("callidx"), d, idx)
            }

            // CallExt: dst, ext[idx]
            Opcode::CallExt => {
                let (d, idx) = self.ri16();
                let val = consts.get(idx as usize).and_then(|c| match c {
                    ConstPoolEntry::Str(s) => Some(format!("={:?}", s)),
                    _ => None,
                }).unwrap_or_default();
                format!("{}r{}, ext[{}]{} (args={})", pad("callext"), d, idx, val, self.flags)
            }

            // Jumps — Jz/Jnz check reg vs 0
            Opcode::Jz | Opcode::Jnz => {
                let (r, tgt) = self.ri16();
                if r == 0 {
                    format!("{}@{}", pad(&name), tgt)
                } else {
                    format!("{}r{}, @{}", pad(&name), r, tgt)
                }
            }

            // Other conditional jumps (after Cmp, ops[0]=0)
            Opcode::Je | Opcode::Jne | Opcode::Jg | Opcode::Jge | Opcode::Jl | Opcode::Jle
            | Opcode::Ja | Opcode::Jb | Opcode::Jmp => {
                let (_, tgt) = self.ri16();
                format!("{}@{}", pad(&name), tgt)
            }

            // CallArg / Drop: single reg
            Opcode::CallArg | Opcode::Drop => {
                format!("{}r{}", pad(&name), self.ops[0])
            }

            // MEM
            Opcode::Load => {
                let (d, base, off) = self.mem();
                format!("{}r{}, [r{} + {}]", pad("load"), d, base, off)
            }
            Opcode::Store => {
                let (src, base, off) = self.mem();
                format!("{}[r{} + {}], r{}", pad("store"), base, off, src)
            }
            Opcode::Lea => {
                let (d, base, off) = self.mem();
                format!("{}r{}, &[r{} + {}]", pad("lea"), d, base, off)
            }

            // Syscall: dst, sys#num (args=flags)
            Opcode::Syscall => {
                let (d, num) = self.ri16();
                format!("{}r{}, sys#{} (args={})", pad("syscall"), d, num, self.flags)
            }

            // New/NewObj
            Opcode::New | Opcode::NewObj => {
                let (d, imm) = self.ri16();
                format!("{}r{}, #{}", pad(&name), d, imm)
            }

            // Spawn
            Opcode::Spawn => {
                let (d, s, _) = self.rrr();
                format!("{}r{}, r{}", pad("spawn"), d, s)
            }
        }
    }

    // ── Serialisation ─────────────────────────────────────────────────────────

    pub fn to_bytes(self) -> [u8; 6] {
        [self.opcode, self.ops[0], self.ops[1], self.ops[2], self.ops[3], self.flags]
    }

    pub fn from_bytes(b: [u8; 6]) -> Self {
        Self { opcode: b[0], ops: [b[1], b[2], b[3], b[4]], flags: b[5] }
    }
}

// ── Builders for common shapes ────────────────────────────────────────────────

pub fn rrr(op: Opcode, dst: u8, src1: u8, src2: u8) -> Instruction {
    Instruction::new(op, [dst, src1, src2, 0], 0)
}

pub fn ri16(op: Opcode, dst: u8, imm: u16) -> Instruction {
    let [lo, hi] = imm.to_le_bytes();
    Instruction::new(op, [dst, lo, hi, 0], 0)
}

pub fn mem_load(base: u8, dst: u8, offset: i16) -> Instruction {
    let [ol, oh] = offset.to_le_bytes();
    Instruction::new(Opcode::Load, [dst, base, ol, oh], 0)
}

pub fn mem_store(base: u8, src: u8, offset: i16) -> Instruction {
    let [ol, oh] = offset.to_le_bytes();
    Instruction::new(Opcode::Store, [src, base, ol, oh], 0)
}

pub fn mem_lea(base: u8, dst: u8, offset: i16) -> Instruction {
    let [ol, oh] = offset.to_le_bytes();
    Instruction::new(Opcode::Lea, [dst, base, ol, oh], 0)
}

pub fn jmp(target_idx: u16) -> Instruction {
    ri16(Opcode::Jmp, 0, target_idx)
}

pub fn call_idx(fn_idx: u16) -> Instruction {
    ri16(Opcode::CallIdx, 0, fn_idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instruction_is_six_bytes() {
        assert_eq!(std::mem::size_of::<Instruction>(), 6);
    }

    #[test]
    fn roundtrip_bytes() {
        let instr = rrr(Opcode::Add, 1, 2, 3);
        assert_eq!(Instruction::from_bytes(instr.to_bytes()), instr);
    }

    #[test]
    fn ri16_encodes_and_decodes_imm() {
        let instr = ri16(Opcode::MovI, 5, 0xABCD);
        let (dst, imm) = instr.ri16();
        assert_eq!(dst, 5);
        assert_eq!(imm, 0xABCD);
    }

    #[test]
    fn mem_load_encodes_signed_offset() {
        let instr = mem_load(2, 0, -8);
        let (dst, base, off) = instr.mem();
        assert_eq!(dst, 0);
        assert_eq!(base, 2);
        assert_eq!(off, -8);
    }

    #[test]
    fn opcode_roundtrip_all_defined() {
        for byte in 0u8..=0xFF {
            if let Some(op) = Opcode::from_u8(byte) {
                assert_eq!(op as u8, byte);
            }
        }
    }
}
