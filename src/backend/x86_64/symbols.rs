// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::HashMap;

use object::write::{Object, SectionId, Symbol, SymbolId, SymbolSection};
use object::{SymbolFlags, SymbolKind, SymbolScope};

pub struct SymbolTable {
    /// Defined symbols: name → (id, offset_in_text)
    defined: HashMap<String, (SymbolId, u64)>,
    /// Undefined (external) symbols.
    undefined: HashMap<String, SymbolId>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self {
            defined: HashMap::new(),
            undefined: HashMap::new(),
        }
    }

    /// Register a global function symbol in the .text section.
    pub fn define_function(
        &mut self,
        obj: &mut Object<'_>,
        text_id: SectionId,
        name: &str,
        text_offset: u64,
        size: u64,
        local: bool,
    ) -> SymbolId {
        let id = obj.add_symbol(Symbol {
            name: name.as_bytes().to_vec(),
            value: text_offset,
            size,
            kind: SymbolKind::Text,
            scope: if local { SymbolScope::Compilation } else { SymbolScope::Linkage },
            weak: false,
            section: SymbolSection::Section(text_id),
            flags: SymbolFlags::None,
        });
        self.defined.insert(name.to_string(), (id, text_offset));
        id
    }

    /// Register a data symbol in a given section.
    pub fn define_data(
        &mut self,
        obj: &mut Object<'_>,
        section_id: SectionId,
        name: &str,
        offset: u64,
        size: u64,
    ) -> SymbolId {
        let id = obj.add_symbol(Symbol {
            name: name.as_bytes().to_vec(),
            value: offset,
            size,
            kind: SymbolKind::Data,
            scope: SymbolScope::Compilation,
            weak: false,
            section: SymbolSection::Section(section_id),
            flags: SymbolFlags::None,
        });
        self.defined.insert(name.to_string(), (id, offset));
        id
    }

    /// Get or create an UNDEF symbol for an external reference.
    pub fn get_or_add_undef(&mut self, obj: &mut Object<'_>, name: &str) -> SymbolId {
        if let Some(&id) = self.undefined.get(name) {
            return id;
        }
        let id = obj.add_symbol(Symbol {
            name: name.as_bytes().to_vec(),
            value: 0,
            size: 0,
            kind: SymbolKind::Text,
            scope: SymbolScope::Dynamic,
            weak: false,
            section: SymbolSection::Undefined,
            flags: SymbolFlags::None,
        });
        self.undefined.insert(name.to_string(), id);
        id
    }

    /// Look up a defined symbol by name.
    pub fn get_defined(&self, name: &str) -> Option<SymbolId> {
        self.defined.get(name).map(|&(id, _)| id)
    }
}
