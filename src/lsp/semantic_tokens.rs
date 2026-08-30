// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use tower_lsp::lsp_types::{
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens, SemanticTokensLegend,
};

use crate::lexer::Lexer;
use crate::lexer::token::TokenKind;
use crate::parser::ast::Span;
use crate::semantic::{SemanticReport, SymbolKind};

use super::span::span_to_range;

const KEYWORD: u32 = 0;
const TYPE: u32 = 1;
const FUNCTION: u32 = 2;
const PARAMETER: u32 = 3;
const VARIABLE: u32 = 4;
const NUMBER: u32 = 5;
const STRING: u32 = 6;
const OPERATOR: u32 = 7;

pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::KEYWORD,
            SemanticTokenType::TYPE,
            SemanticTokenType::FUNCTION,
            SemanticTokenType::PARAMETER,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::NUMBER,
            SemanticTokenType::STRING,
            SemanticTokenType::OPERATOR,
        ],
        token_modifiers: Vec::<SemanticTokenModifier>::new(),
    }
}

pub fn tokens_for(report: &SemanticReport, source: &str) -> SemanticTokens {
    let mut data = Vec::new();
    let mut previous_line = 0u32;
    let mut previous_start = 0u32;

    for token in Lexer::new(source).tokenize() {
        let Some(token_type) = classify(&token.kind, report) else {
            continue;
        };
        let range = span_to_range(
            Span::new(
                token.span.line,
                token.span.col,
                token.span.start,
                token.span.end,
            ),
            source,
        );
        if range.start.line != range.end.line || range.start == range.end {
            continue;
        }
        let delta_line = range.start.line - previous_line;
        let delta_start = if delta_line == 0 {
            range.start.character - previous_start
        } else {
            range.start.character
        };
        data.push(SemanticToken {
            delta_line,
            delta_start,
            length: range.end.character - range.start.character,
            token_type,
            token_modifiers_bitset: 0,
        });
        previous_line = range.start.line;
        previous_start = range.start.character;
    }

    SemanticTokens {
        result_id: None,
        data,
    }
}

fn classify(kind: &TokenKind, report: &SemanticReport) -> Option<u32> {
    match kind {
        TokenKind::Int(_) | TokenKind::Float(_) => Some(NUMBER),
        TokenKind::StringLit(_) | TokenKind::ByteStringLit(_) => Some(STRING),
        TokenKind::True | TokenKind::False => Some(KEYWORD),
        TokenKind::Int8
        | TokenKind::Int16
        | TokenKind::Int32
        | TokenKind::Int64
        | TokenKind::Uint8
        | TokenKind::Uint16
        | TokenKind::Uint32
        | TokenKind::Uint64
        | TokenKind::Isize
        | TokenKind::Usize
        | TokenKind::Float16
        | TokenKind::Float32
        | TokenKind::Float64
        | TokenKind::Bool
        | TokenKind::Str
        | TokenKind::Bytes
        | TokenKind::Void
        | TokenKind::Any => Some(TYPE),
        TokenKind::Fn
        | TokenKind::Var
        | TokenKind::Const
        | TokenKind::Return
        | TokenKind::If
        | TokenKind::Else
        | TokenKind::While
        | TokenKind::Import
        | TokenKind::Impl
        | TokenKind::Struct
        | TokenKind::Union
        | TokenKind::Trait
        | TokenKind::Enum
        | TokenKind::Match
        | TokenKind::As
        | TokenKind::For
        | TokenKind::Pub
        | TokenKind::Unsafe
        | TokenKind::Break
        | TokenKind::Continue
        | TokenKind::Type
        | TokenKind::Platform => Some(KEYWORD),
        TokenKind::Eq
        | TokenKind::FatArrow
        | TokenKind::Plus
        | TokenKind::Minus
        | TokenKind::Star
        | TokenKind::StarStar
        | TokenKind::Slash
        | TokenKind::Percent
        | TokenKind::Ampersand
        | TokenKind::Pipe
        | TokenKind::Caret
        | TokenKind::Lt
        | TokenKind::Gt
        | TokenKind::Shl
        | TokenKind::Shr
        | TokenKind::LtEq
        | TokenKind::GtEq
        | TokenKind::EqEq
        | TokenKind::NotEq
        | TokenKind::Bang
        | TokenKind::PlusEq
        | TokenKind::MinusEq
        | TokenKind::StarEq
        | TokenKind::SlashEq
        | TokenKind::PercentEq
        | TokenKind::PlusPlus
        | TokenKind::MinusMinus
        | TokenKind::Question
        | TokenKind::DotDot
        | TokenKind::DotDotEq => Some(OPERATOR),
        TokenKind::Ident(name) => report
            .symbol_table
            .entries
            .iter()
            .find(|entry| entry.name == *name || entry.name.rsplit('.').next() == Some(name))
            .map(|entry| match entry.symbol.kind {
                SymbolKind::Function => FUNCTION,
                SymbolKind::Parameter => PARAMETER,
                SymbolKind::Variable { .. } => VARIABLE,
                SymbolKind::TypeName => TYPE,
            })
            .or(Some(VARIABLE)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{STRING, tokens_for};
    use crate::lsp::analysis::analyze_source;

    #[test]
    fn emits_utf16_lengths_for_unicode_literals() {
        let source = r#"
fn main() void {
    const rocket: str = "🚀";
    ret;
}
"#;
        let report = analyze_source(source).expect("analyze source");
        let tokens = tokens_for(&report, source);

        assert!(
            tokens
                .data
                .iter()
                .any(|token| token.token_type == STRING && token.length == 4)
        );
    }

    #[test]
    fn classifies_known_function_and_parameter_identifiers() {
        let source = r#"
fn helper(value: i32) i32 { ret value; }
fn main() i32 { ret helper(1); }
"#;
        let report = analyze_source(source).expect("analyze source");
        let tokens = tokens_for(&report, source);

        assert!(tokens.data.iter().any(|token| token.token_type == 2));
        assert!(tokens.data.iter().any(|token| token.token_type == 3));
    }
}
