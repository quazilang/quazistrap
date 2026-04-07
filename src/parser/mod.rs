// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

pub mod ast; // AST types here!!
pub mod common; // contains implementation of common functions like peek(), advance(), etc.
pub mod items; // implementations of parsing items liek the function, structs, etc.
use crate::lexer::token::Token;
use crate::parser::ast::*;

/// parser defined here.
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn parse_item(&mut self) -> Result<Item, String> {
        match self.peek() {
            // TODO: we must handle not only import but also Fn, Struct, Trait, Impl
            Token::Import => self.parse_import(),
            Token::Fn => self.parse_fn(),
            Token::Impl => self.parse_impl(),
            Token::Trait => self.parse_trait(),
            Token::Struct => self.parse_struct(),
            token => Err(format!("unexpected token: {:?}", token)),
        }
    }

    pub fn parse(&mut self) -> Result<Program, String> {
        let mut items = Vec::new();
        while self.peek() != &Token::Eof {
            items.push(self.parse_item()?);
        }
        Ok(Program { items })
    }
}
