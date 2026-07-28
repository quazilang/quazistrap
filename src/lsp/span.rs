// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use tower_lsp::lsp_types::{Position, Range};

use crate::parser::ast::Span;

pub fn span_to_range(span: Span, _source: &str) -> Range {
    let start = Position {
        line: span.line.saturating_sub(1) as u32,
        character: span.col.saturating_sub(1) as u32,
    };
    let end = Position {
        line: start.line,
        character: start.character + (span.end.saturating_sub(span.start)) as u32,
    };
    Range { start, end }
}

pub fn position_to_byte_offset(pos: Position, source: &str) -> Option<usize> {
    let mut current_line = 0u32;
    let mut line_start = 0usize;
    for (i, ch) in source.char_indices() {
        if current_line == pos.line {
            let line_bytes = &source[line_start..];
            let mut col_utf16 = 0u32;
            for (j, c) in line_bytes.char_indices() {
                if col_utf16 == pos.character {
                    return Some(line_start + j);
                }
                col_utf16 += if (c as u32) < 0x10000 { 1 } else { 2 };
            }
            return Some(line_start + line_bytes.len());
        }
        if ch == '\n' {
            current_line += 1;
            line_start = i + 1;
        }
    }
    if current_line == pos.line {
        Some(source.len())
    } else {
        None
    }
}
