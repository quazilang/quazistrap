// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, Range};

use crate::semantic::{SemanticReport, SymbolKind};

use super::span::{position_to_byte_offset, position_to_char_offset, span_to_range};

pub fn hover_at(report: &SemanticReport, source: &str, pos: Position) -> Option<Hover> {
    let byte_offset = position_to_byte_offset(pos, source)?;
    let char_offset = position_to_char_offset(pos, source)?;

    // Tightest ExprAnnotation containing the cursor
    let best = report
        .annotated_exprs
        .iter()
        .filter(|a| a.span.start <= char_offset && char_offset < a.span.end)
        .min_by_key(|a| a.span.end - a.span.start);

    if let Some(ann) = best
        && let Some(ty) = &ann.ty
    {
        let range = span_to_range(ann.span, source);
        let value = match &ann.const_value {
            Some(cv) => format!("```quazi\n{ty} = {cv}\n```"),
            None => format!("```quazi\n{ty}\n```"),
        };
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(range),
        });
    }

    // Fallback: identifier word → symbol table
    let word = word_at_offset(source, byte_offset)?;
    let entry = report
        .symbol_table
        .entries
        .iter()
        .find(|e| e.name == word)?;
    let sym = &entry.symbol;

    let ty_str = sym
        .ty
        .as_ref()
        .map(|t| t.to_string())
        .unwrap_or_else(|| "?".to_string());

    let kind_str = match sym.kind {
        SymbolKind::Function => "fn",
        SymbolKind::Variable { mutable: true } => "var",
        SymbolKind::Variable { mutable: false } => "const",
        SymbolKind::Parameter => "param",
        SymbolKind::TypeName => "type",
    };

    let params_str = if sym.kind == SymbolKind::Function && !sym.params.is_empty() {
        let ps: Vec<String> = sym.params.iter().map(|p| p.to_string()).collect();
        format!("({}) ", ps.join(", "))
    } else {
        String::new()
    };

    let value = format!("```quazi\n{kind_str} {word}: {params_str}{ty_str}\n```");

    let word_start = word.as_ptr() as usize - source.as_ptr() as usize;
    let range = Range {
        start: super::span::char_offset_to_position(
            super::span::byte_offset_to_char_offset(word_start, source)?,
            source,
        ),
        end: super::span::char_offset_to_position(
            super::span::byte_offset_to_char_offset(word_start + word.len(), source)?,
            source,
        ),
    };

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(range),
    })
}

pub fn word_at_offset(source: &str, offset: usize) -> Option<&str> {
    if offset >= source.len() {
        return None;
    }
    let bytes = source.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    if !is_word(bytes[offset]) {
        return None;
    }
    let start = (0..=offset)
        .rev()
        .find(|&i| !is_word(bytes[i]))
        .map(|i| i + 1)
        .unwrap_or(0);
    let end = (offset..source.len())
        .find(|&i| !is_word(bytes[i]))
        .unwrap_or(source.len());
    Some(&source[start..end])
}
