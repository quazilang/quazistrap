// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use tower_lsp::lsp_types::{Position, Range};

use crate::parser::ast::Span;

/// Convert compiler span offsets (Unicode scalar indexes) to LSP UTF-16
/// positions. Compiler line/column fields cannot be used directly because LSP
/// counts supplementary Unicode scalars as two code units.
pub fn span_to_range(span: Span, source: &str) -> Range {
    Range {
        start: char_offset_to_position(span.start, source),
        end: char_offset_to_position(span.end, source),
    }
}

pub fn position_to_byte_offset(pos: Position, source: &str) -> Option<usize> {
    let mut line = 0u32;
    let mut utf16_column = 0u32;
    for (byte_offset, ch) in source.char_indices() {
        if line == pos.line {
            if utf16_column == pos.character {
                return Some(byte_offset);
            }
            if ch == '\n' {
                return None;
            }
            let width = ch.len_utf16() as u32;
            if pos.character < utf16_column + width {
                return None;
            }
            utf16_column += width;
        }
        if ch == '\n' {
            line += 1;
            utf16_column = 0;
        }
    }
    if line == pos.line && utf16_column == pos.character {
        Some(source.len())
    } else {
        None
    }
}

pub fn position_to_char_offset(pos: Position, source: &str) -> Option<usize> {
    let byte_offset = position_to_byte_offset(pos, source)?;
    byte_offset_to_char_offset(byte_offset, source)
}

pub fn byte_offset_to_char_offset(byte_offset: usize, source: &str) -> Option<usize> {
    source
        .get(..byte_offset)
        .map(|prefix| prefix.chars().count())
}

pub fn char_offset_to_position(offset: usize, source: &str) -> Position {
    let mut line = 0u32;
    let mut utf16_column = 0u32;
    for (char_offset, ch) in source.chars().enumerate() {
        if char_offset == offset {
            return Position::new(line, utf16_column);
        }
        if ch == '\n' {
            line += 1;
            utf16_column = 0;
        } else {
            utf16_column += ch.len_utf16() as u32;
        }
    }
    Position::new(line, utf16_column)
}

/// Return the exclusive end position of a complete LSP document range.
/// This deliberately walks Unicode scalars because LSP columns are UTF-16
/// code units; `str::len` is a byte count and is invalid for this purpose.
pub fn document_end_position(source: &str) -> Position {
    char_offset_to_position(source.chars().count(), source)
}

#[cfg(test)]
mod tests {
    use super::{document_end_position, position_to_byte_offset, span_to_range};
    use crate::parser::ast::Span;
    use tower_lsp::lsp_types::Position;

    #[test]
    fn converts_unicode_scalar_spans_to_utf16_ranges() {
        let source = "const rocket = \"🚀\";\n";
        let start = source.find('🚀').expect("rocket byte offset");
        let start_chars = source[..start].chars().count();
        let range = span_to_range(
            Span::new(1, start_chars + 1, start_chars, start_chars + 1),
            source,
        );
        let utf16_start = source[..start].encode_utf16().count() as u32;
        assert_eq!(range.start, Position::new(0, utf16_start));
        assert_eq!(range.end, Position::new(0, utf16_start + 2));
    }

    #[test]
    fn rejects_positions_inside_a_utf16_surrogate_pair() {
        assert_eq!(position_to_byte_offset(Position::new(0, 1), "🚀"), None);
        assert_eq!(position_to_byte_offset(Position::new(0, 2), "🚀"), Some(4));
    }

    #[test]
    fn finds_end_of_document_in_utf16_with_a_trailing_newline() {
        assert_eq!(document_end_position("let icon = \"🚀\";\n"), Position::new(1, 0));
        assert_eq!(document_end_position("🚀"), Position::new(0, 2));
    }
}
