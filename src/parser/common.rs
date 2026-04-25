// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use crate::lexer::token::{Span as TokenSpan, Token, TokenKind};
use crate::parser::Parser;
use crate::parser::ast;

impl Parser {
    pub(crate) fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or_else(|| {
            self.tokens
                .last()
                .expect("parser requires at least EOF token")
        })
    }

    pub(crate) fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    pub(crate) fn checkpoint(&self) -> usize {
        self.pos
    }

    pub(crate) fn restore(&mut self, checkpoint: usize) {
        self.pos = checkpoint;
    }

    pub(crate) fn at(&self, expected: TokenKind) -> bool {
        self.peek_kind() == &expected
    }

    pub(crate) fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or_else(|| {
            self.tokens
                .last()
                .cloned()
                .expect("parser requires EOF token")
        });
        self.pos += 1;
        tok
    }

    pub(crate) fn expect(&mut self, expected: TokenKind) -> Result<Token, String> {
        if self.at(expected.clone()) {
            Ok(self.advance())
        } else {
            Err(self.err_here_with_code(
                "E02",
                format!(
                "expected {}, found {}",
                expected,
                self.peek_kind()
                ),
            ))
        }
    }

    pub(crate) fn parse_ident(&mut self) -> Result<String, String> {
        let tok = self.expect_ident_token()?;
        match tok.kind {
            TokenKind::Ident(name) => Ok(name),
            _ => unreachable!(),
        }
    }

    pub(crate) fn expect_ident_token(&mut self) -> Result<Token, String> {
        match self.peek_kind() {
            TokenKind::Ident(_) => Ok(self.advance()),
            other => Err(self.err_here_with_code(
                "E01",
                format!("expected identifier, found '{}'", other),
            )),
        }
    }

    pub(crate) fn match_and_and(&mut self) -> bool {
        if self.at(TokenKind::Ampersand) {
            let save = self.pos;
            self.advance();
            if self.at(TokenKind::Ampersand) {
                self.advance();
                return true;
            }
            self.pos = save;
        }
        false
    }

    pub(crate) fn match_or_or(&mut self) -> bool {
        if self.at(TokenKind::Pipe) {
            let save = self.pos;
            self.advance();
            if self.at(TokenKind::Pipe) {
                self.advance();
                return true;
            }
            self.pos = save;
        }
        false
    }

    pub(crate) fn current_span(&self) -> TokenSpan {
        self.peek().span
    }

    pub(crate) fn err_here(&self, msg: String) -> String {
        let s = self.current_span();
        self.render_diagnostic("E00", msg, s)
    }

    pub(crate) fn err_here_with_code(&self, code: &str, msg: String) -> String {
        let s = self.current_span();
        self.render_diagnostic(code, msg, s)
    }

    pub(crate) fn err_tok(&self, s: TokenSpan, msg: String) -> String {
        self.render_diagnostic("E00", msg, s)
    }

    pub(crate) fn err_tok_with_code(&self, s: TokenSpan, code: &str, msg: String) -> String {
        self.render_diagnostic(code, msg, s)
    }

    fn render_diagnostic(&self, code: &str, msg: String, span: TokenSpan) -> String {
        let mut out = format!("error[{}]: {}\nat {}:{}", code, msg, span.line, span.col);

        if let Some(source) = &self.source {
            if let Some(line_text) = source.lines().nth(span.line.saturating_sub(1)) {
                let line_no = span.line;
                let line_no_width = line_no.to_string().len();
                let caret_offset = span.col.saturating_sub(1);
                let caret_width = (span.end.saturating_sub(span.start)).max(1);

                out.push('\n');
                out.push_str(&format!("{} | {}", line_no, line_text));
                out.push('\n');
                out.push_str(&" ".repeat(line_no_width));
                out.push_str(" | ");
                out.push_str(&" ".repeat(caret_offset));
                out.push_str(&"^".repeat(caret_width));
            }
        }

        out
    }

    pub(crate) fn synchronize_item(&mut self) {
        while !self.at(TokenKind::Eof) {
            match self.peek_kind() {
                TokenKind::Import
                | TokenKind::Fn
                | TokenKind::Struct
                | TokenKind::Trait
                | TokenKind::Enum
                | TokenKind::Impl => break,
                _ => {
                    self.advance();
                }
            }
        }
    }

    pub(crate) fn synchronize_stmt(&mut self) {
        while !self.at(TokenKind::Eof)
            && !self.at(TokenKind::Semicolon)
            && !self.at(TokenKind::RBrace)
        {
            self.advance();
        }

        if self.at(TokenKind::Semicolon) {
            self.advance();
        }
    }
}

pub(crate) fn merge_token_spans(a: TokenSpan, b: TokenSpan) -> TokenSpan {
    let (line, col, start) = if a.start <= b.start {
        (a.line, a.col, a.start)
    } else {
        (b.line, b.col, b.start)
    };
    let end = a.end.max(b.end);
    TokenSpan {
        line,
        col,
        start,
        end,
    }
}

pub(crate) fn to_ast_span(s: TokenSpan) -> ast::Span {
    ast::Span::new(s.line, s.col, s.start, s.end)
}
