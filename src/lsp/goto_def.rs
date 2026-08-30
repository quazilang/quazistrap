// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Url};

use crate::semantic::{SemanticReport, SymbolTableEntry};

use super::hover::word_at_offset;
use super::span::{position_to_byte_offset, position_to_char_offset, span_to_range};

pub fn goto_definition(
    report: &SemanticReport,
    source: &str,
    uri: &Url,
    pos: Position,
) -> Option<GotoDefinitionResponse> {
    let offset = position_to_byte_offset(pos, source)?;
    let char_offset = position_to_char_offset(pos, source)?;
    let word = word_at_offset(source, offset)?;

    let resolved_call = report
        .annotated_exprs
        .iter()
        .filter(|annotation| {
            annotation.span.start <= char_offset && char_offset < annotation.span.end
        })
        .min_by_key(|annotation| annotation.span.end - annotation.span.start)
        .and_then(|annotation| annotation.resolved_fn.as_deref());

    let entry = resolved_call
        .and_then(|name| definition_for_name(report, name, char_offset))
        .or_else(|| definition_for_name(report, word, char_offset))?;

    Some(GotoDefinitionResponse::Scalar(Location {
        uri: uri.clone(),
        range: span_to_range(entry.symbol.span, source),
    }))
}

fn definition_for_name<'a>(
    report: &'a SemanticReport,
    name: &str,
    cursor: usize,
) -> Option<&'a SymbolTableEntry> {
    report
        .symbol_table
        .entries
        .iter()
        .filter(|entry| {
            (entry.name == name || entry.name.rsplit('.').next() == Some(name))
                && entry.symbol.span.end > entry.symbol.span.start
        })
        .max_by_key(|entry| {
            (
                entry.scope_depth,
                usize::from(entry.symbol.span.start <= cursor),
                entry.symbol.span.start,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::goto_definition;
    use crate::lsp::analysis::analyze_source;
    use tower_lsp::lsp_types::{GotoDefinitionResponse, Position, Url};

    #[test]
    fn resolves_a_method_call_using_the_semantic_function_name() {
        let source = r#"
struct Counter { value: i32 }
impl Counter {
    fn get(self: Counter) i32 { ret self.value; }
}
fn main() i32 {
    var counter = Counter { value: 1 };
    ret counter.get();
}
"#;
        let report = analyze_source(source).expect("analyze source");
        let position = Position::new(7, 16);
        let uri = Url::parse("file:///workspace/main.qz").expect("URI");

        let Some(GotoDefinitionResponse::Scalar(location)) =
            goto_definition(&report, source, &uri, position)
        else {
            panic!("expected a definition location");
        };
        assert_eq!(location.uri, uri);
        assert_eq!(location.range.start.line, 3);
    }

    #[test]
    fn prefers_the_current_function_local_binding() {
        let source = r#"
fn first() i32 {
    const value: i32 = 1;
    ret value;
}
fn main() i32 {
    const value: i32 = 2;
    ret value;
}
"#;
        let report = analyze_source(source).expect("analyze source");
        let position = Position::new(7, 8);
        let uri = Url::parse("file:///workspace/main.qz").expect("URI");

        let Some(GotoDefinitionResponse::Scalar(location)) =
            goto_definition(&report, source, &uri, position)
        else {
            panic!("expected a definition location");
        };
        assert_eq!(location.range.start.line, 6);
    }
}
