// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Minimal in-process linker for Quazi's own x86-64 ELF objects.
//!
//! The experimental linker accepts x86-64 ELF relocatable objects directly.
//! Unsupported external dependencies are explicit errors instead of silently
//! pulling in libc or another host runtime.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use object::{
    Object as _, ObjectSection as _, ObjectSymbol as _, RelocationKind, RelocationTarget,
    SectionIndex, SectionKind, SymbolIndex, SymbolSection,
};

const ELF_BASE: u64 = 0x400000;
const PAGE_SIZE: u64 = 0x1000;
const ELF_HEADER_SIZE: usize = 64;
const PROGRAM_HEADER_SIZE: usize = 56;
const PROGRAM_HEADER_COUNT: usize = 4;

#[derive(Debug, Clone)]
struct InputSection {
    object: usize,
    index: SectionIndex,
    kind: SectionKind,
    align: u64,
    data: Vec<u8>,
    memory_size: u64,
}

#[derive(Debug, Clone, Copy)]
struct SectionLayout {
    file_offset: u64,
    address: u64,
}

#[derive(Debug, Clone, Copy)]
struct SegmentLayout {
    file_offset: u64,
    address: u64,
    file_size: u64,
    memory_size: u64,
    flags: u32,
}

/// Link one or more x86-64 ELF relocatable objects into a static executable.
pub fn link_elf_objects(objects: &[&[u8]], output: &Path) -> Result<(), String> {
    if objects.is_empty() {
        return Err("built-in linker: no input objects".to_string());
    }
    let files = objects
        .iter()
        .enumerate()
        .map(|(index, bytes)| {
            object::File::parse(*bytes).map_err(|error| {
                format!(
                    "built-in linker: invalid input object #{}: {error}",
                    index + 1
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (index, file) in files.iter().enumerate() {
        if file.format() != object::BinaryFormat::Elf
            || file.architecture() != object::Architecture::X86_64
        {
            return Err(format!(
                "built-in linker: input object #{} is not x86-64 ELF",
                index + 1
            ));
        }
    }

    let mut sections = Vec::new();
    for (object, file) in files.iter().enumerate() {
        for section in file.sections() {
            let kind = section.kind();
            if !matches!(
                kind,
                SectionKind::Text
                    | SectionKind::ReadOnlyData
                    | SectionKind::ReadOnlyString
                    | SectionKind::Data
                    | SectionKind::UninitializedData
            ) {
                continue;
            }
            let memory_size = section.size();
            let data = if kind == SectionKind::UninitializedData {
                vec![
                    0;
                    usize::try_from(memory_size)
                        .map_err(|_| "built-in linker: section is too large".to_string())?
                ]
            } else {
                section
                    .uncompressed_data()
                    .map_err(|error| format!("built-in linker: cannot read section: {error}"))?
                    .into_owned()
            };
            sections.push(InputSection {
                object,
                index: section.index(),
                kind,
                align: section.align().max(1),
                data,
                memory_size,
            });
        }
    }

    let text_sections: Vec<_> = sections
        .iter()
        .filter(|section| section.kind == SectionKind::Text)
        .collect();
    let read_only_sections: Vec<_> = sections
        .iter()
        .filter(|section| {
            matches!(
                section.kind,
                SectionKind::ReadOnlyData | SectionKind::ReadOnlyString
            )
        })
        .collect();
    let data_sections: Vec<_> = sections
        .iter()
        .filter(|section| {
            matches!(
                section.kind,
                SectionKind::Data | SectionKind::UninitializedData
            )
        })
        .collect();

    if text_sections.is_empty() {
        return Err("built-in linker: object has no executable code".to_string());
    }

    let start_symbol = find_named_symbol(&files, "_start");
    let main_symbol = find_named_symbol(&files, "main");
    if start_symbol.is_none() && main_symbol.is_none() {
        return Err("built-in linker: object defines neither `_start` nor `main`".to_string());
    }

    let mut layouts = HashMap::new();
    let mut image = vec![0; PAGE_SIZE as usize];

    let text_start = PAGE_SIZE;
    let mut text_end = place_sections(&mut image, text_start, &text_sections, &mut layouts)?;
    // Compile-only objects deliberately omit the process entry point. Reserve
    // room for a tiny Linux entry stub when `build`/`run` receives only objects.
    let synthesized_entry = if start_symbol.is_none() {
        let offset = text_end;
        text_end = text_end
            .checked_add(15)
            .ok_or_else(|| "built-in linker: image size overflow".to_string())?;
        resize_to(&mut image, text_end)?;
        Some(offset)
    } else {
        None
    };
    let rodata_start = align_up(text_end, PAGE_SIZE)?;
    resize_to(&mut image, rodata_start)?;
    let rodata_end = place_sections(&mut image, rodata_start, &read_only_sections, &mut layouts)?;
    let data_start = align_up(rodata_end, PAGE_SIZE)?;
    resize_to(&mut image, data_start)?;
    let mut data_end = place_sections(&mut image, data_start, &data_sections, &mut layouts)?;

    let mut unresolved = BTreeSet::new();
    let mut symbol_addresses = HashMap::new();
    let mut global_addresses = HashMap::new();
    for (object, file) in files.iter().enumerate() {
        for symbol in file.symbols() {
            let address = match symbol.section() {
                SymbolSection::Section(section_index) => layouts
                    .get(&(object, section_index))
                    .map(|layout| layout.address + symbol.address()),
                SymbolSection::Absolute => Some(symbol.address()),
                SymbolSection::Common => {
                    data_end = align_up(data_end, symbol.address().max(1))?;
                    let address = ELF_BASE + data_end;
                    data_end = data_end
                        .checked_add(symbol.size())
                        .ok_or_else(|| "built-in linker: common symbol overflow".to_string())?;
                    resize_to(&mut image, data_end)?;
                    Some(address)
                }
                _ => None,
            };
            let Some(address) = address else { continue };
            symbol_addresses.insert((object, symbol.index()), address);
            if symbol.is_global()
                && let Ok(name) = symbol.name()
                && !name.is_empty()
            {
                match global_addresses.get(name).copied() {
                    None => {
                        global_addresses.insert(name.to_string(), (address, symbol.is_weak()));
                    }
                    Some((_, true)) if !symbol.is_weak() => {
                        global_addresses.insert(name.to_string(), (address, false));
                    }
                    Some((_, false)) if symbol.is_weak() => {}
                    Some((previous, true)) if symbol.is_weak() && previous == address => {}
                    Some(_) => {
                        return Err(format!("built-in linker: duplicate global symbol `{name}`"));
                    }
                }
            }
        }
    }
    for file in &files {
        for symbol in file
            .symbols()
            .filter(|symbol| symbol.section() == SymbolSection::Undefined && !symbol.is_weak())
        {
            if let Ok(name) = symbol.name()
                && !name.is_empty()
                && !global_addresses.contains_key(name)
            {
                unresolved.insert(name.to_string());
            }
        }
    }

    if !unresolved.is_empty() {
        return Err(format!(
            "built-in linker cannot resolve external symbol{}: {}\n\
             hint: implement the dependency in Quazi/runtime assembly, or explicitly select an \
             external linker and native library (for example `--linker ld.lld -l c`)",
            if unresolved.len() == 1 { "" } else { "s" },
            unresolved.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }

    for (object, file) in files.iter().enumerate() {
        for section in file.sections() {
            let Some(layout) = layouts.get(&(object, section.index())).copied() else {
                continue;
            };
            for (offset, relocation) in section.relocations() {
                let symbol_index = match relocation.target() {
                    RelocationTarget::Symbol(index) => index,
                    target => {
                        return Err(format!(
                            "built-in linker: unsupported relocation target {target:?}"
                        ));
                    }
                };
                let symbol = file
                    .symbol_by_index(symbol_index)
                    .map_err(|error| format!("built-in linker: invalid symbol: {error}"))?;
                let symbol_address = symbol_addresses
                    .get(&(object, symbol_index))
                    .copied()
                    .or_else(|| {
                        symbol
                            .name()
                            .ok()
                            .and_then(|name| global_addresses.get(name).map(|entry| entry.0))
                    })
                    .or_else(|| symbol.is_weak().then_some(0))
                    .ok_or_else(|| {
                        format!(
                            "built-in linker: relocation references unresolved symbol #{}",
                            symbol_index.0
                        )
                    })?;
                apply_relocation(
                    &mut image,
                    layout,
                    offset,
                    relocation.kind(),
                    relocation.size(),
                    relocation.addend(),
                    symbol_address,
                )?;
            }
        }
    }

    let entry = if let Some(offset) = synthesized_entry {
        let main = main_symbol
            .and_then(|key| symbol_addresses.get(&key).copied())
            .ok_or_else(|| "built-in linker: object does not define `main`".to_string())?;
        let entry = ELF_BASE + offset;
        let displacement = i32::try_from(i128::from(main) - i128::from(entry + 5))
            .map_err(|_| "built-in linker: `main` is out of call range".to_string())?;
        let mut stub = [0u8; 15];
        stub[0] = 0xe8; // call main
        stub[1..5].copy_from_slice(&displacement.to_le_bytes());
        stub[5..8].copy_from_slice(&[0x48, 0x89, 0xc7]); // mov rdi, rax
        stub[8..13].copy_from_slice(&[0xb8, 60, 0, 0, 0]); // mov eax, SYS_exit
        stub[13..15].copy_from_slice(&[0x0f, 0x05]); // syscall
        write_at(
            &mut image,
            usize::try_from(offset)
                .map_err(|_| "built-in linker: entry offset overflow".to_string())?,
            &stub,
        )?;
        entry
    } else {
        start_symbol
            .and_then(|key| symbol_addresses.get(&key).copied())
            .ok_or_else(|| "built-in linker: object does not define `_start`".to_string())?
    };

    let text_segment = SegmentLayout {
        file_offset: 0,
        address: ELF_BASE,
        file_size: text_end,
        memory_size: text_end,
        flags: 0x5, // PF_R | PF_X
    };
    let rodata_segment = SegmentLayout {
        file_offset: rodata_start,
        address: ELF_BASE + rodata_start,
        file_size: rodata_end.saturating_sub(rodata_start),
        memory_size: rodata_end.saturating_sub(rodata_start),
        flags: 0x4, // PF_R
    };
    let data_segment = SegmentLayout {
        file_offset: data_start,
        address: ELF_BASE + data_start,
        file_size: data_end.saturating_sub(data_start),
        memory_size: data_end.saturating_sub(data_start),
        flags: 0x6, // PF_R | PF_W
    };
    write_elf_headers(
        &mut image,
        entry,
        [text_segment, rodata_segment, data_segment],
    );

    std::fs::write(output, &image)
        .map_err(|error| format!("cannot write {}: {error}", output.display()))?;
    make_executable(output)?;
    Ok(())
}

fn place_sections(
    image: &mut Vec<u8>,
    start: u64,
    sections: &[&InputSection],
    layouts: &mut HashMap<(usize, SectionIndex), SectionLayout>,
) -> Result<u64, String> {
    let mut cursor = start;
    for section in sections {
        cursor = align_up(cursor, section.align)?;
        resize_to(image, cursor)?;
        let file_offset = cursor;
        image.extend_from_slice(&section.data);
        layouts.insert(
            (section.object, section.index),
            SectionLayout {
                file_offset,
                address: ELF_BASE + file_offset,
            },
        );
        cursor = cursor
            .checked_add(section.memory_size.max(section.data.len() as u64))
            .ok_or_else(|| "built-in linker: image size overflow".to_string())?;
        resize_to(image, cursor)?;
    }
    Ok(cursor)
}

fn apply_relocation(
    image: &mut [u8],
    section: SectionLayout,
    offset: u64,
    kind: RelocationKind,
    size: u8,
    addend: i64,
    symbol_address: u64,
) -> Result<(), String> {
    let place = section
        .address
        .checked_add(offset)
        .ok_or_else(|| "built-in linker: relocation address overflow".to_string())?;
    let file_offset = section
        .file_offset
        .checked_add(offset)
        .ok_or_else(|| "built-in linker: relocation offset overflow".to_string())?;
    let file_offset = usize::try_from(file_offset)
        .map_err(|_| "built-in linker: relocation is outside the image".to_string())?;

    match (kind, size) {
        (RelocationKind::Relative | RelocationKind::PltRelative, 32) => {
            let value = i128::from(symbol_address) + i128::from(addend) - i128::from(place);
            let value = i32::try_from(value).map_err(|_| {
                "built-in linker: PC-relative relocation is out of range".to_string()
            })?;
            write_at(image, file_offset, &value.to_le_bytes())
        }
        (RelocationKind::Absolute, 64) => {
            let value = i128::from(symbol_address) + i128::from(addend);
            let value = u64::try_from(value)
                .map_err(|_| "built-in linker: absolute relocation overflow".to_string())?;
            write_at(image, file_offset, &value.to_le_bytes())
        }
        _ => Err(format!(
            "built-in linker: unsupported {kind:?}/{size}-bit relocation"
        )),
    }
}

fn find_named_symbol<'data>(
    files: &[object::File<'data>],
    name: &str,
) -> Option<(usize, SymbolIndex)> {
    files.iter().enumerate().find_map(|(object, file)| {
        file.symbols()
            .find(|symbol| symbol.name() == Ok(name) && symbol.is_definition())
            .map(|symbol| (object, symbol.index()))
    })
}

fn write_at(image: &mut [u8], offset: usize, bytes: &[u8]) -> Result<(), String> {
    let end = offset
        .checked_add(bytes.len())
        .ok_or_else(|| "built-in linker: relocation offset overflow".to_string())?;
    let destination = image
        .get_mut(offset..end)
        .ok_or_else(|| "built-in linker: relocation is outside the image".to_string())?;
    destination.copy_from_slice(bytes);
    Ok(())
}

fn write_elf_headers(image: &mut [u8], entry: u64, segments: [SegmentLayout; 3]) {
    let header = &mut image[..ELF_HEADER_SIZE];
    header[..16].copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    put_u16(header, 16, 2); // ET_EXEC
    put_u16(header, 18, 62); // EM_X86_64
    put_u32(header, 20, 1);
    put_u64(header, 24, entry);
    put_u64(header, 32, ELF_HEADER_SIZE as u64);
    put_u64(header, 40, 0);
    put_u32(header, 48, 0);
    put_u16(header, 52, ELF_HEADER_SIZE as u16);
    put_u16(header, 54, PROGRAM_HEADER_SIZE as u16);
    put_u16(header, 56, PROGRAM_HEADER_COUNT as u16);
    put_u16(header, 58, 0);
    put_u16(header, 60, 0);
    put_u16(header, 62, 0);

    for (index, segment) in segments.into_iter().enumerate() {
        let offset = ELF_HEADER_SIZE + index * PROGRAM_HEADER_SIZE;
        let ph = &mut image[offset..offset + PROGRAM_HEADER_SIZE];
        put_u32(ph, 0, 1); // PT_LOAD
        put_u32(ph, 4, segment.flags);
        put_u64(ph, 8, segment.file_offset);
        put_u64(ph, 16, segment.address);
        put_u64(ph, 24, segment.address);
        put_u64(ph, 32, segment.file_size);
        put_u64(ph, 40, segment.memory_size);
        put_u64(ph, 48, PAGE_SIZE);
    }

    let offset = ELF_HEADER_SIZE + 3 * PROGRAM_HEADER_SIZE;
    let stack = &mut image[offset..offset + PROGRAM_HEADER_SIZE];
    put_u32(stack, 0, 0x6474_e551); // PT_GNU_STACK
    put_u32(stack, 4, 0x6); // PF_R | PF_W (never executable)
    put_u64(stack, 48, 16);
}

fn align_up(value: u64, align: u64) -> Result<u64, String> {
    let mask = align
        .checked_sub(1)
        .ok_or_else(|| "built-in linker: invalid zero alignment".to_string())?;
    if !align.is_power_of_two() {
        return Err(format!("built-in linker: unsupported alignment {align}"));
    }
    value
        .checked_add(mask)
        .map(|value| value & !mask)
        .ok_or_else(|| "built-in linker: alignment overflow".to_string())
}

fn resize_to(image: &mut Vec<u8>, size: u64) -> Result<(), String> {
    let size = usize::try_from(size)
        .map_err(|_| "built-in linker: output image is too large".to_string())?;
    if image.len() < size {
        image.resize(size, 0);
    }
    Ok(())
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    let mut permissions = std::fs::metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions)
        .map_err(|error| format!("cannot make {} executable: {error}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use object::write::{Object, Relocation, StandardSection, Symbol, SymbolSection};
    use object::{
        Architecture, BinaryFormat, Endianness, RelocationEncoding, RelocationFlags, SymbolFlags,
        SymbolKind, SymbolScope,
    };

    use super::*;
    use crate::backend::target::{Abi, Arch, Os};
    use crate::backend::{Backend, TargetSpec};
    use crate::bytecode::instruction::{ri16, rrr};
    use crate::bytecode::{Chunk, Opcode};

    fn tiny_object(with_undefined_symbol: bool) -> Vec<u8> {
        let mut object = Object::new(BinaryFormat::Elf, Architecture::X86_64, Endianness::Little);
        let text = object.section_id(StandardSection::Text);
        // exit(0): xor edi,edi; mov eax,60; syscall
        object.append_section_data(text, &[0x31, 0xff, 0xb8, 60, 0, 0, 0, 0x0f, 0x05], 16);
        object.add_symbol(Symbol {
            name: b"_start".to_vec(),
            value: 0,
            size: 9,
            kind: SymbolKind::Text,
            scope: SymbolScope::Linkage,
            weak: false,
            section: SymbolSection::Section(text),
            flags: SymbolFlags::None,
        });
        if with_undefined_symbol {
            let missing = object.add_symbol(Symbol {
                name: b"malloc".to_vec(),
                value: 0,
                size: 0,
                kind: SymbolKind::Text,
                scope: SymbolScope::Dynamic,
                weak: false,
                section: SymbolSection::Undefined,
                flags: SymbolFlags::None,
            });
            object
                .add_relocation(
                    text,
                    Relocation {
                        offset: 1,
                        symbol: missing,
                        addend: -4,
                        flags: RelocationFlags::Generic {
                            kind: RelocationKind::PltRelative,
                            encoding: RelocationEncoding::X86Branch,
                            size: 32,
                        },
                    },
                )
                .expect("relocation");
        }
        object.write().expect("object")
    }

    fn main_only_object() -> Vec<u8> {
        let mut object = Object::new(BinaryFormat::Elf, Architecture::X86_64, Endianness::Little);
        let text = object.section_id(StandardSection::Text);
        // Return 23 from main. The linker must turn that return value into the
        // process exit status without relying on CRT startup code.
        object.append_section_data(text, &[0xb8, 23, 0, 0, 0, 0xc3], 16);
        object.add_symbol(Symbol {
            name: b"main".to_vec(),
            value: 0,
            size: 6,
            kind: SymbolKind::Text,
            scope: SymbolScope::Linkage,
            weak: false,
            section: SymbolSection::Section(text),
            flags: SymbolFlags::None,
        });
        object.write().expect("object")
    }

    fn object_calling_helper() -> Vec<u8> {
        let mut object = Object::new(BinaryFormat::Elf, Architecture::X86_64, Endianness::Little);
        let text = object.section_id(StandardSection::Text);
        // call helper; mov rdi,rax; mov eax,60; syscall
        object.append_section_data(
            text,
            &[
                0xe8, 0, 0, 0, 0, 0x48, 0x89, 0xc7, 0xb8, 60, 0, 0, 0, 0x0f, 0x05,
            ],
            16,
        );
        object.add_symbol(Symbol {
            name: b"_start".to_vec(),
            value: 0,
            size: 15,
            kind: SymbolKind::Text,
            scope: SymbolScope::Linkage,
            weak: false,
            section: SymbolSection::Section(text),
            flags: SymbolFlags::None,
        });
        let helper = object.add_symbol(Symbol {
            name: b"helper".to_vec(),
            value: 0,
            size: 0,
            kind: SymbolKind::Text,
            scope: SymbolScope::Dynamic,
            weak: false,
            section: SymbolSection::Undefined,
            flags: SymbolFlags::None,
        });
        object
            .add_relocation(
                text,
                Relocation {
                    offset: 1,
                    symbol: helper,
                    addend: -4,
                    flags: RelocationFlags::Generic {
                        kind: RelocationKind::PltRelative,
                        encoding: RelocationEncoding::X86Branch,
                        size: 32,
                    },
                },
            )
            .expect("helper relocation");
        object.write().expect("object")
    }

    fn helper_object() -> Vec<u8> {
        let mut object = Object::new(BinaryFormat::Elf, Architecture::X86_64, Endianness::Little);
        let text = object.section_id(StandardSection::Text);
        object.append_section_data(text, &[0xb8, 23, 0, 0, 0, 0xc3], 16);
        object.add_symbol(Symbol {
            name: b"helper".to_vec(),
            value: 0,
            size: 6,
            kind: SymbolKind::Text,
            scope: SymbolScope::Linkage,
            weak: false,
            section: SymbolSection::Section(text),
            flags: SymbolFlags::None,
        });
        object.write().expect("object")
    }

    #[test]
    fn links_a_static_elf_without_an_external_tool() {
        let directory = std::env::temp_dir();
        let output = directory.join(format!("qz_builtin_linker_{}", std::process::id()));
        let object = tiny_object(false);
        link_elf_objects(&[&object], &output).expect("link");
        let bytes = std::fs::read(&output).expect("read output");
        let _ = std::fs::remove_file(output);

        assert_eq!(&bytes[..4], b"\x7fELF");
        assert_eq!(u16::from_le_bytes([bytes[16], bytes[17]]), 2);
        assert_eq!(u16::from_le_bytes([bytes[56], bytes[57]]), 4);
    }

    #[test]
    fn rejects_implicit_native_dependencies() {
        let output = std::env::temp_dir().join(format!(
            "qz_builtin_linker_unresolved_{}",
            std::process::id()
        ));
        let object = tiny_object(true);
        let error =
            link_elf_objects(&[&object], &output).expect_err("must reject unresolved symbol");
        assert!(error.contains("malloc"));
        assert!(error.contains("explicitly select an external linker"));
    }

    #[test]
    fn synthesizes_start_for_compile_only_objects() {
        let output = std::env::temp_dir().join(format!(
            "qz_builtin_linker_main_only_{}",
            std::process::id()
        ));
        let object = main_only_object();
        link_elf_objects(&[&object], &output).expect("link main-only object");
        let bytes = std::fs::read(&output).expect("read output");
        let _ = std::fs::remove_file(output);
        let entry = u64::from_le_bytes(bytes[24..32].try_into().expect("entry bytes"));
        assert_eq!(entry, ELF_BASE + PAGE_SIZE + 6);
        assert_eq!(bytes[(PAGE_SIZE + 6) as usize], 0xe8);
    }

    #[test]
    fn resolves_symbols_across_multiple_objects() {
        let caller = object_calling_helper();
        let helper = helper_object();
        let output = std::env::temp_dir().join(format!(
            "qz_builtin_linker_multiple_objects_{}",
            std::process::id()
        ));
        link_elf_objects(&[&caller, &helper], &output).expect("link multiple objects");
        let bytes = std::fs::read(&output).expect("read output");
        let _ = std::fs::remove_file(output);
        assert_eq!(&bytes[..4], b"\x7fELF");
        assert_ne!(
            &bytes[(PAGE_SIZE + 1) as usize..(PAGE_SIZE + 5) as usize],
            &[0; 4]
        );
    }

    #[test]
    fn links_a_real_quazi_object_without_libc() {
        let mut main = Chunk::new("main");
        main.reg_count = 3;
        main.emit(ri16(Opcode::MovI, 0, 64));
        main.emit(ri16(Opcode::Intrinsic, 0, 3)); // embedded malloc
        main.emit(ri16(Opcode::MovI, 1, 0x41));
        main.emit(ri16(Opcode::MovI, 2, 64));
        main.emit(ri16(Opcode::Intrinsic, 0, 7)); // embedded memset
        main.emit(ri16(Opcode::Intrinsic, 0, 4)); // embedded free
        main.emit(ri16(Opcode::MovI, 0, 7));
        main.emit(rrr(Opcode::Ret, 0, 0, 0));
        let target = TargetSpec {
            arch: Arch::X86_64,
            os: Os::Linux,
            abi: Abi::SysV,
            emit_start: true,
            no_crash: false,
        };
        let object = crate::backend::x86_64::ElfBackend
            .compile(&[main], &target, None)
            .expect("compile Linux object");
        let output = std::env::temp_dir().join(format!(
            "qz_builtin_linker_real_object_{}",
            std::process::id()
        ));
        link_elf_objects(&[&object.bytes], &output).expect("link compiler object");
        let executable = std::fs::read(&output).expect("read executable");
        let _ = std::fs::remove_file(output);
        assert_eq!(&executable[..4], b"\x7fELF");
    }
}
