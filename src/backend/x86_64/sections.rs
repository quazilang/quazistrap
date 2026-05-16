// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use object::write::{Object, SectionId, StandardSection};

use crate::bytecode::{Chunk, ConstPoolEntry, Opcode};

use super::symbols::SymbolTable;

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
    /// Adds a data symbol for each string entry and for __void_fmt_ld.
    /// Returns the symbol name for each const-pool slot (None for non-Str entries).
    pub fn build_rodata(
        &self,
        obj: &mut Object<'_>,
        sym_table: &mut SymbolTable,
        chunks: &[Chunk],
    ) -> Vec<Vec<Option<String>>> {
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
                "__void_fmt_ld",
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
                "__void_fmt_g",
                offset,
                fmt.len() as u64,
            );
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
                            let sym_name = format!("__void_str_{}_{}", lbl, i);
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
                        let zeroes = [0u8; 32];
                        let offset = obj.append_section_data(self.data_id, &zeroes, 8);
                        let sym_name = format!("__void_itoa_{}_{}", lbl, idx);
                        sym_table.define_data(obj, self.data_id, &sym_name, offset, 32);
                        Some(sym_name)
                    })
                    .collect()
            })
            .collect()
    }

    /// Write accumulated .text bytes into the object and return the text section offset.
    pub fn write_text(&self, obj: &mut Object<'_>, bytes: &[u8]) -> u64 {
        obj.append_section_data(self.text_id, bytes, 16)
    }
}
