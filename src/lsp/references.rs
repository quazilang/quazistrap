// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::collections::HashMap;

use tower_lsp::lsp_types::{Location, Position, Range, TextEdit, Url, WorkspaceEdit};

use crate::lexer::Lexer;
use crate::lexer::token::TokenKind;
use crate::parser::ast::Span;
use crate::semantic::{ResolvedBinding, SemanticReport, SymbolTableEntry};

use super::analysis::LoadedSnapshot;
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
            .filter_map(|annotation| annotation.binding_span)
            .map(|span| span_to_range(span, source)),
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

/// Find every compiler-resolved occurrence of the binding at `position` in a
/// loader-backed import graph. Compiler spans are translated through the
/// loader's effective source map, so unsaved open buffers remain authoritative
/// and locations retain UTF-16 LSP coordinates in their owning files.
pub fn loaded_references_at(
    snapshot: &LoadedSnapshot,
    source: &str,
    uri: &Url,
    position: Position,
    include_declaration: bool,
) -> Option<Vec<Location>> {
    let binding = loaded_binding_at(snapshot, source, uri, position)?;
    loaded_binding_locations(snapshot, &binding, include_declaration, true)
}

fn loaded_binding_locations(
    snapshot: &LoadedSnapshot,
    binding: &ResolvedBinding,
    include_declaration: bool,
    include_alias_uses: bool,
) -> Option<Vec<Location>> {
    let definition = definition_entry(&snapshot.report, &binding)?;
    let name = definition
        .name
        .rsplit('.')
        .next()
        .unwrap_or(&definition.name);

    let mut locations = Vec::new();
    if include_declaration {
        let declaration = snapshot
            .report
            .binding_declarations
            .iter()
            .find(|declaration| declaration.binding == *binding)
            .and_then(|declaration| loaded_location(snapshot, declaration.name_span))
            .or_else(|| loaded_identifier_location(snapshot, definition.symbol.span, name))?;
        locations.push(declaration);
    }
    locations.extend(
        snapshot
            .report
            .annotated_exprs
            .iter()
            .filter(|annotation| annotation.resolved_binding.as_ref() == Some(binding))
            .filter_map(|annotation| annotation.binding_span)
            .filter(|span| include_alias_uses || !is_alias_use_span(snapshot, binding, *span))
            .filter_map(|span| loaded_location(snapshot, span)),
    );
    locations.extend(
        snapshot
            .report
            .binding_imports
            .iter()
            .filter(|import| import.binding == *binding)
            .filter_map(|import| loaded_location(snapshot, import.selector_span)),
    );
    locations.sort_by(|left, right| {
        left.uri
            .as_str()
            .cmp(right.uri.as_str())
            .then_with(|| left.range.start.line.cmp(&right.range.start.line))
            .then_with(|| left.range.start.character.cmp(&right.range.start.character))
            .then_with(|| left.range.end.line.cmp(&right.range.end.line))
            .then_with(|| left.range.end.character.cmp(&right.range.end.character))
    });
    locations.dedup();
    Some(locations)
}

/// Build a multi-file rename edit only when every resolved occurrence belongs
/// to a client-authorized workspace source. A rename must never silently edit
/// the standard library, a dependency, or a file outside negotiated roots.
pub fn loaded_rename_edits(
    snapshot: &LoadedSnapshot,
    source: &str,
    uri: &Url,
    position: Position,
    new_name: &str,
    can_edit: impl Fn(&Url) -> bool,
) -> Option<WorkspaceEdit> {
    if !is_identifier(new_name) {
        return None;
    }
    let binding = loaded_binding_at(snapshot, source, uri, position)?;
    let locations = loaded_binding_locations(snapshot, &binding, true, false)?;
    if locations.iter().any(|location| !can_edit(&location.uri)) {
        return None;
    }
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    for location in locations {
        changes.entry(location.uri).or_default().push(TextEdit {
            range: location.range,
            new_text: new_name.to_string(),
        });
    }
    Some(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
}

fn is_alias_use_span(snapshot: &LoadedSnapshot, binding: &ResolvedBinding, span: Span) -> bool {
    snapshot
        .report
        .binding_imports
        .iter()
        .filter(|import| import.binding == *binding && import.alias_span.is_some())
        .any(|import| {
            same_loaded_file(snapshot, import.selector_span, span)
                && loaded_span_text(snapshot, span) == Some(import.local_name.as_str())
        })
}

fn same_loaded_file(snapshot: &LoadedSnapshot, left: Span, right: Span) -> bool {
    snapshot
        .source_files
        .iter()
        .any(|file| file.contains(left) && file.contains(right))
}

fn loaded_span_text<'a>(snapshot: &'a LoadedSnapshot, span: Span) -> Option<&'a str> {
    let file = snapshot
        .source_files
        .iter()
        .find(|file| file.contains(span))?;
    let source = snapshot
        .effective_sources
        .get(&std::path::PathBuf::from(&file.path))?;
    let start = span.start.checked_sub(file.start)?;
    let end = span.end.checked_sub(file.start)?;
    source_span(Span::new(0, 0, start, end), source)
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
            annotation
                .binding_span
                .is_some_and(|span| span.start <= char_offset && char_offset < span.end)
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

fn loaded_binding_at(
    snapshot: &LoadedSnapshot,
    source: &str,
    uri: &Url,
    position: Position,
) -> Option<ResolvedBinding> {
    let local_offset = position_to_char_offset(position, source)?;
    let path = uri.to_file_path().ok()?.canonicalize().ok()?;
    let file = snapshot
        .source_files
        .iter()
        .find(|file| std::path::Path::new(&file.path) == path)?;
    let merged_offset = file.start.checked_add(local_offset)?;
    if merged_offset >= file.end {
        return None;
    }
    if let Some(binding) = snapshot
        .report
        .binding_imports
        .iter()
        .find(|import| {
            import.selector_span.start <= merged_offset && merged_offset < import.selector_span.end
        })
        .map(|import| import.binding.clone())
    {
        return Some(binding);
    }
    if let Some(binding) = snapshot
        .report
        .annotated_exprs
        .iter()
        .filter(|annotation| {
            annotation
                .binding_span
                .is_some_and(|span| span.start <= merged_offset && merged_offset < span.end)
        })
        .min_by_key(|annotation| annotation.span.end - annotation.span.start)
        .and_then(|annotation| annotation.resolved_binding.clone())
    {
        return Some(binding);
    }

    let byte_offset = position_to_byte_offset(position, source)?;
    let word = word_at_offset(source, byte_offset)?;
    snapshot
        .report
        .symbol_table
        .entries
        .iter()
        .find(|entry| {
            (entry.name == word || entry.name.rsplit('.').next() == Some(word))
                && entry.symbol.span.start <= merged_offset
                && merged_offset <= entry.symbol.span.end
        })
        .map(|entry| ResolvedBinding {
            name: entry.name.clone(),
            span: entry.symbol.span,
            kind: entry.symbol.kind,
        })
}

fn loaded_location(snapshot: &LoadedSnapshot, span: Span) -> Option<Location> {
    let file = snapshot
        .source_files
        .iter()
        .find(|file| file.contains(span))?;
    let path = std::path::PathBuf::from(&file.path);
    let source = snapshot.effective_sources.get(&path)?;
    let start = span.start.checked_sub(file.start)?;
    let end = span.end.checked_sub(file.start)?;
    if end > source.chars().count() {
        return None;
    }
    Some(Location {
        uri: Url::from_file_path(path).ok()?,
        range: span_to_range(Span::new(0, 0, start, end), source),
    })
}

fn loaded_identifier_location(
    snapshot: &LoadedSnapshot,
    span: Span,
    name: &str,
) -> Option<Location> {
    let file = snapshot
        .source_files
        .iter()
        .find(|file| file.contains(span))?;
    let path = std::path::PathBuf::from(&file.path);
    let source = snapshot.effective_sources.get(&path)?;
    let start = span.start.checked_sub(file.start)?;
    let end = span.end.checked_sub(file.start)?;
    let local_span = Span::new(0, 0, start, end);
    Some(Location {
        uri: Url::from_file_path(path).ok()?,
        range: identifier_range(local_span, name, source),
    })
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
    use std::collections::HashMap;
    use std::fs;

    use super::{loaded_references_at, loaded_rename_edits, references_at, rename_edits};
    use crate::lsp::analysis::{analyze_loaded_document, analyze_source};
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
        let edits = &edit.changes.as_ref().expect("changes")[&uri];
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[1].range, locations[1].range);
        assert_eq!(
            edits[1].range.end.character - edits[1].range.start.character,
            "helper".encode_utf16().count() as u32
        );
        assert!(references_at(&report, source, &uri, Position::new(3, 14)).is_none());
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

    #[test]
    fn loaded_references_and_rename_follow_an_imported_binding() {
        let root = std::env::temp_dir().join(format!(
            "quazi_lsp_loaded_references_{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create workspace");
        let helper_path = root.join("helper.qz");
        fs::write(&helper_path, "pub fn old_answer() i32 { ret 0; }\n").expect("write helper");
        let main_path = root.join("main.qz");
        let source = "// 🚀\nimport helper.answer as local;\nfn main() i32 { ret local(); }\n";
        fs::write(&main_path, "fn main() i32 { ret 0; }\n").expect("write importer");
        let mut overlays = HashMap::new();
        overlays.insert(
            helper_path.canonicalize().expect("canonical helper"),
            "// unsaved helper\npub fn answer() i32 { ret 42; }\n".to_string(),
        );
        overlays.insert(
            main_path.canonicalize().expect("canonical importer"),
            source.to_string(),
        );
        let snapshot = analyze_loaded_document(&main_path, &overlays).expect("load source");
        assert!(
            snapshot.report.errors.is_empty(),
            "{:?}",
            snapshot.report.errors
        );
        let uri = Url::from_file_path(&main_path).expect("URI");
        let helper_uri = Url::from_file_path(&helper_path).expect("helper URI");

        let references = loaded_references_at(&snapshot, source, &uri, Position::new(2, 23), true)
            .expect("cross-file references");
        assert_eq!(references.len(), 3, "references: {references:?}");
        assert!(
            references
                .iter()
                .any(|location| location.uri == helper_uri && location.range.start.line == 1)
        );
        assert_eq!(
            references
                .iter()
                .filter(|location| location.uri == uri)
                .count(),
            2
        );
        let without_declaration =
            loaded_references_at(&snapshot, source, &uri, Position::new(1, 16), false)
                .expect("import selector references");
        assert_eq!(without_declaration.len(), 2);

        let edit = loaded_rename_edits(
            &snapshot,
            source,
            &uri,
            Position::new(2, 23),
            "result",
            |_| true,
        )
        .expect("cross-file rename");
        let changes = edit.changes.expect("changes");
        assert_eq!(changes[&helper_uri].len(), 1);
        assert_eq!(changes[&uri].len(), 1, "the local alias stays unchanged");
        assert!(
            loaded_rename_edits(
                &snapshot,
                source,
                &uri,
                Position::new(2, 23),
                "result",
                |candidate| candidate == &uri,
            )
            .is_none()
        );

        fs::remove_dir_all(root).expect("remove workspace");
    }
}
