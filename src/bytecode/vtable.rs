// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::collections::{BTreeMap, HashSet};

use super::chunk::{Chunk, ConstPoolEntry};

/// Target-neutral dynamic dispatch slots, containing emitted Quazi symbols.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VtableLayout {
    pub type_name: String,
    pub trait_name: String,
    pub methods: Vec<String>,
}

impl VtableLayout {
    pub fn validate(&self) -> Result<(), String> {
        fn name(value: &str, field: &str) -> Result<(), String> {
            if value.is_empty() || value.len() > u16::MAX as usize {
                return Err(format!("QZI vtable {field} must contain 1..=65535 bytes"));
            }
            Ok(())
        }
        name(&self.type_name, "type name")?;
        name(&self.trait_name, "trait name")?;
        if self.methods.len() > 256 {
            return Err("QZI vtable exceeds the 256 addressable method slots".to_string());
        }
        for method in &self.methods {
            name(method, "method symbol")?;
        }
        Ok(())
    }

    /// Encode a layout after its caller has validated the encoding bounds.
    pub fn encode(&self, buf: &mut Vec<u8>) {
        fn string(buf: &mut Vec<u8>, value: &str) {
            buf.extend_from_slice(&(value.len() as u16).to_le_bytes());
            buf.extend_from_slice(value.as_bytes());
        }
        string(buf, &self.type_name);
        string(buf, &self.trait_name);
        buf.extend_from_slice(&(self.methods.len() as u16).to_le_bytes());
        for method in &self.methods {
            string(buf, method);
        }
    }

    pub fn decode(buf: &[u8], pos: &mut usize) -> Result<Self, String> {
        fn short(buf: &[u8], pos: &mut usize) -> Result<u16, String> {
            let end = pos.checked_add(2).ok_or("truncated QZI vtable length")?;
            let bytes = buf.get(*pos..end).ok_or("truncated QZI vtable length")?;
            *pos = end;
            Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
        }
        fn string(buf: &[u8], pos: &mut usize) -> Result<String, String> {
            let len = short(buf, pos)? as usize;
            let end = pos.checked_add(len).ok_or("truncated QZI vtable name")?;
            let bytes = buf.get(*pos..end).ok_or("truncated QZI vtable name")?;
            let value = std::str::from_utf8(bytes)
                .map_err(|_| "invalid UTF-8 in QZI vtable name".to_string())?;
            *pos = end;
            Ok(value.to_string())
        }
        let type_name = string(buf, pos)?;
        let trait_name = string(buf, pos)?;
        let count = short(buf, pos)? as usize;
        if count > 256 {
            return Err("QZI vtable exceeds the 256 addressable method slots".to_string());
        }
        if count > buf.len().saturating_sub(*pos) / 3 {
            return Err("QZI vtable method count exceeds remaining payload".to_string());
        }
        let mut methods = Vec::with_capacity(count);
        for _ in 0..count {
            methods.push(string(buf, pos)?);
        }
        let layout = Self {
            type_name,
            trait_name,
            methods,
        };
        layout.validate()?;
        Ok(layout)
    }

    /// Hex components preserve identity, including punctuation and UTF-8 bytes.
    pub fn symbol(&self) -> String {
        fn hex(value: &str) -> String {
            const DIGITS: &[u8; 16] = b"0123456789abcdef";
            let mut encoded = String::with_capacity(value.len() * 2);
            for byte in value.bytes() {
                encoded.push(DIGITS[(byte >> 4) as usize] as char);
                encoded.push(DIGITS[(byte & 15) as usize] as char);
            }
            encoded
        }
        format!(
            "__quazi_vtable_{}_{}",
            hex(&self.type_name),
            hex(&self.trait_name)
        )
    }
}

/// Collect consistent layouts without requiring a complete function bundle.
pub fn collect_vtable_layouts(chunks: &[Chunk]) -> Result<Vec<VtableLayout>, String> {
    let mut layouts = BTreeMap::new();
    for chunk in chunks {
        for constant in &chunk.constants {
            match constant {
                ConstPoolEntry::Vtable(layout) => {
                    layout.validate()?;
                    let key = (layout.type_name.clone(), layout.trait_name.clone());
                    if let Some(previous) = layouts.get(&key) {
                        if previous != layout {
                            return Err(format!(
                                "conflicting QZI vtable layouts for type `{}` and trait `{}`; rebuild the artifacts from matching sources",
                                layout.type_name, layout.trait_name
                            ));
                        }
                    } else {
                        layouts.insert(key, layout.clone());
                    }
                }
                ConstPoolEntry::VtableAddr(type_name, trait_name) => {
                    return Err(format!(
                        "QZI vtable for type `{type_name}` and trait `{trait_name}` lacks retained method layout; rebuild this artifact from source with the current compiler"
                    ));
                }
                _ => {}
            }
        }
    }
    Ok(layouts.into_values().collect())
}

/// Validate method symbols when the full linked/native function bundle exists.
pub fn validate_vtable_targets(chunks: &[Chunk]) -> Result<Vec<VtableLayout>, String> {
    let layouts = collect_vtable_layouts(chunks)?;
    let symbols = chunks
        .iter()
        .map(|chunk| chunk.name.as_str())
        .collect::<HashSet<_>>();
    for layout in &layouts {
        for method in &layout.methods {
            if !symbols.contains(method.as_str()) {
                return Err(format!(
                    "unresolved QZI vtable method `{method}` for type `{}` and trait `{}`; link the defining artifact or rebuild from source",
                    layout.type_name, layout.trait_name
                ));
            }
        }
    }
    Ok(layouts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> VtableLayout {
        VtableLayout {
            type_name: "Counter".to_string(),
            trait_name: "Readable".to_string(),
            methods: vec!["Counter.z".to_string(), "Counter.a".to_string()],
        }
    }

    fn chunk(layout: VtableLayout) -> Chunk {
        let mut chunk = Chunk::new("main");
        chunk.constants.push(ConstPoolEntry::Vtable(layout));
        chunk
    }

    #[test]
    fn layout_encoding_preserves_slot_order_and_rejects_every_truncated_prefix() {
        let expected = layout();
        let mut bytes = Vec::new();
        expected.encode(&mut bytes);
        for end in 0..bytes.len() {
            assert!(
                VtableLayout::decode(&bytes[..end], &mut 0).is_err(),
                "prefix {end}"
            );
        }
        let mut pos = 0;
        assert_eq!(VtableLayout::decode(&bytes, &mut pos).unwrap(), expected);
        assert_eq!(pos, bytes.len());
        let mut overflow = usize::MAX;
        assert!(VtableLayout::decode(&bytes, &mut overflow).is_err());
    }

    #[test]
    fn layout_bounds_allow_marker_traits_and_all_addressable_slots() {
        let mut value = layout();
        value.methods.clear();
        value.validate().unwrap();
        let mut bytes = Vec::new();
        value.encode(&mut bytes);
        assert_eq!(VtableLayout::decode(&bytes, &mut 0).unwrap(), value);
        value.methods = vec!["method".to_string(); 256];
        value.type_name = "x".repeat(u16::MAX as usize);
        value.validate().unwrap();
        bytes.clear();
        value.encode(&mut bytes);
        assert_eq!(VtableLayout::decode(&bytes, &mut 0).unwrap(), value);
        value.methods.push("overflow".to_string());
        assert!(value.validate().is_err());
        value.methods.clear();
        value.type_name.push('x');
        assert!(value.validate().is_err());
        for field in 0..3 {
            let mut value = layout();
            match field {
                0 => value.type_name.clear(),
                1 => value.trait_name.clear(),
                _ => value.methods[0].clear(),
            }
            assert!(value.validate().is_err());
        }
        let mut value = layout();
        value.methods[0] = "é".repeat(32768);
        assert!(value.validate().is_err(), "bounds measure UTF-8 bytes");
        let mut value = layout();
        value.trait_name = "x".repeat(u16::MAX as usize + 1);
        assert!(value.validate().is_err());
    }

    #[test]
    fn malformed_utf8_names_and_excessive_counts_are_rejected() {
        assert!(
            VtableLayout::decode(&[1, 0, 0xff], &mut 0)
                .unwrap_err()
                .contains("UTF-8")
        );
        // Two valid one-byte names followed by an unrepresentable method count.
        assert!(
            VtableLayout::decode(&[1, 0, b'T', 1, 0, b'R', 1, 1], &mut 0)
                .unwrap_err()
                .contains("256")
        );
        assert!(
            VtableLayout::decode(&[1, 0, b'T', 1, 0, b'R', 2, 0], &mut 0)
                .unwrap_err()
                .contains("remaining payload")
        );
        assert!(VtableLayout::decode(&[0, 0, 1, 0, b'R', 0, 0], &mut 0).is_err());
    }

    #[test]
    fn layouts_deduplicate_sort_and_reject_conflicting_slot_order() {
        let first = layout();
        let mut second = first.clone();
        second.type_name = "Another".to_string();
        let chunks = [
            chunk(first.clone()),
            chunk(second.clone()),
            chunk(first.clone()),
        ];
        assert_eq!(
            collect_vtable_layouts(&chunks).unwrap(),
            vec![second, first.clone()]
        );
        let mut conflicting = first.clone();
        conflicting.methods.reverse();
        assert!(
            collect_vtable_layouts(&[chunk(first), chunk(conflicting)])
                .unwrap_err()
                .contains("conflicting")
        );
    }

    #[test]
    fn final_targets_are_required_but_collection_allows_partial_cache_units() {
        let value = layout();
        let mut chunks = vec![chunk(value.clone())];
        assert_eq!(
            collect_vtable_layouts(&chunks).unwrap(),
            vec![value.clone()]
        );
        let error = validate_vtable_targets(&chunks).unwrap_err();
        assert!(error.contains("Counter.z") && error.contains("link the defining artifact"));
        chunks.extend(value.methods.iter().map(|name| Chunk::new(name)));
        assert_eq!(validate_vtable_targets(&chunks).unwrap(), vec![value]);
        let mut historical = Chunk::new("old");
        historical
            .constants
            .push(ConstPoolEntry::VtableAddr("T".into(), "R".into()));
        assert!(
            collect_vtable_layouts(&[historical])
                .unwrap_err()
                .contains("rebuild this artifact from source")
        );
    }

    #[test]
    fn symbols_preserve_punctuation_component_boundaries_and_utf8() {
        let mut symbols = HashSet::new();
        for (ty, tr) in [
            ("a.b", "c"),
            ("a_b", "c"),
            ("a", "b_c"),
            ("a_b", "c_"),
            ("é", "c"),
            ("c3a9", "c"),
        ] {
            let value = VtableLayout {
                type_name: ty.into(),
                trait_name: tr.into(),
                methods: Vec::new(),
            };
            assert!(symbols.insert(value.symbol()));
        }
        assert_eq!(
            VtableLayout {
                type_name: "é".into(),
                trait_name: "R".into(),
                methods: Vec::new()
            }
            .symbol(),
            "__quazi_vtable_c3a9_52"
        );
    }
}
