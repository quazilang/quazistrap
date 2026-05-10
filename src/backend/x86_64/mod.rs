// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

pub mod encoder;
pub mod relocations;
pub mod sections;
pub mod start;
pub mod symbols;

use object::Endianness;
use object::write::Object;

use crate::backend::{Backend, BackendError, ObjectFormat, ObjectOutput, TargetSpec, target::Os};
use crate::bytecode::Chunk;

use encoder::FnEncoder;
use relocations::write_reloc;
use sections::SectionAccumulator;
use start::StartStub;
use symbols::SymbolTable;

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

pub struct ElfBackend;

impl Backend for ElfBackend {
    fn compile(&self, chunks: &[Chunk], target: &TargetSpec) -> Result<ObjectOutput, BackendError> {
        let mut obj = Object::new(
            target.binary_format(),
            target.object_architecture(),
            Endianness::Little,
        );

        let mut sym_table = SymbolTable::new();
        let section_acc = SectionAccumulator::new(&mut obj);

        // 1. Build .rodata (strings) and .data (PrimToStr buffers).
        let str_syms_per_chunk = section_acc.build_rodata(&mut obj, &mut sym_table, chunks);
        let bss_syms_per_chunk = section_acc.build_bss(&mut obj, &mut sym_table, chunks);

        // 2. Encode each chunk to machine code, accumulating into text_bytes.
        let mut text_bytes: Vec<u8> = Vec::new();
        let mut all_relocs: Vec<(relocations::PendingReloc, Option<String>)> = Vec::new();

        let fn_names: Vec<String> = chunks.iter().map(|c| safe_label(&c.name)).collect();

        for (chunk_idx, chunk) in chunks.iter().enumerate() {
            let fn_offset = text_bytes.len();
            let str_syms = &str_syms_per_chunk[chunk_idx];
            let bss_syms = &bss_syms_per_chunk[chunk_idx];

            let encoder = FnEncoder {
                chunk,
                fn_table: &fn_names,
                fn_offset,
                str_syms,
                bss_syms,
                target,
            };

            let (fn_bytes, fn_relocs) = encoder.encode()?;

            // Define function symbol before appending bytes so the offset is correct.
            sym_table.define_function(
                &mut obj,
                section_acc.text_id,
                &safe_label(&chunk.name),
                fn_offset as u64,
                fn_bytes.len() as u64,
            );

            for reloc in fn_relocs {
                all_relocs.push((reloc, None));
            }
            text_bytes.extend_from_slice(&fn_bytes);
        }

        // 3. Append entry stub when requested.
        if target.emit_start {
            let stub_offset = text_bytes.len();
            let (stub, entry_name): (StartStub, &[u8]) = match target.os {
                Os::Windows => (StartStub::generate_windows(stub_offset), b"mainCRTStartup"),
                _ => (StartStub::generate(stub_offset), b"_start"),
            };

            obj.add_symbol(object::write::Symbol {
                name: entry_name.to_vec(),
                value: stub_offset as u64,
                size: stub.bytes.len() as u64,
                kind: object::SymbolKind::Text,
                scope: object::SymbolScope::Linkage,
                weak: false,
                section: object::write::SymbolSection::Section(section_acc.text_id),
                flags: object::SymbolFlags::None,
            });

            for reloc in stub.relocs {
                all_relocs.push((reloc, None));
            }
            text_bytes.extend_from_slice(&stub.bytes);
        }

        // 4. Write accumulated .text bytes to the object.
        let text_base = section_acc.write_text(&mut obj, &text_bytes);
        debug_assert_eq!(text_base, 0, "text section should start at offset 0");

        // 5. Write all relocations.
        let bin_fmt = target.binary_format();
        for (reloc, _) in all_relocs {
            let sym_id = if sym_table.get_defined(&reloc.symbol).is_some() {
                sym_table.get_defined(&reloc.symbol).unwrap()
            } else {
                sym_table.get_or_add_undef(&mut obj, &reloc.symbol)
            };
            write_reloc(
                &mut obj,
                section_acc.text_id,
                reloc.offset_in_text as u64,
                reloc.kind,
                sym_id,
                reloc.addend,
                bin_fmt,
            );
        }

        // 6. Serialize.
        let bytes = obj.write().map_err(|e| BackendError(e.to_string()))?;
        let format = match target.os {
            Os::Windows => ObjectFormat::PeCoff,
            _ => ObjectFormat::Elf,
        };
        Ok(ObjectOutput { bytes, format })
    }
}
