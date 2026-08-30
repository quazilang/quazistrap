// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::collections::HashMap;

use tower_lsp::lsp_types::{Location, Position, Range, TextEdit, Url, WorkspaceEdit};

use crate::lexer::Lexer;
use crate::lexer::token::TokenKind;
use crate::parser::ast::Span;
use crate::semantic::{ResolvedBinding, SemanticReport, SymbolTableEntry};

use super::hover::word_at_offset;
use super::span::{position_to_byte_offset, position_to_char_offset, span_to_range};

pub fn references_at(
    report: &SemanticReport,
    source: &str,
    uri: &Url,
    position: Position,
) -> Option<Vec<Location>> {
    let binding = binding_at(report, source, position)?;
    let definition = definition_entry(report, &binding)?;
    let name = definition
        .name
        .rsplit('.')
        .next()
        .unwrap_or(&definition.name);

    let mut ranges = vec![identifier_range(definition.symbol.span, name, source)];
    ranges.extend(
        report
            .annotated_exprs
            .iter()
            .filter(|annotation| annotation.resolved_binding.as_ref() == Some(&binding))
            .map(|annotation| reference_range(annotation.span, name, source)),
    );
    ranges.sort_by_key(|range| (range.start, range.end));
    ranges.dedup();

    Some(
        ranges
            .into_iter()
            .map(|range| Location {
                uri: uri.clone(),
                range,
            })
            .collect(),
    )
}

pub fn rename_edits(
    report: &SemanticReport,
    source: &str,
    uri: &Url,
    position: Position,
    new_name: &str,
) -> Option<WorkspaceEdit> {
    if !is_identifier(new_name) {
        return None;
    }
    let locations = references_at(report, source, uri, position)?;
    let edits = locations
        .into_iter()
        .map(|location| TextEdit {
            range: location.range,
            new_text: new_name.to_string(),
        })
        .collect();
    Some(WorkspaceEdit {
        changes: Some(HashMap::from([(uri.clone(), edits)])),
        document_changes: None,
        change_annotations: None,
    })
}

fn binding_at(
    report: &SemanticReport,
    source: &str,
    position: Position,
) -> Option<ResolvedBinding> {
    let char_offset = position_to_char_offset(position, source)?;
    if let Some(binding) = report
        .annotated_exprs
        .iter()
        .filter(|annotation| {
            annotation.span.start <= char_offset && char_offset < annotation.span.end
        })
        .min_by_key(|annotation| annotation.span.end - annotation.span.start)
        .and_then(|annotation| annotation.resolved_binding.clone())
    {
        return Some(binding);
    }

    let byte_offset = position_to_byte_offset(position, source)?;
    let word = word_at_offset(source, byte_offset)?;
    report
        .symbol_table
        .entries
        .iter()
        .find(|entry| {
            (entry.name == word || entry.name.rsplit('.').next() == Some(word))
                && entry.symbol.span.start <= char_offset
                && char_offset <= entry.symbol.span.end
        })
        .map(|entry| ResolvedBinding {
            name: entry.name.clone(),
            span: entry.symbol.span,
            kind: entry.symbol.kind,
        })
}

fn definition_entry<'a>(
    report: &'a SemanticReport,
    binding: &ResolvedBinding,
) -> Option<&'a SymbolTableEntry> {
    report.symbol_table.entries.iter().find(|entry| {
        entry.name == binding.name
            && entry.symbol.span == binding.span
            && entry.symbol.kind == binding.kind
    })
}

fn reference_range(span: Span, name: &str, source: &str) -> Range {
    let source_text = source_span(span, source);
    if source_text == Some(name) {
        return span_to_range(span, source);
    }
    callable_name_range(span, name, source).unwrap_or_else(|| span_to_range(span, source))
}

fn identifier_range(span: Span, name: &str, source: &str) -> Range {
    let Some((start, _)) = char_span_to_bytes(span, source) else {
        return span_to_range(span, source);
    };
    let Some(offset) = source_span(span, source).and_then(|text| text.find(name)) else {
        return span_to_range(span, source);
    };
    let name_start = source[..start + offset].chars().count();
    let name_end = name_start + name.chars().count();
    span_to_range(Span::new(0, 0, name_start, name_end), source)
}

fn callable_name_range(span: Span, name: &str, source: &str) -> Option<Range> {
    let text = source_span(span, source)?;
    let base = span.start;
    let tokens = Lexer::new(text).tokenize();
    for (index, token) in tokens.iter().enumerate() {
        let TokenKind::Ident(candidate) = &token.kind else {
            continue;
        };
        if candidate != name || !is_call_callee(&tokens[index + 1..]) {
            continue;
        }
        let start = base + token.span.start;
        let end = base + token.span.end;
        return Some(span_to_range(Span::new(0, 0, start, end), source));
    }
    None
}

fn is_call_callee(tokens: &[crate::lexer::token::Token]) -> bool {
    match tokens.first().map(|token| &token.kind) {
        Some(TokenKind::LParen) => true,
        Some(TokenKind::LBracket) => {
            let mut depth = 0usize;
            for (index, token) in tokens.iter().enumerate() {
                match &token.kind {
                    TokenKind::LBracket => depth += 1,
                    TokenKind::RBracket => {
                        depth -= 1;
                        if depth == 0 {
                            return matches!(
                                tokens.get(index + 1),
                                Some(next) if matches!(next.kind, TokenKind::LParen)
                            );
                        }
                    }
                    _ => {}
                }
            }
            false
        }
        _ => false,
    }
}

fn source_span(span: Span, source: &str) -> Option<&str> {
    let (start, end) = char_span_to_bytes(span, source)?;
    source.get(start..end)
}

fn char_span_to_bytes(span: Span, source: &str) -> Option<(usize, usize)> {
    Some((
        char_to_byte(span.start, source)?,
        char_to_byte(span.end, source)?,
    ))
}

fn char_to_byte(offset: usize, source: &str) -> Option<usize> {
    if offset == source.chars().count() {
        return Some(source.len());
    }
    source.char_indices().nth(offset).map(|(byte, _)| byte)
}

fn is_identifier(candidate: &str) -> bool {
    let mut lexer = Lexer::new(candidate);
    let tokens = lexer.tokenize();
    matches!(tokens.first().map(|token| &token.kind), Some(TokenKind::Ident(name)) if name == candidate)
        && matches!(tokens.get(1).map(|token| &token.kind), Some(TokenKind::Eof))
}

#[cfg(test)]
mod tests {
    use super::{references_at, rename_edits};
    use crate::lsp::analysis::analyze_source;
    use tower_lsp::lsp_types::{Position, Url};

    #[test]
    fn finds_only_references_to_the_resolved_shadowed_binding() {
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
        let uri = Url::parse("file:///workspace/main.qz").expect("URI");
        let locations =
            references_at(&report, source, &uri, Position::new(7, 8)).expect("references");

        assert_eq!(locations.len(), 2, "locations: {locations:?}");
        assert_eq!(locations[0].range.start.line, 6);
        assert_eq!(locations[1].range.start.line, 7);
    }

    #[test]
    fn rename_returns_precise_edits_and_rejects_keywords() {
        let source = r#"
fn main() i32 {
    const value: i32 = 2;
    ret value;
}
"#;
        let report = analyze_source(source).expect("analyze source");
        let uri = Url::parse("file:///workspace/main.qz").expect("URI");
        let edit = rename_edits(&report, source, &uri, Position::new(3, 8), "result")
            .expect("rename edit");

        assert_eq!(edit.changes.as_ref().expect("changes")[&uri].len(), 2);
        assert!(rename_edits(&report, source, &uri, Position::new(3, 8), "fn").is_none());
    }

    #[test]
    fn function_references_and_rename_target_only_the_callee() {
        let source = r#"
fn helper(value: i32) i32 { ret value; }
fn main() i32 {
    ret helper(1);
}
"#;
        let report = analyze_source(source).expect("analyze source");
        let uri = Url::parse("file:///workspace/main.qz").expect("URI");
        let locations =
            references_at(&report, source, &uri, Position::new(3, 8)).expect("function references");

        assert_eq!(locations.len(), 2, "locations: {locations:?}");
        assert_eq!(locations[0].range.start.line, 1);
        assert_eq!(locations[1].range.start.line, 3);
        assert_eq!(
            locations[1].range.end.character - locations[1].range.start.character,
            6
        );

        let edit = rename_edits(&report, source, &uri, Position::new(3, 8), "compute")
            .expect("function rename");
        assert_eq!(edit.changes.as_ref().expect("changes")[&uri].len(), 2);
    }

    #[test]
    fn distinguishes_a_parameter_from_a_same_named_function() {
        let source = r#"
fn value(value: i32) i32 { ret value; }
fn main() i32 { ret value(1); }
"#;
        let report = analyze_source(source).expect("analyze source");
        let uri = Url::parse("file:///workspace/main.qz").expect("URI");
        let use_position = crate::lsp::span::char_offset_to_position(
            source
                .rfind("value;")
                .map(|byte| source[..byte].chars().count())
                .expect("parameter use"),
            source,
        );
        let locations = references_at(&report, source, &uri, use_position).expect("references");

        assert_eq!(locations.len(), 2);
        assert!(
            locations
                .iter()
                .all(|location| location.range.start.line == 1)
        );
        assert!(locations[0].range.start.character > 3);
    }
}
