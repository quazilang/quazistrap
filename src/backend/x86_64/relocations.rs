// quazi - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use object::write::{Object, Relocation, SectionId, SymbolId};
use object::{BinaryFormat, RelocationEncoding, RelocationFlags, RelocationKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocKind {
    /// R_X86_64_PLT32 — relative call to function symbol.
    Plt32,
    /// R_X86_64_PC32 — RIP-relative reference to data symbol.
    Pc32,
}

#[derive(Debug, Clone)]
pub struct PendingReloc {
    /// Byte offset from the start of the .text section.
    pub offset_in_text: usize,
    pub kind: RelocKind,
    pub symbol: String,
    pub addend: i64,
}

pub fn write_reloc(
    obj: &mut Object<'_>,
    section_id: SectionId,
    offset: u64,
    kind: RelocKind,
    sym_id: SymbolId,
    addend: i64,
    fmt: BinaryFormat,
) {
    let flags = match kind {
        RelocKind::Plt32 => {
            // COFF has no PLT; IMAGE_REL_AMD64_REL32 covers all near calls.
            let reloc_kind = if fmt == BinaryFormat::Coff {
                RelocationKind::Relative
            } else {
                RelocationKind::PltRelative
            };
            RelocationFlags::Generic {
                kind: reloc_kind,
                encoding: RelocationEncoding::X86Branch,
                size: 32,
            }
        }
        RelocKind::Pc32 => RelocationFlags::Generic {
            kind: RelocationKind::Relative,
            encoding: RelocationEncoding::Generic,
            size: 32,
        },
    };
    obj.add_relocation(
        section_id,
        Relocation {
            offset,
            symbol: sym_id,
            addend,
            flags,
        },
    )
    .expect("relocation add failed");
}
