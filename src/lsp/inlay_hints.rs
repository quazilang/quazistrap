// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Position, Range};

use crate::lexer::Lexer;
use crate::lexer::token::TokenKind;
use crate::parser::Parser;
use crate::parser::ast::{Block, ForLoop, Item, ItemKind, Span, Stmt, StmtKind, TypeKind};
use crate::semantic::{SemanticReport, SymbolKind};

use super::span::span_to_range;

/// Return type hints only for `var` and `const` declarations that omit a
/// written type. Declaration syntax comes from the parser and the displayed
/// type comes from semantic analysis, so a hint is never inferred from text or
/// a similarly named binding in another scope.
pub fn type_hints(report: &SemanticReport, source: &str, requested: Range) -> Vec<InlayHint> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new_with_source(tokens, source);
    let Ok(program) = parser.parse() else {
        return Vec::new();
    };

    let mut declarations = Vec::new();
    for item in &program.items {
        collect_item_declarations(item, &mut declarations);
    }

    declarations
        .into_iter()
        .filter_map(|declaration| {
            let entry = report.symbol_table.entries.iter().find(|entry| {
                matches!(entry.symbol.kind, SymbolKind::Variable { .. })
                    && entry.name == declaration.name
                    && entry.symbol.span == declaration.span
            })?;
            let ty = entry.symbol.ty.as_ref()?;
            if matches!(ty, TypeKind::Error) {
                return None;
            }
            let name_span = declaration_name_span(source, &declaration)?;
            let position = span_to_range(name_span, source).end;
            position_in_range(position, requested).then_some(InlayHint {
                position,
                label: InlayHintLabel::String(format!(": {ty}")),
                kind: Some(InlayHintKind::TYPE),
                text_edits: None,
                tooltip: None,
                padding_left: Some(true),
                padding_right: None,
                data: None,
            })
        })
        .collect()
}

struct Declaration {
    name: String,
    span: Span,
}

fn collect_item_declarations(item: &Item, declarations: &mut Vec<Declaration>) {
    match &item.node {
        ItemKind::Fn {
            body: Some(body), ..
        } => collect_block_declarations(body, declarations),
        ItemKind::Impl { methods, .. } => {
            for method in methods {
                collect_item_declarations(method, declarations);
            }
        }
        _ => {}
    }
}

fn collect_block_declarations(block: &Block, declarations: &mut Vec<Declaration>) {
    for statement in &block.stmts {
        collect_statement_declarations(statement, declarations);
    }
}

fn collect_statement_declarations(statement: &Stmt, declarations: &mut Vec<Declaration>) {
    match &statement.node {
        StmtKind::Var { name, ty: None, .. } | StmtKind::Const { name, ty: None, .. } => {
            declarations.push(Declaration {
                name: name.clone(),
                span: statement.span,
            })
        }
        StmtKind::If {
            then_block,
            else_if,
            else_block,
            ..
        } => {
            collect_block_declarations(then_block, declarations);
            for (_, block) in else_if {
                collect_block_declarations(block, declarations);
            }
            if let Some(block) = else_block {
                collect_block_declarations(block, declarations);
            }
        }
        StmtKind::For { kind, body } => {
            if let ForLoop::CStyle {
                init: Some(init), ..
            } = kind
            {
                collect_statement_declarations(init, declarations);
            }
            collect_block_declarations(body, declarations);
        }
        StmtKind::CfgBlock { body, .. } | StmtKind::UnsafeBlock { body } => {
            collect_block_declarations(body, declarations);
        }
        _ => {}
    }
}

fn declaration_name_span(source: &str, declaration: &Declaration) -> Option<Span> {
    let mut saw_declaration_keyword = false;
    for token in Lexer::new(source).tokenize() {
        if token.span.start < declaration.span.start || declaration.span.end < token.span.end {
            continue;
        }
        if matches!(&token.kind, TokenKind::Var | TokenKind::Const) {
            saw_declaration_keyword = true;
            continue;
        }
        if saw_declaration_keyword
            && matches!(&token.kind, TokenKind::Ident(name) if name == &declaration.name)
        {
            return Some(Span::new(0, 0, token.span.start, token.span.end));
        }
    }
    None
}

fn position_in_range(position: Position, range: Range) -> bool {
    range.start <= position && position <= range.end
}

#[cfg(test)]
mod tests {
    use super::type_hints;
    use crate::lsp::analysis::analyze_source;
    use tower_lsp::lsp_types::{Position, Range};

    #[test]
    fn shows_semantic_types_only_for_unannotated_declarations_in_range() {
        let source = r#"
fn main() void {
    const answer = 42;
    var message = "🚀"; var after_rocket = 1;
    var explicit: i32 = 7;
    const explicit_const: i64 = 8;
    if (true) {
        const nested = false;
    }
    ret;
}
"#;
        let report = analyze_source(source).expect("analyze source");
        let hints = type_hints(
            &report,
            source,
            Range::new(Position::new(0, 0), Position::new(9, 0)),
        );

        let labels: Vec<_> = hints
            .iter()
            .map(|hint| match &hint.label {
                tower_lsp::lsp_types::InlayHintLabel::String(label) => label.as_str(),
                tower_lsp::lsp_types::InlayHintLabel::LabelParts(_) => "",
            })
            .collect();
        assert_eq!(labels, [": i32", ": &str", ": i32", ": bool"]);
        assert_eq!(hints[1].position.line, 3);
        assert_eq!(
            hints[1].position.character, 15,
            "UTF-16 position after message"
        );
        let after_rocket = source.find("after_rocket").expect("name offset");
        let line_start = source[..after_rocket]
            .rfind('\n')
            .map_or(0, |offset| offset + 1);
        assert_eq!(
            hints[2].position.character,
            source[line_start..after_rocket].encode_utf16().count() as u32
                + "after_rocket".encode_utf16().count() as u32
        );

        let narrow = type_hints(
            &report,
            source,
            Range::new(Position::new(6, 0), Position::new(8, 0)),
        );
        assert_eq!(narrow.len(), 1);
        assert!(matches!(
            &narrow[0].label,
            tower_lsp::lsp_types::InlayHintLabel::String(label) if label == ": bool"
        ));
    }
}
