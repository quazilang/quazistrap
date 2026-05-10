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

use crate::backend::target;
use crate::backend::{Backend, BackendError, ObjectFormat, ObjectOutput, TargetSpec};
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
pub struct PeBackend;

fn emit_native_object(
    chunks: &[Chunk],
    target: &TargetSpec,
    output_format: ObjectFormat,
    entry_name: &[u8],
) -> Result<ObjectOutput, BackendError> {
    let mut obj = Object::new(
        target.binary_format(),
        target.object_architecture(),
        Endianness::Little,
    );

    let mut sym_table = SymbolTable::new();
    let section_acc = SectionAccumulator::new(&mut obj);

    let str_syms_per_chunk = section_acc.build_rodata(&mut obj, &mut sym_table, chunks);
    let bss_syms_per_chunk = section_acc.build_bss(&mut obj, &mut sym_table, chunks);

    let mut text_bytes: Vec<u8> = Vec::new();
    let mut all_relocs: Vec<(relocations::PendingReloc, Option<String>)> = Vec::new();
    let fn_names: Vec<String> = chunks.iter().map(|c| safe_label(&c.name)).collect();

    for (chunk_idx, chunk) in chunks.iter().enumerate() {
        let fn_offset = text_bytes.len();
        let encoder = FnEncoder {
            chunk,
            fn_table: &fn_names,
            fn_offset,
            str_syms: &str_syms_per_chunk[chunk_idx],
            bss_syms: &bss_syms_per_chunk[chunk_idx],
            target,
        };

        let (fn_bytes, fn_relocs) = encoder.encode()?;

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

    if target.emit_start {
        let stub_offset = text_bytes.len();
        let stub = if target.os == target::Os::Windows {
            StartStub::generate_windows(stub_offset)
        } else {
            StartStub::generate(stub_offset)
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

    section_acc.write_text(&mut obj, &text_bytes);
    let bin_fmt = target.binary_format();

    for (reloc, _) in all_relocs {
        let sym_id = sym_table
            .get_defined(&reloc.symbol)
            .unwrap_or_else(|| sym_table.get_or_add_undef(&mut obj, &reloc.symbol));

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

    let bytes = obj.write().map_err(|e| BackendError(e.to_string()))?;
    Ok(ObjectOutput {
        bytes,
        format: output_format,
    })
}

impl Backend for ElfBackend {
    fn compile(&self, chunks: &[Chunk], target: &TargetSpec) -> Result<ObjectOutput, BackendError> {
        emit_native_object(chunks, target, ObjectFormat::Elf, b"_start")
    }
}

impl Backend for PeBackend {
    fn compile(&self, chunks: &[Chunk], target: &TargetSpec) -> Result<ObjectOutput, BackendError> {
        emit_native_object(chunks, target, ObjectFormat::PeCoff, b"mainCRTStartup")
    }
}
