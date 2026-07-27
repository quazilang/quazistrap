// quazi - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use crate::lexer::token::TokenKind;
use crate::parser::Parser;
use crate::parser::ast::*;
use crate::parser::common::{merge_token_spans, to_ast_span};

impl Parser {
    pub fn parse_fn(
        &mut self,
        attributes: Vec<Attribute>,
        unsafe_fn: bool,
        pub_fn: bool,
    ) -> Result<Item, String> {
        let start = self.expect(TokenKind::Fn)?.span;

        let name = self.parse_ident()?;
        let generic_params = self.parse_optional_generic_params()?;
        self.expect(TokenKind::LParen)?;

        let mut params: Vec<Param> = Vec::new();
        if !self.at(TokenKind::RParen) {
            loop {
                let param_attributes = if self.at(TokenKind::At) {
                    self.parse_attributes()?
                } else {
                    Vec::new()
                };
                let variadic = if self.at(TokenKind::DotDotDot) {
                    self.advance();
                    true
                } else {
                    false
                };
                let param_name = self.parse_ident()?;
                self.expect(TokenKind::Colon)?;
                let param_ty = self.parse_type()?;
                let is_last = self.at(TokenKind::RParen);
                if variadic && !is_last {
                    return Err(
                        self.err_here("variadic parameter must be the last parameter".to_string())
                    );
                }
                params.push(Param {
                    name: param_name,
                    ty: param_ty,
                    variadic,
                    attributes: param_attributes,
                });
                if self.at(TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
        }

        self.expect(TokenKind::RParen)?;

        let return_ty = if self.at(TokenKind::LBrace) || self.at(TokenKind::Semicolon) {
            Spanned::new(TypeKind::Void, to_ast_span(start))
        } else {
            self.parse_type()?
        };

        let (body, end_span) = if self.at(TokenKind::Semicolon) {
            let end = self.expect(TokenKind::Semicolon)?.span;
            (None, to_ast_span(end))
        } else {
            let block = self.parse_block()?;
            let s = block.span;
            (Some(block), s)
        };
        let span = Span::merge(to_ast_span(start), end_span);

        Ok(Spanned::new(
            ItemKind::Fn {
                name,
                generic_params,
                params,
                return_ty,
                body,
                attributes,
                unsafe_fn,
                pub_fn,
            },
            span,
        ))
    }

    pub fn parse_struct(&mut self, attributes: Vec<Attribute>) -> Result<Item, String> {
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
                attributes,
            },
            span,
        ))
    }

    pub fn parse_trait(&mut self, attributes: Vec<Attribute>) -> Result<Item, String> {
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
                attributes,
            },
            span,
        ))
    }

    pub fn parse_enum(&mut self, attributes: Vec<Attribute>) -> Result<Item, String> {
        let start = self.expect(TokenKind::Enum)?.span;
        let name = self.parse_ident()?;
        let generic_params = self.parse_optional_generic_params()?;

        self.expect(TokenKind::LBrace)?;
        let mut variants = Vec::new();

        while !self.at(TokenKind::RBrace) {
            if self.at(TokenKind::Eof) {
                return Err(self.err_here("unexpected EOF while parsing enum".to_string()));
            }

            let variant_name_tok = self.expect_ident_token()?;
            let variant_name = match &variant_name_tok.kind {
                TokenKind::Ident(s) => s.clone(),
                _ => unreachable!(),
            };

            let mut payload_types = Vec::new();
            let mut variant_end = variant_name_tok.span;

            if self.at(TokenKind::LParen) {
                self.advance();

                if !self.at(TokenKind::RParen) {
                    loop {
                        payload_types.push(self.parse_type()?);

                        if self.at(TokenKind::Comma) {
                            self.advance();
                            continue;
                        }
                        break;
                    }
                }

                variant_end = self.expect(TokenKind::RParen)?.span;
            }

            variants.push(EnumVariant {
                name: variant_name,
                payload_types,
                span: to_ast_span(merge_token_spans(variant_name_tok.span, variant_end)),
            });

            if self.at(TokenKind::Comma) || self.at(TokenKind::Semicolon) {
                self.advance();
            }
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        let span = to_ast_span(merge_token_spans(start, end));

        Ok(Spanned::new(
            ItemKind::Enum {
                name,
                generic_params,
                variants,
                attributes,
            },
            span,
        ))
    }

    pub fn parse_impl(&mut self) -> Result<Item, String> {
        let start = self.expect(TokenKind::Impl)?.span;

        let first_ty = self.parse_type()?;
        let (trait_ty, for_ty) = if self.at(TokenKind::For) {
            self.advance();
            let for_ty = self.parse_type()?;
            (Some(first_ty), for_ty)
        } else {
            (None, first_ty)
        };

        self.expect(TokenKind::LBrace)?;
        let mut methods = Vec::new();

        while !self.at(TokenKind::RBrace) {
            if self.at(TokenKind::Eof) {
                return Err(self.err_here("unexpected EOF while parsing impl".to_string()));
            }

            let attrs = self.parse_attributes()?;
            let mut is_pub = false;
            let mut is_unsafe = false;
            loop {
                if self.at(TokenKind::Pub) {
                    self.advance();
                    is_pub = true;
                } else if self.at(TokenKind::Unsafe) {
                    self.advance();
                    is_unsafe = true;
                } else {
                    break;
                }
            }
            let item = self.parse_fn(attrs, is_unsafe, is_pub)?;
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

    pub fn parse_type_alias(&mut self, attributes: Vec<Attribute>) -> Result<Item, String> {
        let start = self.expect(TokenKind::Type)?.span;
        let name = self.parse_ident()?;
        let generic_params = self.parse_optional_generic_params()?;
        self.expect(TokenKind::Eq)?;
        let aliased_type = self.parse_type()?;
        let end = self.expect(TokenKind::Semicolon)?.span;
        let span = to_ast_span(merge_token_spans(start, end));
        Ok(Spanned::new(
            ItemKind::TypeAlias {
                name,
                generic_params,
                aliased_type,
                attributes,
            },
            span,
        ))
    }

    pub fn parse_import(&mut self, pub_import: bool, is_reexport: bool) -> Result<Item, String> {
        let start = if is_reexport {
            self.peek().span
        } else {
            self.expect(TokenKind::Import)?.span
        };

        // Detect `./` prefix: Dot + Slash → relative import (local-only, skips module resolver).
        let relative = if self.at(TokenKind::Dot) && matches!(self.peek_n(1).kind, TokenKind::Slash)
        {
            self.advance(); // consume '.'
            self.advance(); // consume '/'
            true
        } else {
            false
        };

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
            if self.at(TokenKind::As) {
                self.advance();
                let alias = self.parse_ident()?;
                ImportItems::Aliased(last, alias)
            } else {
                ImportItems::Single(last)
            }
        };

        let end = self.expect(TokenKind::Semicolon)?.span;
        let span_tok = merge_token_spans(start, end);

        Ok(Spanned::new(
            ItemKind::Import(ImportPath {
                path,
                items,
                pub_import,
                is_reexport,
                relative,
                span: to_ast_span(span_tok),
            }),
            to_ast_span(span_tok),
        ))
    }
}
