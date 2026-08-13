// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Minimal in-process PE32+ linker for compiler-produced x86-64 COFF objects.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use object::{
    Object as _, ObjectSection as _, ObjectSymbol as _, RelocationKind, RelocationTarget,
    SectionIndex, SectionKind, SymbolIndex, SymbolSection,
};

const IMAGE_BASE: u64 = 0x140000000;
const SECTION_ALIGN: u32 = 0x1000;
const FILE_ALIGN: u32 = 0x200;
const HEADERS: u32 = 0x400;

fn align(value: u32, alignment: u32) -> Result<u32, String> {
    value
        .checked_add(alignment - 1)
        .map(|v| v & !(alignment - 1))
        .ok_or_else(|| "built-in PE linker: image size overflow".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use object::write::{Object, Symbol, SymbolSection};
    use object::{
        Architecture, BinaryFormat, Endianness, SectionKind, SymbolFlags, SymbolKind, SymbolScope,
    };

    #[test]
    fn links_minimal_coff_executable() {
        let mut object = Object::new(BinaryFormat::Coff, Architecture::X86_64, Endianness::Little);
        let text = object.add_section(Vec::new(), b".text".to_vec(), SectionKind::Text);
        object.append_section_data(text, &[0x31, 0xc0, 0xc3], 16);
        object.add_symbol(Symbol {
            name: b"mainCRTStartup".to_vec(),
            value: 0,
            size: 3,
            kind: SymbolKind::Text,
            scope: SymbolScope::Dynamic,
            weak: false,
            section: SymbolSection::Section(text),
            flags: SymbolFlags::None,
        });
        let bytes = object.write().expect("write COFF");
        let output = std::env::temp_dir().join(format!("qz_pe_link_{}.exe", std::process::id()));
        link_coff_objects(&[&bytes], &output).expect("link PE");
        let image = std::fs::read(&output).expect("read PE");
        let parsed = object::File::parse(image.as_slice()).expect("parse PE");
        assert_eq!(parsed.format(), BinaryFormat::Pe);
        assert_eq!(parsed.kind(), object::ObjectKind::Executable);
        let _ = std::fs::remove_file(output);
    }
}

fn dll_for(symbol: &str) -> Option<&'static str> {
    if matches!(symbol, "CommandLineToArgvW") {
        return Some("SHELL32.dll");
    }
    if matches!(
        symbol,
        "WSAStartup"
            | "WSACleanup"
            | "WSAGetLastError"
            | "socket"
            | "closesocket"
            | "bind"
            | "listen"
            | "accept"
            | "connect"
            | "send"
            | "recv"
            | "inet_pton"
            | "setsockopt"
            | "shutdown"
            | "getaddrinfo"
            | "freeaddrinfo"
    ) {
        return Some("WS2_32.dll");
    }
    if matches!(
        symbol,
        "printf"
            | "sprintf"
            | "snprintf"
            | "malloc"
            | "calloc"
            | "realloc"
            | "free"
            | "memcpy"
            | "memmove"
            | "memset"
            | "memcmp"
            | "strlen"
    ) {
        return Some("ucrtbase.dll");
    }
    // Quazi's CRT-free Windows backend imports Win32 APIs from Kernel32.
    if symbol
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
    {
        return Some("KERNEL32.dll");
    }
    None
}

pub fn link_coff_objects(objects: &[&[u8]], output: &Path) -> Result<(), String> {
    let files = objects
        .iter()
        .enumerate()
        .map(|(i, bytes)| {
            object::File::parse(*bytes)
                .map_err(|e| format!("built-in PE linker: invalid object #{}: {e}", i + 1))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for file in &files {
        if file.format() != object::BinaryFormat::Coff
            || file.architecture() != object::Architecture::X86_64
        {
            return Err("built-in PE linker accepts x86-64 COFF objects only".into());
        }
    }

    let mut section_data = [Vec::<u8>::new(), Vec::new(), Vec::new()];
    let mut section_offsets = HashMap::<(usize, SectionIndex), u32>::new();
    for (object_index, file) in files.iter().enumerate() {
        for section in file.sections() {
            let bucket = match section.kind() {
                SectionKind::Text => 0,
                SectionKind::ReadOnlyData | SectionKind::ReadOnlyString => 1,
                SectionKind::Data | SectionKind::UninitializedData => 2,
                _ => continue,
            };
            let offset = align(
                section_data[bucket].len() as u32,
                section.align().max(1) as u32,
            )?;
            section_data[bucket].resize(offset as usize, 0);
            let data = if section.kind() == SectionKind::UninitializedData {
                vec![0; section.size() as usize]
            } else {
                section
                    .uncompressed_data()
                    .map_err(|e| format!("built-in PE linker: cannot read section: {e}"))?
                    .into_owned()
            };
            section_offsets.insert((object_index, section.index()), offset);
            section_data[bucket].extend_from_slice(&data);
        }
    }

    let mut definitions = HashMap::<String, (usize, SymbolIndex)>::new();
    let mut imports = BTreeMap::<String, String>::new();
    for (object_index, file) in files.iter().enumerate() {
        for symbol in file.symbols() {
            let name = symbol.name().unwrap_or("");
            if name.is_empty() {
                continue;
            }
            if matches!(symbol.section(), SymbolSection::Section(_))
                && symbol.is_global()
                && definitions
                    .insert(name.into(), (object_index, symbol.index()))
                    .is_some()
            {
                return Err(format!("built-in PE linker: duplicate symbol `{name}`"));
            }
        }
    }
    for file in &files {
        for symbol in file
            .symbols()
            .filter(|s| s.section() == SymbolSection::Undefined && !s.is_weak())
        {
            let name = symbol.name().unwrap_or("");
            if name.is_empty() || definitions.contains_key(name) {
                continue;
            }
            let dll = dll_for(name).ok_or_else(|| format!(
                "built-in PE linker cannot resolve `{name}`; select an external linker and library"))?;
            imports.insert(name.into(), dll.into());
        }
    }

    // One 6-byte `jmp [rip+IAT]` thunk per imported function.
    let thunk_start = align(section_data[0].len() as u32, 16)?;
    section_data[0].resize(thunk_start as usize, 0);
    let import_names: Vec<String> = imports.keys().cloned().collect();
    for _ in &import_names {
        section_data[0].extend_from_slice(&[0xff, 0x25, 0, 0, 0, 0]);
    }

    let text_rva = SECTION_ALIGN;
    let rdata_rva = align(text_rva + section_data[0].len() as u32, SECTION_ALIGN)?;
    let data_rva = align(rdata_rva + section_data[1].len() as u32, SECTION_ALIGN)?;
    let idata_rva = align(data_rva + section_data[2].len() as u32, SECTION_ALIGN)?;

    let bucket_rva = [text_rva, rdata_rva, data_rva];
    let symbol_rva = |object_index: usize, symbol: object::Symbol<'_, '_>| -> Result<u32, String> {
        match symbol.section() {
            SymbolSection::Section(index) => {
                let section = files[object_index]
                    .section_by_index(index)
                    .map_err(|e| e.to_string())?;
                let bucket = match section.kind() {
                    SectionKind::Text => 0,
                    SectionKind::ReadOnlyData | SectionKind::ReadOnlyString => 1,
                    SectionKind::Data | SectionKind::UninitializedData => 2,
                    _ => return Err("built-in PE linker: symbol in unsupported section".into()),
                };
                Ok(bucket_rva[bucket]
                    + section_offsets[&(object_index, index)]
                    + symbol.address() as u32)
            }
            SymbolSection::Absolute => Ok(symbol.address() as u32),
            _ => Err("built-in PE linker: unresolved symbol".into()),
        }
    };

    // Build import directory grouped by DLL.
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    for (name, dll) in &imports {
        groups.entry(dll.clone()).or_default().push(name.clone());
    }
    let descriptor_size = ((groups.len() + 1) * 20) as u32;
    let mut idata = vec![0; descriptor_size as usize];
    let mut import_iat_rva = HashMap::<String, u32>::new();
    let mut first_iat = 0;
    let mut total_iat_size = 0;
    for (descriptor, (dll, names)) in groups.iter().enumerate() {
        let ilt_off = align(idata.len() as u32, 8)?;
        idata.resize(ilt_off as usize, 0);
        idata.resize(idata.len() + (names.len() + 1) * 8, 0);
        let iat_off = align(idata.len() as u32, 8)?;
        idata.resize(iat_off as usize, 0);
        idata.resize(idata.len() + (names.len() + 1) * 8, 0);
        if first_iat == 0 {
            first_iat = idata_rva + iat_off;
        }
        total_iat_size += ((names.len() + 1) * 8) as u32;
        let dll_off = idata.len() as u32;
        idata.extend_from_slice(dll.as_bytes());
        idata.push(0);
        for (i, name) in names.iter().enumerate() {
            let hint_off = align(idata.len() as u32, 2)?;
            idata.resize(hint_off as usize, 0);
            idata.extend_from_slice(&[0, 0]);
            idata.extend_from_slice(name.as_bytes());
            idata.push(0);
            let value = u64::from(idata_rva + hint_off).to_le_bytes();
            idata[ilt_off as usize + i * 8..ilt_off as usize + i * 8 + 8].copy_from_slice(&value);
            idata[iat_off as usize + i * 8..iat_off as usize + i * 8 + 8].copy_from_slice(&value);
            import_iat_rva.insert(name.clone(), idata_rva + iat_off + (i * 8) as u32);
        }
        let base = descriptor * 20;
        idata[base..base + 4].copy_from_slice(&(idata_rva + ilt_off).to_le_bytes());
        idata[base + 12..base + 16].copy_from_slice(&(idata_rva + dll_off).to_le_bytes());
        idata[base + 16..base + 20].copy_from_slice(&(idata_rva + iat_off).to_le_bytes());
    }
    for (i, name) in import_names.iter().enumerate() {
        let thunk_rva = text_rva + thunk_start + (i * 6) as u32;
        let displacement = import_iat_rva[name] as i64 - (thunk_rva + 6) as i64;
        section_data[0][thunk_start as usize + i * 6 + 2..thunk_start as usize + i * 6 + 6]
            .copy_from_slice(&(displacement as i32).to_le_bytes());
    }

    let thunk_rvas: HashMap<String, u32> = import_names
        .iter()
        .enumerate()
        .map(|(i, name)| (name.clone(), text_rva + thunk_start + (i * 6) as u32))
        .collect();
    for (object_index, file) in files.iter().enumerate() {
        for section in file.sections() {
            let bucket = match section.kind() {
                SectionKind::Text => 0,
                SectionKind::ReadOnlyData | SectionKind::ReadOnlyString => 1,
                SectionKind::Data | SectionKind::UninitializedData => 2,
                _ => continue,
            };
            let section_base = section_offsets[&(object_index, section.index())];
            for (offset, relocation) in section.relocations() {
                let RelocationTarget::Symbol(index) = relocation.target() else {
                    return Err("built-in PE linker: section relocations are unsupported".into());
                };
                let symbol = file.symbol_by_index(index).map_err(|e| e.to_string())?;
                let name = symbol.name().unwrap_or("");
                let target_rva = if symbol.section() == SymbolSection::Undefined {
                    if let Some((owner, defined)) = definitions.get(name) {
                        symbol_rva(
                            *owner,
                            files[*owner]
                                .symbol_by_index(*defined)
                                .map_err(|e| e.to_string())?,
                        )?
                    } else {
                        *thunk_rvas
                            .get(name)
                            .ok_or_else(|| format!("unresolved `{name}`"))?
                    }
                } else {
                    symbol_rva(object_index, symbol)?
                };
                let place = bucket_rva[bucket] + section_base + offset as u32;
                let at = (section_base + offset as u32) as usize;
                match relocation.kind() {
                    RelocationKind::Relative | RelocationKind::PltRelative => {
                        let implicit = i32::from_le_bytes(
                            section_data[bucket][at..at + 4].try_into().unwrap(),
                        ) as i64;
                        let value =
                            target_rva as i64 + relocation.addend() + implicit - place as i64;
                        section_data[bucket][at..at + 4]
                            .copy_from_slice(&(value as i32).to_le_bytes());
                    }
                    RelocationKind::ImageOffset => {
                        let value = target_rva as i64 + relocation.addend();
                        section_data[bucket][at..at + 4]
                            .copy_from_slice(&(value as u32).to_le_bytes());
                    }
                    RelocationKind::Absolute if relocation.size() == 64 => {
                        let value = IMAGE_BASE + target_rva as u64 + relocation.addend() as u64;
                        section_data[bucket][at..at + 8].copy_from_slice(&value.to_le_bytes());
                    }
                    kind => {
                        return Err(format!(
                            "built-in PE linker: unsupported relocation {kind:?}/{}",
                            relocation.size()
                        ));
                    }
                }
            }
        }
    }

    let entry = definitions
        .get("mainCRTStartup")
        .or_else(|| definitions.get("main"))
        .ok_or("built-in PE linker: object defines neither `mainCRTStartup` nor `main`")?;
    let entry_rva = symbol_rva(
        entry.0,
        files[entry.0]
            .symbol_by_index(entry.1)
            .map_err(|e| e.to_string())?,
    )?;
    write_pe(
        output,
        &section_data,
        &idata,
        entry_rva,
        idata_rva,
        descriptor_size,
        first_iat,
        total_iat_size,
    )
}

fn write_pe(
    output: &Path,
    sections: &[Vec<u8>; 3],
    idata: &[u8],
    entry: u32,
    idata_rva: u32,
    import_size: u32,
    iat_rva: u32,
    iat_size: u32,
) -> Result<(), String> {
    let rvas = [
        SECTION_ALIGN,
        align(SECTION_ALIGN + sections[0].len() as u32, SECTION_ALIGN)?,
        0,
        idata_rva,
    ];
    let data_rva = align(rvas[1] + sections[1].len() as u32, SECTION_ALIGN)?;
    let rvas = [rvas[0], rvas[1], data_rva, rvas[3]];
    let contents = [&sections[0][..], &sections[1][..], &sections[2][..], idata];
    let mut files = [0u32; 4];
    let mut cursor = HEADERS;
    for i in 0..4 {
        files[i] = cursor;
        cursor += align(contents[i].len() as u32, FILE_ALIGN)?;
    }
    let image_size = align(idata_rva + idata.len() as u32, SECTION_ALIGN)?;
    let mut image = vec![0; cursor as usize];
    image[0..2].copy_from_slice(b"MZ");
    image[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    let pe = 0x80usize;
    image[pe..pe + 4].copy_from_slice(b"PE\0\0");
    let coff = pe + 4;
    image[coff..coff + 2].copy_from_slice(&0x8664u16.to_le_bytes());
    image[coff + 2..coff + 4].copy_from_slice(&4u16.to_le_bytes());
    image[coff + 16..coff + 18].copy_from_slice(&240u16.to_le_bytes());
    image[coff + 18..coff + 20].copy_from_slice(&0x0022u16.to_le_bytes());
    let o = coff + 20;
    image[o..o + 2].copy_from_slice(&0x20bu16.to_le_bytes());
    image[o + 2] = 14;
    image[o + 4..o + 8]
        .copy_from_slice(&align(sections[0].len() as u32, FILE_ALIGN)?.to_le_bytes());
    let initialized = align(sections[1].len() as u32, FILE_ALIGN)?
        + align(sections[2].len() as u32, FILE_ALIGN)?
        + align(idata.len() as u32, FILE_ALIGN)?;
    image[o + 8..o + 12].copy_from_slice(&initialized.to_le_bytes());
    image[o + 16..o + 20].copy_from_slice(&entry.to_le_bytes());
    image[o + 20..o + 24].copy_from_slice(&rvas[0].to_le_bytes());
    image[o + 24..o + 32].copy_from_slice(&IMAGE_BASE.to_le_bytes());
    image[o + 32..o + 36].copy_from_slice(&SECTION_ALIGN.to_le_bytes());
    image[o + 36..o + 40].copy_from_slice(&FILE_ALIGN.to_le_bytes());
    image[o + 40..o + 42].copy_from_slice(&6u16.to_le_bytes());
    image[o + 48..o + 50].copy_from_slice(&6u16.to_le_bytes());
    image[o + 56..o + 60].copy_from_slice(&image_size.to_le_bytes());
    image[o + 60..o + 64].copy_from_slice(&HEADERS.to_le_bytes());
    image[o + 68..o + 70].copy_from_slice(&3u16.to_le_bytes());
    image[o + 70..o + 72].copy_from_slice(&0x8100u16.to_le_bytes());
    image[o + 72..o + 80].copy_from_slice(&0x100000u64.to_le_bytes());
    image[o + 80..o + 88].copy_from_slice(&0x1000u64.to_le_bytes());
    image[o + 88..o + 96].copy_from_slice(&0x100000u64.to_le_bytes());
    image[o + 96..o + 104].copy_from_slice(&0x1000u64.to_le_bytes());
    image[o + 108..o + 112].copy_from_slice(&16u32.to_le_bytes());
    image[o + 120..o + 124].copy_from_slice(&idata_rva.to_le_bytes());
    image[o + 124..o + 128].copy_from_slice(&import_size.to_le_bytes());
    image[o + 208..o + 212].copy_from_slice(&iat_rva.to_le_bytes());
    image[o + 212..o + 216].copy_from_slice(&iat_size.to_le_bytes());
    let names = [b".text\0\0\0", b".rdata\0\0", b".data\0\0\0", b".idata\0\0"];
    let chars = [0x60000020u32, 0x40000040, 0xc0000040, 0xc0000040];
    let sh = o + 240;
    for i in 0..4 {
        let p = sh + i * 40;
        image[p..p + 8].copy_from_slice(names[i]);
        image[p + 8..p + 12].copy_from_slice(&(contents[i].len() as u32).to_le_bytes());
        image[p + 12..p + 16].copy_from_slice(&rvas[i].to_le_bytes());
        image[p + 16..p + 20]
            .copy_from_slice(&align(contents[i].len() as u32, FILE_ALIGN)?.to_le_bytes());
        image[p + 20..p + 24].copy_from_slice(&files[i].to_le_bytes());
        image[p + 36..p + 40].copy_from_slice(&chars[i].to_le_bytes());
        image[files[i] as usize..files[i] as usize + contents[i].len()]
            .copy_from_slice(contents[i]);
    }
    std::fs::write(output, image).map_err(|e| format!("cannot write '{}': {e}", output.display()))
}
