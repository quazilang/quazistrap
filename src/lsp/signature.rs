// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use tower_lsp::lsp_types::{
    ParameterInformation, ParameterLabel, Position, SignatureHelp, SignatureInformation,
};

use crate::lexer::Lexer;
use crate::lexer::token::TokenKind;
use crate::semantic::{SemanticReport, SymbolKind};

use super::span::position_to_byte_offset;

pub fn signature_help_at(
    report: &SemanticReport,
    source: &str,
    position: Position,
) -> Option<SignatureHelp> {
    let byte_offset = position_to_byte_offset(position, source)?;
    let (open_offset, active_parameter) = active_call(source, byte_offset)?;
    let name = callee_name_before(source, open_offset)?;
    let entry = report.symbol_table.entries.iter().find(|entry| {
        matches!(entry.symbol.kind, SymbolKind::Function)
            && (entry.name == name || entry.name.rsplit('.').next() == Some(name))
    })?;

    let parameter_labels: Vec<String> = entry
        .symbol
        .params
        .iter()
        .map(ToString::to_string)
        .collect();
    let label = format!(
        "{}({}) {}",
        name,
        parameter_labels.join(", "),
        entry
            .symbol
            .ty
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "void".to_string())
    );

    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label,
            documentation: None,
            parameters: Some(
                parameter_labels
                    .into_iter()
                    .map(|label| ParameterInformation {
                        label: ParameterLabel::Simple(label),
                        documentation: None,
                    })
                    .collect(),
            ),
            active_parameter: None,
        }],
        active_signature: Some(0),
        active_parameter: Some(active_parameter),
    })
}

fn active_call(source: &str, byte_offset: usize) -> Option<(usize, u32)> {
    let prefix = source.get(..byte_offset)?;
    let mut calls: Vec<(usize, u32)> = Vec::new();
    for token in Lexer::new(prefix).tokenize() {
        match token.kind {
            TokenKind::LParen => calls.push((token.span.start, 0)),
            TokenKind::RParen => {
                calls.pop();
            }
            TokenKind::Comma => {
                if let Some((_, commas)) = calls.last_mut() {
                    *commas += 1;
                }
            }
            _ => {}
        }
    }
    calls.pop()
}

fn callee_name_before(source: &str, open_char_offset: usize) -> Option<&str> {
    let open_byte_offset = source.char_indices().nth(open_char_offset)?.0;
    let before = strip_trailing_type_arguments(source.get(..open_byte_offset)?.trim_end());
    let end = before.len();
    let start = before
        .as_bytes()
        .iter()
        .rposition(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
        .map(|index| index + 1)
        .unwrap_or(0);
    (start < end).then(|| &before[start..end])
}

fn strip_trailing_type_arguments(before: &str) -> &str {
    if !before.ends_with(']') {
        return before;
    }

    let mut depth = 0usize;
    for (offset, ch) in before.char_indices().rev() {
        match ch {
            ']' => depth += 1,
            '[' => {
                depth -= 1;
                if depth == 0 {
                    return before[..offset].trim_end();
                }
            }
            _ => {}
        }
    }
    before
}

#[cfg(test)]
mod tests {
    use super::signature_help_at;
    use crate::lsp::analysis::analyze_source;
    use crate::lsp::span::char_offset_to_position;

    #[test]
    fn reports_the_active_parameter_of_an_open_call() {
        let source = r#"
fn combine(left: i32, right: str) void { ret; }
fn main() void {
    combine(1, "");
}
"#;
        let report = analyze_source(source).expect("analyze source");
        let position = char_offset_to_position(source.find("\"\"").expect("cursor"), source);
        let signature = signature_help_at(&report, source, position).expect("signature help");

        assert_eq!(signature.active_parameter, Some(1));
        assert_eq!(signature.signatures[0].label, "combine(i32, str) void");
    }

    #[test]
    fn counts_only_commas_of_the_active_nested_call() {
        let source = r#"
fn outer(left: i32, right: i32) void { ret; }
fn inner(value: i32) i32 { ret value; }
fn main() void {
    outer(inner(1), 2);
}
"#;
        let report = analyze_source(source).expect("analyze source");
        let position = char_offset_to_position(source.find("2);").expect("cursor"), source);
        let signature = signature_help_at(&report, source, position).expect("signature help");

        assert_eq!(signature.active_parameter, Some(1));
        assert!(signature.signatures[0].label.starts_with("outer("));
    }

    #[test]
    fn identifies_generic_call_callees() {
        let source = r#"
fn identity[T](value: T) T { ret value; }
fn main() i32 {
    ret identity[i32](1);
}
"#;
        let report = analyze_source(source).expect("analyze source");
        let position = char_offset_to_position(source.find("1);").expect("cursor"), source);
        let signature = signature_help_at(&report, source, position).expect("signature help");

        assert!(signature.signatures[0].label.starts_with("identity("));
    }
}
