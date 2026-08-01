// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

pub mod token;
use token::{Span, Token, TokenKind};

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.input.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.input.get(self.pos).copied();
        if let Some(c) = ch {
            self.pos += 1;
            if c == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
        ch
    }

    fn make_span(&self, start: usize, end: usize, line: usize, col: usize) -> Span {
        Span {
            line,
            col,
            start,
            end,
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(ch) if ch.is_whitespace() => {
                    self.advance();
                }
                Some('/') if self.peek_next() == Some('/') => {
                    // consume //
                    self.advance();
                    self.advance();

                    // skip until newline (newline itself is handled by whitespace pass)
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    fn read_string(&mut self, start: usize, line: usize, col: usize) -> Token {
        let mut s = String::new();

        while let Some(ch) = self.advance() {
            match ch {
                '"' => {
                    let span = self.make_span(start, self.pos, line, col);
                    return Token::new(TokenKind::StringLit(s), span);
                }
                '\\' => {
                    if let Some(esc) = self.advance() {
                        let mapped = match esc {
                            'n' => '\n',
                            't' => '\t',
                            'r' => '\r',
                            'e' => '\u{001b}',
                            '"' => '"',
                            '\\' => '\\',
                            other => other,
                        };
                        s.push(mapped);
                    } else {
                        break;
                    }
                }
                other => s.push(other),
            }
        }

        // Unterminated string: still emit StringLit so parser can continue.
        let span = self.make_span(start, self.pos, line, col);
        Token::new(TokenKind::StringLit(s), span)
    }

    fn read_raw_string(&mut self, start: usize, line: usize, col: usize) -> Token {
        let mut s = String::new();

        while let Some(ch) = self.advance() {
            if ch == '`' {
                let span = self.make_span(start, self.pos, line, col);
                return Token::new(TokenKind::StringLit(s), span);
            }
            s.push(ch);
        }

        let span = self.make_span(start, self.pos, line, col);
        Token::new(TokenKind::StringLit(s), span)
    }

    fn read_number(&mut self, first: char, start: usize, line: usize, col: usize) -> Token {
        let mut s = String::from(first);
        let mut is_float = false;

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.advance();
            } else if ch == '.' && !is_float {
                // only treat as float if next char is digit
                if self.peek_next().is_some_and(|n| n.is_ascii_digit()) {
                    is_float = true;
                    s.push(ch);
                    self.advance();
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let kind = if is_float {
            TokenKind::Float(s.parse().unwrap_or(0.0))
        } else {
            TokenKind::Int(s.parse().unwrap_or(0))
        };

        let span = self.make_span(start, self.pos, line, col);
        Token::new(kind, span)
    }

    fn read_ident_or_keyword(
        &mut self,
        first: char,
        start: usize,
        line: usize,
        col: usize,
    ) -> Token {
        let mut s = String::from(first);

        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        let kind = match s.as_str() {
            // keywords
            "fn" => TokenKind::Fn,
            "var" => TokenKind::Var,
            "const" => TokenKind::Const,
            "ret" => TokenKind::Return,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "import" => TokenKind::Import,
            "impl" => TokenKind::Impl,
            "struct" => TokenKind::Struct,
            "trait" => TokenKind::Trait,
            "enum" => TokenKind::Enum,
            "match" => TokenKind::Match,
            "as" => TokenKind::As,
            "for" => TokenKind::For,
            "pub" => TokenKind::Pub,
            "unsafe" => TokenKind::Unsafe,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "type" => TokenKind::Type,
            "platform" => TokenKind::Platform,

            // primitive types
            "i8" => TokenKind::Int8,
            "i16" => TokenKind::Int16,
            "i32" => TokenKind::Int32,
            "i64" => TokenKind::Int64,
            "u8" => TokenKind::Uint8,
            "u16" => TokenKind::Uint16,
            "u32" => TokenKind::Uint32,
            "u64" => TokenKind::Uint64,
            "isize" => TokenKind::Isize,
            "usize" => TokenKind::Usize,
            "f16" => TokenKind::Float16,
            "f32" => TokenKind::Float32,
            "f64" => TokenKind::Float64,
            "bool" => TokenKind::Bool,
            "str" => TokenKind::Str,
            "void" => TokenKind::Void,
            "any" => TokenKind::Any,
            "true" => TokenKind::True,
            "false" => TokenKind::False,

            _ => TokenKind::Ident(s),
        };

        let span = self.make_span(start, self.pos, line, col);
        Token::new(kind, span)
    }

    fn lex_error(&self, ch: char, start: usize, line: usize, col: usize) -> Token {
        let escaped: String = ch.escape_default().collect();
        let span = self.make_span(start, self.pos, line, col);
        Token::new(
            TokenKind::Error(format!("unexpected character '{}'", escaped)),
            span,
        )
    }

    pub fn next_token(&mut self) -> Token {
        self.skip_whitespace_and_comments();

        let start = self.pos;
        let line = self.line;
        let col = self.col;

        match self.advance() {
            None => Token::eof(self.make_span(self.pos, self.pos, self.line, self.col)),
            Some(ch) => {
                let kind = match ch {
                    '(' => TokenKind::LParen,
                    ')' => TokenKind::RParen,
                    '[' => TokenKind::LBracket,
                    ']' => TokenKind::RBracket,
                    '{' => TokenKind::LBrace,
                    '}' => TokenKind::RBrace,
                    ';' => TokenKind::Semicolon,
                    ':' => TokenKind::Colon,
                    ',' => TokenKind::Comma,
                    '.' => {
                        if self.peek() == Some('.') {
                            self.advance();
                            if self.peek() == Some('.') {
                                self.advance();
                                TokenKind::DotDotDot
                            } else {
                                TokenKind::DotDot
                            }
                        } else {
                            TokenKind::Dot
                        }
                    }
                    '&' => TokenKind::Ampersand,
                    '|' => TokenKind::Pipe,
                    '#' => TokenKind::Hash,
                    '@' => TokenKind::At,

                    '+' => {
                        if self.peek() == Some('=') {
                            self.advance();
                            TokenKind::PlusEq
                        } else if self.peek() == Some('+') {
                            self.advance();
                            TokenKind::PlusPlus
                        } else {
                            TokenKind::Plus
                        }
                    }
                    '-' => {
                        if self.peek() == Some('=') {
                            self.advance();
                            TokenKind::MinusEq
                        } else if self.peek() == Some('-') {
                            self.advance();
                            TokenKind::MinusMinus
                        } else {
                            TokenKind::Minus
                        }
                    }
                    '*' => {
                        if self.peek() == Some('=') {
                            self.advance();
                            TokenKind::StarEq
                        } else if self.peek() == Some('*') {
                            self.advance();
                            TokenKind::StarStar
                        } else {
                            TokenKind::Star
                        }
                    }
                    '/' => {
                        if self.peek() == Some('=') {
                            self.advance();
                            TokenKind::SlashEq
                        } else {
                            TokenKind::Slash
                        }
                    }
                    '%' => {
                        if self.peek() == Some('=') {
                            self.advance();
                            TokenKind::PercentEq
                        } else {
                            TokenKind::Percent
                        }
                    }

                    '<' => {
                        if self.peek() == Some('<') {
                            self.advance();
                            TokenKind::Shl
                        } else if self.peek() == Some('=') {
                            self.advance();
                            TokenKind::LtEq
                        } else {
                            TokenKind::Lt
                        }
                    }
                    '>' => {
                        if self.peek() == Some('>') {
                            self.advance();
                            TokenKind::Shr
                        } else if self.peek() == Some('=') {
                            self.advance();
                            TokenKind::GtEq
                        } else {
                            TokenKind::Gt
                        }
                    }
                    '^' => TokenKind::Caret,
                    '=' => {
                        if self.peek() == Some('=') {
                            self.advance();
                            TokenKind::EqEq
                        } else if self.peek() == Some('>') {
                            self.advance();
                            TokenKind::FatArrow
                        } else {
                            TokenKind::Eq
                        }
                    }
                    '!' => {
                        if self.peek() == Some('=') {
                            self.advance();
                            TokenKind::NotEq
                        } else {
                            TokenKind::Bang
                        }
                    }
                    '?' => TokenKind::Question,

                    '"' => return self.read_string(start, line, col),
                    '`' => return self.read_raw_string(start, line, col),

                    c if c.is_ascii_digit() => return self.read_number(c, start, line, col),
                    c if c.is_alphabetic() || c == '_' => {
                        return self.read_ident_or_keyword(c, start, line, col);
                    }

                    _ => return self.lex_error(ch, start, line, col),
                };

                let span = self.make_span(start, self.pos, line, col);
                Token::new(kind, span)
            }
        }
    }

    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();

        loop {
            let tok = self.next_token();
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }

        tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_at_token_for_at_sign() {
        let mut lexer = Lexer::new("@");
        let tokens = lexer.tokenize();
        assert!(matches!(
            tokens.first().map(|t| &t.kind),
            Some(TokenKind::At)
        ));
        assert!(matches!(
            tokens.last().map(|t| &t.kind),
            Some(TokenKind::Eof)
        ));
    }

    #[test]
    fn emits_error_token_for_unknown_character() {
        let mut lexer = Lexer::new("$");
        let tokens = lexer.tokenize();
        assert!(matches!(
            tokens.first().map(|t| &t.kind),
            Some(TokenKind::Error(_))
        ));
    }

    #[test]
    fn isize_usize_are_keywords() {
        let mut lexer = Lexer::new("isize usize");
        let tokens = lexer.tokenize();
        assert!(matches!(tokens[0].kind, TokenKind::Isize));
        assert!(matches!(tokens[1].kind, TokenKind::Usize));
    }

    #[test]
    fn numeric_type_keywords_use_short_names() {
        let mut lexer = Lexer::new("i8 i16 i32 i64 u8 u16 u32 u64 f16 f32 f64");
        let tokens = lexer.tokenize();
        assert!(matches!(tokens[0].kind, TokenKind::Int8));
        assert!(matches!(tokens[1].kind, TokenKind::Int16));
        assert!(matches!(tokens[2].kind, TokenKind::Int32));
        assert!(matches!(tokens[3].kind, TokenKind::Int64));
        assert!(matches!(tokens[4].kind, TokenKind::Uint8));
        assert!(matches!(tokens[5].kind, TokenKind::Uint16));
        assert!(matches!(tokens[6].kind, TokenKind::Uint32));
        assert!(matches!(tokens[7].kind, TokenKind::Uint64));
        assert!(matches!(tokens[8].kind, TokenKind::Float16));
        assert!(matches!(tokens[9].kind, TokenKind::Float32));
        assert!(matches!(tokens[10].kind, TokenKind::Float64));
    }
}

#[cfg(test)]
mod string_tests {
    use super::*;

    fn string_value(source: &str) -> String {
        match Lexer::new(source).next_token().kind {
            TokenKind::StringLit(value) => value,
            other => panic!("expected string literal, got {other:?}"),
        }
    }

    #[test]
    fn quoted_strings_decode_ansi_escape() {
        assert_eq!(string_value(r#""\e[31mred\e[0m""#), "\u{001b}[31mred\u{001b}[0m");
    }

    #[test]
    fn raw_strings_preserve_backslashes_and_quotes() {
        assert_eq!(string_value(r#"`line\n\e[31m"quoted"`"#), r#"line\n\e[31m"quoted""#);
    }

    #[test]
    fn raw_strings_can_span_lines() {
        assert_eq!(string_value("`first\nsecond`"), "first\nsecond");
    }
}
