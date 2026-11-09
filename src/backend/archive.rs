// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Static archive (`.a`) member selection for the built-in linkers.
//!
//! Like a traditional Unix linker, only archive members that define a
//! symbol which is still undefined are linked, repeatedly, until no member
//! adds anything; members that nothing needs (and their own undefined
//! references) stay out of the image.

use std::collections::HashSet;

use object::read::archive::ArchiveFile;
use object::{Object, ObjectSymbol};

/// Global symbols an object defines and references.
fn global_symbols(data: &[u8], label: &str) -> Result<(Vec<String>, Vec<String>), String> {
    let file = object::File::parse(data)
        .map_err(|error| format!("cannot read archive member {label}: {error}"))?;
    let mut defined = Vec::new();
    let mut undefined = Vec::new();
    for symbol in file.symbols() {
        if !symbol.is_global() {
            continue;
        }
        let Ok(name) = symbol.name() else { continue };
        if name.is_empty() {
            continue;
        }
        if symbol.is_undefined() {
            undefined.push(name.to_string());
        } else if symbol.is_definition() || symbol.is_common() {
            defined.push(name.to_string());
        }
    }
    Ok((defined, undefined))
}

/// The archive members `objects` need, in archive order, transitively.
pub fn select_members(
    objects: &[&[u8]],
    archives: &[(String, Vec<u8>)],
) -> Result<Vec<Vec<u8>>, String> {
    let mut defined = HashSet::new();
    let mut undefined = HashSet::new();
    fn note(
        defined: &mut HashSet<String>,
        undefined: &mut HashSet<String>,
        defs: &[String],
        refs: &[String],
    ) {
        defined.extend(defs.iter().cloned());
        undefined.extend(refs.iter().cloned());
    }
    for (index, object) in objects.iter().enumerate() {
        let (defs, refs) = global_symbols(object, &format!("input {index}"))?;
        note(&mut defined, &mut undefined, &defs, &refs);
    }

    struct Member<'a> {
        data: &'a [u8],
        defines: Vec<String>,
        references: Vec<String>,
    }
    let mut members = Vec::new();
    for (path, bytes) in archives {
        let archive = ArchiveFile::parse(bytes.as_slice())
            .map_err(|error| format!("cannot read archive {path}: {error}"))?;
        if archive.is_thin() {
            return Err(format!("thin archive {path} is not supported by the built-in linker"));
        }
        for member in archive.members() {
            let member = member.map_err(|error| format!("cannot read archive {path}: {error}"))?;
            let name = String::from_utf8_lossy(member.name()).into_owned();
            let data = member
                .data(bytes.as_slice())
                .map_err(|error| format!("cannot read {path}({name}): {error}"))?;
            // Symbol tables and other non-object members are skipped.
            if object::File::parse(data).is_err() {
                continue;
            }
            let (defines, references) = global_symbols(data, &format!("{path}({name})"))?;
            members.push(Member {
                data,
                defines,
                references,
            });
        }
    }

    let mut taken = vec![false; members.len()];
    loop {
        let mut progress = false;
        for (index, member) in members.iter().enumerate() {
            if taken[index] {
                continue;
            }
            let needed = member
                .defines
                .iter()
                .any(|symbol| undefined.contains(symbol) && !defined.contains(symbol));
            if needed {
                taken[index] = true;
                progress = true;
                note(&mut defined, &mut undefined, &member.defines, &member.references);
            }
        }
        if !progress {
            break;
        }
    }
    Ok(members
        .iter()
        .zip(taken)
        .filter(|(_, taken)| *taken)
        .map(|(member, _)| member.data.to_vec())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::select_members;
    use object::write::{Object, Symbol, SymbolSection};
    use object::{Architecture, BinaryFormat, Endianness, SymbolFlags, SymbolKind, SymbolScope};

    /// A relocatable ELF object defining `defines` and referencing `references`.
    fn object_file(defines: &[&str], references: &[&str]) -> Vec<u8> {
        let mut object = Object::new(BinaryFormat::Elf, Architecture::X86_64, Endianness::Little);
        let text = object.add_section(Vec::new(), b".text".to_vec(), object::SectionKind::Text);
        for name in defines {
            let offset = object.append_section_data(text, &[0xC3], 1);
            object.add_symbol(Symbol {
                name: name.as_bytes().to_vec(),
                value: offset,
                size: 1,
                kind: SymbolKind::Text,
                scope: SymbolScope::Linkage,
                weak: false,
                section: SymbolSection::Section(text),
                flags: SymbolFlags::None,
            });
        }
        for name in references {
            object.add_symbol(Symbol {
                name: name.as_bytes().to_vec(),
                value: 0,
                size: 0,
                kind: SymbolKind::Text,
                scope: SymbolScope::Unknown,
                weak: false,
                section: SymbolSection::Undefined,
                flags: SymbolFlags::None,
            });
        }
        object.write().expect("write object")
    }

    /// A System V `ar` archive of `(name, data)` members.
    fn archive(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut bytes = b"!<arch>\n".to_vec();
        for (name, data) in members {
            let header = format!("{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", format!("{name}/"), 0, 0, 0, 644, data.len());
            bytes.extend_from_slice(header.as_bytes());
            bytes.extend_from_slice(data);
            if data.len() % 2 == 1 {
                bytes.push(b'\n');
            }
        }
        bytes
    }

    #[test]
    fn selects_needed_members_transitively_and_skips_the_rest() {
        let main = object_file(&["main"], &["add"]);
        let add = object_file(&["add"], &["double"]);
        let unused = object_file(&["unused"], &["never_defined"]);
        let double = object_file(&["double"], &[]);
        let library = archive(&[
            ("add.o", add.clone()),
            ("unused.o", unused),
            ("double.o", double.clone()),
        ]);
        let selected = select_members(&[main.as_slice()], &[("libx.a".into(), library)])
            .expect("select members");
        assert_eq!(selected, vec![add, double]);
    }
}
