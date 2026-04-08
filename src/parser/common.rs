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
            Err(self.err_here(format!(
                "expected {:?}, got {:?}",
                expected,
                self.peek_kind()
            )))
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
            other => Err(self.err_here(format!("expected identifier, got {:?}", other))),
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
        format!("{} at {}:{} [{}..{}]", msg, s.line, s.col, s.start, s.end)
    }

    pub(crate) fn err_tok(&self, s: TokenSpan, msg: String) -> String {
        format!("{} at {}:{} [{}..{}]", msg, s.line, s.col, s.start, s.end)
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
