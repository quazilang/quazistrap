// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use crate::parser::Parser;
use crate::parser::Token;
use crate::parser::ast::*;

impl Parser {
    pub fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    pub fn advance(&mut self) -> Token {
        let token = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        token
    }

    pub fn parse_ident(&mut self) -> Result<String, String> {
        match self.advance() {
            Token::Ident(name) => Ok(name),
            other => Err(format!("expected identifier, got {:?}", other)),
        }
    }

    pub fn parse_stmt(&mut self) -> Result<Stmt, String> {
        match self.peek() {
            Token::Let => self.parse_let(),
            Token::Return => self.parse_return(),
            Token::If => self.parse_if(),
            _ => self.parse_expr_stmt(),
        }
    }

    pub fn parse_let(&mut self) -> Result<Stmt, String> {
        self.expect(Token::Let)?;

        let mutable = if self.peek() == &Token::Mut {
            self.advance();
            true
        } else {
            false
        };

        let name = self.parse_ident()?;

        let ty = if self.peek() == &Token::Colon {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };

        let value = if self.peek() == &Token::Eq {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        self.expect(Token::Semicolon)?;

        Ok(Stmt::Let {
            name,
            mutable,
            ty,
            value,
        })
    }

    pub fn parse_return(&mut self) -> Result<Stmt, String> {
        self.expect(Token::Return)?;

        let expr = if self.peek() != &Token::Semicolon {
            Some(self.parse_expr()?)
        } else {
            None
        };

        self.expect(Token::Semicolon)?;

        Ok(Stmt::Return(expr))
    }

    pub fn parse_if(&mut self) -> Result<Stmt, String> {
        self.expect(Token::If)?;

        // если у тебя есть скобки: if (x > y)
        let condition = if self.peek() == &Token::LParen {
            self.advance();
            let cond = self.parse_expr()?;
            self.expect(Token::RParen)?;
            cond
        } else {
            self.parse_expr()?
        };

        let then_block = self.parse_block()?;

        let else_block = if self.peek() == &Token::Else {
            self.advance();
            Some(self.parse_block()?)
        } else {
            None
        };

        Ok(Stmt::If {
            condition,
            then_block,
            else_block,
        })
    }

    pub fn parse_expr_stmt(&mut self) -> Result<Stmt, String> {
        let expr = self.parse_expr()?;
        self.expect(Token::Semicolon)?;
        Ok(Stmt::ExprStmt(expr))
    }

    pub fn parse_expr(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.peek() {
                Token::Dot => {
                    self.advance(); // съели '.'

                    let method = self.parse_ident()?;

                    self.expect(Token::LParen)?;

                    let mut args = Vec::new();

                    while self.peek() != &Token::RParen {
                        args.push(self.parse_expr()?);

                        if self.peek() == &Token::Comma {
                            self.advance();
                        } else {
                            break;
                        }
                    }

                    self.expect(Token::RParen)?;

                    expr = Expr::MethodCall {
                        object: Box::new(expr),
                        method,
                        args,
                    };
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    pub fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.advance() {
            Token::Int(n) => Ok(Expr::IntLit(n)),
            Token::Float(f) => Ok(Expr::FloatLit(f)),
            Token::StringLit(s) => Ok(Expr::StringLit(s)),
            Token::Ident(name) => Ok(Expr::Ident(name)),
            other => Err(format!("unexpected token in expression: {:?}", other)),
        }
    }

    pub fn parse_block(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect(Token::LBrace)?;

        let mut stmts = Vec::new();

        while self.peek() != &Token::RBrace {
            stmts.push(self.parse_stmt()?);
        }

        self.expect(Token::RBrace)?;

        Ok(stmts)
    }

    pub fn parse_type(&mut self) -> Result<Type, String> {
        match self.advance() {
            Token::Uint8 => Ok(Type::Uint8),
            Token::Uint16 => Ok(Type::Uint16),
            Token::Uint32 => Ok(Type::Uint32),
            Token::Uint64 => Ok(Type::Uint64),

            Token::Int8 => Ok(Type::Int8),
            Token::Int16 => Ok(Type::Int16),
            Token::Int32 => Ok(Type::Int32),
            Token::Int64 => Ok(Type::Int64),

            Token::Float16 => Ok(Type::Float16),
            Token::Float32 => Ok(Type::Float32),
            Token::Float64 => Ok(Type::Float64),

            Token::Bool => Ok(Type::Bool),
            Token::Str => Ok(Type::Str),
            Token::Void => Ok(Type::Void),

            Token::Ident(name) => Ok(Type::Named(name)),

            _ => Err(format!("expected type, found {:?}", self.peek()).into()),
        }
    }

    pub fn expect(&mut self, expected: Token) -> Result<(), String> {
        if self.peek() == &expected {
            self.advance();
            Ok(())
        } else {
            Err(format!("expected {:?}, got {:?}", expected, self.peek()))
        }
    }
}
