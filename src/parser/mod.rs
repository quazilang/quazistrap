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
    source: Option<String>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            source: None,
        }
    }

    pub fn new_with_source(tokens: Vec<Token>, source: &str) -> Self {
        Self {
            tokens,
            pos: 0,
            source: Some(source.to_string()),
        }
    }

    pub fn parse(&mut self) -> Result<Program, String> {
        let mut items = Vec::new();
        let start = self.current_span();
        let mut first_err: Option<String> = None;

        while !self.at(TokenKind::Eof) {
            match self.parse_item() {
                Ok(item) => items.push(item),
                Err(err) => {
                    if first_err.is_none() {
                        first_err = Some(err);
                    }
                    self.synchronize_item();
                }
            }
        }

        if let Some(err) = first_err {
            return Err(err);
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
            TokenKind::Error(msg) => Err(self.err_here_with_code("E00", format!("lexer error: {}", msg))),
            TokenKind::Import => self.parse_import(),
            TokenKind::Fn => self.parse_fn(),
            TokenKind::Struct => self.parse_struct(),
            TokenKind::Trait => self.parse_trait(),
            TokenKind::Enum => self.parse_enum(),
            TokenKind::Impl => self.parse_impl(),
            other => Err(self.err_here_with_code(
                "E03",
                format!("unexpected token in item position: {}", other),
            )),
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
        let mut first_err: Option<String> = None;

        while !self.at(TokenKind::RBrace) {
            if self.at(TokenKind::Eof) {
                return Err(self.err_here_with_code(
                    "E04",
                    "unexpected EOF while parsing block".to_string(),
                ));
            }

            match self.parse_stmt() {
                Ok(stmt) => stmts.push(stmt),
                Err(err) => {
                    if first_err.is_none() {
                        first_err = Some(err);
                    }
                    self.synchronize_stmt();
                }
            }
        }

        let rbrace = self.expect(TokenKind::RBrace)?.span;

        if let Some(err) = first_err {
            return Err(err);
        }

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
    // factor (*, /, %)
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
                TokenKind::Percent => {
                    self.advance();
                    Some(BinOpKind::Mod)
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
            let call_checkpoint = self.checkpoint();
            let type_args = self.try_parse_type_args_for_call();

            if self.at(TokenKind::LParen) {
                let args = self.parse_call_args()?;
                let end = args.last().map(|a| a.span).unwrap_or(expr.span);
                let span = Span::merge(expr.span, end);
                expr = Spanned::new(
                    ExprKind::Call {
                        callee: Box::new(expr),
                        type_args,
                        args,
                    },
                    span,
                );
                continue;
            }

            if !type_args.is_empty() {
                self.restore(call_checkpoint);
            }

            if self.at(TokenKind::Dot) {
                self.advance();
                let name_tok = self.expect_ident_token()?;
                let name = match &name_tok.kind {
                    TokenKind::Ident(s) => s.clone(),
                    _ => unreachable!(),
                };

                let method_checkpoint = self.checkpoint();
                let method_type_args = self.try_parse_type_args_for_call();

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
                            type_args: method_type_args,
                            args,
                        },
                        span,
                    );
                } else {
                    if !method_type_args.is_empty() {
                        self.restore(method_checkpoint);
                    }

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

    fn parse_match_expr(&mut self, match_span: crate::lexer::token::Span) -> Result<Expr, String> {
        let scrutinee = self.parse_expr()?;
        self.expect(TokenKind::LBrace)?;

        let mut arms = Vec::new();

        while !self.at(TokenKind::RBrace) {
            if self.at(TokenKind::Eof) {
                return Err(self.err_here_with_code(
                    "E04",
                    "unexpected EOF while parsing match expression".to_string(),
                ));
            }

            let pattern = self.parse_pattern()?;
            self.expect(TokenKind::FatArrow)?;
            let arm_expr = self.parse_expr()?;

            let arm_span = Span::merge(pattern.span, arm_expr.span);
            arms.push(MatchArm {
                pattern,
                expr: arm_expr,
                span: arm_span,
            });

            if self.at(TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        let rbrace = self.expect(TokenKind::RBrace)?.span;
        let span = Span::merge(to_ast_span(match_span), to_ast_span(rbrace));

        Ok(Spanned::new(
            ExprKind::Match {
                scrutinee: Box::new(scrutinee),
                arms,
            },
            span,
        ))
    }

    fn parse_pattern(&mut self) -> Result<Pattern, String> {
        let first_tok = self.expect_ident_token()?;
        let first = match &first_tok.kind {
            TokenKind::Ident(s) => s.clone(),
            _ => unreachable!(),
        };

        if first == "_" {
            return Ok(Spanned::new(PatternKind::Wildcard, to_ast_span(first_tok.span)));
        }

        let mut enum_name = None;
        let mut variant = first;
        let mut end = first_tok.span;

        if self.at(TokenKind::Dot) {
            self.advance();
            let variant_tok = self.expect_ident_token()?;
            enum_name = Some(variant);
            variant = match &variant_tok.kind {
                TokenKind::Ident(s) => s.clone(),
                _ => unreachable!(),
            };
            end = variant_tok.span;
        }

        let mut bindings = Vec::new();
        if self.at(TokenKind::LParen) {
            self.advance();

            if !self.at(TokenKind::RParen) {
                loop {
                    bindings.push(self.parse_ident()?);

                    if self.at(TokenKind::Comma) {
                        self.advance();
                        continue;
                    }
                    break;
                }
            }

            end = self.expect(TokenKind::RParen)?.span;
        }

        Ok(Spanned::new(
            PatternKind::Variant {
                enum_name,
                variant,
                bindings,
            },
            to_ast_span(merge_token_spans(first_tok.span, end)),
        ))
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
            TokenKind::Match => self.parse_match_expr(tok.span),
            TokenKind::Error(msg) => {
                Err(self.err_tok_with_code(tok.span, "E00", format!("lexer error: {}", msg)))
            }
            other => Err(self.err_tok(
                tok.span,
                format!("unexpected token in expression: {}", other),
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
            TokenKind::Ident(name) => {
                let type_args = self.parse_optional_type_args()?;
                TypeKind::Named { name, type_args }
            }
            other => {
                return Err(self.err_tok_with_code(
                    tok.span,
                    "E05",
                    format!("expected type, found {}", other),
                ));
            }
        };

        Ok(Spanned::new(kind, to_ast_span(tok.span)))
    }

    pub(crate) fn parse_optional_generic_params(&mut self) -> Result<Vec<String>, String> {
        if !self.at(TokenKind::LBracket) {
            return Ok(Vec::new());
        }

        self.expect(TokenKind::LBracket)?;
        let mut params = Vec::new();

        loop {
            params.push(self.parse_ident()?);

            if self.at(TokenKind::Comma) {
                self.advance();
                continue;
            }
            break;
        }

        self.expect(TokenKind::RBracket)?;
        Ok(params)
    }

    fn parse_optional_type_args(&mut self) -> Result<Vec<Type>, String> {
        if !self.at(TokenKind::LBracket) {
            return Ok(Vec::new());
        }

        self.parse_type_args()
    }

    fn parse_type_args(&mut self) -> Result<Vec<Type>, String> {
        self.expect(TokenKind::LBracket)?;
        let mut args = Vec::new();

        loop {
            args.push(self.parse_type()?);

            if self.at(TokenKind::Comma) {
                self.advance();
                continue;
            }
            break;
        }

        self.expect(TokenKind::RBracket)?;
        Ok(args)
    }

    fn try_parse_type_args_for_call(&mut self) -> Vec<Type> {
        if !self.at(TokenKind::LBracket) {
            return Vec::new();
        }

        let checkpoint = self.checkpoint();
        match self.parse_type_args() {
            Ok(type_args) => type_args,
            Err(_) => {
                self.restore(checkpoint);
                Vec::new()
            }
        }
    }

}

#[cfg(test)]
mod tests {
    use crate::lexer::Lexer;

    use super::*;

    fn parse_program(src: &str) -> Program {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        parser.parse().expect("source should parse")
    }

    fn parse_program_err(src: &str) -> String {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new_with_source(tokens, src);
        parser.parse().expect_err("source should fail to parse")
    }

    #[test]
    fn parses_modulo_as_factor_operator() {
        let program = parse_program(
            r#"
fn main() void {
    var x: int32 = 10 % 3 * 2;
}
"#,
        );

        let ItemKind::Fn { body, .. } = &program.items[0].node else {
            panic!("expected function item");
        };

        let StmtKind::Var {
            value: Some(expr), ..
        } = &body.stmts[0].node
        else {
            panic!("expected var statement with initializer");
        };

        let ExprKind::Binary {
            left,
            op: top_op,
            right: _,
        } = &expr.node
        else {
            panic!("expected binary expression");
        };
        assert!(matches!(top_op, BinOpKind::Mul));

        let ExprKind::Binary { op: left_op, .. } = &left.node else {
            panic!("expected left side to be binary");
        };
        assert!(matches!(left_op, BinOpKind::Mod));
    }

    #[test]
    fn reports_lexer_error_token_at_top_level() {
        let mut lexer = Lexer::new("@");
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);

        let err = parser.parse().expect_err("parser should fail on lexer error token");
        assert!(err.contains("lexer error"));
    }

    #[test]
    fn parses_generic_fn_struct_trait_and_impl_headers() {
        let program = parse_program(
            r#"
struct Box[T] {
    value: T,
}

trait Iterable[T] {
    fn map[U](item: T) U;
}

impl Iterable[int32] for Box[int32] {
    fn map[U](item: int32) U {
        return item;
    }
}

fn id[T](x: T) T {
    return x;
}
"#,
        );

        let ItemKind::Struct {
            generic_params,
            fields,
            ..
        } = &program.items[0].node
        else {
            panic!("expected struct item");
        };
        assert_eq!(generic_params, &vec!["T".to_string()]);
        assert_eq!(fields.len(), 1);

        let ItemKind::Trait {
            generic_params,
            methods,
            ..
        } = &program.items[1].node
        else {
            panic!("expected trait item");
        };
        assert_eq!(generic_params, &vec!["T".to_string()]);
        assert_eq!(methods[0].generic_params, vec!["U".to_string()]);

        let ItemKind::Impl { trait_ty, for_ty, .. } = &program.items[2].node else {
            panic!("expected impl item");
        };

        match &trait_ty.node {
            TypeKind::Named { name, type_args } => {
                assert_eq!(name, "Iterable");
                assert_eq!(type_args.len(), 1);
            }
            other => panic!("expected named trait type, got {other:?}"),
        }

        match &for_ty.node {
            TypeKind::Named { name, type_args } => {
                assert_eq!(name, "Box");
                assert_eq!(type_args.len(), 1);
            }
            other => panic!("expected named for type, got {other:?}"),
        }

        let ItemKind::Fn { generic_params, .. } = &program.items[3].node else {
            panic!("expected fn item");
        };
        assert_eq!(generic_params, &vec!["T".to_string()]);
    }

    #[test]
    fn parses_generic_call_and_nested_type_args() {
        let program = parse_program(
            r#"
fn main() void {
    alloc[Vec[int32]]();
}
"#,
        );

        let ItemKind::Fn { body, .. } = &program.items[0].node else {
            panic!("expected function item");
        };

        let StmtKind::ExprStmt(expr) = &body.stmts[0].node else {
            panic!("expected expression statement");
        };

        let ExprKind::Call {
            callee,
            type_args,
            args,
        } = &expr.node
        else {
            panic!("expected call expression");
        };

        assert!(args.is_empty());
        assert_eq!(type_args.len(), 1);

        let ExprKind::Ident(name) = &callee.node else {
            panic!("expected identifier callee");
        };
        assert_eq!(name, "alloc");

        match &type_args[0].node {
            TypeKind::Named { name, type_args } => {
                assert_eq!(name, "Vec");
                assert_eq!(type_args.len(), 1);
                assert!(matches!(type_args[0].node, TypeKind::Int32));
            }
            other => panic!("expected nested named type arg, got {other:?}"),
        }
    }

    #[test]
    fn keeps_less_than_as_comparison_when_not_call_syntax() {
        let program = parse_program(
            r#"
fn main() void {
    a < b;
}
"#,
        );

        let ItemKind::Fn { body, .. } = &program.items[0].node else {
            panic!("expected function item");
        };

        let StmtKind::ExprStmt(expr) = &body.stmts[0].node else {
            panic!("expected expression statement");
        };

        let ExprKind::Binary { op, .. } = &expr.node else {
            panic!("expected binary expression");
        };
        assert!(matches!(op, BinOpKind::Lt));
    }

    #[test]
    fn parses_method_call_with_type_args() {
        let program = parse_program(
            r#"
fn main() void {
    value.transform[Vec[int32]]();
}
"#,
        );

        let ItemKind::Fn { body, .. } = &program.items[0].node else {
            panic!("expected function item");
        };

        let StmtKind::ExprStmt(expr) = &body.stmts[0].node else {
            panic!("expected expression statement");
        };

        let ExprKind::MethodCall {
            method,
            type_args,
            args,
            ..
        } = &expr.node
        else {
            panic!("expected method call expression");
        };

        assert_eq!(method, "transform");
        assert!(args.is_empty());
        assert_eq!(type_args.len(), 1);
    }

    #[test]
    fn parses_nested_generic_type_annotation() {
        let program = parse_program(
            r#"
fn main() void {
    var x: Vec[Map[str, int32]];
}
"#,
        );

        let ItemKind::Fn { body, .. } = &program.items[0].node else {
            panic!("expected function item");
        };

        let StmtKind::Var { ty: Some(ty), .. } = &body.stmts[0].node else {
            panic!("expected var statement with type annotation");
        };

        let TypeKind::Named {
            name,
            type_args: outer_args,
        } = &ty.node
        else {
            panic!("expected outer named type");
        };

        assert_eq!(name, "Vec");
        assert_eq!(outer_args.len(), 1);

        let TypeKind::Named {
            name,
            type_args: inner_args,
        } = &outer_args[0].node
        else {
            panic!("expected inner named type");
        };

        assert_eq!(name, "Map");
        assert_eq!(inner_args.len(), 2);
        assert!(matches!(inner_args[0].node, TypeKind::Str));
        assert!(matches!(inner_args[1].node, TypeKind::Int32));
    }

    #[test]
    fn parses_enum_and_match_expression() {
        let program = parse_program(
            r#"
enum Option[T] {
    Some(T),
    None,
}

fn unwrap_or_zero(x: Option[int32]) int32 {
    return match x {
        Some(v) => v,
        Option.None => 0,
    };
}
"#,
        );

        let ItemKind::Enum {
            name,
            generic_params,
            variants,
        } = &program.items[0].node
        else {
            panic!("expected enum item");
        };

        assert_eq!(name, "Option");
        assert_eq!(generic_params, &vec!["T".to_string()]);
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].name, "Some");
        assert_eq!(variants[0].payload_types.len(), 1);
        assert_eq!(variants[1].name, "None");
        assert!(variants[1].payload_types.is_empty());

        let ItemKind::Fn { body, .. } = &program.items[1].node else {
            panic!("expected function item");
        };

        let StmtKind::Return(Some(expr)) = &body.stmts[0].node else {
            panic!("expected return with expression");
        };

        let ExprKind::Match { arms, .. } = &expr.node else {
            panic!("expected match expression");
        };

        assert_eq!(arms.len(), 2);

        let PatternKind::Variant {
            enum_name,
            variant,
            bindings,
        } = &arms[0].pattern.node
        else {
            panic!("expected first arm variant pattern");
        };
        assert!(enum_name.is_none());
        assert_eq!(variant, "Some");
        assert_eq!(bindings, &vec!["v".to_string()]);

        let PatternKind::Variant {
            enum_name,
            variant,
            bindings,
        } = &arms[1].pattern.node
        else {
            panic!("expected second arm variant pattern");
        };
        assert_eq!(enum_name.as_deref(), Some("Option"));
        assert_eq!(variant, "None");
        assert!(bindings.is_empty());
    }

    #[test]
    fn parses_match_with_wildcard_pattern() {
        let program = parse_program(
            r#"
enum Color {
    Red,
    Blue,
}

fn value(c: Color) int32 {
    return match c {
        Red => 1,
        _ => 0,
    };
}
"#,
        );

        let ItemKind::Fn { body, .. } = &program.items[1].node else {
            panic!("expected function item");
        };

        let StmtKind::Return(Some(expr)) = &body.stmts[0].node else {
            panic!("expected return with expression");
        };

        let ExprKind::Match { arms, .. } = &expr.node else {
            panic!("expected match expression");
        };

        assert!(matches!(arms[1].pattern.node, PatternKind::Wildcard));
    }

    #[test]
    fn reports_readable_expected_token_in_parser_error() {
        let err = parse_program_err(
            r#"
fn main() void {
    const x int32;
}
"#,
        );

        assert!(err.contains("expected ="));
        assert!(err.contains("found int32"));
        assert!(!err.contains("Eq"));
        assert!(!err.contains("Int32"));
    }

    #[test]
    fn reports_e01_identifier_error_with_snippet_and_underline() {
        let err = parse_program_err(
            r#"
fn main() void {
    var ;
}
"#,
        );

        assert!(err.contains("error[E01]: expected identifier, found ';'"));
        assert!(err.contains("at 3:9"));
        assert!(err.contains("3 |     var ;"));
        assert!(err.contains("|         ^"));
    }
}
