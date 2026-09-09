// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::collections::HashMap;

use tower_lsp::lsp_types::{
    CodeAction, CodeActionContext, CodeActionKind, CodeActionOrCommand, Diagnostic, Range,
    TextEdit, Url, WorkspaceEdit,
};

use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::parser::ast::{ImportItems, ItemKind, Span};
use crate::semantic::SemanticReport;

use super::diagnostics;
use super::span::span_to_range;

/// Return an edit only when the compiler reported W03 on a complete,
/// single-selector import declaration. Broader imports can bind multiple
/// symbols, so removing their complete source span would not be safe.
pub fn unused_import_actions(
    report: &SemanticReport,
    source: &str,
    uri: &Url,
    requested: Range,
    context: &CodeActionContext,
) -> Vec<CodeActionOrCommand> {
    if context
        .only
        .as_ref()
        .is_some_and(|kinds| !kinds.iter().any(|kind| kind == &CodeActionKind::QUICKFIX))
    {
        return Vec::new();
    }

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new_with_source(tokens, source);
    let Ok(program) = parser.parse() else {
        return Vec::new();
    };
    let diagnostics = diagnostics::to_lsp_diagnostics(report, source);

    program
        .items
        .iter()
        .filter_map(|item| {
            let ItemKind::Import(import) = &item.node else {
                return None;
            };
            if !matches!(
                import.items,
                ImportItems::Single(_) | ImportItems::Aliased(_, _)
            ) {
                return None;
            }
            let diagnostic = diagnostics
                .iter()
                .find(|diagnostic| {
                    diagnostic_code(diagnostic) == Some("W03")
                        && diagnostic.range == span_to_range(item.span, source)
                })?
                .clone();
            if !context_allows_diagnostic(context, &diagnostic) {
                return None;
            }
            let edit_range = import_removal_range(item.span, source)?;
            if !ranges_intersect(edit_range, requested) {
                return None;
            }
            Some(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Remove unused import".to_string(),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diagnostic]),
                edit: Some(WorkspaceEdit {
                    changes: Some(HashMap::from([(
                        uri.clone(),
                        vec![TextEdit {
                            range: edit_range,
                            new_text: String::new(),
                        }],
                    )])),
                    document_changes: None,
                    change_annotations: None,
                }),
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: None,
            }))
        })
        .collect()
}

fn diagnostic_code(diagnostic: &Diagnostic) -> Option<&str> {
    match diagnostic.code.as_ref()? {
        tower_lsp::lsp_types::NumberOrString::String(code) => Some(code),
        tower_lsp::lsp_types::NumberOrString::Number(_) => None,
    }
}

fn context_allows_diagnostic(context: &CodeActionContext, diagnostic: &Diagnostic) -> bool {
    context.diagnostics.is_empty()
        || context.diagnostics.iter().any(|candidate| {
            diagnostic_code(candidate) == Some("W03") && candidate.range == diagnostic.range
        })
}

fn import_removal_range(span: Span, source: &str) -> Option<Range> {
    let start = char_to_byte(span.start, source)?;
    let mut end = char_to_byte(span.end, source)?;
    if source[end..].starts_with("\r\n") {
        end += 2;
    } else if source[end..].starts_with('\n') {
        end += 1;
    }
    let start_chars = source[..start].chars().count();
    let end_chars = source[..end].chars().count();
    Some(span_to_range(
        Span::new(0, 0, start_chars, end_chars),
        source,
    ))
}

fn char_to_byte(offset: usize, source: &str) -> Option<usize> {
    if offset == source.chars().count() {
        return Some(source.len());
    }
    source.char_indices().nth(offset).map(|(byte, _)| byte)
}

fn ranges_intersect(left: Range, right: Range) -> bool {
    if right.start == right.end {
        return left.start <= right.start && right.start < left.end;
    }
    left.start < right.end && right.start < left.end
}

#[cfg(test)]
mod tests {
    use super::{import_removal_range, unused_import_actions};
    use crate::lsp::analysis::analyze_source;
    use crate::parser::ast::Span;
    use tower_lsp::lsp_types::{
        CodeActionContext, CodeActionKind, CodeActionOrCommand, Diagnostic, DiagnosticSeverity,
        Position, Range, Url,
    };

    #[test]
    fn offers_a_precise_quick_fix_for_one_unused_import() {
        let source = "import helper.answer;\nfn main() void { const x: i32 = 1; ret; }\n";
        let report = analyze_source(source).expect("analyze source");
        assert!(
            report.warnings.iter().any(|warning| warning.code == "W03"),
            "warnings: {:?}",
            report
                .warnings
                .iter()
                .map(|warning| (&warning.code, &warning.message, warning.span))
                .collect::<Vec<_>>()
        );
        let uri = Url::parse("file:///workspace/main.qz").expect("URI");
        let actions = unused_import_actions(
            &report,
            source,
            &uri,
            Range::new(Position::new(0, 0), Position::new(0, 21)),
            &CodeActionContext::default(),
        );

        assert_eq!(actions.len(), 1);
        let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
            panic!("quick fix action");
        };
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        let changes = action
            .edit
            .as_ref()
            .and_then(|edit| edit.changes.as_ref())
            .expect("edit");
        let edit = &changes[&uri][0];
        assert_eq!(edit.range.start, Position::new(0, 0));
        assert_eq!(edit.range.end, Position::new(1, 0));
        assert!(
            unused_import_actions(
                &report,
                source,
                &uri,
                Range::new(Position::new(0, 0), Position::new(1, 0)),
                &CodeActionContext {
                    only: Some(vec![CodeActionKind::SOURCE]),
                    ..Default::default()
                },
            )
            .is_empty()
        );
        assert!(
            unused_import_actions(
                &report,
                source,
                &uri,
                Range::new(Position::new(0, 0), Position::new(0, 1)),
                &CodeActionContext {
                    diagnostics: vec![Diagnostic {
                        range: Range::new(Position::new(0, 0), Position::new(0, 1)),
                        severity: Some(DiagnosticSeverity::WARNING),
                        code: Some(tower_lsp::lsp_types::NumberOrString::String(
                            "W01".to_string()
                        )),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            )
            .is_empty()
        );
        assert!(
            unused_import_actions(
                &report,
                source,
                &uri,
                Range::new(Position::new(1, 0), Position::new(1, 0)),
                &CodeActionContext::default(),
            )
            .is_empty()
        );
    }

    #[test]
    fn removal_range_consumes_a_crlf_line_ending_after_unicode_text() {
        let source = "// 🚀\r\nimport helper.answer;\r\nfn main() void { ret; }\r\n";
        let start_byte = source.find("import").expect("import");
        let end_byte = start_byte + "import helper.answer;".len();
        let span = Span::new(
            0,
            0,
            source[..start_byte].chars().count(),
            source[..end_byte].chars().count(),
        );
        let range = import_removal_range(span, source).expect("removal range");
        assert_eq!(range.start, Position::new(1, 0));
        assert_eq!(range.end, Position::new(2, 0));
    }
}
