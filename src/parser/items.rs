// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use crate::parser::Parser;
use crate::parser::Token;
use crate::parser::ast::*;

impl Parser {
    pub fn parse_struct(&mut self) -> Result<Item, String> {
        self.expect(Token::Struct)?;

        let name = self.parse_ident()?;

        self.expect(Token::LBrace)?;

        let mut fields = Vec::new();

        while self.peek() != &Token::RBrace {
            let field_name = self.parse_ident()?;
            self.expect(Token::Colon)?;
            let ty = self.parse_type()?;

            let is_const = if self.peek() == &Token::Const {
                self.advance();
                true
            } else {
                false
            };

            fields.push((field_name, ty, is_const));

            if self.peek() == &Token::Comma {
                self.advance();
            }
        }

        self.expect(Token::RBrace)?;

        Ok(Item::Struct { name, fields })
    }

    pub fn parse_trait(&mut self) -> Result<Item, String> {
        self.expect(Token::Trait)?;

        let name = self.parse_ident()?;

        self.expect(Token::LBrace)?;

        let mut methods = Vec::new();

        while self.peek() != &Token::RBrace {
            let method_name = self.parse_ident()?;

            self.expect(Token::LParen)?;

            let mut params = Vec::new();
            while self.peek() != &Token::RParen {
                let ty = self.parse_type()?;
                params.push(ty);

                if self.peek() == &Token::Comma {
                    self.advance();
                } else {
                    break;
                }
            }

            self.expect(Token::RParen)?;

            let return_ty = if self.peek() != &Token::Semicolon {
                self.parse_type()?
            } else {
                Type::Void
            };

            self.expect(Token::Semicolon)?;

            methods.push(TraitMethod {
                name: method_name,
                params,
                return_ty,
            });
        }

        self.expect(Token::RBrace)?;

        Ok(Item::Trait { name, methods })
    }

    pub fn parse_impl(&mut self) -> Result<Item, String> {
        self.expect(Token::Impl)?;

        let trait_name = self.parse_ident()?;

        self.expect(Token::For)?;

        let for_type = self.parse_ident()?;

        self.expect(Token::LBrace)?;

        let mut methods = Vec::new();

        while self.peek() != &Token::RBrace {
            methods.push(self.parse_fn()?);
        }

        self.expect(Token::RBrace)?;

        Ok(Item::Impl {
            trait_name,
            for_type,
            methods,
        })
    }

    pub fn parse_fn(&mut self) -> Result<Item, String> {
        self.expect(Token::Fn).unwrap();
        let name = self.parse_ident()?;
        self.expect(Token::LParen)?;

        let mut params = Vec::new();

        while self.peek() != &Token::RParen {
            let param_name = self.parse_ident()?;
            self.expect(Token::Colon)?;
            let param_type = self.parse_type()?;

            params.push((param_name, param_type));

            if self.peek() == &Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        self.expect(Token::RParen)?;

        let return_ty = if self.peek() != &Token::LBrace {
            self.parse_type()?
        } else {
            Type::Void
        };

        let body = self.parse_block()?;
        return Ok(Item::Fn {
            name,
            params,
            return_ty,
            body,
        });
    }

    pub fn parse_import(&mut self) -> Result<Item, String> {
        self.advance();
        let mut path = Vec::new();
        loop {
            if let Token::Ident(name) = self.advance() {
                path.push(name);
            }
            if self.peek() == &Token::Dot {
                self.advance();
            } else {
                break;
            }
        }
        let items = if path.is_empty() {
            return Err("expected at least one identifier in import".into());
        } else if self.peek() == &Token::LBrace {
            self.advance();
            let mut names = Vec::new();
            loop {
                if let Token::Ident(name) = self.advance() {
                    names.push(name);
                }
                if self.peek() == &Token::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(Token::RBrace)?;
            ImportItems::Multiple(names)
        } else if self.peek() == &Token::Star {
            self.advance();
            ImportItems::All
        } else if matches!(self.peek(), Token::Ident(_)) {
            let name = if let Token::Ident(n) = self.advance() {
                n
            } else {
                unreachable!()
            };
            if self.peek() == &Token::As {
                self.advance();
                if let Token::Ident(alias) = self.advance() {
                    ImportItems::Aliased(name, alias)
                } else {
                    return Err("expected identifier after 'as'".into());
                }
            } else {
                ImportItems::Single(name)
            }
        } else {
            let item = path.pop().unwrap();
            ImportItems::Single(item)
        };
        self.expect(Token::Semicolon)?;
        Ok(Item::Import(ImportPath { path, items }))
    }
}
