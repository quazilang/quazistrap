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
    Enum,
    Match,
    As,
    For,
    Pub,

    // types
    Int8,
    Int16,
    Int32,
    Int64,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Isize,
    Usize,
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
    LBracket,  // [
    RBracket,  // ]
    LBrace,    // {
    RBrace,    // }
    Semicolon, // ;
    Colon,     // :
    Dot,       // .
    Comma,     // ,
    Ampersand, // &
    Pipe,      // |
    Hash,      // #
    At,        // @
    DotDot,    // ..
    DotDotDot, // ...
    Percent,   // %

    // operators
    Eq,    // =
    FatArrow, // =>
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

    // compound assignment
    PlusEq,    // +=
    MinusEq,   // -=
    StarEq,    // *=
    SlashEq,   // /=
    PercentEq, // %=

    // increment / decrement
    PlusPlus,   // ++
    MinusMinus, // --

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

impl std::fmt::Display for TokenKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenKind::Int(value) => write!(f, "integer literal {}", value),
            TokenKind::Float(value) => write!(f, "float literal {}", value),
            TokenKind::StringLit(_) => write!(f, "string literal"),
            TokenKind::Ident(name) => write!(f, "identifier {}", name),
            TokenKind::Error(msg) => write!(f, "lexer error {}", msg),

            TokenKind::Fn => write!(f, "fn"),
            TokenKind::Var => write!(f, "var"),
            TokenKind::Const => write!(f, "const"),
            TokenKind::Return => write!(f, "ret"),
            TokenKind::If => write!(f, "if"),
            TokenKind::Else => write!(f, "else"),
            TokenKind::While => write!(f, "while"),
            TokenKind::Import => write!(f, "import"),
            TokenKind::Impl => write!(f, "impl"),
            TokenKind::Struct => write!(f, "struct"),
            TokenKind::Trait => write!(f, "trait"),
            TokenKind::Enum => write!(f, "enum"),
            TokenKind::Match => write!(f, "match"),
            TokenKind::As => write!(f, "as"),
            TokenKind::For => write!(f, "for"),
            TokenKind::Pub => write!(f, "pub"),

            TokenKind::Int8 => write!(f, "i8"),
            TokenKind::Int16 => write!(f, "i16"),
            TokenKind::Int32 => write!(f, "i32"),
            TokenKind::Int64 => write!(f, "i64"),
            TokenKind::Uint8 => write!(f, "u8"),
            TokenKind::Uint16 => write!(f, "u16"),
            TokenKind::Uint32 => write!(f, "u32"),
            TokenKind::Uint64 => write!(f, "u64"),
            TokenKind::Isize => write!(f, "isize"),
            TokenKind::Usize => write!(f, "usize"),
            TokenKind::Float16 => write!(f, "f16"),
            TokenKind::Float32 => write!(f, "f32"),
            TokenKind::Float64 => write!(f, "f64"),
            TokenKind::Bool => write!(f, "bool"),
            TokenKind::Str => write!(f, "str"),
            TokenKind::Void => write!(f, "void"),
            TokenKind::Any => write!(f, "any"),

            TokenKind::LParen => write!(f, "("),
            TokenKind::RParen => write!(f, ")"),
            TokenKind::LBracket => write!(f, "["),
            TokenKind::RBracket => write!(f, "]"),
            TokenKind::LBrace => write!(f, "{{"),
            TokenKind::RBrace => write!(f, "}}"),
            TokenKind::Semicolon => write!(f, ";"),
            TokenKind::Colon => write!(f, ":"),
            TokenKind::Dot => write!(f, "."),
            TokenKind::Comma => write!(f, ","),
            TokenKind::Ampersand => write!(f, "&"),
            TokenKind::Pipe => write!(f, "|"),
            TokenKind::Hash => write!(f, "#"),
            TokenKind::At => write!(f, "@"),
            TokenKind::DotDot => write!(f, ".."),
            TokenKind::DotDotDot => write!(f, "..."),
            TokenKind::Percent => write!(f, "%"),

            TokenKind::Eq => write!(f, "="),
            TokenKind::FatArrow => write!(f, "=>"),
            TokenKind::Plus => write!(f, "+"),
            TokenKind::Minus => write!(f, "-"),
            TokenKind::Star => write!(f, "*"),
            TokenKind::Slash => write!(f, "/"),
            TokenKind::Lt => write!(f, "<"),
            TokenKind::Gt => write!(f, ">"),
            TokenKind::LtEq => write!(f, "<="),
            TokenKind::GtEq => write!(f, ">="),
            TokenKind::EqEq => write!(f, "=="),
            TokenKind::NotEq => write!(f, "!="),
            TokenKind::Bang => write!(f, "!"),

            TokenKind::PlusEq => write!(f, "+="),
            TokenKind::MinusEq => write!(f, "-="),
            TokenKind::StarEq => write!(f, "*="),
            TokenKind::SlashEq => write!(f, "/="),
            TokenKind::PercentEq => write!(f, "%="),
            TokenKind::PlusPlus => write!(f, "++"),
            TokenKind::MinusMinus => write!(f, "--"),

            TokenKind::Platform => write!(f, "platform"),
            TokenKind::Eof => write!(f, "EOF"),
        }
    }
}
