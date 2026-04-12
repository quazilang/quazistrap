// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use crate::lexer::token::TokenKind;
use crate::parser::Parser;
use crate::parser::ast::*;
use crate::parser::common::{merge_token_spans, to_ast_span};

impl Parser {
    pub fn parse_fn(&mut self) -> Result<Item, String> {
        let start = self.expect(TokenKind::Fn)?.span;

        let name = self.parse_ident()?;
        let generic_params = self.parse_optional_generic_params()?;
        self.expect(TokenKind::LParen)?;

        let mut params: Vec<(String, Type)> = Vec::new();
        if !self.at(TokenKind::RParen) {
            loop {
                let param_name = self.parse_ident()?;
                self.expect(TokenKind::Colon)?;
                let param_ty = self.parse_type()?;
                params.push((param_name, param_ty));

                if self.at(TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
        }

        self.expect(TokenKind::RParen)?;

        let return_ty = if self.at(TokenKind::LBrace) {
            Spanned::new(TypeKind::Void, to_ast_span(start))
        } else {
            self.parse_type()?
        };

        let body = self.parse_block()?;
        let span = Span::merge(to_ast_span(start), body.span);

        Ok(Spanned::new(
            ItemKind::Fn {
                name,
                generic_params,
                params,
                return_ty,
                body,
            },
            span,
        ))
    }

    pub fn parse_struct(&mut self) -> Result<Item, String> {
        let start = self.expect(TokenKind::Struct)?.span;
        let name = self.parse_ident()?;
        let generic_params = self.parse_optional_generic_params()?;

        self.expect(TokenKind::LBrace)?;
        let mut fields: Vec<(String, Type, bool)> = Vec::new();

        while !self.at(TokenKind::RBrace) {
            if self.at(TokenKind::Eof) {
                return Err(self.err_here("unexpected EOF while parsing struct".to_string()));
            }

            let is_const = if self.at(TokenKind::Const) {
                self.advance();
                true
            } else {
                false
            };

            let field_name = self.parse_ident()?;
            self.expect(TokenKind::Colon)?;
            let field_ty = self.parse_type()?;

            fields.push((field_name, field_ty, is_const));

            if self.at(TokenKind::Comma) || self.at(TokenKind::Semicolon) {
                self.advance();
            }
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        let span = to_ast_span(merge_token_spans(start, end));

        Ok(Spanned::new(
            ItemKind::Struct {
                name,
                generic_params,
                fields,
            },
            span,
        ))
    }

    pub fn parse_trait(&mut self) -> Result<Item, String> {
        let start = self.expect(TokenKind::Trait)?.span;
        let name = self.parse_ident()?;
        let generic_params = self.parse_optional_generic_params()?;

        self.expect(TokenKind::LBrace)?;
        let mut methods = Vec::new();

        while !self.at(TokenKind::RBrace) {
            if self.at(TokenKind::Eof) {
                return Err(self.err_here("unexpected EOF while parsing trait".to_string()));
            }

            let method_start = self.current_span();
            self.expect(TokenKind::Fn)?;
            let method_name = self.parse_ident()?;
            let method_generic_params = self.parse_optional_generic_params()?;

            self.expect(TokenKind::LParen)?;
            let mut params = Vec::new();
            if !self.at(TokenKind::RParen) {
                loop {
                    let _param_name = self.parse_ident()?;
                    self.expect(TokenKind::Colon)?;
                    params.push(self.parse_type()?);

                    if self.at(TokenKind::Comma) {
                        self.advance();
                        continue;
                    }
                    break;
                }
            }
            self.expect(TokenKind::RParen)?;

            let return_ty = if self.at(TokenKind::Semicolon) {
                Spanned::new(TypeKind::Void, to_ast_span(method_start))
            } else {
                self.parse_type()?
            };

            let end = self.expect(TokenKind::Semicolon)?.span;
            let span = to_ast_span(merge_token_spans(method_start, end));

            methods.push(TraitMethod {
                name: method_name,
                generic_params: method_generic_params,
                params,
                return_ty,
                span,
            });
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        let span = to_ast_span(merge_token_spans(start, end));

        Ok(Spanned::new(
            ItemKind::Trait {
                name,
                generic_params,
                methods,
            },
            span,
        ))
    }

    pub fn parse_impl(&mut self) -> Result<Item, String> {
        let start = self.expect(TokenKind::Impl)?.span;

        let trait_ty = self.parse_type()?;
        self.expect(TokenKind::For)?;
        let for_ty = self.parse_type()?;

        self.expect(TokenKind::LBrace)?;
        let mut methods = Vec::new();

        while !self.at(TokenKind::RBrace) {
            if self.at(TokenKind::Eof) {
                return Err(self.err_here("unexpected EOF while parsing impl".to_string()));
            }

            let item = self.parse_fn()?;
            methods.push(item);
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        let span = to_ast_span(merge_token_spans(start, end));

        Ok(Spanned::new(
            ItemKind::Impl {
                trait_ty,
                for_ty,
                methods,
            },
            span,
        ))
    }

    pub fn parse_import(&mut self) -> Result<Item, String> {
        let start = self.expect(TokenKind::Import)?.span;

        let mut path: Vec<String> = Vec::new();

        // base path: a.b.c
        let first = self.parse_ident()?;
        path.push(first);

        while self.at(TokenKind::Dot) {
            let save = self.pos;
            self.advance(); // consume '.'

            // stop if this dot starts a trailing selector form:
            // import a.b.{x,y}; / import a.b.*;
            if self.at(TokenKind::LBrace) || self.at(TokenKind::Star) {
                self.pos = save;
                break;
            }

            match self.peek_kind() {
                TokenKind::Ident(_) => {
                    path.push(self.parse_ident()?);
                }
                _ => {
                    self.pos = save;
                    break;
                }
            }
        }

        let items = if self.at(TokenKind::Dot) {
            self.advance();

            if self.at(TokenKind::LBrace) {
                self.advance();
                let mut names = Vec::new();

                if !self.at(TokenKind::RBrace) {
                    loop {
                        names.push(self.parse_ident()?);

                        if self.at(TokenKind::Comma) {
                            self.advance();
                            continue;
                        }
                        break;
                    }
                }

                self.expect(TokenKind::RBrace)?;
                ImportItems::Multiple(names)
            } else if self.at(TokenKind::Star) {
                self.advance();
                ImportItems::All
            } else {
                let name = self.parse_ident()?;
                if self.at(TokenKind::As) {
                    self.advance();
                    let alias = self.parse_ident()?;
                    ImportItems::Aliased(name, alias)
                } else {
                    ImportItems::Single(name)
                }
            }
        } else {
            let last = path
                .pop()
                .ok_or_else(|| self.err_here("invalid import path".to_string()))?;
            ImportItems::Single(last)
        };

        let end = self.expect(TokenKind::Semicolon)?.span;
        let span_tok = merge_token_spans(start, end);

        Ok(Spanned::new(
            ItemKind::Import(ImportPath {
                path,
                items,
                span: to_ast_span(span_tok),
            }),
            to_ast_span(span_tok),
        ))
    }
}

