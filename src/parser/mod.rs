// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

pub mod ast;
pub mod common;
pub mod items;

use crate::parser::ast::*;
use crate::parser::common::{merge_token_spans, to_ast_span};
use crate::lexer::token::{Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse(&mut self) -> Result<Program, String> {
        let mut items = Vec::new();
        let start = self.current_span();

        while !self.at(TokenKind::Eof) {
            items.push(self.parse_item()?);
        }

        let end = self.current_span();
        let span = if items.is_empty() {
            None
        } else {
            Some(to_ast_span(merge_token_spans(start, end)))
        };

        Ok(Program { items, span })
    }

    fn parse_item(&mut self) -> Result<Item, String> {
        match self.peek_kind() {
            TokenKind::Import => self.parse_import(),
            TokenKind::Fn => self.parse_fn(),
            TokenKind::Struct => self.parse_struct(),
            TokenKind::Trait => self.parse_trait(),
            TokenKind::Impl => self.parse_impl(),
            other => Err(self.err_here(format!("unexpected token in item position: {:?}", other))),
        }
    }

    // ===== statements =====

    pub fn parse_stmt(&mut self) -> Result<Stmt, String> {
        match self.peek_kind() {
            TokenKind::Var => self.parse_var_stmt(),
            TokenKind::Const => self.parse_const_stmt(),
            TokenKind::Return => self.parse_return_stmt(),
            TokenKind::If => self.parse_if_stmt(),
            TokenKind::While => self.parse_while_stmt(),
            _ => self.parse_expr_stmt(),
        }
    }

    pub fn parse_block(&mut self) -> Result<Block, String> {
        let lbrace = self.expect(TokenKind::LBrace)?.span;
        let mut stmts = Vec::new();

        while !self.at(TokenKind::RBrace) {
            if self.at(TokenKind::Eof) {
                return Err(self.err_here("unexpected EOF while parsing block".to_string()));
            }
            stmts.push(self.parse_stmt()?);
        }

        let rbrace = self.expect(TokenKind::RBrace)?.span;

        Ok(Block {
            stmts,
            span: to_ast_span(merge_token_spans(lbrace, rbrace)),
        })
    }

    fn parse_var_stmt(&mut self) -> Result<Stmt, String> {
        let start = self.expect(TokenKind::Var)?.span;

        let name = self.parse_ident()?;

        let ty = if self.at(TokenKind::Colon) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };

        let value = if self.at(TokenKind::Eq) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        let semi = self.expect(TokenKind::Semicolon)?.span;

        Ok(Spanned::new(
            StmtKind::Var { name, ty, value },
            to_ast_span(merge_token_spans(start, semi)),
        ))
    }

    fn parse_const_stmt(&mut self) -> Result<Stmt, String> {
        let start = self.expect(TokenKind::Const)?.span;

        let name = self.parse_ident()?;

        let ty = if self.at(TokenKind::Colon) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };

        self.expect(TokenKind::Eq)?;
        let value = self.parse_expr()?;
        let semi = self.expect(TokenKind::Semicolon)?.span;

        Ok(Spanned::new(
            StmtKind::Const { name, ty, value },
            to_ast_span(merge_token_spans(start, semi)),
        ))
    }

    fn parse_return_stmt(&mut self) -> Result<Stmt, String> {
        let start = self.expect(TokenKind::Return)?.span;

        let expr = if self.at(TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expr()?)
        };

        let semi = self.expect(TokenKind::Semicolon)?.span;

        Ok(Spanned::new(
            StmtKind::Return(expr),
            to_ast_span(merge_token_spans(start, semi)),
        ))
    }

    fn parse_if_stmt(&mut self) -> Result<Stmt, String> {
        let start = self.expect(TokenKind::If)?.span;

        let condition = if self.at(TokenKind::LParen) {
            self.advance();
            let c = self.parse_expr()?;
            self.expect(TokenKind::RParen)?;
            c
        } else {
            self.parse_expr()?
        };

        let then_block = self.parse_block()?;

        let else_block = if self.at(TokenKind::Else) {
            self.advance();
            Some(self.parse_block()?)
        } else {
            None
        };

        let end_span = else_block
            .as_ref()
            .map(|b| b.span)
            .unwrap_or(then_block.span);

        Ok(Spanned::new(
            StmtKind::If {
                condition,
                then_block,
                else_block,
            },
            Span::merge(to_ast_span(start), end_span),
        ))
    }

    fn parse_while_stmt(&mut self) -> Result<Stmt, String> {
        let start = self.expect(TokenKind::While)?.span;

        let condition = if self.at(TokenKind::LParen) {
            self.advance();
            let c = self.parse_expr()?;
            self.expect(TokenKind::RParen)?;
            c
        } else {
            self.parse_expr()?
        };

        let body = self.parse_block()?;
        let end = body.span;

        Ok(Spanned::new(
            StmtKind::While { condition, body },
            Span::merge(to_ast_span(start), end),
        ))
    }

    fn parse_expr_stmt(&mut self) -> Result<Stmt, String> {
        let expr = self.parse_expr()?;
        let semi = self.expect(TokenKind::Semicolon)?.span;
        let span = Span::merge(expr.span, to_ast_span(semi));
        Ok(Spanned::new(StmtKind::ExprStmt(expr), span))
    }

    // ===== expressions (precedence climbing) =====
    // assignment (=, right-assoc)
    // logical or (||)
    // logical and (&&)
    // equality (==, !=)
    // comparison (<, <=, >, >=)
    // term (+, -)
    // factor (*, /)
    // unary (!, -)
    // postfix (call, field, method-call)
    // primary

    pub fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Result<Expr, String> {
        let left = self.parse_logical_or()?;

        if self.at(TokenKind::Eq) {
            self.advance();
            let value = self.parse_assignment()?;
            let span = Span::merge(left.span, value.span);

            return Ok(Spanned::new(
                ExprKind::Assign {
                    target: Box::new(left),
                    value: Box::new(value),
                },
                span,
            ));
        }

        Ok(left)
    }

    fn parse_logical_or(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_logical_and()?;

        while self.match_or_or() {
            let right = self.parse_logical_and()?;
            let span = Span::merge(expr.span, right.span);
            expr = Spanned::new(
                ExprKind::Binary {
                    left: Box::new(expr),
                    op: BinOpKind::OrOr,
                    right: Box::new(right),
                },
                span,
            );
        }

        Ok(expr)
    }

    fn parse_logical_and(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_equality()?;

        while self.match_and_and() {
            let right = self.parse_equality()?;
            let span = Span::merge(expr.span, right.span);
            expr = Spanned::new(
                ExprKind::Binary {
                    left: Box::new(expr),
                    op: BinOpKind::AndAnd,
                    right: Box::new(right),
                },
                span,
            );
        }

        Ok(expr)
    }

    fn parse_equality(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_comparison()?;

        loop {
            let op = if self.at(TokenKind::EqEq) {
                self.advance();
                Some(BinOpKind::EqEq)
            } else if self.at(TokenKind::NotEq) {
                self.advance();
                Some(BinOpKind::NotEq)
            } else {
                None
            };

            let Some(op) = op else { break };

            let right = self.parse_comparison()?;
            let span = Span::merge(expr.span, right.span);
            expr = Spanned::new(
                ExprKind::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                },
                span,
            );
        }

        Ok(expr)
    }

    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_term()?;

        loop {
            let op = match self.peek_kind() {
                TokenKind::Lt => {
                    self.advance();
                    Some(BinOpKind::Lt)
                }
                TokenKind::LtEq => {
                    self.advance();
                    Some(BinOpKind::LtEq)
                }
                TokenKind::Gt => {
                    self.advance();
                    Some(BinOpKind::Gt)
                }
                TokenKind::GtEq => {
                    self.advance();
                    Some(BinOpKind::GtEq)
                }
                _ => None,
            };

            let Some(op) = op else { break };

            let right = self.parse_term()?;
            let span = Span::merge(expr.span, right.span);
            expr = Spanned::new(
                ExprKind::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                },
                span,
            );
        }

        Ok(expr)
    }

    fn parse_term(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_factor()?;

        loop {
            let op = match self.peek_kind() {
                TokenKind::Plus => {
                    self.advance();
                    Some(BinOpKind::Add)
                }
                TokenKind::Minus => {
                    self.advance();
                    Some(BinOpKind::Sub)
                }
                _ => None,
            };

            let Some(op) = op else { break };

            let right = self.parse_factor()?;
            let span = Span::merge(expr.span, right.span);
            expr = Spanned::new(
                ExprKind::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                },
                span,
            );
        }

        Ok(expr)
    }

    fn parse_factor(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_unary()?;

        loop {
            let op = match self.peek_kind() {
                TokenKind::Star => {
                    self.advance();
                    Some(BinOpKind::Mul)
                }
                TokenKind::Slash => {
                    self.advance();
                    Some(BinOpKind::Div)
                }
                _ => None,
            };

            let Some(op) = op else { break };

            let right = self.parse_unary()?;
            let span = Span::merge(expr.span, right.span);
            expr = Spanned::new(
                ExprKind::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                },
                span,
            );
        }

        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        if self.at(TokenKind::Bang) {
            let t = self.advance().span;
            let expr = self.parse_unary()?;
            let span = Span::merge(to_ast_span(t), expr.span);
            return Ok(Spanned::new(
                ExprKind::Unary {
                    op: UnaryOpKind::Not,
                    expr: Box::new(expr),
                },
                span,
            ));
        }

        if self.at(TokenKind::Minus) {
            let t = self.advance().span;
            let expr = self.parse_unary()?;
            let span = Span::merge(to_ast_span(t), expr.span);
            return Ok(Spanned::new(
                ExprKind::Unary {
                    op: UnaryOpKind::Neg,
                    expr: Box::new(expr),
                },
                span,
            ));
        }

        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_primary()?;

        loop {
            if self.at(TokenKind::LParen) {
                let args = self.parse_call_args()?;
                let end = args.last().map(|a| a.span).unwrap_or(expr.span);
                let span = Span::merge(expr.span, end);
                expr = Spanned::new(
                    ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    span,
                );
                continue;
            }

            if self.at(TokenKind::Dot) {
                self.advance();
                let name_tok = self.expect_ident_token()?;
                let name = match &name_tok.kind {
                    TokenKind::Ident(s) => s.clone(),
                    _ => unreachable!(),
                };

                if self.at(TokenKind::LParen) {
                    let args = self.parse_call_args()?;
                    let end = args
                        .last()
                        .map(|a| a.span)
                        .unwrap_or(to_ast_span(name_tok.span));
                    let span = Span::merge(expr.span, end);
                    expr = Spanned::new(
                        ExprKind::MethodCall {
                            object: Box::new(expr),
                            method: name,
                            args,
                        },
                        span,
                    );
                } else {
                    let span = Span::merge(expr.span, to_ast_span(name_tok.span));
                    expr = Spanned::new(
                        ExprKind::Field {
                            object: Box::new(expr),
                            name,
                        },
                        span,
                    );
                }

                continue;
            }

            break;
        }

        Ok(expr)
    }

    fn parse_call_args(&mut self) -> Result<Vec<Expr>, String> {
        self.expect(TokenKind::LParen)?;
        let mut args = Vec::new();

        if !self.at(TokenKind::RParen) {
            loop {
                args.push(self.parse_expr()?);

                if self.at(TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
        }

        self.expect(TokenKind::RParen)?;
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        let tok = self.advance();

        match tok.kind {
            TokenKind::Int(n) => Ok(Spanned::new(
                ExprKind::Literal(Literal::Int(n)),
                to_ast_span(tok.span),
            )),
            TokenKind::Float(f) => Ok(Spanned::new(
                ExprKind::Literal(Literal::Float(f)),
                to_ast_span(tok.span),
            )),
            TokenKind::StringLit(s) => Ok(Spanned::new(
                ExprKind::Literal(Literal::String(s)),
                to_ast_span(tok.span),
            )),
            TokenKind::Ident(name) => {
                Ok(Spanned::new(ExprKind::Ident(name), to_ast_span(tok.span)))
            }
            TokenKind::LParen => {
                let expr = self.parse_expr()?;
                let r = self.expect(TokenKind::RParen)?.span;
                let span = Span::merge(to_ast_span(tok.span), to_ast_span(r));
                Ok(Spanned::new(ExprKind::Group(Box::new(expr)), span))
            }
            other => Err(self.err_tok(
                tok.span,
                format!("unexpected token in expression: {:?}", other),
            )),
        }
    }

    // ===== types =====

    pub fn parse_type(&mut self) -> Result<Type, String> {
        let tok = self.advance();
        let kind = match tok.kind {
            TokenKind::Uint8 => TypeKind::Uint8,
            TokenKind::Uint16 => TypeKind::Uint16,
            TokenKind::Uint32 => TypeKind::Uint32,
            TokenKind::Uint64 => TypeKind::Uint64,
            TokenKind::Int8 => TypeKind::Int8,
            TokenKind::Int16 => TypeKind::Int16,
            TokenKind::Int32 => TypeKind::Int32,
            TokenKind::Int64 => TypeKind::Int64,
            TokenKind::Float16 => TypeKind::Float16,
            TokenKind::Float32 => TypeKind::Float32,
            TokenKind::Float64 => TypeKind::Float64,
            TokenKind::Bool => TypeKind::Bool,
            TokenKind::Str => TypeKind::Str,
            TokenKind::Void => TypeKind::Void,
            TokenKind::Any => TypeKind::Any,
            TokenKind::Ident(name) => TypeKind::Named(name),
            other => {
                return Err(self.err_tok(tok.span, format!("expected type, found {:?}", other)));
            }
        };

        Ok(Spanned::new(kind, to_ast_span(tok.span)))
    }

}
