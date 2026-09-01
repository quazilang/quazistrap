// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use tower_lsp::lsp_types::{
    DocumentSymbol, Location, Range, SymbolInformation, SymbolKind as LspSymbolKind, Url,
};

use crate::semantic::{SemanticReport, SymbolKind};

use super::span::span_to_range;

/// Produces a flat document outline from declarations in the current semantic
/// snapshot. Library symbols have synthetic zero spans and do not belong in it.
#[allow(deprecated)] // Required by lsp-types 0.94's backward-compatible struct shape.
pub fn document_symbols(report: &SemanticReport, source: &str) -> Vec<DocumentSymbol> {
    let mut symbols: Vec<_> = report
        .symbol_table
        .entries
        .iter()
        .filter(|entry| !entry.symbol.is_import && entry.symbol.span.end > entry.symbol.span.start)
        .map(|entry| {
            let range = span_to_range(entry.symbol.span, source);
            DocumentSymbol {
                name: display_name(&entry.name),
                detail: symbol_detail(entry),
                kind: lsp_kind(entry.symbol.kind),
                tags: None,
                deprecated: None,
                range,
                selection_range: selection_range(&entry.name, entry.symbol.span, source, range),
                children: None,
            }
        })
        .collect();

    symbols.sort_by(|left, right| {
        left.range
            .start
            .cmp(&right.range.start)
            .then_with(|| left.name.cmp(&right.name))
    });
    symbols
}

/// Returns matching top-level declarations for a workspace symbol query.
///
/// Library imports and local bindings do not have stable project-wide locations,
/// so this intentionally exposes only declarations in the module scope.
#[allow(deprecated)] // Required by lsp-types 0.94's backward-compatible struct shape.
pub fn workspace_symbols(
    report: &SemanticReport,
    source: &str,
    uri: &Url,
    query: &str,
) -> Vec<SymbolInformation> {
    let query = query.to_lowercase();
    let mut symbols: Vec<_> = report
        .symbol_table
        .entries
        .iter()
        .filter(|entry| {
            entry.scope_depth == 0
                && !entry.symbol.is_import
                && entry.symbol.span.end > entry.symbol.span.start
                && (query.is_empty() || entry.name.to_lowercase().contains(&query))
        })
        .map(|entry| SymbolInformation {
            name: display_name(&entry.name),
            kind: lsp_kind(entry.symbol.kind),
            tags: None,
            deprecated: None,
            location: Location::new(uri.clone(), span_to_range(entry.symbol.span, source)),
            container_name: None,
        })
        .collect();

    symbols.sort_by(|left, right| left.name.cmp(&right.name));
    symbols
}

fn display_name(name: &str) -> String {
    name.rsplit('.').next().unwrap_or(name).to_string()
}

fn symbol_detail(entry: &crate::semantic::SymbolTableEntry) -> Option<String> {
    let symbol = &entry.symbol;
    match symbol.kind {
        SymbolKind::Function => {
            let params = symbol
                .params
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            let result = symbol
                .ty
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default();
            Some(format!("fn ({params}) {result}"))
        }
        _ => symbol.ty.as_ref().map(ToString::to_string),
    }
}

fn lsp_kind(kind: SymbolKind) -> LspSymbolKind {
    match kind {
        SymbolKind::Function => LspSymbolKind::FUNCTION,
        SymbolKind::Variable { mutable: true } => LspSymbolKind::VARIABLE,
        SymbolKind::Variable { mutable: false } => LspSymbolKind::CONSTANT,
        SymbolKind::Parameter => LspSymbolKind::VARIABLE,
        SymbolKind::TypeName => LspSymbolKind::STRUCT,
    }
}

fn selection_range(
    name: &str,
    span: crate::parser::ast::Span,
    source: &str,
    fallback: Range,
) -> Range {
    let display_name = display_name(name);
    let Some(start_byte) = char_offset_to_byte(span.start, source) else {
        return fallback;
    };
    let Some(end_byte) = char_offset_to_byte(span.end, source) else {
        return fallback;
    };
    let Some(relative_byte) = source[start_byte..end_byte].find(&display_name) else {
        return fallback;
    };
    let start = source[..start_byte + relative_byte].chars().count();
    let end = start + display_name.chars().count();
    span_to_range(crate::parser::ast::Span::new(0, 0, start, end), source)
}

fn char_offset_to_byte(offset: usize, source: &str) -> Option<usize> {
    if offset == source.chars().count() {
        return Some(source.len());
    }
    source.char_indices().nth(offset).map(|(byte, _)| byte)
}

#[cfg(test)]
mod tests {
    use super::{document_symbols, workspace_symbols};
    use crate::lsp::analysis::analyze_source;
    use tower_lsp::lsp_types::{SymbolKind, Url};

    #[test]
    fn lists_user_declarations_in_source_order() {
        let source = r#"
struct Point { x: i32 }
fn add(left: i32, right: i32) i32 {
    const total = left + right;
    ret total;
}
"#;
        let report = analyze_source(source).expect("analyze source");

        let symbols = document_symbols(&report, source);
        let names: Vec<_> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["Point", "add", "left", "right", "total"]);
        assert_eq!(symbols[0].kind, SymbolKind::STRUCT);
        assert_eq!(symbols[1].kind, SymbolKind::FUNCTION);
        assert_eq!(symbols[4].kind, SymbolKind::CONSTANT);
        assert_eq!(symbols[1].selection_range.start.line, 2);
    }

    #[test]
    fn workspace_symbols_only_include_matching_module_declarations() {
        let source = r#"
struct Point { x: i32 }
fn add(left: i32, right: i32) i32 {
    const total = left + right;
    ret total;
}
fn address() i32 { ret 1; }
"#;
        let report = analyze_source(source).expect("analyze source");
        let uri = Url::parse("file:///workspace/main.qz").expect("test URI");

        let symbols = workspace_symbols(&report, source, &uri, "ad");
        let names: Vec<_> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();

        assert_eq!(names, ["add", "address"]);
        assert!(symbols.iter().all(|symbol| symbol.location.uri == uri));
    }
}
