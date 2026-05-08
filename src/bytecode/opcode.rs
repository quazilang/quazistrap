// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Opcode {
    // 0x00–0x0F  Data movement
    Nop      = 0x00,
    Mov      = 0x01, // dst = src
    MovI     = 0x02, // dst = imm16
    MovConst = 0x03, // dst = constant_pool[idx]

    // 0x10–0x1F  Arithmetic & logic
    Add = 0x10,
    Sub = 0x11,
    Mul = 0x12,
    Div = 0x13,
    Mod = 0x14,
    Neg = 0x15,
    Inc = 0x16,
    Dec = 0x17,
    And = 0x18,
    Or  = 0x19,
    Xor = 0x1A,
    Not = 0x1B,
    Shl = 0x1C,
    Shr = 0x1D, // logical right shift
    Sar = 0x1E, // arithmetic right shift
    Pow = 0x1F, // dst = src1 ** src2 (calls pow/powi at AOT)

    // 0x20–0x2F  Memory & ownership
    Load  = 0x20, // dst = *(base + offset16)
    Store = 0x21, // *(base + offset16) = src
    Lea   = 0x22, // dst = &(base + offset)
    Move  = 0x23, // ownership move — src invalidated after
    Drop  = 0x24, // drop value, run destructor (RAII scope exit)
    Dup   = 0x25, // copy if type allows (Copy types only)

    // 0x30–0x3F  Control flow
    Cmp     = 0x30,
    Jmp     = 0x31,
    Je      = 0x32,
    Jne     = 0x33,
    Jg      = 0x34,
    Jge     = 0x35,
    Jl      = 0x36,
    Jle     = 0x37,
    Ja      = 0x38, // unsigned above
    Jb      = 0x39, // unsigned below
    Jz      = 0x3A,
    Jnz     = 0x3B,
    CallIdx = 0x3C, // call by function table index
    CallReg = 0x3D, // call by address in register
    Ret     = 0x3E,
    CallArg = 0x3F, // push arg register for upcoming CallIdx

    // 0x40–0x4F  Structs & objects
    New        = 0x40, // allocate, size from imm16
    NewObj     = 0x41, // allocate struct instance by type index
    FieldLoad  = 0x42, // dst = obj->field (imm16 byte offset)
    FieldStore = 0x43, // obj->field = src
    VtblLoad   = 0x44, // dst = obj->vtable_ptr

    // 0x50–0x5F  Atomics, threading & foreign calls
    AtomicAdd = 0x50,
    AtomicCas = 0x51, // compare-and-swap
    MemFence  = 0x52,
    Spawn     = 0x53,
    CallExt   = 0x5D, // call external symbol (FFI/API)
    Syscall   = 0x5E, // syscall — RI16: ops[0]=dst, ops[1..2]=syscall_num; flags=arg_count

    // 0x60–0x6F  String operations
    StrLen    = 0x60, // dst = len field of str fat pointer — RRR: ops[0]=dst, ops[1]=src
    StrConcat = 0x61, // dst = concat(src1, src2) → new heap String — RRR
    StrToInt   = 0x62, // parse str → i64  — RR: ops[0]=dst ops[1]=src
    StrToFloat = 0x63, // parse str → f64 — RR
    PrimToStr  = 0x64, // primitive/str → String (heap alloc) — RR
    StrAsStr   = 0x65, // String → str view — RR
}

impl Opcode {
    pub fn from_u8(byte: u8) -> Option<Self> {
        match byte {
            0x00 => Some(Self::Nop),
            0x01 => Some(Self::Mov),
            0x02 => Some(Self::MovI),
            0x03 => Some(Self::MovConst),
            0x10 => Some(Self::Add),
            0x11 => Some(Self::Sub),
            0x12 => Some(Self::Mul),
            0x13 => Some(Self::Div),
            0x14 => Some(Self::Mod),
            0x15 => Some(Self::Neg),
            0x16 => Some(Self::Inc),
            0x17 => Some(Self::Dec),
            0x18 => Some(Self::And),
            0x19 => Some(Self::Or),
            0x1A => Some(Self::Xor),
            0x1B => Some(Self::Not),
            0x1C => Some(Self::Shl),
            0x1D => Some(Self::Shr),
            0x1E => Some(Self::Sar),
            0x1F => Some(Self::Pow),
            0x20 => Some(Self::Load),
            0x21 => Some(Self::Store),
            0x22 => Some(Self::Lea),
            0x23 => Some(Self::Move),
            0x24 => Some(Self::Drop),
            0x25 => Some(Self::Dup),
            0x30 => Some(Self::Cmp),
            0x31 => Some(Self::Jmp),
            0x32 => Some(Self::Je),
            0x33 => Some(Self::Jne),
            0x34 => Some(Self::Jg),
            0x35 => Some(Self::Jge),
            0x36 => Some(Self::Jl),
            0x37 => Some(Self::Jle),
            0x38 => Some(Self::Ja),
            0x39 => Some(Self::Jb),
            0x3A => Some(Self::Jz),
            0x3B => Some(Self::Jnz),
            0x3C => Some(Self::CallIdx),
            0x3D => Some(Self::CallReg),
            0x3E => Some(Self::Ret),
            0x3F => Some(Self::CallArg),
            0x40 => Some(Self::New),
            0x41 => Some(Self::NewObj),
            0x42 => Some(Self::FieldLoad),
            0x43 => Some(Self::FieldStore),
            0x44 => Some(Self::VtblLoad),
            0x50 => Some(Self::AtomicAdd),
            0x51 => Some(Self::AtomicCas),
            0x52 => Some(Self::MemFence),
            0x53 => Some(Self::Spawn),
            0x5D => Some(Self::CallExt),
            0x5E => Some(Self::Syscall),
            0x60 => Some(Self::StrLen),
            0x61 => Some(Self::StrConcat),
            0x62 => Some(Self::StrToInt),
            0x63 => Some(Self::StrToFloat),
            0x64 => Some(Self::PrimToStr),
            0x65 => Some(Self::StrAsStr),
            _ => None,
        }
    }
}

impl std::fmt::Display for Opcode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
