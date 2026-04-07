// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

pub mod token;
use token::Token;

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.input.get(self.pos).copied();
        self.pos += 1;
        ch
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_comment(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == '\n' {
                break;
            }
            self.advance();
        }
    }

    fn read_string(&mut self) -> Token {
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('"') | None => break,
                Some(ch) => s.push(ch),
            }
        }
        Token::StringLit(s)
    }

    fn read_number(&mut self, first: char) -> Token {
        let mut s = String::from(first);
        let mut is_float = false;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.advance();
            } else if ch == '.' && !is_float {
                is_float = true;
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        if is_float {
            Token::Float(s.parse().unwrap())
        } else {
            Token::Int(s.parse().unwrap())
        }
    }

    fn read_ident(&mut self, first: char) -> Token {
        let mut s = String::from(first);
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        match s.as_str() {
            "fn" => Token::Fn,
            "let" => Token::Let,
            "mut" => Token::Mut,
            "return" => Token::Return,
            "if" => Token::If,
            "else" => Token::Else,
            "import" => Token::Import,
            "impl" => Token::Impl,
            "struct" => Token::Struct,
            "trait" => Token::Trait,
            "const" => Token::Const,
            "for" => Token::For,
            "platform" => Token::Platform,
            "uint8" => Token::Uint8,
            "uint16" => Token::Uint16,
            "uint32" => Token::Uint32,
            "uint64" => Token::Uint64,
            "int8" => Token::Int8,
            "int16" => Token::Int16,
            "int32" => Token::Int32,
            "int64" => Token::Int64,
            "float16" => Token::Float16,
            "float32" => Token::Float32,
            "float64" => Token::Float64,
            "bool" => Token::Bool,
            "str" => Token::Str,
            "void" => Token::Void,
            "any" => Token::Any,
            "as" => Token::As,
            _ => Token::Ident(s),
        }
    }

    pub fn next_token(&mut self) -> Token {
        self.skip_whitespace();
        match self.advance() {
            None => Token::Eof,
            Some(ch) => match ch {
                '(' => Token::LParen,
                ')' => Token::RParen,
                '{' => Token::LBrace,
                '}' => Token::RBrace,
                ';' => Token::Semicolon,
                ':' => Token::Colon,
                ',' => Token::Comma,
                '&' => Token::Ampersand,
                '#' => Token::Hash,
                '+' => Token::Plus,
                '-' => Token::Minus,
                '*' => Token::Star,
                '"' => self.read_string(),
                '.' => {
                    if self.peek() == Some('.') {
                        self.advance();
                        if self.peek() == Some('.') {
                            self.advance();
                            return Token::DotDotDot;
                        }
                    }
                    Token::Dot
                }
                '/' => {
                    if self.peek() == Some('/') {
                        self.skip_comment();
                        self.next_token()
                    } else {
                        Token::Slash
                    }
                }
                '<' => {
                    if self.peek() == Some('=') {
                        self.advance();
                        Token::LtEq
                    } else {
                        Token::Lt
                    }
                }
                '>' => {
                    if self.peek() == Some('=') {
                        self.advance();
                        Token::GtEq
                    } else {
                        Token::Gt
                    }
                }
                '=' => {
                    if self.peek() == Some('=') {
                        self.advance();
                        Token::EqEq
                    } else {
                        Token::Eq
                    }
                }
                '!' => {
                    if self.peek() == Some('=') {
                        self.advance();
                        Token::NotEq
                    } else {
                        Token::Bang
                    }
                }
                ch if ch.is_ascii_digit() => self.read_number(ch),
                ch if ch.is_alphabetic() || ch == '_' => self.read_ident(ch),
                _ => self.next_token(),
            },
        }
    }

    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens: Vec<Token> = Vec::new();
        loop {
            let token = self.next_token();
            let is_eof = token == Token::Eof;
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        tokens
    }
}
