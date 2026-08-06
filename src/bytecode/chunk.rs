// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use super::instruction::Instruction;
use super::instruction::{ri16, rrr};
use super::opcode::Opcode;
use crate::abi::ForeignSymbol;

/// Constant pool value — lives alongside bytecode, referenced by MovConst.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstPoolEntry {
    Int(i64),
    Float(f64),
    Str(String),
    FnAddr(String),
    /// Address of a vtable for a (type, trait) pair — tag 4.
    VtableAddr(String, String),
    /// External C symbol plus its target-neutral ABI signature — tag 5.
    ForeignSymbol(ForeignSymbol),
}

/// A single function's bytecode + its constant pool.
#[derive(Debug, Default, Clone)]
pub struct Chunk {
    pub code: Vec<Instruction>,
    pub constants: Vec<ConstPoolEntry>,
    pub name: String,
    pub param_count: usize,
    pub reg_count: u8,
    pub intrinsic: bool,
    pub variadic: bool,
    /// True when this is an @api function wrapping a C variadic (ends with bare `...`).
    /// Portable call-site metadata records the promoted actual argument types.
    pub c_variadic: bool,
    /// C-facing entry point metadata for a synthetic export adapter chunk.
    pub export: Option<ForeignSymbol>,
}

impl Chunk {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn with_params(name: impl Into<String>, param_count: usize) -> Self {
        Self {
            name: name.into(),
            param_count,
            ..Default::default()
        }
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

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let name_bytes = self.name.as_bytes();
        buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        buf.extend_from_slice(name_bytes);
        buf.extend_from_slice(&(self.param_count as u16).to_le_bytes());
        buf.push(self.reg_count);
        let mut flags = 0u8;
        if self.intrinsic {
            flags |= 1;
        }
        if self.variadic {
            flags |= 2;
        }
        if self.c_variadic {
            flags |= 4;
        }
        if self.export.is_some() {
            flags |= 8;
        }
        buf.push(flags);
        if let Some(export) = &self.export {
            export.encode(&mut buf);
        }
        buf.extend_from_slice(&(self.constants.len() as u16).to_le_bytes());
        for c in &self.constants {
            match c {
                ConstPoolEntry::Int(v) => {
                    buf.push(0);
                    buf.extend_from_slice(&v.to_le_bytes());
                }
                ConstPoolEntry::Float(v) => {
                    buf.push(1);
                    buf.extend_from_slice(&v.to_le_bytes());
                }
                ConstPoolEntry::Str(s) => {
                    buf.push(2);
                    let sb = s.as_bytes();
                    buf.extend_from_slice(&(sb.len() as u16).to_le_bytes());
                    buf.extend_from_slice(sb);
                }
                ConstPoolEntry::FnAddr(name) => {
                    buf.push(3);
                    let nb = name.as_bytes();
                    buf.extend_from_slice(&(nb.len() as u16).to_le_bytes());
                    buf.extend_from_slice(nb);
                }
                ConstPoolEntry::VtableAddr(type_name, trait_name) => {
                    buf.push(4);
                    let tn = type_name.as_bytes();
                    buf.extend_from_slice(&(tn.len() as u16).to_le_bytes());
                    buf.extend_from_slice(tn);
                    let tr = trait_name.as_bytes();
                    buf.extend_from_slice(&(tr.len() as u16).to_le_bytes());
                    buf.extend_from_slice(tr);
                }
                ConstPoolEntry::ForeignSymbol(symbol) => {
                    buf.push(5);
                    symbol.encode(&mut buf);
                }
            }
        }
        buf.extend_from_slice(&(self.code.len() as u32).to_le_bytes());
        buf.extend_from_slice(&self.to_bytes());
        buf
    }
}

pub fn deserialize_qzi(buf: &[u8]) -> Result<Vec<Chunk>, String> {
    use super::instruction::Instruction;

    let mut pos = 0;

    if buf.len() < 4 || &buf[0..4] != QZI_MAGIC.as_slice() {
        return Err("invalid QZI magic".to_string());
    }
    pos += 4;

    if buf.len() <= pos {
        return Err("truncated QZI header".to_string());
    }
    let version = buf[pos];
    if version != 1 && version != 2 && version != 3 {
        return Err(format!("unsupported QZI version {}", version));
    }
    pos += 1;

    if buf.len() < pos + 4 {
        return Err("truncated QZI chunk count".to_string());
    }
    let chunk_count = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
    pos += 4;

    let mut chunks = Vec::with_capacity(chunk_count);
    for _ in 0..chunk_count {
        if buf.len() < pos + 2 {
            return Err("truncated chunk name length".to_string());
        }
        let name_len = u16::from_le_bytes(buf[pos..pos + 2].try_into().unwrap()) as usize;
        pos += 2;

        if buf.len() < pos + name_len {
            return Err("truncated chunk name".to_string());
        }
        let name = String::from_utf8(buf[pos..pos + name_len].to_vec())
            .map_err(|_| "invalid UTF-8 in chunk name".to_string())?;
        pos += name_len;

        let (param_count, reg_count, intrinsic, variadic, c_variadic, has_export) = if version >= 2
        {
            if buf.len() < pos + 4 {
                return Err("truncated chunk params/regs/flags".to_string());
            }
            let pc = u16::from_le_bytes(buf[pos..pos + 2].try_into().unwrap()) as usize;
            let rc = buf[pos + 2];
            let flags = buf[pos + 3];
            pos += 4;
            (
                pc,
                rc,
                (flags & 1) != 0,
                (flags & 2) != 0,
                (flags & 4) != 0,
                (flags & 8) != 0,
            )
        } else {
            (0, 0, false, false, false, false)
        };

        let export = if version >= 3 && has_export {
            Some(ForeignSymbol::decode(buf, &mut pos)?)
        } else {
            None
        };

        if buf.len() < pos + 2 {
            return Err("truncated chunk const count".to_string());
        }
        let const_count = u16::from_le_bytes(buf[pos..pos + 2].try_into().unwrap()) as usize;
        pos += 2;

        let mut constants = Vec::with_capacity(const_count);
        for _ in 0..const_count {
            if buf.len() <= pos {
                return Err("truncated const tag".to_string());
            }
            let tag = buf[pos];
            pos += 1;
            match tag {
                0 => {
                    if buf.len() < pos + 8 {
                        return Err("truncated const int".to_string());
                    }
                    let v = i64::from_le_bytes(buf[pos..pos + 8].try_into().unwrap());
                    pos += 8;
                    constants.push(ConstPoolEntry::Int(v));
                }
                1 => {
                    if buf.len() < pos + 8 {
                        return Err("truncated const float".to_string());
                    }
                    let v = f64::from_le_bytes(buf[pos..pos + 8].try_into().unwrap());
                    pos += 8;
                    constants.push(ConstPoolEntry::Float(v));
                }
                2 => {
                    if buf.len() < pos + 2 {
                        return Err("truncated const str length".to_string());
                    }
                    let str_len =
                        u16::from_le_bytes(buf[pos..pos + 2].try_into().unwrap()) as usize;
                    pos += 2;
                    if buf.len() < pos + str_len {
                        return Err("truncated const str data".to_string());
                    }
                    let s = String::from_utf8(buf[pos..pos + str_len].to_vec())
                        .map_err(|_| "invalid UTF-8 in const str".to_string())?;
                    pos += str_len;
                    constants.push(ConstPoolEntry::Str(s));
                }
                3 => {
                    if buf.len() < pos + 2 {
                        return Err("truncated const fnaddr length".to_string());
                    }
                    let name_len =
                        u16::from_le_bytes(buf[pos..pos + 2].try_into().unwrap()) as usize;
                    pos += 2;
                    if buf.len() < pos + name_len {
                        return Err("truncated const fnaddr data".to_string());
                    }
                    let name = String::from_utf8(buf[pos..pos + name_len].to_vec())
                        .map_err(|_| "invalid UTF-8 in const fnaddr".to_string())?;
                    pos += name_len;
                    constants.push(ConstPoolEntry::FnAddr(name));
                }
                4 => {
                    if buf.len() < pos + 2 {
                        return Err("truncated const vtableaddr type length".to_string());
                    }
                    let tn_len = u16::from_le_bytes(buf[pos..pos + 2].try_into().unwrap()) as usize;
                    pos += 2;
                    if buf.len() < pos + tn_len {
                        return Err("truncated const vtableaddr type data".to_string());
                    }
                    let type_name = String::from_utf8(buf[pos..pos + tn_len].to_vec())
                        .map_err(|_| "invalid UTF-8 in const vtableaddr type".to_string())?;
                    pos += tn_len;
                    if buf.len() < pos + 2 {
                        return Err("truncated const vtableaddr trait length".to_string());
                    }
                    let tr_len = u16::from_le_bytes(buf[pos..pos + 2].try_into().unwrap()) as usize;
                    pos += 2;
                    if buf.len() < pos + tr_len {
                        return Err("truncated const vtableaddr trait data".to_string());
                    }
                    let trait_name = String::from_utf8(buf[pos..pos + tr_len].to_vec())
                        .map_err(|_| "invalid UTF-8 in const vtableaddr trait".to_string())?;
                    pos += tr_len;
                    constants.push(ConstPoolEntry::VtableAddr(type_name, trait_name));
                }
                5 if version >= 3 => {
                    constants.push(ConstPoolEntry::ForeignSymbol(ForeignSymbol::decode(
                        buf, &mut pos,
                    )?));
                }
                _ => return Err(format!("unknown const tag {}", tag)),
            }
        }

        if buf.len() < pos + 4 {
            return Err("truncated instr count".to_string());
        }
        let instr_count = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;

        let instr_bytes = instr_count * 6;
        if buf.len() < pos + instr_bytes {
            return Err("truncated instructions".to_string());
        }
        let mut code = Vec::with_capacity(instr_count);
        for _ in 0..instr_count {
            let mut arr = [0u8; 6];
            arr.copy_from_slice(&buf[pos..pos + 6]);
            code.push(Instruction::from_bytes(arr));
            pos += 6;
        }

        chunks.push(Chunk {
            name,
            param_count,
            reg_count,
            intrinsic,
            variadic,
            c_variadic,
            export,
            constants,
            code,
        });
    }

    Ok(chunks)
}

pub const QZI_MAGIC: &[u8; 4] = b"\x00QZI";
pub const QZI_VERSION: u8 = 3;

pub fn serialize_qzi(chunks: &[Chunk]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(QZI_MAGIC);
    buf.push(QZI_VERSION);
    buf.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
    for chunk in chunks {
        buf.extend_from_slice(&chunk.serialize());
    }
    buf
}

impl std::fmt::Display for Chunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "\x1b[1;36mfn\x1b[0m \x1b[1m{}\x1b[0m  \x1b[2m({} instrs · {} params · {} regs)\x1b[0m",
            self.name,
            self.code.len(),
            self.param_count,
            self.reg_count
        )?;
        if !self.constants.is_empty() {
            writeln!(f, "\x1b[2m  consts:\x1b[0m")?;
            for (i, c) in self.constants.iter().enumerate() {
                let val = match c {
                    ConstPoolEntry::Int(v) => format!("\x1b[33m{v}\x1b[0m"),
                    ConstPoolEntry::Float(v) => format!("\x1b[33m{v}\x1b[0m"),
                    ConstPoolEntry::Str(s) => format!("\x1b[32m{s:?}\x1b[0m"),
                    ConstPoolEntry::FnAddr(name) => format!("\x1b[36m{name}\x1b[0m"),
                    ConstPoolEntry::VtableAddr(tn, tr) => {
                        format!("\x1b[35mvtable({tn}::{tr})\x1b[0m")
                    }
                    ConstPoolEntry::ForeignSymbol(symbol) => {
                        format!("\x1b[35mforeign({})\x1b[0m", symbol.symbol)
                    }
                };
                writeln!(f, "  \x1b[2m[{i:>2}]\x1b[0m  {val}")?;
            }
        }

        // Pass 1: annotate each callarg with (arg_index, callee_name).
        // callarg* sequences are always immediately followed by callidx/callext/callreg.
        let mut callarg_info: Vec<Option<(usize, String)>> = vec![None; self.code.len()];
        {
            let mut pending: Vec<usize> = Vec::new();
            for (i, instr) in self.code.iter().enumerate() {
                match instr.opcode() {
                    Some(Opcode::CallArg) => pending.push(i),
                    Some(Opcode::CallIdx) => {
                        let (_, idx) = instr.ri16();
                        let callee = format!("fn[{}]", idx);
                        for (pos, &pi) in pending.iter().enumerate() {
                            callarg_info[pi] = Some((pos, callee.clone()));
                        }
                        pending.clear();
                    }
                    Some(Opcode::CallExt) => {
                        let (_, idx) = instr.ri16();
                        let callee = self
                            .constants
                            .get(idx as usize)
                            .and_then(|c| {
                                if let ConstPoolEntry::Str(s) = c {
                                    Some(s.clone())
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_else(|| format!("ext[{}]", idx));
                        for (pos, &pi) in pending.iter().enumerate() {
                            callarg_info[pi] = Some((pos, callee.clone()));
                        }
                        pending.clear();
                    }
                    Some(Opcode::CallReg) => {
                        let (_, src, _) = instr.rrr();
                        let callee = format!("r{}", src);
                        for (pos, &pi) in pending.iter().enumerate() {
                            callarg_info[pi] = Some((pos, callee.clone()));
                        }
                        pending.clear();
                    }
                    _ => {
                        pending.clear();
                    }
                }
            }
        }

        // Pass 2: display with inline annotations.
        for (i, instr) in self.code.iter().enumerate() {
            let line = instr.disasm(&self.constants);
            if let Some((pos, callee)) = &callarg_info[i] {
                writeln!(
                    f,
                    "  \x1b[2m{i:04} │\x1b[0m  {line}  \x1b[2m; arg {pos} → {callee}\x1b[0m"
                )?;
            } else {
                writeln!(f, "  \x1b[2m{i:04} │\x1b[0m  {line}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::{AbiSignature, AbiType, ForeignSymbol};

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

    #[test]
    fn qzi_v3_roundtrips_foreign_abi_metadata() {
        let signature = AbiSignature {
            params: vec![AbiType::Float64],
            return_type: AbiType::Float64,
            variadic: false,
        };
        let mut chunk = Chunk::with_params("sin_adapter", 1);
        chunk.export = Some(ForeignSymbol {
            symbol: "quazi_sin".to_string(),
            signature: signature.clone(),
        });
        chunk
            .constants
            .push(ConstPoolEntry::ForeignSymbol(ForeignSymbol {
                symbol: "sin".to_string(),
                signature,
            }));
        chunk.emit_rrr(Opcode::Ret, 0, 0, 0);

        let encoded = serialize_qzi(&[chunk]);
        let decoded = deserialize_qzi(&encoded).expect("QZI v3 should decode");
        assert_eq!(decoded[0].export.as_ref().unwrap().symbol, "quazi_sin");
        assert!(matches!(
            decoded[0].constants.as_slice(),
            [ConstPoolEntry::ForeignSymbol(symbol)] if symbol.symbol == "sin"
        ));
    }
}
