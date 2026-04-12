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
    pub ty: Option<TypeKind>,
    pub span: Span,
    pub params: Vec<TypeKind>,
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

fn unwrap_type(ty: &Type) -> TypeKind {
    ty.node.clone()
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
                ItemKind::Fn {
                    name,
                    return_ty,
                    params,
                    ..
                } => self.declare(
                    name.clone(),
                    Symbol {
                        kind: SymbolKind::Function,
                        span: item.span,
                        ty: Some(unwrap_type(return_ty)),
                        params: params.iter().map(|(_, t)| unwrap_type(t)).collect(),
                    },
                ),
                ItemKind::Struct { name, .. } | ItemKind::Trait { name, .. } => self.declare(
                    name.clone(),
                    Symbol {
                        kind: SymbolKind::TypeName,
                        span: item.span,
                        ty: None,
                        params: vec![],
                    },
                ),
                ItemKind::Import(import_path) => match &import_path.items {
                    ImportItems::Single(name) => self.declare(
                        name.clone(),
                        Symbol {
                            kind: SymbolKind::Variable { mutable: false },
                            span: item.span,
                            ty: None,
                            params: vec![],
                        },
                    ),
                    ImportItems::Aliased(_name, alias) => self.declare(
                        alias.clone(),
                        Symbol {
                            kind: SymbolKind::Variable { mutable: false },
                            span: item.span,
                            ty: None,
                            params: vec![],
                        },
                    ),
                    ImportItems::Multiple(names) => {
                        for name in names {
                            self.declare(
                                name.clone(),
                                Symbol {
                                    kind: SymbolKind::Variable { mutable: false },
                                    span: item.span,
                                    ty: None,
                                    params: vec![],
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

    fn types_compatible(&self, a: &TypeKind, b: &TypeKind) -> bool {
        match (a, b) {
            (TypeKind::Any, _) | (_, TypeKind::Any) => true,
            (TypeKind::Named { .. }, _) | (_, TypeKind::Named { .. }) => true,
            _ => std::mem::discriminant(a) == std::mem::discriminant(b),
        }
    }

    fn analyze_item(&mut self, item: &Item) {
        match &item.node {
            ItemKind::Fn {
                params,
                return_ty,
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
                            ty: Some(unwrap_type(_param_ty)),
                            params: params.iter().map(|(_, t)| unwrap_type(t)).collect(),
                        },
                    );
                }
                let expected = unwrap_type(return_ty);
                self.analyze_block_with_return(body, &expected);
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

    fn analyze_block_with_return(&mut self, block: &Block, expected_return: &TypeKind) {
        self.enter_scope();
        for stmt in &block.stmts {
            if let StmtKind::Return(Some(expr)) = &stmt.node {
                let ty = self.analyze_expr(expr);
                if let Some(actual) = ty {
                    if !self.types_compatible(expected_return, &actual) {
                        self.push_error(
                            stmt.span,
                            format!(
                                "return type mismatch: expected {}, got {}",
                                expected_return, actual
                            ),
                        );
                    }
                }
            } else {
                self.analyze_stmt(stmt);
            }
        }
        self.exit_scope();
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
            StmtKind::Var {
                name, value, ty, ..
            } => {
                let value_ty = value.as_ref().and_then(|v| self.analyze_expr(v));
                let declared_ty = ty.as_ref().map(|t| t.node.clone());
                if let (Some(ann), Some(val)) = (&declared_ty, &value_ty) {
                    if !self.types_compatible(ann, val) {
                        self.push_error(
                            stmt.span,
                            format!("type mismatch: declared {}, got {}", ann, val),
                        );
                    }
                }

                self.declare(
                    name.clone(),
                    Symbol {
                        kind: SymbolKind::Variable { mutable: true },
                        span: stmt.span,
                        ty: declared_ty.or(value_ty),
                        params: vec![],
                    },
                );
            }
            StmtKind::Const {
                name, value, ty, ..
            } => {
                let value_ty = self.analyze_expr(value);
                let declared_ty = ty.as_ref().map(|t| t.node.clone());

                if let (Some(ann), Some(val)) = (&declared_ty, &value_ty) {
                    if !self.types_compatible(ann, val) {
                        self.push_error(
                            stmt.span,
                            format!("type mismatch: declared {}, got {}", ann, val),
                        );
                    }
                }

                self.declare(
                    name.clone(),
                    Symbol {
                        kind: SymbolKind::Variable { mutable: false },
                        span: stmt.span,
                        ty: declared_ty.or(value_ty),
                        params: vec![],
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
            StmtKind::ExprStmt(expr) => {
                self.analyze_expr(expr);
            }
        }
    }

    fn analyze_expr(&mut self, expr: &Expr) -> Option<TypeKind> {
        match &expr.node {
            ExprKind::Literal(lit) => Some(match lit {
                Literal::Int(_) => TypeKind::Int32,
                Literal::Float(_) => TypeKind::Float64,
                Literal::String(_) => TypeKind::Str,
                Literal::Bool(_) => TypeKind::Bool,
            }),
            ExprKind::Ident(name) => match self.resolve(name) {
                None => {
                    self.push_error(expr.span, format!("unknown identifier '{}'", name));
                    return None;
                }
                Some(sym) => sym.ty.clone(),
            },
            ExprKind::Group(inner) => self.analyze_expr(inner),
            ExprKind::Unary { expr, op, .. } => {
                let ty = self.analyze_expr(expr);
                match (op, &ty) {
                    (UnaryOpKind::Not, Some(t)) if !matches!(t, TypeKind::Bool) => {
                        self.push_error(expr.span, format!("! requires bool, got {}", t));
                        None
                    }
                    (UnaryOpKind::Neg, Some(t)) if matches!(t, TypeKind::Str | TypeKind::Bool) => {
                        self.push_error(expr.span, format!("unary - not valid for {}", t));
                        None
                    }
                    _ => ty,
                }
            }
            ExprKind::Binary { left, right, .. } => {
                let lt = self.analyze_expr(left);
                let rt = self.analyze_expr(right);
                match (&lt, &rt) {
                    (Some(l), Some(r)) if !self.types_compatible(l, r) => {
                        self.push_error(
                            expr.span,
                            format!("type mismatch in binary op: {} vs {}", l, r),
                        );
                        None
                    }
                    (Some(_), _) => lt,
                    _ => rt,
                }
            }
            ExprKind::Assign { target, value } => {
                self.analyze_assign_target(target);
                let value_ty = self.analyze_expr(value);
                if let ExprKind::Ident(name) = &target.node {
                    if let Some(sym) = self.resolve(name) {
                        if let (Some(var_ty), Some(val_ty)) = (sym.ty, value_ty) {
                            if !self.types_compatible(&var_ty, &val_ty) {
                                self.push_error(
                                    target.span,
                                    format!(
                                        "type mismatch in assignment: expected {}, got {}",
                                        var_ty, val_ty
                                    ),
                                );
                            }
                        }
                    }
                }
                None
            }
            ExprKind::Call {
                callee,
                type_args: _,
                args,
            } => {
                let arg_tys: Vec<Option<TypeKind>> =
                    args.iter().map(|a| self.analyze_expr(a)).collect();
                if let ExprKind::Ident(name) = &callee.node {
                    if let Some(sym) = self.resolve(name) {
                        if sym.params.len() != args.len() {
                            self.push_error(
                                callee.span,
                                format!("expected {} args, got {}", sym.params.len(), args.len()),
                            );
                        } else {
                            for (i, (param_ty, arg_ty)) in
                                sym.params.iter().zip(arg_tys.iter()).enumerate()
                            {
                                if let Some(at) = arg_ty {
                                    if !self.types_compatible(param_ty, at) {
                                        self.push_error(
                                            args[i].span,
                                            format!(
                                                "arg {}: expected {}, got {}",
                                                i + 1,
                                                param_ty,
                                                at
                                            ),
                                        );
                                    }
                                }
                            }
                        }
                        return sym.ty.clone();
                    }
                }
                None
            }
            ExprKind::MethodCall { object, args, .. } => {
                self.analyze_expr(object);
                for arg in args {
                    self.analyze_expr(arg);
                }
                None
            }
            ExprKind::Field { object, .. } => {
                self.analyze_expr(object);
                None
                // TODO: resolve field type from struct definition
            }
        }
    }

    fn analyze_assign_target(&mut self, target: &Expr) {
        match &target.node {
            ExprKind::Ident(name) => match self.resolve_kind(name) {
                None => self.push_error(target.span, format!("unknown identifier '{}'", name)),
                Some(SymbolKind::Variable { mutable: false }) => {
                    self.push_error(target.span, format!("cannot assign to const '{}'", name));
                }
                Some(SymbolKind::Function) | Some(SymbolKind::TypeName) => {
                    self.push_error(target.span, format!("cannot assign to '{}'", name));
                }
                Some(SymbolKind::Parameter) | Some(SymbolKind::Variable { mutable: true }) => {}
            },
            ExprKind::Field { object, .. } => {
                self.analyze_expr(object);
            }
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

    fn resolve(&self, name: &str) -> Option<Symbol> {
        for scope in self.scopes.iter().rev() {
            if let Some(symbol) = scope.get(name) {
                return Some(symbol.to_owned());
            }
        }
        None
    }

    fn resolve_kind(&self, name: &str) -> Option<SymbolKind> {
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
    fn reports_type_mismatch_in_const() {
        let program = parse_program(
            r#"
fn main() void {
    const x: int32 = "";
}
"#,
        );
        let mut analyzer = Analyzer::new();
        let errors = analyzer.analyze_program(&program);
        assert!(errors.iter().any(|e| e.message.contains("type mismatch")));
    }

    #[test]
    fn reports_type_mismatch_in_var() {
        let program = parse_program(
            r#"
fn main() void {
    var x: bool = 123;
}
    "#,
        );
        let mut analyzer = Analyzer::new();
        let errors = analyzer.analyze_program(&program);
        assert!(errors.iter().any(|e| e.message.contains("type mismatch")));
    }

    #[test]
    fn reports_readable_type_names_in_errors() {
        let program = parse_program(
            r#"
fn main() void {
    const x: int32 = "";
}
"#,
        );

        let mut analyzer = Analyzer::new();
        let errors = analyzer.analyze_program(&program);
        let combined = errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(combined.contains("declared int32"));
        assert!(combined.contains("got str"));
        assert!(!combined.contains("Int32"));
        assert!(!combined.contains("Str"));
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

        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("unknown identifier 'x'"))
        );
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

        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("duplicate declaration 'x'"))
        );
    }
}
