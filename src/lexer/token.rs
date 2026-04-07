// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

 #[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // literals
    Int(i64),
    Float(f64),
    StringLit(String),
    Ident(String),

    // keywords
    Fn,
    Let,
    Mut,
    Return,
    If,
    Else,
    Import,
    Impl,
    Struct,
    Trait,
    Const,
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
    Hash,      // #
    DotDotDot, // ...

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
