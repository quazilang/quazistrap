// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use super::instruction::Instruction;
use super::instruction::{ri16, rrr};
use super::opcode::Opcode;
use crate::abi::{ForeignGlobal, ForeignSymbol};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QziModuleKind {
    Executable,
    Library,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QziMetadata {
    pub name: String,
    pub version: Option<String>,
    pub kind: QziModuleKind,
    pub main_takes_args: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QziCallRelocation {
    pub chunk_index: u32,
    pub instruction_index: u32,
    pub symbol: String,
}

#[derive(Debug, Clone)]
pub struct QziModule {
    pub metadata: QziMetadata,
    /// Quazi declarations exposed to source consumers. Sectioned QZI keeps this as
    /// UTF-8 so newer compilers can reject unsupported syntax explicitly.
    pub interface: String,
    pub call_relocations: Vec<QziCallRelocation>,
    pub chunks: Vec<Chunk>,
}

impl QziModule {
    pub fn from_chunks(chunks: Vec<Chunk>) -> Self {
        let kind = if chunks.iter().any(|chunk| chunk.name == "main") {
            QziModuleKind::Executable
        } else {
            QziModuleKind::Library
        };
        Self {
            metadata: QziMetadata {
                name: String::new(),
                version: None,
                kind,
                main_takes_args: false,
            },
            interface: String::new(),
            call_relocations: infer_call_relocations(&chunks).unwrap_or_default(),
            chunks,
        }
    }

    pub fn qualify_library_root_symbols(&mut self) {
        if self.metadata.kind != QziModuleKind::Library || self.metadata.name.is_empty() {
            return;
        }
        let mut renamed = std::collections::HashMap::new();
        for chunk in &mut self.chunks {
            if !chunk.name.contains('.') {
                let old = chunk.name.clone();
                chunk.name = format!("{}.{}", self.metadata.name, old);
                renamed.insert(old, chunk.name.clone());
            }
        }
        for chunk in &mut self.chunks {
            for constant in &mut chunk.constants {
                if let ConstPoolEntry::FnAddr(name) = constant
                    && let Some(replacement) = renamed.get(name)
                {
                    *name = replacement.clone();
                }
            }
        }
        self.call_relocations = infer_call_relocations(&self.chunks).unwrap_or_default();
    }

    pub fn alias_library_namespace(&mut self, alias: &str) {
        let original = self.metadata.name.clone();
        if self.metadata.kind != QziModuleKind::Library || original.is_empty() || original == alias
        {
            return;
        }
        let rename = |symbol: &str| {
            if symbol == original {
                alias.to_string()
            } else {
                symbol
                    .strip_prefix(&format!("{original}."))
                    .map(|suffix| format!("{alias}.{suffix}"))
                    .unwrap_or_else(|| symbol.to_string())
            }
        };
        self.metadata.name = alias.to_string();
        for chunk in &mut self.chunks {
            chunk.name = rename(&chunk.name);
            for constant in &mut chunk.constants {
                if let ConstPoolEntry::FnAddr(symbol) = constant {
                    *symbol = rename(symbol);
                }
            }
        }
        for relocation in &mut self.call_relocations {
            relocation.symbol = rename(&relocation.symbol);
        }
        if let Ok(mut bundle) = crate::bytecode::interface::parse_qzi_interface(&self.interface) {
            for module in &mut bundle.modules {
                module.name = rename(&module.name);
            }
            if let Ok(interface) = toml::to_string(&bundle) {
                self.interface = interface;
            }
        }
    }
}

/// Constant pool value — lives alongside bytecode, referenced by MovConst.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstPoolEntry {
    Int(i64),
    Float(f64),
    Str(String),
    /// Exact bytes, stored with a u64 length prefix in native read-only data.
    Bytes(Vec<u8>),
    FnAddr(String),
    /// Address of a vtable for a (type, trait) pair — tag 4.
    VtableAddr(String, String),
    /// External C symbol plus its target-neutral ABI signature — tag 5.
    ForeignSymbol(ForeignSymbol),
    /// Addressable external C data symbol and its source-level ABI type — tag 7.
    ForeignGlobal(ForeignGlobal),
}

/// A single function's bytecode + its constant pool.
#[derive(Debug, Default, Clone, PartialEq)]
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
    /// Emit this package function under its source-level name in native objects.
    /// This bit is stored in QZI so incremental and library builds preserve it.
    pub native_unmangled: bool,
    /// C-facing entry point metadata for a synthetic export adapter chunk.
    pub export: Option<ForeignSymbol>,
    /// Set when a caller attempted to allocate an unencodable constant index.
    /// Validation turns this into a compile error before bytes are emitted.
    constant_pool_overflowed: bool,
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
        if self.constants.len() >= u16::MAX as usize {
            self.constant_pool_overflowed = true;
            return 0;
        }
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
        if self.native_unmangled {
            flags |= 16;
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
                ConstPoolEntry::Bytes(bytes) => {
                    buf.push(6);
                    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                    buf.extend_from_slice(bytes);
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
                ConstPoolEntry::ForeignGlobal(global) => {
                    buf.push(7);
                    global.encode(&mut buf);
                }
            }
        }
        buf.extend_from_slice(&(self.code.len() as u32).to_le_bytes());
        buf.extend_from_slice(&self.to_bytes());
        buf
    }
}

fn deserialize_qzi_legacy(buf: &[u8]) -> Result<Vec<Chunk>, String> {
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
    if version == 1 {
        return Err(
            "QZI version 1 cannot be loaded safely because its writer omitted parameter and register-frame metadata; rebuild the artifact from source"
                .to_string(),
        );
    }
    if !matches!(version, 2..=5) {
        return Err(format!("unsupported QZI version {}", version));
    }
    pos += 1;

    if buf.len() < pos + 4 {
        return Err("truncated QZI chunk count".to_string());
    }
    let chunk_count = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
    pos += 4;

    let min_chunk_bytes = if version >= 3 { 12 } else { 11 };
    if chunk_count > buf.len().saturating_sub(pos) / min_chunk_bytes {
        return Err("QZI chunk count exceeds remaining file size".to_string());
    }
    let mut chunks = Vec::new();
    chunks
        .try_reserve_exact(chunk_count)
        .map_err(|_| "QZI chunk count is too large".to_string())?;
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

        let (param_count, reg_count, intrinsic, variadic, c_variadic, has_export, native_unmangled) =
            if version >= 2 {
                // v2 writes only param_count + reg_count; the chunk flags byte
                // (intrinsic/variadic/export marks) was introduced with v3, so
                // v2 artifacts default to no flags.
                let header_len = if version >= 3 { 4 } else { 3 };
                if buf.len() < pos + header_len {
                    return Err("truncated chunk params/regs/flags".to_string());
                }
                let pc = u16::from_le_bytes(buf[pos..pos + 2].try_into().unwrap()) as usize;
                if pc > u8::MAX as usize {
                    return Err(format!("QZI chunk parameter count {pc} exceeds 255"));
                }
                let rc = buf[pos + 2];
                let flags = if version >= 3 { buf[pos + 3] } else { 0 };
                pos += header_len;
                (
                    pc,
                    rc,
                    (flags & 1) != 0,
                    (flags & 2) != 0,
                    (flags & 4) != 0,
                    (flags & 8) != 0,
                    (flags & 16) != 0,
                )
            } else {
                (0, 0, false, false, false, false, false)
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

        if const_count > buf.len().saturating_sub(pos) {
            return Err("QZI constant count exceeds remaining file size".to_string());
        }
        let mut constants = Vec::new();
        constants
            .try_reserve_exact(const_count)
            .map_err(|_| "QZI constant count is too large".to_string())?;
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
                6 if version >= 4 => {
                    if buf.len() < pos + 4 {
                        return Err("truncated const bytes length".to_string());
                    }
                    let bytes_len =
                        u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
                    pos += 4;
                    if buf.len() < pos + bytes_len {
                        return Err("truncated const bytes data".to_string());
                    }
                    constants.push(ConstPoolEntry::Bytes(buf[pos..pos + bytes_len].to_vec()));
                    pos += bytes_len;
                }
                7 if version >= 5 => {
                    constants.push(ConstPoolEntry::ForeignGlobal(ForeignGlobal::decode(
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

        let instr_bytes = instr_count
            .checked_mul(Instruction::SIZE)
            .ok_or_else(|| "QZI instruction byte count overflow".to_string())?;
        if buf.len() < pos + instr_bytes {
            return Err("truncated instructions".to_string());
        }
        let mut code = Vec::new();
        code.try_reserve_exact(instr_count)
            .map_err(|_| "QZI instruction count is too large".to_string())?;
        for _ in 0..instr_count {
            let mut arr = [0u8; 6];
            arr.copy_from_slice(&buf[pos..pos + 6]);
            let instruction = Instruction::from_bytes(arr);
            if instruction.opcode().is_none() {
                return Err(format!("unknown QZI opcode 0x{:02x}", instruction.opcode));
            }
            code.push(instruction);
            pos += 6;
        }

        chunks.push(Chunk {
            name,
            param_count,
            reg_count,
            intrinsic,
            variadic,
            c_variadic,
            native_unmangled,
            export,
            constants,
            code,
            constant_pool_overflowed: false,
        });
    }

    if pos != buf.len() {
        return Err(format!("QZI has {} trailing byte(s)", buf.len() - pos));
    }
    validate_qzi_chunks(&chunks)?;
    Ok(chunks)
}

pub(crate) fn validate_qzi_chunks(chunks: &[Chunk]) -> Result<(), String> {
    use super::opcode::Opcode;

    for (chunk_index, chunk) in chunks.iter().enumerate() {
        if chunk.constant_pool_overflowed {
            return Err(format!(
                "chunk `{}` exceeded the QZI constant-pool limit",
                chunk.name
            ));
        }
        if chunk.param_count > u8::MAX as usize {
            return Err(format!("chunk `{}` has too many parameters", chunk.name));
        }
        if chunk.param_count > chunk.reg_count as usize {
            return Err(format!(
                "chunk `{}` has fewer register slots than parameters",
                chunk.name
            ));
        }
        for (instruction_index, instruction) in chunk.code.iter().enumerate() {
            let opcode = instruction
                .opcode()
                .ok_or_else(|| format!("unknown opcode in chunk `{}`", chunk.name))?;
            let fail = |message: &str| {
                Err(format!(
                    "invalid QZI chunk `{}` instruction {}: {}",
                    chunk.name, instruction_index, message
                ))
            };
            if opcode == Opcode::Lea
                && instruction.flags != 0
                && instruction.ops[1]
                    .checked_add(instruction.flags - 1)
                    .is_none()
            {
                return fail("contiguous register block wraps past r255");
            }
            for register in crate::bytecode::regalloc::instruction_registers(instruction) {
                // A zero-register void function still carries `Ret r0` by the
                // historical QZI convention. The backend reserves that return slot.
                let legacy_void_return =
                    chunk.reg_count == 0 && opcode == Opcode::Ret && register == 0;
                if !legacy_void_return && register as usize >= chunk.reg_count as usize {
                    return fail(&format!(
                        "register r{register} is outside the declared frame ({} slots)",
                        chunk.reg_count
                    ));
                }
            }
            if matches!(
                opcode,
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
                    | Opcode::Jnz
            ) && instruction.ri16().1 as usize > chunk.code.len()
            {
                return fail("jump target is outside the chunk");
            }
            if matches!(opcode, Opcode::MovConst | Opcode::Syscall | Opcode::CallExt)
                && instruction.ri16().1 as usize >= chunk.constants.len()
            {
                return fail("constant-pool index is out of bounds");
            }
            if opcode == Opcode::CallExt {
                let constant = &chunk.constants[instruction.ri16().1 as usize];
                if !matches!(
                    constant,
                    ConstPoolEntry::Str(_) | ConstPoolEntry::ForeignSymbol(_)
                ) {
                    return fail("external call metadata has the wrong constant-pool kind");
                }
            }
            if opcode == Opcode::Syscall {
                let constant = &chunk.constants[instruction.ri16().1 as usize];
                let valid = match constant {
                    ConstPoolEntry::Str(_) => true,
                    ConstPoolEntry::Int(value) => *value >= 0,
                    _ => false,
                };
                if !valid {
                    return fail("syscall metadata must be a name or non-negative number");
                }
            }
            if opcode == Opcode::CallCReg
                && instruction.call_c_reg_parts().2 as usize >= chunk.constants.len()
            {
                return fail("C callback signature index is out of bounds");
            }
            if opcode == Opcode::CallCReg {
                let constant = &chunk.constants[instruction.call_c_reg_parts().2 as usize];
                if !matches!(constant, ConstPoolEntry::ForeignSymbol(_)) {
                    return fail("C callback signature metadata has the wrong constant-pool kind");
                }
            }
            if opcode == Opcode::CallIdx && instruction.ri16().1 as usize >= chunks.len() {
                return fail("function-table index is out of bounds");
            }
            if matches!(opcode, Opcode::Intrinsic | Opcode::Syscall) {
                let start = instruction.ops[0] as usize;
                let count = instruction.flags as usize;
                if start + count > u8::MAX as usize {
                    return fail("consecutive argument register range overflows QZI slots");
                }
            }
            if opcode == Opcode::Intrinsic {
                let id = instruction.ri16().1;
                if !matches!(id, 0..=36) {
                    return fail("unknown intrinsic id");
                }
            }
        }
        let _ = chunk_index;
    }
    Ok(())
}

pub const QZI_MAGIC: &[u8; 4] = b"\x00QZI";
pub const QZI_VERSION: u8 = 8;
const QZI_LEGACY_VERSION: u8 = 5;
const QZI_FIRST_SECTIONED_VERSION: u8 = 6;

fn chunks_use_preownership_function_values(chunks: &[Chunk]) -> bool {
    chunks.iter().any(|chunk| {
        chunk.name.starts_with("__quazi_closure_")
            || chunk.name.starts_with("__quazi_fwd_")
            || chunk.constants.iter().any(
                |constant| matches!(constant, ConstPoolEntry::FnAddr(name) if name.starts_with("__quazi_closure_") || name.starts_with("__quazi_fwd_")),
            )
    })
}

fn serialize_qzi_legacy(chunks: &[Chunk]) -> Result<Vec<u8>, String> {
    validate_qzi_chunks(chunks)?;
    if chunks.len() > u32::MAX as usize {
        return Err("too many QZI chunks".to_string());
    }
    for chunk in chunks {
        if chunk.name.len() > u16::MAX as usize {
            return Err(format!("QZI chunk name `{}` is too long", chunk.name));
        }
        if chunk.param_count > u16::MAX as usize {
            return Err(format!(
                "QZI chunk `{}` has too many parameters",
                chunk.name
            ));
        }
        if chunk.constants.len() > u16::MAX as usize {
            return Err(format!("QZI chunk `{}` has too many constants", chunk.name));
        }
        if chunk.code.len() > u32::MAX as usize {
            return Err(format!(
                "QZI chunk `{}` has too many instructions",
                chunk.name
            ));
        }
        for constant in &chunk.constants {
            match constant {
                ConstPoolEntry::Str(value) | ConstPoolEntry::FnAddr(value)
                    if value.len() > u16::MAX as usize =>
                {
                    return Err(format!(
                        "QZI string constant in `{}` is too long",
                        chunk.name
                    ));
                }
                ConstPoolEntry::VtableAddr(type_name, trait_name)
                    if type_name.len() > u16::MAX as usize
                        || trait_name.len() > u16::MAX as usize =>
                {
                    return Err(format!("QZI vtable name in `{}` is too long", chunk.name));
                }
                ConstPoolEntry::Bytes(value) if value.len() > u32::MAX as usize => {
                    return Err(format!(
                        "QZI byte constant in `{}` is too large",
                        chunk.name
                    ));
                }
                ConstPoolEntry::ForeignSymbol(symbol) => validate_foreign_symbol(symbol)?,
                ConstPoolEntry::ForeignGlobal(global) => {
                    if global.symbol.len() > u16::MAX as usize {
                        return Err("QZI foreign-global symbol is too long".to_string());
                    }
                    validate_abi_type(&global.ty, 0)?;
                }
                _ => {}
            }
        }
        if let Some(export) = &chunk.export {
            validate_foreign_symbol(export)?;
        }
    }
    let mut buf = Vec::new();
    buf.extend_from_slice(QZI_MAGIC);
    buf.push(QZI_LEGACY_VERSION);
    buf.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
    for chunk in chunks {
        buf.extend_from_slice(&chunk.serialize());
    }
    Ok(buf)
}

const QZI_SECTION_METADATA: u8 = 1;
const QZI_SECTION_INTERFACE: u8 = 2;
const QZI_SECTION_CALL_RELOCATIONS: u8 = 3;
const QZI_SECTION_BYTECODE: u8 = 4;
const QZI_SECTION_ENTRY_SIZE: usize = 12;

fn infer_call_relocations(chunks: &[Chunk]) -> Result<Vec<QziCallRelocation>, String> {
    let mut relocations = Vec::new();
    for (chunk_index, chunk) in chunks.iter().enumerate() {
        for (instruction_index, instruction) in chunk.code.iter().enumerate() {
            if instruction.opcode == Opcode::CallIdx as u8 {
                let target = instruction.ri16().1 as usize;
                let target_chunk = chunks.get(target).ok_or_else(|| {
                    format!(
                        "QZI call in `{}` references missing function index {}",
                        chunk.name, target
                    )
                })?;
                relocations.push(QziCallRelocation {
                    chunk_index: u32::try_from(chunk_index)
                        .map_err(|_| "too many QZI chunks".to_string())?,
                    instruction_index: u32::try_from(instruction_index)
                        .map_err(|_| "QZI function has too many instructions".to_string())?,
                    symbol: target_chunk.name.clone(),
                });
            }
        }
    }
    Ok(relocations)
}

fn encode_qzi_metadata(metadata: &QziMetadata) -> Result<Vec<u8>, String> {
    if metadata.kind == QziModuleKind::Library && metadata.main_takes_args {
        return Err("QZI library cannot declare an executable entry signature".to_string());
    }
    if metadata.name.len() > u16::MAX as usize {
        return Err("QZI module name is too long".to_string());
    }
    let version = metadata.version.as_deref().unwrap_or("");
    if version.len() > u16::MAX as usize {
        return Err("QZI module version is too long".to_string());
    }
    let mut bytes = Vec::new();
    bytes.push(match metadata.kind {
        QziModuleKind::Executable => 0,
        QziModuleKind::Library => 1,
    });
    bytes.push(u8::from(metadata.main_takes_args));
    bytes.extend_from_slice(&(metadata.name.len() as u16).to_le_bytes());
    bytes.extend_from_slice(metadata.name.as_bytes());
    bytes.extend_from_slice(&(version.len() as u16).to_le_bytes());
    bytes.extend_from_slice(version.as_bytes());
    Ok(bytes)
}

fn decode_qzi_metadata(bytes: &[u8]) -> Result<QziMetadata, String> {
    let mut pos = 0usize;
    let kind = match read_u8(bytes, &mut pos, "QZI module kind")? {
        0 => QziModuleKind::Executable,
        1 => QziModuleKind::Library,
        value => return Err(format!("unknown QZI module kind {value}")),
    };
    let main_takes_args = match read_u8(bytes, &mut pos, "QZI entry flags")? {
        0 => false,
        1 => true,
        value => return Err(format!("invalid QZI entry flags {value}")),
    };
    let name = read_short_string(bytes, &mut pos, "QZI module name")?;
    let version = read_short_string(bytes, &mut pos, "QZI module version")?;
    if pos != bytes.len() {
        return Err("QZI metadata has trailing bytes".to_string());
    }
    Ok(QziMetadata {
        name,
        version: (!version.is_empty()).then_some(version),
        kind,
        main_takes_args,
    })
}

fn encode_call_relocations(relocations: &[QziCallRelocation]) -> Result<Vec<u8>, String> {
    if relocations.len() > u32::MAX as usize {
        return Err("too many QZI call relocations".to_string());
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(relocations.len() as u32).to_le_bytes());
    for relocation in relocations {
        if relocation.symbol.len() > u16::MAX as usize {
            return Err("QZI relocation symbol is too long".to_string());
        }
        bytes.extend_from_slice(&relocation.chunk_index.to_le_bytes());
        bytes.extend_from_slice(&relocation.instruction_index.to_le_bytes());
        bytes.extend_from_slice(&(relocation.symbol.len() as u16).to_le_bytes());
        bytes.extend_from_slice(relocation.symbol.as_bytes());
    }
    Ok(bytes)
}

fn decode_call_relocations(bytes: &[u8]) -> Result<Vec<QziCallRelocation>, String> {
    let mut pos = 0usize;
    let count = read_u32(bytes, &mut pos, "QZI relocation count")? as usize;
    if count > bytes.len().saturating_sub(pos) / 10 {
        return Err("QZI relocation count exceeds section size".to_string());
    }
    let mut relocations = Vec::new();
    relocations
        .try_reserve_exact(count)
        .map_err(|_| "QZI relocation count is too large".to_string())?;
    for _ in 0..count {
        let chunk_index = read_u32(bytes, &mut pos, "QZI relocation chunk")?;
        let instruction_index = read_u32(bytes, &mut pos, "QZI relocation instruction")?;
        let symbol = read_short_string(bytes, &mut pos, "QZI relocation symbol")?;
        relocations.push(QziCallRelocation {
            chunk_index,
            instruction_index,
            symbol,
        });
    }
    if pos != bytes.len() {
        return Err("QZI relocation section has trailing bytes".to_string());
    }
    Ok(relocations)
}

fn read_u8(bytes: &[u8], pos: &mut usize, what: &str) -> Result<u8, String> {
    let value = *bytes.get(*pos).ok_or_else(|| format!("truncated {what}"))?;
    *pos += 1;
    Ok(value)
}

fn read_u16(bytes: &[u8], pos: &mut usize, what: &str) -> Result<u16, String> {
    let end = pos
        .checked_add(2)
        .ok_or_else(|| format!("truncated {what}"))?;
    let slice = bytes
        .get(*pos..end)
        .ok_or_else(|| format!("truncated {what}"))?;
    *pos = end;
    Ok(u16::from_le_bytes(slice.try_into().unwrap()))
}

fn read_u32(bytes: &[u8], pos: &mut usize, what: &str) -> Result<u32, String> {
    let end = pos
        .checked_add(4)
        .ok_or_else(|| format!("truncated {what}"))?;
    let slice = bytes
        .get(*pos..end)
        .ok_or_else(|| format!("truncated {what}"))?;
    *pos = end;
    Ok(u32::from_le_bytes(slice.try_into().unwrap()))
}

fn read_short_string(bytes: &[u8], pos: &mut usize, what: &str) -> Result<String, String> {
    let len = read_u16(bytes, pos, what)? as usize;
    let end = pos
        .checked_add(len)
        .ok_or_else(|| format!("truncated {what}"))?;
    let slice = bytes
        .get(*pos..end)
        .ok_or_else(|| format!("truncated {what}"))?;
    *pos = end;
    String::from_utf8(slice.to_vec()).map_err(|_| format!("invalid UTF-8 in {what}"))
}

pub fn serialize_qzi_module(module: &QziModule) -> Result<Vec<u8>, String> {
    validate_qzi_chunks(&module.chunks)?;
    if module
        .chunks
        .iter()
        .flat_map(|chunk| &chunk.code)
        .any(|instruction| instruction.opcode() == Some(Opcode::Lea) && instruction.flags == 0)
    {
        return Err(
            "QZI v7 requires explicit register-block metadata on every `Lea`; rebuild codegen output before serialization"
                .to_string(),
        );
    }
    if module.interface.len() > u32::MAX as usize {
        return Err("QZI interface is too large".to_string());
    }
    let mut relocation_map = std::collections::BTreeMap::new();
    for relocation in infer_call_relocations(&module.chunks)? {
        relocation_map.insert(
            (relocation.chunk_index, relocation.instruction_index),
            relocation,
        );
    }
    for relocation in &module.call_relocations {
        relocation_map.insert(
            (relocation.chunk_index, relocation.instruction_index),
            relocation.clone(),
        );
    }
    let relocations: Vec<_> = relocation_map.into_values().collect();
    let sections = [
        (QZI_SECTION_METADATA, encode_qzi_metadata(&module.metadata)?),
        (QZI_SECTION_INTERFACE, module.interface.as_bytes().to_vec()),
        (
            QZI_SECTION_CALL_RELOCATIONS,
            encode_call_relocations(&relocations)?,
        ),
        (QZI_SECTION_BYTECODE, serialize_qzi_legacy(&module.chunks)?),
    ];
    let directory_len = sections
        .len()
        .checked_mul(QZI_SECTION_ENTRY_SIZE)
        .ok_or_else(|| "QZI section directory overflow".to_string())?;
    let mut offset = 7usize
        .checked_add(directory_len)
        .ok_or_else(|| "QZI section directory overflow".to_string())?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(QZI_MAGIC);
    bytes.push(QZI_VERSION);
    bytes.extend_from_slice(&(sections.len() as u16).to_le_bytes());
    for (kind, payload) in &sections {
        let section_offset = u32::try_from(offset)
            .map_err(|_| "QZI file exceeds the 4 GiB section limit".to_string())?;
        let section_len = u32::try_from(payload.len())
            .map_err(|_| "QZI section exceeds the 4 GiB limit".to_string())?;
        bytes.push(*kind);
        bytes.extend_from_slice(&[0, 0, 0]);
        bytes.extend_from_slice(&section_offset.to_le_bytes());
        bytes.extend_from_slice(&section_len.to_le_bytes());
        offset = offset
            .checked_add(payload.len())
            .ok_or_else(|| "QZI file size overflow".to_string())?;
    }
    for (_, payload) in sections {
        bytes.extend_from_slice(&payload);
    }
    Ok(bytes)
}

pub fn serialize_qzi(chunks: &[Chunk]) -> Result<Vec<u8>, String> {
    serialize_qzi_module(&QziModule::from_chunks(chunks.to_vec()))
}

pub fn deserialize_qzi_module(bytes: &[u8]) -> Result<QziModule, String> {
    if bytes.len() < 5 || &bytes[..4] != QZI_MAGIC.as_slice() {
        return Err("invalid QZI magic".to_string());
    }
    let version = bytes[4];
    if version <= QZI_LEGACY_VERSION {
        let chunks = deserialize_qzi_legacy(bytes)?;
        reject_implicit_lea_metadata(version, &chunks)?;
        if chunks_use_preownership_function_values(&chunks) {
            return Err(format!(
                "QZI v{version} contains function values compiled before affine closure ownership; rebuild this dependency with QZI v7 or publish it as source"
            ));
        }
        let relocations = infer_call_relocations(&chunks)?;
        return Ok(QziModule {
            metadata: QziMetadata {
                name: String::new(),
                version: None,
                kind: if chunks.iter().any(|chunk| chunk.name == "main") {
                    QziModuleKind::Executable
                } else {
                    QziModuleKind::Library
                },
                main_takes_args: false,
            },
            interface: String::new(),
            call_relocations: relocations,
            chunks,
        });
    }
    if !(QZI_FIRST_SECTIONED_VERSION..=QZI_VERSION).contains(&version) {
        return Err(format!("unsupported QZI version {version}"));
    }
    let mut pos = 5usize;
    let section_count = read_u16(bytes, &mut pos, "QZI section count")? as usize;
    let directory_end = pos
        .checked_add(
            section_count
                .checked_mul(QZI_SECTION_ENTRY_SIZE)
                .ok_or_else(|| "QZI section count overflow".to_string())?,
        )
        .ok_or_else(|| "QZI section directory overflow".to_string())?;
    if directory_end > bytes.len() {
        return Err("truncated QZI section directory".to_string());
    }
    let mut sections = std::collections::HashMap::new();
    let mut section_ranges = Vec::with_capacity(section_count);
    for _ in 0..section_count {
        let kind = read_u8(bytes, &mut pos, "QZI section kind")?;
        pos += 3;
        let offset = read_u32(bytes, &mut pos, "QZI section offset")? as usize;
        let len = read_u32(bytes, &mut pos, "QZI section length")? as usize;
        let end = offset
            .checked_add(len)
            .ok_or_else(|| "QZI section range overflow".to_string())?;
        if offset < directory_end || end > bytes.len() {
            return Err("QZI section is outside the file".to_string());
        }
        section_ranges.push((offset, end));
        if sections.insert(kind, &bytes[offset..end]).is_some() {
            return Err(format!("duplicate QZI section {kind}"));
        }
    }
    section_ranges.sort_unstable();
    let mut expected_offset = directory_end;
    for (offset, end) in section_ranges {
        if offset != expected_offset {
            return Err("QZI sections contain a gap or overlap".to_string());
        }
        expected_offset = end;
    }
    if expected_offset != bytes.len() {
        return Err(format!(
            "QZI has {} trailing byte(s)",
            bytes.len() - expected_offset
        ));
    }
    let metadata = decode_qzi_metadata(
        sections
            .remove(&QZI_SECTION_METADATA)
            .ok_or_else(|| "QZI metadata section is missing".to_string())?,
    )?;
    let interface = String::from_utf8(
        sections
            .remove(&QZI_SECTION_INTERFACE)
            .unwrap_or_default()
            .to_vec(),
    )
    .map_err(|_| "invalid UTF-8 in QZI interface".to_string())?;
    if version == 6
        && !interface.is_empty()
        && crate::bytecode::interface::qzi_v6_interface_has_ambiguous_trait_receivers(&interface)?
    {
        return Err(
            "QZI v6 trait interfaces do not preserve explicit receiver names safely; rebuild this dependency with QZI v7 or publish it as source"
                .to_string(),
        );
    }
    if version == 6
        && !interface.is_empty()
        && crate::bytecode::interface::qzi_v6_interface_has_owned_function_values(&interface)?
    {
        return Err(
            "QZI v6 public interface uses function values compiled before affine closure ownership; rebuild this dependency with QZI v7 or publish it as source"
                .to_string(),
        );
    }
    if version == 6
        && !interface.is_empty()
        && crate::bytecode::interface::qzi_v6_interface_has_runtime_any(&interface)?
    {
        return Err(
            "QZI v6 public interface contains runtime `any`, which has no portable representation; rebuild this dependency with QZI v7 after migrating the API or publish it as source"
                .to_string(),
        );
    }
    let call_relocations = decode_call_relocations(
        sections
            .remove(&QZI_SECTION_CALL_RELOCATIONS)
            .ok_or_else(|| "QZI relocation section is missing".to_string())?,
    )?;
    let chunks = deserialize_qzi_legacy(
        sections
            .remove(&QZI_SECTION_BYTECODE)
            .ok_or_else(|| "QZI bytecode section is missing".to_string())?,
    )?;
    reject_implicit_lea_metadata(version, &chunks)?;
    if version < 7 && chunks_use_preownership_function_values(&chunks) {
        return Err(
            "QZI v6 contains function values compiled before affine closure ownership; rebuild this dependency with QZI v7 or publish it as source"
                .to_string(),
        );
    }
    if version < 7
        && chunks
            .iter()
            .flat_map(|chunk| &chunk.code)
            .any(|instruction| {
                instruction.flags & crate::bytecode::instruction::UNSIGNED_FLAG != 0
                    && matches!(
                        instruction.opcode(),
                        Some(
                            Opcode::Div
                                | Opcode::Mod
                                | Opcode::Jg
                                | Opcode::Jge
                                | Opcode::Jl
                                | Opcode::Jle
                        )
                    )
            })
    {
        return Err("QZI v6 cannot contain v7 unsigned-integer instruction flags".to_string());
    }
    if version < 7
        && chunks
            .iter()
            .flat_map(|chunk| &chunk.code)
            .any(|instruction| {
                instruction.opcode() == Some(Opcode::Trap)
                    || (instruction.opcode() == Some(Opcode::Lea) && instruction.flags != 0)
            })
    {
        return Err(
            "QZI v6 cannot contain v7 safety opcodes or register-block metadata".to_string(),
        );
    }
    for relocation in &call_relocations {
        let chunk = chunks
            .get(relocation.chunk_index as usize)
            .ok_or_else(|| "QZI relocation references a missing chunk".to_string())?;
        let instruction = chunk
            .code
            .get(relocation.instruction_index as usize)
            .ok_or_else(|| "QZI relocation references a missing instruction".to_string())?;
        if instruction.opcode != Opcode::CallIdx as u8 {
            return Err("QZI call relocation does not reference CallIdx".to_string());
        }
    }
    Ok(QziModule {
        metadata,
        interface,
        call_relocations,
        chunks,
    })
}

fn reject_implicit_lea_metadata(version: u8, chunks: &[Chunk]) -> Result<(), String> {
    if chunks
        .iter()
        .flat_map(|chunk| &chunk.code)
        .any(|instruction| instruction.opcode() == Some(Opcode::Lea) && instruction.flags == 0)
    {
        return Err(format!(
            "QZI v{version} contains `Lea` without address-taken register metadata; rebuild this dependency from source with the current compiler"
        ));
    }
    Ok(())
}

pub fn deserialize_qzi(bytes: &[u8]) -> Result<Vec<Chunk>, String> {
    Ok(deserialize_qzi_module(bytes)?.chunks)
}

pub fn link_qzi_modules(modules: &[QziModule]) -> Result<Vec<Chunk>, String> {
    let mut symbols = std::collections::HashMap::new();
    let mut chunk_maps = Vec::with_capacity(modules.len());
    let mut linked = Vec::new();
    for module in modules {
        let mut chunk_map = Vec::with_capacity(module.chunks.len());
        for chunk in &module.chunks {
            let global_index = if let Some(&existing) = symbols.get(&chunk.name) {
                if !equivalent_relocatable_chunks(&linked[existing as usize], chunk) {
                    return Err(format!(
                        "conflicting QZI definitions for symbol `{}`",
                        chunk.name
                    ));
                }
                existing
            } else {
                let index = u16::try_from(linked.len())
                    .map_err(|_| "linked QZI exceeds the function-table limit".to_string())?;
                symbols.insert(chunk.name.clone(), index);
                linked.push(chunk.clone());
                index
            };
            chunk_map.push(global_index);
        }
        chunk_maps.push(chunk_map);
    }

    let mut patched_targets = std::collections::HashMap::new();
    for (module_index, module) in modules.iter().enumerate() {
        for relocation in &module.call_relocations {
            let target = symbols
                .get(&relocation.symbol)
                .copied()
                .ok_or_else(|| format!("unresolved QZI symbol `{}`", relocation.symbol))?;
            let source = *chunk_maps[module_index]
                .get(relocation.chunk_index as usize)
                .ok_or_else(|| "QZI relocation source is out of bounds".to_string())?;
            let patch_key = (source, relocation.instruction_index);
            if let Some(previous) = patched_targets.insert(patch_key, target)
                && previous != target
            {
                return Err(format!(
                    "conflicting QZI relocations in symbol `{}`",
                    linked[source as usize].name
                ));
            }
            let instruction = linked
                .get_mut(source as usize)
                .and_then(|chunk| chunk.code.get_mut(relocation.instruction_index as usize))
                .ok_or_else(|| "QZI relocation target is out of bounds".to_string())?;
            let [lo, hi] = target.to_le_bytes();
            instruction.ops[1] = lo;
            instruction.ops[2] = hi;
        }
    }
    validate_qzi_chunks(&linked)?;
    Ok(linked)
}

fn equivalent_relocatable_chunks(left: &Chunk, right: &Chunk) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    for chunk in [&mut left, &mut right] {
        for instruction in &mut chunk.code {
            if instruction.opcode == Opcode::CallIdx as u8 {
                instruction.ops[1] = 0;
                instruction.ops[2] = 0;
            }
        }
    }
    left == right
}

fn validate_foreign_symbol(symbol: &crate::abi::ForeignSymbol) -> Result<(), String> {
    if symbol.symbol.len() > u16::MAX as usize {
        return Err("QZI foreign symbol is too long".to_string());
    }
    if symbol.signature.params.len() > u16::MAX as usize {
        return Err("QZI ABI signature has too many parameters".to_string());
    }
    for ty in &symbol.signature.params {
        validate_abi_type(ty, 0)?;
    }
    validate_abi_type(&symbol.signature.return_type, 0)
}

fn validate_abi_type(ty: &crate::abi::AbiType, depth: usize) -> Result<(), String> {
    if depth > 64 {
        return Err("QZI ABI type nesting exceeds 64 levels".to_string());
    }
    match ty {
        crate::abi::AbiType::Integer { bytes, .. } if !matches!(*bytes, 1 | 2 | 4 | 8) => {
            return Err(format!("QZI ABI integer has invalid width {bytes}"));
        }
        crate::abi::AbiType::Aggregate {
            size,
            align,
            fields,
        } => {
            if *align == 0 || !align.is_power_of_two() {
                return Err(format!("QZI ABI aggregate has invalid alignment {align}"));
            }
            if fields.len() > u16::MAX as usize {
                return Err("QZI ABI aggregate has too many fields".to_string());
            }
            for field in fields {
                validate_abi_type(&field.ty, depth + 1)?;
                if field.offset as usize + field.ty.size() > *size as usize {
                    return Err("QZI ABI field extends past aggregate size".to_string());
                }
            }
        }
        _ => {}
    }
    Ok(())
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
                    ConstPoolEntry::Bytes(bytes) => {
                        format!("\x1b[32mbytes({bytes:?})\x1b[0m")
                    }
                    ConstPoolEntry::FnAddr(name) => format!("\x1b[36m{name}\x1b[0m"),
                    ConstPoolEntry::VtableAddr(tn, tr) => {
                        format!("\x1b[35mvtable({tn}::{tr})\x1b[0m")
                    }
                    ConstPoolEntry::ForeignSymbol(symbol) => {
                        format!("\x1b[35mforeign({})\x1b[0m", symbol.symbol)
                    }
                    ConstPoolEntry::ForeignGlobal(global) => {
                        format!("\x1b[35mglobal({})\x1b[0m", global.symbol)
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
                    Some(Opcode::CallCReg) => {
                        let (_, source, _) = instr.call_c_reg_parts();
                        let callee = format!("C r{}", source);
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
    use crate::abi::{AbiSignature, AbiType, ForeignGlobal, ForeignSymbol};

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
    fn current_qzi_preserves_v5_chunk_metadata_bytes_and_globals() {
        let signature = AbiSignature {
            params: vec![AbiType::Float64],
            return_type: AbiType::Float64,
            variadic: false,
        };
        let mut chunk = Chunk::with_params("sin_adapter", 1);
        chunk.reg_count = 1;
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
        chunk
            .constants
            .push(ConstPoolEntry::Bytes(vec![0, 0xff, 1]));
        chunk
            .constants
            .push(ConstPoolEntry::ForeignGlobal(ForeignGlobal {
                symbol: "native_counter".to_string(),
                ty: AbiType::Integer {
                    bytes: 4,
                    signed: true,
                },
            }));
        chunk.emit_rrr(Opcode::Ret, 0, 0, 0);

        let encoded = serialize_qzi(&[chunk]).expect("current QZI should encode");
        let decoded = deserialize_qzi(&encoded).expect("current QZI should decode");
        assert_eq!(decoded[0].export.as_ref().unwrap().symbol, "quazi_sin");
        assert!(matches!(
            decoded[0].constants.as_slice(),
            [ConstPoolEntry::ForeignSymbol(symbol), ConstPoolEntry::Bytes(bytes), ConstPoolEntry::ForeignGlobal(global)]
                if symbol.symbol == "sin" && bytes == &[0, 0xff, 1] && global.symbol == "native_counter"
        ));
    }

    #[test]
    fn current_qzi_roundtrips_module_metadata_and_interface() {
        let mut chunk = Chunk::new("math.add");
        chunk.reg_count = 1;
        chunk.emit_rrr(Opcode::Ret, 0, 0, 0);
        let module = QziModule {
            metadata: QziMetadata {
                name: "math".to_string(),
                version: Some("1.2.3".to_string()),
                kind: QziModuleKind::Executable,
                main_takes_args: true,
            },
            interface: "pub fn add(a: i32, b: i32) i32;".to_string(),
            call_relocations: Vec::new(),
            chunks: vec![chunk],
        };

        let encoded = serialize_qzi_module(&module).expect("serialize current QZI module");
        assert_eq!(encoded[4], QZI_VERSION);
        let decoded = deserialize_qzi_module(&encoded).expect("deserialize current QZI module");
        assert_eq!(decoded.metadata, module.metadata);
        assert_eq!(decoded.interface, module.interface);
        assert_eq!(decoded.chunks[0].name, "math.add");
    }

    #[test]
    fn current_qzi_reader_accepts_v6_sectioned_modules() {
        let mut chunk = Chunk::new("main");
        chunk.emit_rrr(Opcode::Ret, 0, 0, 0);
        let mut encoded = serialize_qzi(&[chunk]).expect("serialize current QZI");
        encoded[4] = 6;

        let decoded = deserialize_qzi(&encoded).expect("read QZI v6 module");
        assert_eq!(decoded[0].name, "main");
    }

    #[test]
    fn qzi_v6_function_values_require_an_ownership_rebuild() {
        let mut chunk = Chunk::new("__quazi_fwd_one");
        chunk.emit_rrr(Opcode::Ret, 0, 0, 0);
        let mut encoded = serialize_qzi(&[chunk]).expect("serialize current QZI");
        encoded[4] = 6;

        let error = deserialize_qzi(&encoded)
            .expect_err("pre-ownership function values must require a source rebuild");
        assert!(error.contains("before affine closure ownership"));
        assert!(error.contains("rebuild this dependency with QZI v7"));
    }

    #[test]
    fn qzi_v6_public_function_value_contracts_require_rebuild() {
        let mut chunk = Chunk::new("legacy.consume");
        chunk.emit_rrr(Opcode::Ret, 0, 0, 0);
        let interface = toml::to_string(&crate::bytecode::interface::QziInterfaceBundle {
            modules: vec![crate::bytecode::interface::QziInterfaceModule {
                name: "legacy".to_string(),
                exports: vec!["consume".to_string()],
                source: "pub fn consume(callback: fn() i32) void;\n".to_string(),
            }],
        })
        .expect("serialize legacy interface");
        let module = QziModule {
            metadata: QziMetadata {
                name: "legacy".to_string(),
                version: Some("1.0.0".to_string()),
                kind: QziModuleKind::Library,
                main_takes_args: false,
            },
            interface,
            call_relocations: Vec::new(),
            chunks: vec![chunk],
        };
        let mut encoded = serialize_qzi_module(&module).expect("serialize sectioned module");
        encoded[4] = 6;

        let error = deserialize_qzi_module(&encoded)
            .expect_err("pre-ownership callable API must require a source rebuild");
        assert!(error.contains("public interface uses function values"));
        assert!(error.contains("rebuild this dependency with QZI v7"));
    }

    #[test]
    fn qzi_v1_requires_source_rebuild_instead_of_guessing_frame_metadata() {
        let bytes = [0, b'Q', b'Z', b'I', 1, 0, 0, 0, 0];
        let error = deserialize_qzi_module(&bytes)
            .expect_err("v1 omitted frame metadata and must fail explicitly");
        assert!(error.contains("omitted parameter and register-frame metadata"));
        assert!(error.contains("rebuild the artifact from source"));
    }

    #[test]
    fn qzi_v6_trait_interfaces_with_parameters_require_rebuild() {
        let mut chunk = Chunk::new("main");
        chunk.emit_rrr(Opcode::Ret, 0, 0, 0);
        let interface = toml::to_string(&crate::bytecode::interface::QziInterfaceBundle {
            modules: vec![crate::bytecode::interface::QziInterfaceModule {
                name: "legacy".to_string(),
                exports: vec!["Write".to_string()],
                // QZI v6 rendered every trait parameter as argN, losing the
                // distinction between an explicit `self` receiver and an
                // ordinary first argument.
                source: "pub trait Write { fn write(arg0: Self, arg1: str) void; }\n".to_string(),
            }],
        })
        .expect("serialize legacy interface");
        let module = QziModule {
            metadata: QziMetadata {
                name: "legacy".to_string(),
                version: Some("1.0.0".to_string()),
                kind: QziModuleKind::Library,
                main_takes_args: false,
            },
            interface,
            call_relocations: Vec::new(),
            chunks: vec![chunk],
        };
        let mut encoded = serialize_qzi_module(&module).expect("serialize sectioned module");
        encoded[4] = 6;

        let error = deserialize_qzi_module(&encoded)
            .expect_err("ambiguous v6 trait receiver metadata must not be guessed");
        assert!(error.contains("do not preserve explicit receiver names safely"));
        assert!(error.contains("rebuild this dependency with QZI v7"));
    }

    #[test]
    fn qzi_v6_public_runtime_any_interfaces_require_rebuild() {
        let mut chunk = Chunk::new("legacy.erase");
        chunk.emit_rrr(Opcode::Ret, 0, 0, 0);
        let interface = toml::to_string(&crate::bytecode::interface::QziInterfaceBundle {
            modules: vec![crate::bytecode::interface::QziInterfaceModule {
                name: "legacy".to_string(),
                exports: vec!["erase".to_string()],
                source: "pub fn erase(value: any) any;\n".to_string(),
            }],
        })
        .expect("serialize legacy interface");
        let module = QziModule {
            metadata: QziMetadata {
                name: "legacy".to_string(),
                version: Some("1.0.0".to_string()),
                kind: QziModuleKind::Library,
                main_takes_args: false,
            },
            interface,
            call_relocations: Vec::new(),
            chunks: vec![chunk],
        };
        let mut encoded = serialize_qzi_module(&module).expect("serialize sectioned module");
        encoded[4] = 6;

        let error = deserialize_qzi_module(&encoded)
            .expect_err("v6 runtime-any interfaces must not enter semantic analysis");
        assert!(error.contains("runtime `any`, which has no portable representation"));
        assert!(error.contains("rebuild this dependency with QZI v7"));
    }

    #[test]
    fn current_qzi_roundtrips_unsigned_flags_and_v6_rejects_them() {
        let mut chunk = Chunk::with_params("divide", 2);
        chunk.reg_count = 3;
        let mut division = crate::bytecode::instruction::rrr(Opcode::Div, 2, 0, 1);
        division.flags |= crate::bytecode::instruction::UNSIGNED_FLAG;
        chunk.emit(division);
        chunk.emit_rrr(Opcode::Ret, 2, 0, 0);
        let encoded = serialize_qzi(&[chunk]).expect("serialize current QZI unsigned division");

        let decoded = deserialize_qzi(&encoded).expect("read current QZI unsigned division");
        assert_ne!(
            decoded[0].code[0].flags & crate::bytecode::instruction::UNSIGNED_FLAG,
            0
        );

        let mut downgraded = encoded;
        downgraded[4] = 6;
        let error = deserialize_qzi(&downgraded).expect_err("v6 must reject v7 flags");
        assert!(error.contains("v7 unsigned-integer instruction flags"));
    }

    #[test]
    fn qzi_v6_rejects_v7_checked_indexing_instructions() {
        for instruction in [
            crate::bytecode::instruction::rrr(Opcode::Trap, 0, 0, 0),
            crate::bytecode::instruction::mem_lea_block(0, 1, 0, 1),
        ] {
            let mut chunk = Chunk::new("checked");
            chunk.reg_count = 2;
            chunk.emit(instruction);
            chunk.emit_rrr(Opcode::Ret, 0, 0, 0);
            let mut encoded = serialize_qzi(&[chunk]).expect("serialize current QZI");
            encoded[4] = 6;
            let error = deserialize_qzi(&encoded).expect_err("v6 must reject v7 safety data");
            assert!(error.contains("v7 safety"));
        }
    }

    #[test]
    fn qzi_linker_resolves_symbolic_calls_between_modules() {
        let mut main = Chunk::new("main");
        main.reg_count = 1;
        main.emit_ri16(Opcode::CallIdx, 0, 0);
        main.emit_rrr(Opcode::Ret, 0, 0, 0);
        let app = QziModule {
            metadata: QziMetadata {
                name: "app".to_string(),
                version: None,
                kind: QziModuleKind::Executable,
                main_takes_args: false,
            },
            interface: String::new(),
            call_relocations: vec![QziCallRelocation {
                chunk_index: 0,
                instruction_index: 0,
                symbol: "math.add".to_string(),
            }],
            chunks: vec![main],
        };
        let mut add = Chunk::new("math.add");
        add.reg_count = 1;
        add.emit_rrr(Opcode::Ret, 0, 0, 0);
        let math = QziModule {
            metadata: QziMetadata {
                name: "math".to_string(),
                version: None,
                kind: QziModuleKind::Library,
                main_takes_args: false,
            },
            interface: String::new(),
            call_relocations: Vec::new(),
            chunks: vec![add],
        };

        let linked = link_qzi_modules(&[app, math]).expect("link QZI modules");
        assert_eq!(linked[0].code[0].ri16().1, 1);
    }

    #[test]
    fn qzi_linker_deduplicates_equivalent_dependency_chunks() {
        let ret_chunk = |name: &str| {
            let mut chunk = Chunk::new(name);
            chunk.reg_count = 1;
            chunk.emit_rrr(Opcode::Ret, 0, 0, 0);
            chunk
        };
        let mut main = Chunk::new("main");
        main.reg_count = 1;
        main.emit_ri16(Opcode::CallIdx, 0, 1);
        main.emit_rrr(Opcode::Ret, 0, 0, 0);
        let mut shared_from_app = Chunk::new("std.shared");
        shared_from_app.reg_count = 1;
        shared_from_app.emit_ri16(Opcode::CallIdx, 0, 2);
        shared_from_app.emit_rrr(Opcode::Ret, 0, 0, 0);
        let mut shared_from_library = shared_from_app.clone();
        shared_from_library.code[0] = ri16(Opcode::CallIdx, 0, 1);

        let app = QziModule::from_chunks(vec![main, shared_from_app, ret_chunk("std.helper")]);
        let library = QziModule::from_chunks(vec![shared_from_library, ret_chunk("std.helper")]);
        let linked = link_qzi_modules(&[app, library]).expect("deduplicate shared chunks");
        assert_eq!(linked.len(), 3);
        assert_eq!(linked[0].code[0].ri16().1, 1);
        assert_eq!(linked[1].code[0].ri16().1, 2);
    }

    #[test]
    fn qzi_rejects_unknown_opcodes_and_trailing_bytes() {
        let mut chunk = Chunk::new("main");
        chunk.emit_rrr(Opcode::Ret, 0, 0, 0);
        let encoded = serialize_qzi(&[chunk]).expect("valid QZI should encode");

        let mut unknown_opcode = encoded.clone();
        let instruction_start = unknown_opcode.len() - 6;
        unknown_opcode[instruction_start] = 0xff;
        assert!(deserialize_qzi(&unknown_opcode).is_err());

        let mut trailing = encoded;
        trailing.push(0);
        assert!(deserialize_qzi(&trailing).is_err());
    }

    #[test]
    fn qzi_rejects_registers_outside_the_declared_frame() {
        let mut chunk = Chunk::new("bad");
        chunk.reg_count = 1;
        chunk.emit_rrr(Opcode::Add, 0, 0, 1);
        chunk.emit_rrr(Opcode::Ret, 0, 0, 0);
        let error = serialize_qzi(&[chunk]).expect_err("invalid frame must be rejected");
        assert!(error.contains("outside the declared frame"));
    }

    #[test]
    fn qzi_rejects_wrapping_contiguous_register_blocks() {
        let mut chunk = Chunk::new("bad_block");
        chunk.reg_count = u8::MAX;
        chunk.emit(crate::bytecode::instruction::mem_lea_block(250, 0, 0, 10));
        let error = serialize_qzi(&[chunk]).expect_err("wrapping block must be rejected");
        assert!(error.contains("wraps past r255"));
    }

    #[test]
    fn qzi_rejects_missing_address_taken_register_metadata() {
        let mut chunk = Chunk::new("borrow");
        chunk.reg_count = 2;
        chunk.emit(ri16(Opcode::MovI, 0, 7));
        chunk.emit(crate::bytecode::instruction::mem_lea_block(0, 1, 0, 1));
        chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
        let mut encoded = serialize_qzi(&[chunk]).expect("explicit Lea metadata is valid");
        let encoded_lea = [Opcode::Lea as u8, 1, 0, 0, 0, 1];
        let offset = encoded
            .windows(encoded_lea.len())
            .position(|window| window == encoded_lea)
            .expect("serialized Lea instruction");
        encoded[offset + 5] = 0;

        let error = deserialize_qzi_module(&encoded)
            .expect_err("implicit address-taken metadata must require a source rebuild");
        assert!(error.contains("without address-taken register metadata"));
        assert!(error.contains("rebuild this dependency from source"));
    }

    #[test]
    fn legacy_qzi_rejects_missing_address_taken_register_metadata() {
        let mut chunk = Chunk::new("borrow");
        chunk.reg_count = 2;
        chunk.emit(ri16(Opcode::MovI, 0, 7));
        chunk.emit(crate::bytecode::instruction::mem_lea_block(0, 1, 0, 1));
        chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
        let mut encoded = serialize_qzi_legacy(&[chunk]).expect("valid legacy QZI");
        let encoded_lea = [Opcode::Lea as u8, 1, 0, 0, 0, 1];
        let offset = encoded
            .windows(encoded_lea.len())
            .position(|window| window == encoded_lea)
            .expect("serialized Lea instruction");
        encoded[offset + 5] = 0;

        let error = deserialize_qzi_module(&encoded)
            .expect_err("legacy implicit address metadata must require a source rebuild");
        assert!(error.contains("QZI v5"));
        assert!(error.contains("without address-taken register metadata"));
    }

    #[test]
    fn qzi_rejects_impossible_chunk_counts_before_allocating() {
        let mut encoded = Vec::from(QZI_MAGIC.as_slice());
        encoded.push(QZI_VERSION);
        encoded.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(deserialize_qzi(&encoded).is_err());
    }

    #[test]
    fn qzi_rejects_a_constant_pool_that_overflowed_during_codegen() {
        let mut chunk = Chunk::new("overflowed");
        chunk.constant_pool_overflowed = true;
        let error = serialize_qzi(&[chunk]).expect_err("overflow must not be serialized");
        assert!(error.contains("constant-pool limit"));
    }

    #[test]
    fn qzi_rejects_opcode_metadata_with_the_wrong_constant_kind() {
        let mut external = Chunk::new("bad_external");
        external.reg_count = 1;
        external.constants.push(ConstPoolEntry::Int(7));
        external.emit_ri16(Opcode::CallExt, 0, 0);
        external.emit_rrr(Opcode::Ret, 0, 0, 0);
        let error = serialize_qzi(&[external]).expect_err("CallExt metadata must be validated");
        assert!(error.contains("wrong constant-pool kind"));

        let mut syscall = Chunk::new("bad_syscall");
        syscall.reg_count = 1;
        syscall.constants.push(ConstPoolEntry::Int(-1));
        syscall.emit_ri16(Opcode::Syscall, 0, 0);
        syscall.emit_rrr(Opcode::Ret, 0, 0, 0);
        let error = serialize_qzi(&[syscall]).expect_err("syscall metadata must be validated");
        assert!(error.contains("non-negative number"));
    }

    // --- Historical golden fixtures ------------------------------------
    // Immutable artifacts produced by historical compiler writers (provenance
    // and regeneration instructions: fixtures/qzi/README.md). They lock the
    // documented compatible-QZI v2-v6 reading paths against real evidence
    // instead of synthetic header edits.

    const GOLDEN_V2_MAIN: &[u8] = include_bytes!("fixtures/qzi/v2/main.qzi");
    const GOLDEN_V2_LEA: &[u8] = include_bytes!("fixtures/qzi/v2/lea.qzi");
    const GOLDEN_V3_MAIN: &[u8] = include_bytes!("fixtures/qzi/v3/main.qzi");
    const GOLDEN_V3_LEA: &[u8] = include_bytes!("fixtures/qzi/v3/lea.qzi");
    const GOLDEN_V3_FFI: &[u8] = include_bytes!("fixtures/qzi/v3/ffi.qzi");
    const GOLDEN_V4_MAIN: &[u8] = include_bytes!("fixtures/qzi/v4/main.qzi");
    const GOLDEN_V4_LEA: &[u8] = include_bytes!("fixtures/qzi/v4/lea.qzi");
    const GOLDEN_V4_FFI: &[u8] = include_bytes!("fixtures/qzi/v4/ffi.qzi");
    const GOLDEN_V5_MAIN: &[u8] = include_bytes!("fixtures/qzi/v5/main.qzi");
    const GOLDEN_V5_LEA: &[u8] = include_bytes!("fixtures/qzi/v5/lea.qzi");
    const GOLDEN_V5_FFI: &[u8] = include_bytes!("fixtures/qzi/v5/ffi.qzi");
    const GOLDEN_V6_MAIN: &[u8] = include_bytes!("fixtures/qzi/v6/main.qzi");
    const GOLDEN_V6_LEA: &[u8] = include_bytes!("fixtures/qzi/v6/lea.qzi");
    const GOLDEN_V6_FFI: &[u8] = include_bytes!("fixtures/qzi/v6/ffi.qzi");
    const GOLDEN_V6_LIB: &[u8] = include_bytes!("fixtures/qzi/v6/lib.qzi");

    fn golden_chunk_names(module: &QziModule) -> Vec<&str> {
        module
            .chunks
            .iter()
            .map(|chunk| chunk.name.as_str())
            .collect()
    }

    fn golden_has_foreign_symbol(module: &QziModule, symbol: &str) -> bool {
        module.chunks.iter().any(|chunk| {
            chunk.constants.iter().any(
                |constant| matches!(constant, ConstPoolEntry::ForeignSymbol(foreign) if foreign.symbol == symbol),
            )
        })
    }

    #[test]
    fn golden_v2_main_decodes_with_synthesized_metadata() {
        let module = deserialize_qzi_module(GOLDEN_V2_MAIN).expect("v2 main should decode");
        assert_eq!(module.metadata.kind, QziModuleKind::Executable);
        assert!(module.metadata.name.is_empty());
        assert!(module.metadata.version.is_none());
        assert!(module.interface.is_empty());
        let names = golden_chunk_names(&module);
        for expected in ["main", "unwrap_or", "Pair.new", "Pair.sum", "identity<i32>"] {
            assert!(names.contains(&expected), "missing v2 chunk `{expected}`");
        }
        assert!(module.chunks.iter().any(|chunk| {
            chunk.constants.iter().any(
                |constant| matches!(constant, ConstPoolEntry::Str(value) if value == "qzi-golden-v2"),
            )
        }));
        assert!(!module.call_relocations.is_empty());
    }

    #[test]
    fn golden_legacy_main_artifacts_decode() {
        for (version, bytes) in [
            (3, GOLDEN_V3_MAIN),
            (4, GOLDEN_V4_MAIN),
            (5, GOLDEN_V5_MAIN),
        ] {
            let module = deserialize_qzi_module(bytes)
                .unwrap_or_else(|error| panic!("v{version} main should decode: {error}"));
            assert_eq!(module.metadata.kind, QziModuleKind::Executable);
            assert!(golden_chunk_names(&module).contains(&"main"));
        }
        for (version, bytes) in [(4, GOLDEN_V4_MAIN), (5, GOLDEN_V5_MAIN)] {
            let module = deserialize_qzi_module(bytes).expect("byte-string fixture decodes");
            assert!(
                module.chunks.iter().any(|chunk| {
                    chunk
                        .constants
                        .iter()
                        .any(|constant| matches!(constant, ConstPoolEntry::Bytes(_)))
                }),
                "v{version} main should carry a Bytes constant"
            );
        }
    }

    #[test]
    fn golden_v3_ffi_keeps_legacy_string_call_metadata() {
        // Authentic v3 behavior: @api symbols are plain `Str` constants (the
        // `ForeignSymbol` ABI metadata arrived with v4) and @export names are
        // not persisted at all. The backend retains a scalar-only legacy
        // `CallExt` lowering for these artifacts, matching v3's scalar/pointer
        // FFI phase.
        let module = deserialize_qzi_module(GOLDEN_V3_FFI).expect("v3 ffi should decode");
        assert!(module.chunks.iter().any(|chunk| {
            chunk.constants.iter().any(
                |constant| matches!(constant, ConstPoolEntry::Str(value) if value == "qzi_golden_ext"),
            )
        }));
        assert!(module.chunks.iter().all(|chunk| {
            chunk
                .constants
                .iter()
                .all(|constant| !matches!(constant, ConstPoolEntry::ForeignSymbol(_)))
        }));
        assert!(module.chunks.iter().all(|chunk| chunk.export.is_none()));
    }

    #[test]
    fn golden_v4_ffi_preserves_export_metadata() {
        let module = deserialize_qzi_module(GOLDEN_V4_FFI).expect("v4 ffi should decode");
        assert!(golden_has_foreign_symbol(&module, "qzi_golden_ext"));
        assert!(golden_chunk_names(&module).contains(&"__quazi_export_adapter_sum_scalars_3"));
        assert!(module.chunks.iter().any(|chunk| {
            matches!(&chunk.export, Some(export) if export.symbol == "quazi_golden_sum")
        }));
    }

    #[test]
    fn golden_v5_ffi_preserves_foreign_globals() {
        let module = deserialize_qzi_module(GOLDEN_V5_FFI).expect("v5 ffi should decode");
        assert!(module.chunks.iter().any(|chunk| {
            chunk.constants.iter().any(
                |constant| matches!(constant, ConstPoolEntry::ForeignGlobal(global) if global.symbol == "qzi_golden_counter"),
            )
        }));
    }

    #[test]
    fn golden_v6_executables_decode_through_the_sectioned_reader() {
        for bytes in [GOLDEN_V6_MAIN, GOLDEN_V6_FFI] {
            let module = deserialize_qzi_module(bytes).expect("v6 executable should decode");
            assert_eq!(module.metadata.kind, QziModuleKind::Executable);
            assert!(golden_chunk_names(&module).contains(&"main"));
            assert!(!module.call_relocations.is_empty());
        }
    }

    #[test]
    fn golden_v6_library_decodes_metadata_interface_and_relocations() {
        let module = deserialize_qzi_module(GOLDEN_V6_LIB).expect("v6 library should decode");
        assert_eq!(module.metadata.name, "goldenlib");
        assert_eq!(module.metadata.version.as_deref(), Some("0.1.0"));
        assert_eq!(module.metadata.kind, QziModuleKind::Library);
        assert!(module.interface.contains("pub fn add(a: i64, b: i64) i64;"));
        assert!(golden_chunk_names(&module).contains(&"goldenlib.demo"));
        // The v6 writer inlined and tree-shook every callee, so this artifact's
        // relocation section is legitimately empty; non-empty relocations are
        // covered by the v6 executable fixtures above.
        assert!(
            module
                .call_relocations
                .iter()
                .all(|relocation| (relocation.chunk_index as usize) < module.chunks.len())
        );
    }

    #[test]
    fn golden_legacy_lea_artifacts_require_a_source_rebuild() {
        for (version, bytes) in [
            (2, GOLDEN_V2_LEA),
            (3, GOLDEN_V3_LEA),
            (4, GOLDEN_V4_LEA),
            (5, GOLDEN_V5_LEA),
            (6, GOLDEN_V6_LEA),
        ] {
            let error = deserialize_qzi_module(bytes)
                .expect_err("legacy implicit address metadata must be rejected");
            assert!(
                error.contains("without address-taken register metadata"),
                "v{version} lea fixture failed with an unexpected error: {error}"
            );
            assert!(error.contains(&format!("QZI v{version}")));
        }
    }
}
