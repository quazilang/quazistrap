// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use tower_lsp::lsp_types::{Position, Range, TextEdit};

use crate::analysis::format_quazi_source;
use crate::lsp::span::document_end_position;

pub fn format_document(source: &str) -> Option<Vec<TextEdit>> {
    let formatted = format_quazi_source(source);
    if formatted == source {
        return Some(vec![]);
    }
    Some(vec![TextEdit {
        range: Range {
            start: Position::new(0, 0),
            end: document_end_position(source),
        },
        new_text: formatted,
    }])
}

#[cfg(test)]
mod tests {
    use super::format_document;
    use tower_lsp::lsp_types::Position;

    #[test]
    fn replacement_range_uses_utf16_and_trailing_newline_end() {
        let source = "fn main(){\n    print(\"🚀\");\n}  \n";
        let edit = format_document(source)
            .expect("formatter response")
            .into_iter()
            .next()
            .expect("formatting edit");
        assert_eq!(edit.range.end, Position::new(3, 0));
    }
}
