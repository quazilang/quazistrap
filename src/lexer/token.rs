// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub col: usize,
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(line: usize, col: usize, start: usize, end: usize) -> Self {
        Self {
            line,
            col,
            start,
            end,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // literals
    Int(i64),
    Float(f64),
    StringLit(String),
    Ident(String),
    Error(String),

    // keywords
    Fn,
    Var,
    Const,
    Return,
    If,
    Else,
    While,
    Import,
    Impl,
    Struct,
    Trait,
    As,
    For,

    // types
    Int8,
    Int16,
    Int32,
    Int64,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Float16,
    Float32,
    Float64,
    Bool,
    Str,
    Void,
    Any,

    // symbols
    LParen,    // (
    RParen,    // )
    LBrace,    // {
    RBrace,    // }
    Semicolon, // ;
    Colon,     // :
    Dot,       // .
    Comma,     // ,
    Ampersand, // &
    Pipe,      // |
    Hash,      // #
    DotDotDot, // ...
    Percent,   // %

    // operators
    Eq,    // =
    Plus,  // +
    Minus, // -
    Star,  // *
    Slash, // /
    Lt,    // <
    Gt,    // >
    LtEq,  // <=
    GtEq,  // >=
    EqEq,  // ==
    NotEq, // !=
    Bang,  // !

    // directives
    Platform, // platform

    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }

    pub fn eof(span: Span) -> Self {
        Self {
            kind: TokenKind::Eof,
            span,
        }
    }
}
