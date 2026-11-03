// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Small NetBSD/amd64 dynamic ELF linker.
//!
//! Unlike the Linux freestanding ELF writer, the NetBSD port uses libc.so.12.
//! Undefined function symbols are resolved via eager `R_X86_64_GLOB_DAT`
//! relocations into GOT slots and RIP-relative indirect jump trampolines.
//! This avoids a lazy PLT resolver while preserving the System V ABI. It does
//! not attempt to implement a general-purpose libc archive or shared-object
//! linker: native archives and arbitrary shared libraries use the external
//! NetBSD linker path.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use object::{Object as _, ObjectSection as _, ObjectSymbol as _, RelocationKind, RelocationTarget,
    SectionFlags, SectionIndex, SectionKind, SymbolIndex, SymbolSection};

const BASE: u64 = 0x400000;
const PAGE: u64 = 0x1000;
const EH: usize = 64;
const PH: usize = 56;
const PH_COUNT: usize = 8;
const INTERPRETER: &[u8] = b"/usr/libexec/ld.elf_so\0";
const LIBC: &[u8] = b"libc.so.12\0";
// From NetBSD 11.0 amd64 .note.netbsd.ident: namesz=7, descsz=4,
// type=NT_NETBSD_VERSION, vendor=NetBSD, version 1100000000.
pub(super) const NETBSD_NOTE: [u8; 24] = [
    7, 0, 0, 0, 4, 0, 0, 0, 1, 0, 0, 0,
    b'N', b'e', b't', b'B', b'S', b'D', 0, 0,
    0x00, 0xab, 0x90, 0x41,
];

#[derive(Clone)]
struct InputSection { object: usize, index: SectionIndex, kind: SectionKind,
    align: u64, data: Vec<u8>, size: u64 }
#[derive(Clone, Copy)]
struct SectionLayout { offset: u64, address: u64 }

pub fn link_netbsd_objects(objects: &[&[u8]], output: &Path) -> Result<(), String> {
    if objects.is_empty() { return Err("NetBSD linker: no objects".into()); }
    let files = objects.iter().map(|raw| object::File::parse(*raw)
        .map_err(|e| format!("NetBSD linker: invalid ELF object: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    for file in &files {
        if file.format() != object::BinaryFormat::Elf
            || file.architecture() != object::Architecture::X86_64
            || file.endianness() != object::Endianness::Little
            || file.kind() != object::ObjectKind::Relocatable {
            return Err("NetBSD linker: requires little-endian amd64 ELF relocatable objects".into());
        }
    }

    let mut sections = Vec::new();
    for (object, file) in files.iter().enumerate() {
        for section in file.sections() {
            let kind = section.kind();
            let name = section.name().unwrap_or("");
            if !matches!(kind, SectionKind::Text | SectionKind::ReadOnlyData
                | SectionKind::ReadOnlyString | SectionKind::Data | SectionKind::UninitializedData) {
                // The compiler emits this note for external ld. This built-in
                // linker emits its own NetBSD PT_NOTE, so skip the input copy.
                if matches!(name, ".eh_frame" | ".note.gnu.property" | ".note.netbsd.ident") { continue; }
                if matches!(section.flags(), SectionFlags::Elf { sh_flags } if sh_flags & 2 != 0) {
                    return Err(format!("NetBSD linker: allocated section `{name}` requires external linker"));
                }
                continue;
            }
            let size = section.size();
            let data = if kind == SectionKind::UninitializedData {
                vec![0; usize::try_from(size).map_err(|_| "NetBSD linker: section too large")?]
            } else {
                section.uncompressed_data().map_err(|e| e.to_string())?.into_owned()
            };
            sections.push(InputSection { object, index: section.index(), kind,
                align: section.align().max(1), data, size });
        }
    }
    let text: Vec<_> = sections.iter().filter(|s| s.kind == SectionKind::Text).collect();
    let rodata: Vec<_> = sections.iter().filter(|s| matches!(s.kind,
        SectionKind::ReadOnlyData | SectionKind::ReadOnlyString)).collect();
    let data: Vec<_> = sections.iter().filter(|s| matches!(s.kind,
        SectionKind::Data | SectionKind::UninitializedData)).collect();
    if text.is_empty() { return Err("NetBSD linker: no text section".into()); }
    if !files.iter().any(|f| f.symbols().any(|s| s.name() == Ok("_start") && s.is_definition())) {
        return Err("NetBSD linker: an executable needs the NetBSD-specific `_start` stub".into());
    }

    let definitions: BTreeSet<String> = files.iter().flat_map(|f| f.symbols())
        .filter(|s| s.is_definition() && s.is_global())
        .filter_map(|s| s.name().ok().map(str::to_owned)).collect();
    let imports: Vec<String> = files.iter().flat_map(|f| f.symbols())
        .filter(|s| s.section() == SymbolSection::Undefined && !s.is_weak())
        .filter_map(|s| s.name().ok().map(str::to_owned))
        .filter(|name| !name.is_empty() && !definitions.contains(name))
        .collect::<BTreeSet<_>>().into_iter().collect();

    // All non-linker imports must be provided by the NetBSD 11 libc. Do not
    // silently turn unknown native dependencies into Linux-host dependencies.
    let mut dynstr = vec![0u8];
    let libc_offset = dynstr.len() as u32;
    dynstr.extend_from_slice(LIBC);
    let mut name_offsets = Vec::new();
    for name in &imports {
        name_offsets.push(dynstr.len() as u32);
        dynstr.extend_from_slice(name.as_bytes());
        dynstr.push(0);
    }
    let dynsym_len = 24 * (imports.len() + 1);
    let hash_len = 8 + 4 * (1 + imports.len() + 1); // nbucket=1, nchain
    let reloc_len = 24 * imports.len();

    // Read-only metadata lives in a standalone initial PT_LOAD segment.
    let mut meta = (EH + PH * PH_COUNT) as u64;
    let interp_off = meta; meta += INTERPRETER.len() as u64;
    meta = align(meta, 4)?;
    let note_off = meta; meta += NETBSD_NOTE.len() as u64;
    meta = align(meta, 8)?;
    let hash_off = meta; meta += hash_len as u64;
    meta = align(meta, 8)?;
    let dynsym_off = meta; meta += dynsym_len as u64;
    let dynstr_off = meta; meta += dynstr.len() as u64;
    meta = align(meta, 8)?;
    let rela_off = meta; meta += reloc_len as u64;
    let text_start = align(meta, PAGE)?;
    let mut image = vec![0u8; usize::try_from(text_start).map_err(|_| "NetBSD metadata too large")?];

    let mut layout: HashMap<(usize, SectionIndex), SectionLayout> = HashMap::new();
    let mut text_end = place(&mut image, text_start, &text, &mut layout)?;
    // Each imported function gets 6 bytes: jmp qword ptr [rip+rel32].
    let trampoline_start = text_end;
    text_end = text_end.checked_add((6 * imports.len()) as u64)
        .ok_or("NetBSD linker: text overflow")?;
    grow(&mut image, text_end)?;
    let ro_start = align(text_end, PAGE)?;
    grow(&mut image, ro_start)?;
    let ro_end = place(&mut image, ro_start, &rodata, &mut layout)?;
    let data_start = align(ro_end, PAGE)?;
    grow(&mut image, data_start)?;
    let mut data_end = place(&mut image, data_start, &data, &mut layout)?;

    let mut symbols: HashMap<(usize, SymbolIndex), u64> = HashMap::new();
    let mut globals: HashMap<String, (u64, bool)> = HashMap::new();
    for (file_index, file) in files.iter().enumerate() {
        for symbol in file.symbols() {
            let address = match symbol.section() {
                SymbolSection::Section(index) => layout.get(&(file_index, index))
                    .map(|s| s.address + symbol.address()),
                SymbolSection::Absolute => Some(symbol.address()),
                SymbolSection::Common => {
                    data_end = align(data_end, symbol.address().max(1))?;
                    let address = BASE + data_end;
                    data_end = data_end.checked_add(symbol.size()).ok_or("common symbol overflow")?;
                    grow(&mut image, data_end)?;
                    Some(address)
                }
                _ => None,
            };
            let Some(address) = address else { continue };
            symbols.insert((file_index, symbol.index()), address);
            if symbol.is_global() && let Ok(name) = symbol.name() && !name.is_empty() {
                match globals.get(name).copied() {
                    None => { globals.insert(name.into(), (address, symbol.is_weak())); }
                    Some((_, true)) if !symbol.is_weak() => {
                        globals.insert(name.into(), (address, false));
                    }
                    Some((_, false)) if symbol.is_weak() => {}
                    Some((old, true)) if symbol.is_weak() && old == address => {}
                    Some(_) => return Err(format!("NetBSD linker: duplicate symbol {name}")),
                }
            }
        }
    }

    let got_start = align(data_end, 8)?;
    let got_end = got_start + 8 * imports.len() as u64;
    grow(&mut image, got_end)?;
    let dynamic_start = align(got_end, 8)?;
    // DT_NEEDED, HASH, STRTAB, SYMTAB, STRSZ, SYMENT,
    // RELA, RELASZ, RELAENT, NULL.
    let dynamic_size = 10 * 16u64;
    let data_final = dynamic_start + dynamic_size;
    grow(&mut image, data_final)?;
    for (i, name) in imports.iter().enumerate() {
        let trampoline = trampoline_start + (6 * i) as u64;
        let got = got_start + (8 * i) as u64;
        let displacement = i32::try_from((BASE + got) as i128 - (BASE + trampoline + 6) as i128)
            .map_err(|_| "NetBSD linker: GOT out of trampoline range")?;
        let pos = trampoline as usize;
        image[pos..pos + 2].copy_from_slice(&[0xff, 0x25]);
        image[pos + 2..pos + 6].copy_from_slice(&displacement.to_le_bytes());
        // No code relocation directly points into libc. Calls go through
        // these trampolines after the NetBSD loader has patched GOT slots.
        globals.insert(name.clone(), (BASE + trampoline, false));
    }
    for (file_index, file) in files.iter().enumerate() {
        for section in file.sections() {
            let Some(place_layout) = layout.get(&(file_index, section.index())) else { continue };
            for (offset, reloc) in section.relocations() {
                let RelocationTarget::Symbol(sym_index) = reloc.target() else {
                    return Err("NetBSD linker: unsupported relocation target".into());
                };
                let sym = file.symbol_by_index(sym_index).map_err(|e| e.to_string())?;
                let addr = symbols.get(&(file_index, sym_index)).copied()
                    .or_else(|| sym.name().ok().and_then(|n| globals.get(n).map(|g| g.0)))
                    .or_else(|| sym.is_weak().then_some(0))
                    .ok_or("NetBSD linker: unresolved relocation target")?;
                let p = place_layout.address + offset;
                let file_offset = (place_layout.offset + offset) as usize;
                match (reloc.kind(), reloc.size()) {
                    (RelocationKind::Relative | RelocationKind::PltRelative, 32) => {
                        let value = i32::try_from(addr as i128 + reloc.addend() as i128 - p as i128)
                            .map_err(|_| "NetBSD linker: PC32 out of range")?;
                        write(&mut image, file_offset, &value.to_le_bytes())?;
                    }
                    (RelocationKind::Absolute, 64) => {
                        let value = u64::try_from(addr as i128 + reloc.addend() as i128)
                            .map_err(|_| "NetBSD linker: Abs64 overflow")?;
                        write(&mut image, file_offset, &value.to_le_bytes())?;
                    }
                    (kind, width) => return Err(format!(
                        "NetBSD linker: unsupported {kind:?}/{width} relocation; use external linker")),
                }
            }
        }
    }

    // SysV ELF symbol hash. A one-bucket chain is valid, even if not optimal.
    put32(&mut image, hash_off as usize, 1);
    put32(&mut image, hash_off as usize + 4, (imports.len() + 1) as u32);
    put32(&mut image, hash_off as usize + 8, if imports.is_empty() { 0 } else { 1 });
    for i in 1..=imports.len() {
        put32(&mut image, hash_off as usize + 8 + 4 * (1 + i),
            if i == imports.len() { 0 } else { (i + 1) as u32 });
    }
    for (i, name_offset) in name_offsets.iter().enumerate() {
        let off = dynsym_off as usize + 24 * (i + 1);
        put32(&mut image, off, *name_offset);
        image[off + 4] = 0x12; // STB_GLOBAL | STT_FUNC
        // Undefined; filled in by libc through R_X86_64_GLOB_DAT.
    }
    write(&mut image, dynstr_off as usize, &dynstr)?;
    write(&mut image, interp_off as usize, INTERPRETER)?;
    write(&mut image, note_off as usize, &NETBSD_NOTE)?;
    for (i, _) in imports.iter().enumerate() {
        let off = rela_off as usize + 24 * i;
        put64(&mut image, off, BASE + got_start + 8 * i as u64);
        put64(&mut image, off + 8, (((i + 1) as u64) << 32) | 6); // GLOB_DAT
        put64(&mut image, off + 16, 0);
    }
    let dyn_entries: &[(u64, u64)] = &[
        (1, libc_offset as u64), (4, BASE + hash_off), (5, BASE + dynstr_off),
        (6, BASE + dynsym_off), (10, dynstr.len() as u64), (11, 24),
        (7, BASE + rela_off), (8, reloc_len as u64), (9, 24), (0, 0),
    ];
    for (i, (tag, value)) in dyn_entries.iter().enumerate() {
        put64(&mut image, dynamic_start as usize + i * 16, *tag);
        put64(&mut image, dynamic_start as usize + i * 16 + 8, *value);
    }

    let entry = files.iter().enumerate().find_map(|(index, file)| file.symbols()
        .find(|s| s.name() == Ok("_start") && s.is_definition())
        .and_then(|s| symbols.get(&(index, s.index())).copied()))
        .ok_or("NetBSD linker: missing `_start`")?;
    put_elf_header(&mut image, entry);
    let mut phdr = 0usize;
    // PT_PHDR covers its own table within the first load segment.
    ph(&mut image, phdr, 6, 4, EH as u64, BASE + EH as u64,
        (PH * PH_COUNT) as u64, (PH * PH_COUNT) as u64, 8); phdr += 1;
    ph(&mut image, phdr, 3, 4, interp_off, BASE + interp_off,
        INTERPRETER.len() as u64, INTERPRETER.len() as u64, 1); phdr += 1;
    ph(&mut image, phdr, 1, 4, 0, BASE, text_start, text_start, PAGE); phdr += 1;
    ph(&mut image, phdr, 1, 5, text_start, BASE + text_start,
        text_end - text_start, text_end - text_start, PAGE); phdr += 1;
    ph(&mut image, phdr, 1, 4, ro_start, BASE + ro_start,
        ro_end - ro_start, ro_end - ro_start, PAGE); phdr += 1;
    ph(&mut image, phdr, 1, 6, data_start, BASE + data_start,
        data_final - data_start, data_final - data_start, PAGE); phdr += 1;
    ph(&mut image, phdr, 2, 6, dynamic_start, BASE + dynamic_start,
        dynamic_size, dynamic_size, 8); phdr += 1;
    ph(&mut image, phdr, 4, 4, note_off, BASE + note_off,
        NETBSD_NOTE.len() as u64, NETBSD_NOTE.len() as u64, 4);

    std::fs::write(output, &image).map_err(|e| format!("NetBSD linker: {e}"))?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(output).map_err(|e| e.to_string())?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(output, permissions).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn place(image: &mut Vec<u8>, start: u64, sections: &[&InputSection],
    layouts: &mut HashMap<(usize, SectionIndex), SectionLayout>) -> Result<u64, String> {
    let mut at = start;
    for section in sections {
        at = align(at, section.align)?;
        grow(image, at)?;
        let offset = at;
        image.extend_from_slice(&section.data);
        layouts.insert((section.object, section.index), SectionLayout { offset, address: BASE + offset });
        at = at.checked_add(section.size.max(section.data.len() as u64))
            .ok_or("NetBSD linker: output overflow")?;
        grow(image, at)?;
    }
    Ok(at)
}
fn align(v: u64, a: u64) -> Result<u64, String> {
    if !a.is_power_of_two() { return Err(format!("NetBSD linker: invalid alignment {a}")); }
    v.checked_add(a - 1).map(|x| x & !(a - 1)).ok_or("NetBSD linker: alignment overflow".into())
}
fn grow(image: &mut Vec<u8>, n: u64) -> Result<(), String> {
    let n = usize::try_from(n).map_err(|_| "NetBSD linker: output too large")?;
    if image.len() < n { image.resize(n, 0); }
    Ok(())
}
fn write(image: &mut [u8], pos: usize, bytes: &[u8]) -> Result<(), String> {
    let end = pos.checked_add(bytes.len()).ok_or("NetBSD linker: write overflow")?;
    let dst = image.get_mut(pos..end).ok_or("NetBSD linker: write out of bounds")?;
    dst.copy_from_slice(bytes);
    Ok(())
}
fn put16(image: &mut [u8], pos: usize, value: u16) { image[pos..pos + 2].copy_from_slice(&value.to_le_bytes()); }
fn put32(image: &mut [u8], pos: usize, value: u32) { image[pos..pos + 4].copy_from_slice(&value.to_le_bytes()); }
fn put64(image: &mut [u8], pos: usize, value: u64) { image[pos..pos + 8].copy_from_slice(&value.to_le_bytes()); }
fn put_elf_header(image: &mut [u8], entry: u64) {
    image[..16].copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    put16(image, 16, 2); // ET_EXEC
    put16(image, 18, 62); // x86-64
    put32(image, 20, 1);
    put64(image, 24, entry);
    put64(image, 32, EH as u64);
    put64(image, 40, 0);
    put16(image, 52, EH as u16);
    put16(image, 54, PH as u16);
    put16(image, 56, PH_COUNT as u16);
}
fn ph(image: &mut [u8], index: usize, ty: u32, flags: u32, offset: u64,
    vaddr: u64, file_size: u64, memory_size: u64, alignment: u64) {
    let p = EH + PH * index;
    put32(image, p, ty);
    put32(image, p + 4, flags);
    put64(image, p + 8, offset);
    put64(image, p + 16, vaddr);
    put64(image, p + 24, vaddr);
    put64(image, p + 32, file_size);
    put64(image, p + 40, memory_size);
    put64(image, p + 48, alignment);
}

#[cfg(test)]
mod tests {
    use super::*;
    use object::write::{Object as WriteObject, Symbol, SymbolSection};
    use object::{Architecture, BinaryFormat, Endianness, SectionKind, SymbolFlags, SymbolKind, SymbolScope};

    #[test]
    fn netbsd_elf_has_rtld_and_libc() {
        let mut obj = WriteObject::new(BinaryFormat::Elf, Architecture::X86_64, Endianness::Little);
        let text_section = obj.add_section(Vec::new(), b".text".to_vec(), SectionKind::Text);
        obj.append_section_data(text_section, &[0xc3], 16);
        // The compiler carries a NetBSD note for external ld. The built-in
        // linker must accept it without duplicating its own PT_NOTE.
        let note = obj.add_section(Vec::new(), b".note.netbsd.ident".to_vec(), SectionKind::Note);
        obj.section_mut(note).flags = object::SectionFlags::Elf { sh_flags: 0x2 };
        obj.append_section_data(note, &NETBSD_NOTE, 4);
        obj.add_symbol(Symbol {
            name: b"_start".to_vec(), value: 0, size: 1,
            kind: SymbolKind::Text, scope: SymbolScope::Linkage,
            weak: false, section: SymbolSection::Section(text_section),
            flags: SymbolFlags::None,
        });
        let source = obj.write().expect("make minimal amd64 object");
        let dest = std::env::temp_dir().join(format!(
            "qz_netbsd_elf_smoke_{}_{}", std::process::id(),
            std::thread::current().name().unwrap_or("unnamed"),
        ));
        link_netbsd_objects(&[source.as_slice()], &dest).expect("link minimal NetBSD ELF");
        let bytes = std::fs::read(&dest).expect("read generated NetBSD executable");
        std::fs::remove_file(&dest).ok();
        assert_eq!(&bytes[..4], b"\x7fELF");
        assert_eq!(u16::from_le_bytes([bytes[16], bytes[17]]), 2); // ET_EXEC
        assert_eq!(u16::from_le_bytes([bytes[18], bytes[19]]), 62); // AMD64
        assert!(bytes.windows(INTERPRETER.len()).any(|window| window == INTERPRETER));
        assert!(bytes.windows(LIBC.len()).any(|window| window == LIBC));
        assert_eq!(u16::from_le_bytes([bytes[56], bytes[57]]), PH_COUNT as u16);
    }
}
