// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

pub mod encoder;
pub mod relocations;
pub mod runtime;
pub mod sections;
pub mod start;
pub mod symbols;
pub mod sysv_abi;

use object::Endianness;
use object::SymbolScope;
use object::write::Object;

use crate::backend::target;
use crate::backend::{Backend, BackendError, ObjectFormat, ObjectOutput, TargetSpec};
use crate::bytecode::Chunk;
use crate::semantic::SemanticReport;

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
    report: Option<&SemanticReport>,
) -> Result<ObjectOutput, BackendError> {
    let main_takes_args = report.map(|r| r.main_takes_args).unwrap_or(false);
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
    // Intrinsic wrappers get a private prefix so their call_ext targets (e.g. "malloc")
    // resolve to the external CRT symbol, not back to the wrapper itself.
    let embedded_export_symbols: std::collections::HashSet<&str> = chunks
        .iter()
        .filter_map(|chunk| chunk.export.as_ref().map(|export| export.symbol.as_str()))
        .collect();
    let fn_names: Vec<String> = chunks
        .iter()
        .map(|c| {
            if let Some(export) = &c.export {
                export.symbol.clone()
            } else if c.intrinsic {
                format!("__quazi_intr_{}", safe_label(&c.name))
            } else if let Some(symbol) = report
                .and_then(|r| r.exported_symbols.get(&c.name))
                .filter(|symbol| !embedded_export_symbols.contains(symbol.as_str()))
            {
                symbol.clone()
            } else {
                safe_label(&c.name)
            }
        })
        .collect();

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

        let is_exported = chunk.export.is_some()
            || report.is_some_and(|r| {
                r.exported_symbols
                    .get(&chunk.name)
                    .is_some_and(|symbol| !embedded_export_symbols.contains(symbol.as_str()))
            });
        let scope = if chunk.intrinsic {
            SymbolScope::Compilation
        } else if is_exported {
            SymbolScope::Dynamic
        } else {
            SymbolScope::Linkage
        };
        sym_table.define_function(
            &mut obj,
            section_acc.text_id,
            &fn_names[chunk_idx],
            fn_offset as u64,
            fn_bytes.len() as u64,
            scope,
        );
        if chunk.export.is_some() {
            let adapter_name = safe_label(&chunk.name);
            if adapter_name != fn_names[chunk_idx] {
                // C consumers use the stable @export symbol, while a Quazi
                // callback conversion names the synthetic adapter directly.
                // Both symbols must resolve to the same machine-code range.
                sym_table.define_function(
                    &mut obj,
                    section_acc.text_id,
                    &adapter_name,
                    fn_offset as u64,
                    fn_bytes.len() as u64,
                    SymbolScope::Compilation,
                );
            }
        }

        for reloc in fn_relocs {
            all_relocs.push((reloc, None));
        }
        text_bytes.extend_from_slice(&fn_bytes);
    }

    if target.emit_start {
        let stub_offset = text_bytes.len();
        let no_crash = target.no_crash;
        let stub = if target.os == target::Os::Windows {
            StartStub::generate_windows(stub_offset, no_crash, main_takes_args)
        } else {
            StartStub::generate(stub_offset, no_crash, main_takes_args)
        };

        obj.add_symbol(object::write::Symbol {
            name: entry_name.to_vec(),
            value: (stub_offset + stub.start_offset) as u64,
            size: (stub.bytes.len() - stub.start_offset) as u64,
            kind: object::SymbolKind::Text,
            scope: object::SymbolScope::Linkage,
            weak: false,
            section: object::write::SymbolSection::Section(section_acc.text_id),
            flags: object::SymbolFlags::None,
        });

        for (name, offset_in_stub, size) in &stub.extra_symbols {
            sym_table.define_function(
                &mut obj,
                section_acc.text_id,
                name,
                (stub_offset + offset_in_stub) as u64,
                *size as u64,
                SymbolScope::Compilation,
            );
        }

        for reloc in stub.relocs {
            all_relocs.push((reloc, None));
        }
        text_bytes.extend_from_slice(&stub.bytes);
    }

    if target.os == target::Os::Linux {
        let requested_runtime_symbols: std::collections::HashSet<String> = all_relocs
            .iter()
            .filter_map(|(reloc, _)| {
                sym_table
                    .get_defined(&reloc.symbol)
                    .is_none()
                    .then(|| reloc.symbol.clone())
            })
            .collect();
        for function in
            runtime::generate_required(&requested_runtime_symbols).map_err(BackendError)?
        {
            let offset = text_bytes.len();
            sym_table.define_function(
                &mut obj,
                section_acc.text_id,
                function.name,
                offset as u64,
                function.bytes.len() as u64,
                SymbolScope::Compilation,
            );
            text_bytes.extend_from_slice(&function.bytes);
        }
    }

    section_acc.write_text(&mut obj, &text_bytes);

    // Collect VtableAddr refs from all chunks; only emit vtables actually used.
    let used_vtables: std::collections::HashSet<(String, String)> = chunks
        .iter()
        .flat_map(|c| c.constants.iter())
        .filter_map(|e| {
            if let crate::bytecode::chunk::ConstPoolEntry::VtableAddr(tn, tr) = e {
                Some((tn.clone(), tr.clone()))
            } else {
                None
            }
        })
        .collect();

    if let Some(rep) = report {
        section_acc.build_vtables(&mut obj, &mut sym_table, rep, &fn_names, &used_vtables);
    }

    let bin_fmt = target.binary_format();

    for (reloc, _) in all_relocs {
        let sym_id = sym_table.get_defined(&reloc.symbol).unwrap_or_else(|| {
            if reloc.kind == relocations::RelocKind::Pc32 {
                sym_table.get_or_add_undef_data(&mut obj, &reloc.symbol)
            } else {
                sym_table.get_or_add_undef(&mut obj, &reloc.symbol)
            }
        });

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
    fn compile(
        &self,
        chunks: &[Chunk],
        target: &TargetSpec,
        report: Option<&SemanticReport>,
    ) -> Result<ObjectOutput, BackendError> {
        emit_native_object(chunks, target, ObjectFormat::Elf, b"_start", report)
    }
}

impl Backend for PeBackend {
    fn compile(
        &self,
        chunks: &[Chunk],
        target: &TargetSpec,
        report: Option<&SemanticReport>,
    ) -> Result<ObjectOutput, BackendError> {
        emit_native_object(
            chunks,
            target,
            ObjectFormat::PeCoff,
            b"mainCRTStartup",
            report,
        )
    }
}

#[cfg(test)]
mod tests {
    use object::{Object as _, ObjectSymbol as _};

    use super::*;
    use crate::abi::{AbiSignature, AbiType, ForeignSymbol};
    use crate::backend::target::{Abi, Arch, Os};
    use crate::bytecode::chunk::ConstPoolEntry;
    use crate::bytecode::instruction::{ri16, rrr};
    use crate::bytecode::opcode::Opcode;

    #[test]
    fn coff_export_adapter_defines_stable_and_callback_symbols() {
        let adapter_name = "__quazi_export_adapter_increment_0";
        let mut main = Chunk::new("main");
        main.reg_count = 1;
        let address = main.add_constant(ConstPoolEntry::FnAddr(adapter_name.to_string()));
        main.emit(ri16(Opcode::MovConst, 0, address));
        main.emit(rrr(Opcode::Ret, 0, 0, 0));

        let mut adapter = Chunk::new(adapter_name);
        adapter.reg_count = 1;
        adapter.export = Some(ForeignSymbol {
            symbol: "increment".to_string(),
            signature: AbiSignature {
                params: Vec::new(),
                return_type: AbiType::Void,
                variadic: false,
            },
        });
        adapter.emit(rrr(Opcode::Ret, 0, 0, 0));

        let target = TargetSpec {
            arch: Arch::X86_64,
            os: Os::Windows,
            abi: Abi::Win64,
            emit_start: false,
            no_crash: false,
        };
        let output = PeBackend
            .compile(&[main, adapter], &target, None)
            .expect("COFF output should compile");
        let object = object::File::parse(output.bytes.as_slice()).expect("valid COFF object");

        for expected in ["increment", adapter_name] {
            let symbol = object
                .symbols()
                .find(|symbol| symbol.name() == Ok(expected))
                .unwrap_or_else(|| panic!("missing symbol `{expected}`"));
            assert!(
                !symbol.is_undefined(),
                "symbol `{expected}` must be defined"
            );
        }
    }

    #[test]
    fn linux_primitive_formatting_does_not_import_sprintf() {
        let mut main = Chunk::new("main");
        main.reg_count = 2;
        let value = main.add_constant(ConstPoolEntry::Float(1.5));
        main.emit(ri16(Opcode::MovConst, 0, value));
        main.emit(rrr(Opcode::PrimToStr, 1, 0, 1));
        main.emit(rrr(Opcode::Ret, 1, 0, 0));
        let target = TargetSpec {
            arch: Arch::X86_64,
            os: Os::Linux,
            abi: Abi::SysV,
            emit_start: false,
            no_crash: false,
        };
        let output = ElfBackend
            .compile(&[main], &target, None)
            .expect("ELF output should compile");
        let object = object::File::parse(output.bytes.as_slice()).expect("valid ELF object");
        assert!(
            object
                .symbols()
                .all(|symbol| symbol.name() != Ok("sprintf"))
        );
    }

    #[test]
    fn linux_objects_embed_requested_allocator_runtime() {
        let mut main = Chunk::new("main");
        main.reg_count = 1;
        main.emit(ri16(Opcode::MovI, 0, 32));
        main.emit(ri16(Opcode::Intrinsic, 0, 3));
        main.emit(rrr(Opcode::Ret, 0, 0, 0));
        let target = TargetSpec {
            arch: Arch::X86_64,
            os: Os::Linux,
            abi: Abi::SysV,
            emit_start: false,
            no_crash: false,
        };
        let output = ElfBackend
            .compile(&[main], &target, None)
            .expect("ELF output should compile");
        let object = object::File::parse(output.bytes.as_slice()).expect("valid ELF object");
        let malloc = object
            .symbols()
            .find(|symbol| symbol.name() == Ok("malloc"))
            .expect("malloc symbol");
        assert!(!malloc.is_undefined());
    }
}
