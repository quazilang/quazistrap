// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use object::write::{Object, Relocation, SectionId, StandardSection};
use object::{BinaryFormat, RelocationEncoding, RelocationFlags, RelocationKind};

use crate::bytecode::{Chunk, ConstPoolEntry, Opcode};
use crate::semantic::SemanticReport;

use super::symbols::SymbolTable;

// Windows crash handler strings.
pub(super) const CRASH_HEADER: &[u8] = b"== CRASHED ==\n";
pub(super) const FATAL_EXC: &[u8] = b"fatal: exception 0x";
pub(super) const FAULT_PREFIX: &[u8] = b"fault: 0x";
pub(super) const RIP_PREFIX: &[u8] = b"rip: 0x";
pub(super) const TRACE_HINT: &[u8] = b"use QUAZI_TRACE=1 to see full stack trace\n";
pub(super) const ENV_VAR_NAME: &[u8] = b"QUAZI_TRACE\0";

fn safe_label(name: &str) -> String {
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

pub struct SectionAccumulator {
    pub text_id: SectionId,
    pub rodata_id: SectionId,
    pub data_id: SectionId,
}

impl SectionAccumulator {
    pub fn new(obj: &mut Object<'_>) -> Self {
        Self {
            text_id: obj.section_id(StandardSection::Text),
            rodata_id: obj.section_id(StandardSection::ReadOnlyData),
            data_id: obj.section_id(StandardSection::Data),
        }
    }

    /// Lay out .rodata: string constants and the "%ld" format string for PrimToStr.
    /// Adds a data symbol for each string entry and for __quazi_fmt_ld.
    /// Returns the symbol name for each const-pool slot (None for non-Str entries).
    pub fn build_rodata(
        &self,
        obj: &mut Object<'_>,
        sym_table: &mut SymbolTable,
        chunks: &[Chunk],
    ) -> Vec<Vec<Option<String>>> {
        // Windows crash handler strings
        let hdr_offset = obj.append_section_data(self.rodata_id, CRASH_HEADER, 1);
        sym_table.define_data(
            obj,
            self.rodata_id,
            "__quazi_crash_header",
            hdr_offset,
            CRASH_HEADER.len() as u64,
        );
        let fatal_exc_offset = obj.append_section_data(self.rodata_id, FATAL_EXC, 1);
        sym_table.define_data(
            obj,
            self.rodata_id,
            "__quazi_fatal_exc",
            fatal_exc_offset,
            FATAL_EXC.len() as u64,
        );
        let fault_offset = obj.append_section_data(self.rodata_id, FAULT_PREFIX, 1);
        sym_table.define_data(
            obj,
            self.rodata_id,
            "__quazi_fault",
            fault_offset,
            FAULT_PREFIX.len() as u64,
        );
        let rip_offset = obj.append_section_data(self.rodata_id, RIP_PREFIX, 1);
        sym_table.define_data(
            obj,
            self.rodata_id,
            "__quazi_rip",
            rip_offset,
            RIP_PREFIX.len() as u64,
        );
        let hint_offset = obj.append_section_data(self.rodata_id, TRACE_HINT, 1);
        sym_table.define_data(
            obj,
            self.rodata_id,
            "__quazi_trace_hint",
            hint_offset,
            TRACE_HINT.len() as u64,
        );
        let env_offset = obj.append_section_data(self.rodata_id, ENV_VAR_NAME, 1);
        sym_table.define_data(
            obj,
            self.rodata_id,
            "__quazi_env_var_name",
            env_offset,
            ENV_VAR_NAME.len() as u64,
        );

        // Linux enhanced crash/panic handler strings
        let proc_offset = obj.append_section_data(self.rodata_id, b"/proc/self/environ\0", 1);
        sym_table.define_data(
            obj,
            self.rodata_id,
            "__quazi_proc_self_environ",
            proc_offset,
            19,
        );
        let fatal_offset = obj.append_section_data(self.rodata_id, b"fatal: signal 0x ", 1);
        sym_table.define_data(obj, self.rodata_id, "__quazi_fatal_sig", fatal_offset, 16);
        let at_addr_offset = obj.append_section_data(self.rodata_id, b" at address 0x", 1);
        sym_table.define_data(obj, self.rodata_id, "__quazi_at_addr", at_addr_offset, 14);
        let bt_prefix_offset = obj.append_section_data(self.rodata_id, b"stack backtrace:\n", 1);
        sym_table.define_data(
            obj,
            self.rodata_id,
            "__quazi_bt_prefix",
            bt_prefix_offset,
            17,
        );
        let at_0x_offset = obj.append_section_data(self.rodata_id, b"  at 0x", 1);
        sym_table.define_data(obj, self.rodata_id, "__quazi_at_0x", at_0x_offset, 7);
        let nl_offset = obj.append_section_data(self.rodata_id, b"\n", 1);
        sym_table.define_data(obj, self.rodata_id, "__quazi_nl", nl_offset, 1);

        let needs_fmt_ld = chunks.iter().any(|c| {
            c.code.iter().any(|i| {
                i.opcode == Opcode::PrimToStr as u8
                    || (i.opcode == Opcode::Intrinsic as u8 && i.ri16().1 == 15)
            })
        });
        if needs_fmt_ld {
            let fmt = b"%ld\0";
            let offset = obj.append_section_data(self.rodata_id, fmt, 1);
            sym_table.define_data(
                obj,
                self.rodata_id,
                "__quazi_fmt_ld",
                offset,
                fmt.len() as u64,
            );
        }

        let needs_fmt_g = chunks.iter().any(|c| {
            c.code.iter().any(|i| {
                (i.opcode == Opcode::Intrinsic as u8 && i.ri16().1 == 16)
                    || (i.opcode == Opcode::PrimToStr as u8 && i.ops[2] == 1)
            })
        });
        if needs_fmt_g {
            let fmt = b"%g\0";
            let offset = obj.append_section_data(self.rodata_id, fmt, 1);
            sym_table.define_data(
                obj,
                self.rodata_id,
                "__quazi_fmt_g",
                offset,
                fmt.len() as u64,
            );
        }

        // Format strings for PrimToStr extended tags (hex, octal, float precision).
        let fmt_specs: &[(u8, &[u8], &str)] = &[
            (3, b"%llx\0", "__quazi_fmt_llx"),
            (4, b"%llX\0", "__quazi_fmt_llX"),
            (5, b"%llo\0", "__quazi_fmt_llo"),
        ];
        for &(tag, fmt, sym) in fmt_specs {
            if chunks.iter().any(|c| {
                c.code
                    .iter()
                    .any(|i| i.opcode == Opcode::PrimToStr as u8 && i.ops[2] == tag)
            }) {
                let offset = obj.append_section_data(self.rodata_id, fmt, 1);
                sym_table.define_data(obj, self.rodata_id, sym, offset, fmt.len() as u64);
            }
        }
        for prec in 0u8..=9 {
            let tag = 20 + prec;
            if chunks.iter().any(|c| {
                c.code
                    .iter()
                    .any(|i| i.opcode == Opcode::PrimToStr as u8 && i.ops[2] == tag)
            }) {
                let fmt = format!("%.{}f\0", prec);
                let bytes = fmt.as_bytes().to_vec();
                let len = bytes.len() as u64;
                let offset = obj.append_section_data(self.rodata_id, &bytes, 1);
                let sym = format!("__quazi_fmt_prec_{}", prec);
                sym_table.define_data(obj, self.rodata_id, &sym, offset, len);
            }
        }

        chunks
            .iter()
            .map(|chunk| {
                let lbl = safe_label(&chunk.name);
                chunk
                    .constants
                    .iter()
                    .enumerate()
                    .map(|(i, entry)| match entry {
                        ConstPoolEntry::Str(s) => {
                            let mut bytes = s.as_bytes().to_vec();
                            bytes.push(0); // null terminator
                            let offset = obj.append_section_data(self.rodata_id, &bytes, 1);
                            let sym_name = format!("__quazi_str_{}_{}", lbl, i);
                            sym_table.define_data(
                                obj,
                                self.rodata_id,
                                &sym_name,
                                offset,
                                bytes.len() as u64,
                            );
                            Some(sym_name)
                        }
                        ConstPoolEntry::Bytes(value) => {
                            let mut bytes = Vec::with_capacity(8 + value.len());
                            bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
                            bytes.extend_from_slice(value);
                            let offset = obj.append_section_data(self.rodata_id, &bytes, 8);
                            let sym_name = format!("__quazi_bytes_{}_{}", lbl, i);
                            sym_table.define_data(
                                obj,
                                self.rodata_id,
                                &sym_name,
                                offset,
                                bytes.len() as u64,
                            );
                            Some(sym_name)
                        }
                        _ => None,
                    })
                    .collect()
            })
            .collect()
    }

    /// Lay out .data (zero-initialized): 32-byte buffer per PrimToStr instruction.
    /// Returns sym_name[chunk_idx][instr_idx] (None for non-PrimToStr instructions).
    pub fn build_bss(
        &self,
        obj: &mut Object<'_>,
        sym_table: &mut SymbolTable,
        chunks: &[Chunk],
    ) -> Vec<Vec<Option<String>>> {
        // Hex buffer for crash handler inline formatting (16 bytes for 64-bit addr).
        let hex_offset = obj.append_section_data(self.data_id, &[0u8; 16], 1);
        sym_table.define_data(obj, self.data_id, "__quazi_hex_buf", hex_offset, 16);
        // Trace-enable flag (set by _start when QUAZI_TRACE=1).
        let trace_offset = obj.append_section_data(self.data_id, &[0u8], 1);
        sym_table.define_data(obj, self.data_id, "__quazi_trace_enabled", trace_offset, 1);

        chunks
            .iter()
            .map(|chunk| {
                let lbl = safe_label(&chunk.name);
                chunk
                    .code
                    .iter()
                    .enumerate()
                    .map(|(idx, instr)| {
                        if instr.opcode != Opcode::PrimToStr as u8 {
                            return None;
                        }
                        // 80 bytes: enough for 64-bit binary (65 chars + null) and all other formats.
                        let zeroes = [0u8; 80];
                        let offset = obj.append_section_data(self.data_id, &zeroes, 8);
                        let sym_name = format!("__quazi_itoa_{}_{}", lbl, idx);
                        sym_table.define_data(obj, self.data_id, &sym_name, offset, 80);
                        Some(sym_name)
                    })
                    .collect()
            })
            .collect()
    }

    /// Emit vtable arrays in .rodata only for (type, trait) pairs referenced by compiled chunks.
    pub fn build_vtables(
        &self,
        obj: &mut Object<'_>,
        sym_table: &mut SymbolTable,
        report: &SemanticReport,
        fn_names: &[String],
        used_vtables: &std::collections::HashSet<(String, String)>,
    ) {
        let fmt = obj.format();
        for (type_name, trait_names) in &report.trait_impls {
            for trait_name in trait_names {
                if !used_vtables.contains(&(type_name.clone(), trait_name.clone())) {
                    continue;
                }
                let Some(slots) = report.trait_method_slots.get(trait_name.as_str()) else {
                    continue;
                };
                let n = slots.len();
                if n == 0 {
                    continue;
                }
                let zeros = vec![0u8; n * 8];
                let vtbl_sym = format!(
                    "__vtable_{}_{}",
                    safe_label(type_name),
                    safe_label(trait_name)
                );
                let base_offset = obj.append_section_data(self.rodata_id, &zeros, 8);
                let vtbl_id = sym_table.define_data(
                    obj,
                    self.rodata_id,
                    &vtbl_sym,
                    base_offset,
                    (n * 8) as u64,
                );
                let _ = vtbl_id;
                for (slot, method) in slots.iter().enumerate() {
                    let impl_fn = format!("{}.{}", type_name, method);
                    // Resolve to the safe-label name used in fn_names, falling back to safe_label.
                    let fn_sym = fn_names
                        .iter()
                        .find(|s| s.as_str() == safe_label(&impl_fn) || s.as_str() == impl_fn)
                        .cloned()
                        .unwrap_or_else(|| safe_label(&impl_fn));
                    let sym_id = sym_table
                        .get_defined(&fn_sym)
                        .unwrap_or_else(|| sym_table.get_or_add_undef(obj, &fn_sym));
                    let abs_flags = if fmt == BinaryFormat::Coff {
                        RelocationFlags::Generic {
                            kind: RelocationKind::Absolute,
                            encoding: RelocationEncoding::Generic,
                            size: 64,
                        }
                    } else {
                        RelocationFlags::Generic {
                            kind: RelocationKind::Absolute,
                            encoding: RelocationEncoding::Generic,
                            size: 64,
                        }
                    };
                    obj.add_relocation(
                        self.rodata_id,
                        Relocation {
                            offset: base_offset + (slot as u64) * 8,
                            symbol: sym_id,
                            addend: 0,
                            flags: abs_flags,
                        },
                    )
                    .ok();
                }
            }
        }
    }

    /// Write accumulated .text bytes into the object and return the text section offset.
    pub fn write_text(&self, obj: &mut Object<'_>, bytes: &[u8]) -> u64 {
        obj.append_section_data(self.text_id, bytes, 16)
    }
}
