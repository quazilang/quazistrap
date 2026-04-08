// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::HashMap;

use crate::parser::ast::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Variable { mutable: bool },
    Parameter,
    TypeName,
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct SemanticError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for SemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} at {}:{} [{}..{}]",
            self.message, self.span.line, self.span.col, self.span.start, self.span.end
        )
    }
}

pub struct Analyzer {
    scopes: Vec<HashMap<String, Symbol>>,
    errors: Vec<SemanticError>,
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            errors: Vec::new(),
        }
    }

    pub fn analyze_program(&mut self, program: &Program) -> Vec<SemanticError> {
        for item in &program.items {
            match &item.node {
                ItemKind::Fn { name, .. } => self.declare(
                    name.clone(),
                    Symbol {
                        kind: SymbolKind::Function,
                        span: item.span,
                    },
                ),
                ItemKind::Struct { name, .. } | ItemKind::Trait { name, .. } => self.declare(
                    name.clone(),
                    Symbol {
                        kind: SymbolKind::TypeName,
                        span: item.span,
                    },
                ),
                ItemKind::Import(import_path) => match &import_path.items {
                    ImportItems::Single(name) => self.declare(
                        name.clone(),
                        Symbol {
                            kind: SymbolKind::Variable { mutable: false },
                            span: item.span,
                        },
                    ),
                    ImportItems::Aliased(_name, alias) => self.declare(
                        alias.clone(),
                        Symbol {
                            kind: SymbolKind::Variable { mutable: false },
                            span: item.span,
                        },
                    ),
                    ImportItems::Multiple(names) => {
                        for name in names {
                            self.declare(
                                name.clone(),
                                Symbol {
                                    kind: SymbolKind::Variable { mutable: false },
                                    span: item.span,
                                },
                            );
                        }
                    }
                    ImportItems::All => {}
                },
                ItemKind::Impl { .. } => {}
            }
        }

        for item in &program.items {
            self.analyze_item(item);
        }

        std::mem::take(&mut self.errors)
    }

    fn analyze_item(&mut self, item: &Item) {
        match &item.node {
            ItemKind::Fn {
                params,
                return_ty: _,
                body,
                ..
            } => {
                self.enter_scope();
                for (param_name, _param_ty) in params {
                    self.declare(
                        param_name.clone(),
                        Symbol {
                            kind: SymbolKind::Parameter,
                            span: item.span,
                        },
                    );
                }
                self.analyze_block(body);
                self.exit_scope();
            }
            ItemKind::Struct { .. } | ItemKind::Trait { .. } | ItemKind::Import(_) => {}
            ItemKind::Impl { methods, .. } => {
                for method in methods {
                    self.analyze_item(method);
                }
            }
        }
    }

    fn analyze_block(&mut self, block: &Block) {
        self.enter_scope();
        for stmt in &block.stmts {
            self.analyze_stmt(stmt);
        }
        self.exit_scope();
    }

    fn analyze_stmt(&mut self, stmt: &Stmt) {
        match &stmt.node {
            StmtKind::Var { name, value, .. } => {
                if let Some(value_expr) = value {
                    self.analyze_expr(value_expr);
                }

                self.declare(
                    name.clone(),
                    Symbol {
                        kind: SymbolKind::Variable { mutable: true },
                        span: stmt.span,
                    },
                );
            }
            StmtKind::Const { name, value, .. } => {
                self.analyze_expr(value);
                self.declare(
                    name.clone(),
                    Symbol {
                        kind: SymbolKind::Variable { mutable: false },
                        span: stmt.span,
                    },
                );
            }
            StmtKind::Return(expr) => {
                if let Some(expr) = expr {
                    self.analyze_expr(expr);
                }
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.analyze_expr(condition);
                self.analyze_block(then_block);
                if let Some(else_block) = else_block {
                    self.analyze_block(else_block);
                }
            }
            StmtKind::While { condition, body } => {
                self.analyze_expr(condition);
                self.analyze_block(body);
            }
            StmtKind::ExprStmt(expr) => self.analyze_expr(expr),
        }
    }

    fn analyze_expr(&mut self, expr: &Expr) {
        match &expr.node {
            ExprKind::Literal(_) => {}
            ExprKind::Ident(name) => {
                if self.resolve(name).is_none() {
                    self.push_error(expr.span, format!("unknown identifier '{}'", name));
                }
            }
            ExprKind::Group(inner) => self.analyze_expr(inner),
            ExprKind::Unary { expr, .. } => self.analyze_expr(expr),
            ExprKind::Binary { left, right, .. } => {
                self.analyze_expr(left);
                self.analyze_expr(right);
            }
            ExprKind::Assign { target, value } => {
                self.analyze_assign_target(target);
                self.analyze_expr(value);
            }
            ExprKind::Call { callee, args } => {
                self.analyze_expr(callee);
                for arg in args {
                    self.analyze_expr(arg);
                }
            }
            ExprKind::Field { object, .. } => self.analyze_expr(object),
            ExprKind::MethodCall { object, args, .. } => {
                self.analyze_expr(object);
                for arg in args {
                    self.analyze_expr(arg);
                }
            }
        }
    }

    fn analyze_assign_target(&mut self, target: &Expr) {
        match &target.node {
            ExprKind::Ident(name) => match self.resolve(name) {
                None => self.push_error(target.span, format!("unknown identifier '{}'", name)),
                Some(SymbolKind::Variable { mutable: false }) => {
                    self.push_error(target.span, format!("cannot assign to const '{}'", name));
                }
                Some(SymbolKind::Function) | Some(SymbolKind::TypeName) => {
                    self.push_error(target.span, format!("cannot assign to '{}'", name));
                }
                Some(SymbolKind::Parameter) | Some(SymbolKind::Variable { mutable: true }) => {}
            },
            ExprKind::Field { object, .. } => self.analyze_expr(object),
            _ => {
                self.analyze_expr(target);
                self.push_error(target.span, "invalid assignment target".to_string());
            }
        }
    }

    fn declare(&mut self, name: String, symbol: Symbol) {
        let existing = self
            .scopes
            .last()
            .expect("semantic analyzer must always have at least one scope")
            .get(&name)
            .cloned();

        if let Some(prev) = existing {
            self.push_error(
                symbol.span,
                format!(
                    "duplicate declaration '{}' (previous at {}:{} [{}..{}])",
                    name, prev.span.line, prev.span.col, prev.span.start, prev.span.end
                ),
            );
            return;
        }

        let current_scope = self
            .scopes
            .last_mut()
            .expect("semantic analyzer must always have at least one scope");
        current_scope.insert(name, symbol);
    }

    fn resolve(&self, name: &str) -> Option<SymbolKind> {
        for scope in self.scopes.iter().rev() {
            if let Some(symbol) = scope.get(name) {
                return Some(symbol.kind);
            }
        }
        None
    }

    fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn exit_scope(&mut self) {
        let _ = self.scopes.pop();
    }

    fn push_error(&mut self, span: Span, message: String) {
        self.errors.push(SemanticError { message, span });
    }
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    use super::Analyzer;

    fn parse_program(src: &str) -> crate::parser::ast::Program {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        parser.parse().expect("source must parse")
    }

    #[test]
    fn reports_unknown_identifier() {
        let program = parse_program(
            r#"
fn main() void {
    x = 1;
}
"#,
        );

        let mut analyzer = Analyzer::new();
        let errors = analyzer.analyze_program(&program);

        assert!(errors.iter().any(|e| e.message.contains("unknown identifier 'x'")));
    }

    #[test]
    fn reports_duplicate_local_declaration() {
        let program = parse_program(
            r#"
fn main() void {
    var x: int32 = 1;
    var x: int32 = 2;
}
"#,
        );

        let mut analyzer = Analyzer::new();
        let errors = analyzer.analyze_program(&program);

        assert!(errors.iter().any(|e| e.message.contains("duplicate declaration 'x'")));
    }
}
