// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use super::opcode::Opcode;

/// One QZI instruction: 6 bytes total.
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
        Self {
            opcode: opcode as u8,
            ops,
            flags,
        }
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

    /// FIELD layout: (value/destination register, object register, byte offset).
    /// The fourth operand byte was reserved by the original RRR encoding, so
    /// using both bytes for the offset remains backward-compatible with QZI v1-v5.
    pub fn field(&self) -> (u8, u8, u16) {
        (
            self.ops[0],
            self.ops[1],
            u16::from_le_bytes([self.ops[2], self.ops[3]]),
        )
    }

    pub fn opcode(&self) -> Option<Opcode> {
        Opcode::from_u8(self.opcode)
    }

    pub fn disasm(&self, consts: &[crate::bytecode::chunk::ConstPoolEntry]) -> String {
        use crate::bytecode::chunk::ConstPoolEntry;
        use crate::bytecode::opcode::Opcode;

        // ── colour helpers ────────────────────────────────────────────────────
        fn r(n: u8) -> String {
            format!("\x1b[36mr{n}\x1b[0m")
        }
        fn imm(v: impl std::fmt::Display) -> String {
            format!("\x1b[33m#{v}\x1b[0m")
        }
        fn tgt(t: u16) -> String {
            format!("\x1b[35m@{t}\x1b[0m")
        }
        fn dim(s: &str) -> String {
            format!("\x1b[2m{s}\x1b[0m")
        }
        fn cval(c: &ConstPoolEntry) -> String {
            match c {
                ConstPoolEntry::Int(v) => format!("\x1b[33m{v}\x1b[0m"),
                ConstPoolEntry::Float(v) => format!("\x1b[33m{v}\x1b[0m"),
                ConstPoolEntry::Str(s) => format!("\x1b[32m{s:?}\x1b[0m"),
                ConstPoolEntry::Bytes(bytes) => {
                    format!("\x1b[32mbytes({bytes:?})\x1b[0m")
                }
                ConstPoolEntry::FnAddr(name) => format!("\x1b[36m{name}\x1b[0m"),
                ConstPoolEntry::VtableAddr(tn, tr) => format!("\x1b[35mvtable({tn}::{tr})\x1b[0m"),
                ConstPoolEntry::ForeignSymbol(symbol) => {
                    format!("\x1b[35mforeign({})\x1b[0m", symbol.symbol)
                }
                ConstPoolEntry::ForeignGlobal(global) => {
                    format!("\x1b[35mglobal({})\x1b[0m", global.symbol)
                }
            }
        }

        let Some(op) = self.opcode() else {
            return format!(
                "\x1b[1;31m???\x1b[0m \x1b[2m(0x{:02X}) {:?}\x1b[0m",
                self.opcode, self.ops
            );
        };

        let op_color = match op {
            Opcode::Add
            | Opcode::Sub
            | Opcode::Mul
            | Opcode::Div
            | Opcode::Mod
            | Opcode::Pow
            | Opcode::And
            | Opcode::Or
            | Opcode::Xor
            | Opcode::Neg
            | Opcode::Not
            | Opcode::Inc
            | Opcode::Dec
            | Opcode::Shl
            | Opcode::Shr
            | Opcode::Sar => "\x1b[32m", // green  – arithmetic/logic
            Opcode::Cmp
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
            | Opcode::Jmp
            | Opcode::Ret => "\x1b[33m", // yellow – control flow
            Opcode::CallIdx | Opcode::CallReg | Opcode::CallCReg | Opcode::CallArg => "\x1b[1;33m", // bold yellow – calls
            Opcode::Mov
            | Opcode::MovI
            | Opcode::MovConst
            | Opcode::Dup
            | Opcode::Move
            | Opcode::Drop => "\x1b[34m", // blue   – data movement
            Opcode::Load | Opcode::Store | Opcode::Lea | Opcode::ArrayStore | Opcode::ArrayLoad => {
                "\x1b[35m"
            } // magenta – memory
            Opcode::New
            | Opcode::NewObj
            | Opcode::FieldLoad
            | Opcode::FieldStore
            | Opcode::VtblLoad => "\x1b[35m", // magenta – struct/object
            Opcode::StrLen
            | Opcode::StrConcat
            | Opcode::StrToInt
            | Opcode::StrToFloat
            | Opcode::PrimToStr
            | Opcode::StrAsStr => "\x1b[36m", // cyan   – string ops
            Opcode::Syscall
            | Opcode::CallExt
            | Opcode::Intrinsic
            | Opcode::AtomicAdd
            | Opcode::AtomicCas
            | Opcode::MemFence
            | Opcode::Spawn => "\x1b[31m", // red    – foreign/system
            Opcode::Nop => "\x1b[2m",
            _ => "\x1b[0m", // default colour for unclassified ops
        };
        let plain = format!("{:?}", op).to_lowercase();
        let cop = format!("{op_color}{:<12}\x1b[0m", plain);

        match op {
            Opcode::Nop | Opcode::Ret | Opcode::MemFence => cop,

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
            | Opcode::AtomicCas => {
                let (d, s1, s2) = self.rrr();
                format!("{cop}{}, {}, {}", r(d), r(s1), r(s2))
            }

            Opcode::Mov
            | Opcode::Move
            | Opcode::Dup
            | Opcode::Neg
            | Opcode::Not
            | Opcode::Inc
            | Opcode::Dec
            | Opcode::StrLen
            | Opcode::VtblLoad
            | Opcode::FieldLoad
            | Opcode::StrToInt
            | Opcode::StrToFloat
            | Opcode::PrimToStr
            | Opcode::StrAsStr => {
                let (d, s, _) = self.rrr();
                format!("{cop}{}, {}", r(d), r(s))
            }

            Opcode::Cmp => {
                let (_, s1, s2) = self.rrr();
                format!("{cop}{}, {}", r(s1), r(s2))
            }

            Opcode::FieldStore => {
                let (val, obj, _) = self.rrr();
                format!("{cop}[{}], {}", r(obj), r(val))
            }

            Opcode::ArrayStore => {
                let (val, base, idx) = self.rrr();
                format!("{cop}[{} + {}*8], {}", r(base), r(idx), r(val))
            }

            Opcode::ArrayLoad => {
                let (dst, base, idx) = self.rrr();
                format!("{cop}{}, [{} + {}*8]", r(dst), r(base), r(idx))
            }

            Opcode::CallReg | Opcode::Spawn => {
                let (d, s, _) = self.rrr();
                format!("{cop}{}, {}", r(d), r(s))
            }

            Opcode::CallCReg => {
                let index = u16::from_le_bytes([self.ops[2], self.ops[3]]);
                format!(
                    "{cop}{}, {}, {}",
                    r(self.ops[0]),
                    r(self.ops[1]),
                    dim(&format!("C sig[{index}]"))
                )
            }

            Opcode::MovI => {
                let (d, v) = self.ri16();
                format!("{cop}{}, {}", r(d), imm(v))
            }

            Opcode::MovConst => {
                let (d, idx) = self.ri16();
                let val = consts
                    .get(idx as usize)
                    .map(|c| format!(" {}", cval(c)))
                    .unwrap_or_default();
                format!("{cop}{}, {}{val}", r(d), dim(&format!("const[{idx}]")))
            }

            Opcode::CallIdx => {
                let (d, idx) = self.ri16();
                format!("{cop}{}, \x1b[1;33mfn[{idx}]\x1b[0m", r(d))
            }

            Opcode::CallExt => {
                let (d, idx) = self.ri16();
                let sym = consts
                    .get(idx as usize)
                    .and_then(|c| match c {
                        ConstPoolEntry::Str(s) => Some(format!(" \x1b[32m{s:?}\x1b[0m")),
                        _ => None,
                    })
                    .unwrap_or_default();
                format!(
                    "{cop}{}, {}{sym}  {}",
                    r(d),
                    dim(&format!("ext[{idx}]")),
                    dim(&format!("args={}", self.flags))
                )
            }

            Opcode::Jz | Opcode::Jnz => {
                let (reg, t) = self.ri16();
                if reg == 0 {
                    format!("{cop}{}", tgt(t))
                } else {
                    format!("{cop}{}, {}", r(reg), tgt(t))
                }
            }

            Opcode::Je
            | Opcode::Jne
            | Opcode::Jg
            | Opcode::Jge
            | Opcode::Jl
            | Opcode::Jle
            | Opcode::Ja
            | Opcode::Jb
            | Opcode::Jmp => {
                let (_, t) = self.ri16();
                format!("{cop}{}", tgt(t))
            }

            Opcode::CallArg | Opcode::Drop => {
                format!("{cop}{}", r(self.ops[0]))
            }

            Opcode::Load => {
                let (d, base, off) = self.mem();
                format!("{cop}{}, [{}+{}]", r(d), r(base), dim(&off.to_string()))
            }
            Opcode::Store => {
                let (src, base, off) = self.mem();
                format!("{cop}[{}+{}], {}", r(base), dim(&off.to_string()), r(src))
            }
            Opcode::Lea => {
                let (d, base, off) = self.mem();
                format!("{cop}{}, &[{}+{}]", r(d), r(base), dim(&off.to_string()))
            }

            Opcode::Syscall => {
                let (d, idx) = self.ri16();
                let val = consts
                    .get(idx as usize)
                    .map(|c| match c {
                        ConstPoolEntry::Str(s) => format!(" \x1b[32m{s:?}\x1b[0m"),
                        ConstPoolEntry::Bytes(bytes) => format!(" bytes({bytes:?})"),
                        ConstPoolEntry::Int(n) => format!(" \x1b[33m{n}\x1b[0m"),
                        ConstPoolEntry::Float(f) => format!(" \x1b[33m{f}\x1b[0m"),
                        ConstPoolEntry::FnAddr(name) => format!(" \x1b[36m{name}\x1b[0m"),
                        ConstPoolEntry::VtableAddr(tn, tr) => format!(" vtable({tn}::{tr})"),
                        ConstPoolEntry::ForeignSymbol(symbol) => {
                            format!(" foreign({})", symbol.symbol)
                        }
                        ConstPoolEntry::ForeignGlobal(global) => {
                            format!(" global({})", global.symbol)
                        }
                    })
                    .unwrap_or_default();
                format!(
                    "{cop}{}, {}{val}  {}",
                    r(d),
                    dim(&format!("sys[{idx}]")),
                    dim(&format!("args={}", self.flags))
                )
            }

            Opcode::Intrinsic => {
                let (d, id) = self.ri16();
                let iname = match id {
                    0 => "quazi.write",
                    1 => "quazi.read",
                    2 => "quazi.exit",
                    3 => "quazi.malloc",
                    4 => "quazi.free",
                    5 => "quazi.realloc",
                    6 => "quazi.memcpy",
                    7 => "quazi.memset",
                    8 => "quazi.memmove",
                    9 => "quazi.memcmp",
                    10 => "quazi.strlen",
                    11 => "quazi.stderr_write",
                    12 => "quazi.sleep_ms",
                    13 => "quazi.getenv",
                    14 => "quazi.str_concat",
                    15 => "quazi.int_to_str",
                    16 => "quazi.float_to_str",
                    18 => "quazi.thread.spawn",
                    19 => "quazi.thread.join",
                    20 => "quazi.net.bind_tcp",
                    21 => "quazi.net.connect_tcp",
                    23 => "quazi.str.byte_at",
                    24 => "quazi.str.from_byte",
                    25 => "quazi.print_backtrace",
                    26 => "quazi.os.memory_total",
                    27 => "quazi.os.memory_available",
                    28 => "quazi.os.hostname",
                    29 => "quazi.str.as_ptr",
                    30 => "quazi.os.cpu_name",
                    31 => "quazi.os.windows_build",
                    32 => "quazi.os.windows_product",
                    33 => "quazi.str.cmp",
                    34 => "quazi.str.rune_len",
                    _ => "?",
                };
                format!(
                    "{cop}{}, \x1b[31m{iname}\x1b[0m  {}",
                    r(d),
                    dim(&format!("args={}", self.flags))
                )
            }

            Opcode::New | Opcode::NewObj => {
                let (d, v) = self.ri16();
                format!("{cop}{}, {}", r(d), imm(v))
            }
            _ => format!(
                "{cop}{}, {}, {}, {}",
                r(self.ops[0]),
                r(self.ops[1]),
                r(self.ops[2]),
                imm(self.ops[3] as u16)
            ),
        }
    }

    // ── Serialisation ─────────────────────────────────────────────────────────

    pub fn to_bytes(self) -> [u8; 6] {
        [
            self.opcode,
            self.ops[0],
            self.ops[1],
            self.ops[2],
            self.ops[3],
            self.flags,
        ]
    }

    pub fn from_bytes(b: [u8; 6]) -> Self {
        Self {
            opcode: b[0],
            ops: [b[1], b[2], b[3], b[4]],
            flags: b[5],
        }
    }
}

// ── Builders for common shapes ────────────────────────────────────────────────

/// Flag bit: instruction operands are f64 (use SSE float ops in encoder).
pub const FLOAT_FLAG: u8 = 0x01;
/// A relational jump is the logical inverse of its source comparison.
/// This distinction preserves unordered (NaN) floating-point behavior.
pub const NEGATED_COMPARE_FLAG: u8 = 0x02;

const MEM_WIDTH_SHIFT: u8 = 1;
const MEM_WIDTH_MASK: u8 = 0x06;
const MEM_SIGNED_FLAG: u8 = 0x08;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemWidth {
    Qword = 0,
    Byte = 1,
    Word = 2,
    Dword = 3,
}

impl Instruction {
    pub fn mem_width(&self) -> MemWidth {
        match (self.flags & MEM_WIDTH_MASK) >> MEM_WIDTH_SHIFT {
            1 => MemWidth::Byte,
            2 => MemWidth::Word,
            3 => MemWidth::Dword,
            _ => MemWidth::Qword,
        }
    }

    pub fn mem_signed(&self) -> bool {
        self.flags & MEM_SIGNED_FLAG != 0
    }
}

pub fn rrr(op: Opcode, dst: u8, src1: u8, src2: u8) -> Instruction {
    Instruction::new(op, [dst, src1, src2, 0], 0)
}

pub fn rrr_f(op: Opcode, dst: u8, src1: u8, src2: u8) -> Instruction {
    Instruction::new(op, [dst, src1, src2, 0], FLOAT_FLAG)
}

pub fn field_load_w(
    dst: u8,
    object: u8,
    offset: u16,
    width: MemWidth,
    signed: bool,
) -> Instruction {
    field_load_typed(dst, object, offset, width, signed, false)
}

pub fn field_load(dst: u8, object: u8, offset: u16) -> Instruction {
    field_load_w(dst, object, offset, MemWidth::Qword, false)
}

pub fn call_c_reg(dst: u8, function: u8, signature: u16) -> Instruction {
    let [lo, hi] = signature.to_le_bytes();
    Instruction::new(Opcode::CallCReg, [dst, function, lo, hi], 0)
}

impl Instruction {
    pub fn call_c_reg_parts(&self) -> (u8, u8, u16) {
        (
            self.ops[0],
            self.ops[1],
            u16::from_le_bytes([self.ops[2], self.ops[3]]),
        )
    }
}

pub fn field_load_typed(
    dst: u8,
    object: u8,
    offset: u16,
    width: MemWidth,
    signed: bool,
    float32: bool,
) -> Instruction {
    let flags = ((width as u8) << MEM_WIDTH_SHIFT) | if signed { MEM_SIGNED_FLAG } else { 0 };
    let [offset_lo, offset_hi] = offset.to_le_bytes();
    Instruction::new(
        Opcode::FieldLoad,
        [dst, object, offset_lo, offset_hi],
        flags | if float32 { FLOAT_FLAG } else { 0 },
    )
}

pub fn field_store_w(val: u8, object: u8, offset: u16, width: MemWidth) -> Instruction {
    field_store_typed(val, object, offset, width, false)
}

pub fn field_store(val: u8, object: u8, offset: u16) -> Instruction {
    field_store_w(val, object, offset, MemWidth::Qword)
}

pub fn field_store_typed(
    val: u8,
    object: u8,
    offset: u16,
    width: MemWidth,
    float32: bool,
) -> Instruction {
    let [offset_lo, offset_hi] = offset.to_le_bytes();
    Instruction::new(
        Opcode::FieldStore,
        [val, object, offset_lo, offset_hi],
        ((width as u8) << MEM_WIDTH_SHIFT) | if float32 { FLOAT_FLAG } else { 0 },
    )
}

pub fn ri16(op: Opcode, dst: u8, imm: u16) -> Instruction {
    let [lo, hi] = imm.to_le_bytes();
    Instruction::new(op, [dst, lo, hi, 0], 0)
}

pub fn mem_load(base: u8, dst: u8, offset: i16) -> Instruction {
    mem_load_w(base, dst, offset, MemWidth::Qword, false)
}

pub fn mem_load_w(base: u8, dst: u8, offset: i16, width: MemWidth, signed: bool) -> Instruction {
    let [ol, oh] = offset.to_le_bytes();
    let flags = ((width as u8) << MEM_WIDTH_SHIFT) | if signed { MEM_SIGNED_FLAG } else { 0 };
    Instruction::new(Opcode::Load, [dst, base, ol, oh], flags)
}

pub fn mem_store(base: u8, src: u8, offset: i16) -> Instruction {
    mem_store_w(base, src, offset, MemWidth::Qword)
}

pub fn mem_store_w(base: u8, src: u8, offset: i16, width: MemWidth) -> Instruction {
    let [ol, oh] = offset.to_le_bytes();
    Instruction::new(
        Opcode::Store,
        [src, base, ol, oh],
        (width as u8) << MEM_WIDTH_SHIFT,
    )
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
    fn memory_width_flags_roundtrip() {
        for width in [
            MemWidth::Qword,
            MemWidth::Byte,
            MemWidth::Word,
            MemWidth::Dword,
        ] {
            let load = mem_load_w(2, 0, 0, width, true);
            assert_eq!(load.mem_width(), width);
            assert!(load.mem_signed());
            let store = mem_store_w(2, 0, 0, width);
            assert_eq!(store.mem_width(), width);
            assert!(!store.mem_signed());
        }
        assert_eq!(mem_load(0, 1, 0).flags, 0);
        assert_eq!(mem_store(0, 1, 0).flags, 0);
    }

    #[test]
    fn field_offset_uses_all_sixteen_bits() {
        let load = field_load(3, 4, 0xABCD);
        assert_eq!(load.field(), (3, 4, 0xABCD));
        assert_eq!(Instruction::from_bytes(load.to_bytes()).field().2, 0xABCD);
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
