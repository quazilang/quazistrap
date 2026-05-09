use tower_lsp::lsp_types::{Position, Range, TextEdit};

use crate::analysis::format_void_source;

pub fn format_document(source: &str) -> Option<Vec<TextEdit>> {
    let formatted = format_void_source(source);
    if formatted == source {
        return Some(vec![]);
    }
    let line_count = source.lines().count() as u32;
    let last_col = source.lines().last().map(|l| l.len() as u32).unwrap_or(0);
    Some(vec![TextEdit {
        range: Range {
            start: Position::new(0, 0),
            end: Position::new(line_count, last_col),
        },
        new_text: formatted,
    }])
}
