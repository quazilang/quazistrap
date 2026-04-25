// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::{BTreeSet, HashMap};

use crate::parser::ast::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Variable { mutable: bool },
    Parameter,
    TypeName,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
}

impl std::fmt::Display for ConstValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConstValue::Int(v) => write!(f, "{}", v),
            ConstValue::Float(v) => write!(f, "{}", v),
            ConstValue::String(v) => write!(f, "\"{}\"", v),
            ConstValue::Bool(v) => write!(f, "{}", v),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub ty: Option<TypeKind>,
    pub span: Span,
    pub params: Vec<TypeKind>,
    pub used: bool,
    pub initialized: bool,
    pub is_import: bool,
    pub import_path: Option<String>,
    pub const_value: Option<ConstValue>,
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

#[derive(Debug, Clone)]
pub struct SemanticWarning {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for SemanticWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} at {}:{} [{}..{}]",
            self.message, self.span.line, self.span.col, self.span.start, self.span.end
        )
    }
}

#[derive(Debug, Clone)]
pub struct SemanticSuggestion {
    pub message: String,
    pub span: Option<Span>,
}

#[derive(Debug, Clone)]
pub struct ExprAnnotation {
    pub span: Span,
    pub ty: Option<TypeKind>,
    pub const_value: Option<ConstValue>,
    pub reachable: bool,
}

#[derive(Debug, Clone)]
pub struct ConstantEvaluation {
    pub span: Span,
    pub value: ConstValue,
}

#[derive(Debug, Clone)]
pub struct InlineCandidate {
    pub name: String,
    pub span: Span,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct MatchExhaustivenessIssue {
    pub span: Span,
    pub enum_name: String,
    pub missing_variants: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SemanticReport {
    pub errors: Vec<SemanticError>,
    pub warnings: Vec<SemanticWarning>,
    pub suggestions: Vec<SemanticSuggestion>,
    pub used_imports: Vec<String>,
    pub unused_imports: Vec<String>,
    pub annotated_exprs: Vec<ExprAnnotation>,
    pub constant_evaluations: Vec<ConstantEvaluation>,
    pub inline_candidates: Vec<InlineCandidate>,
    pub exhaustiveness_checks: usize,
    pub non_exhaustive_matches: Vec<MatchExhaustivenessIssue>,
}

#[derive(Debug, Clone, Default)]
struct ExprEval {
    ty: Option<TypeKind>,
    const_value: Option<ConstValue>,
}

#[derive(Debug, Clone)]
struct EnumInfo {
    variants: HashMap<String, usize>,
}

#[derive(Debug, Clone)]
enum MatchArmKindInfo {
    Wildcard,
    Variant {
        enum_name: Option<String>,
        variant: String,
    },
}

#[derive(Debug, Clone)]
struct MatchArmInfo {
    span: Span,
    kind: MatchArmKindInfo,
}

#[derive(Debug, Clone)]
struct MatchCandidate {
    span: Span,
    scrutinee_ty: Option<TypeKind>,
    arms: Vec<MatchArmInfo>,
}

pub struct Analyzer {
    scopes: Vec<HashMap<String, Symbol>>,
    finished_scopes: Vec<Vec<(String, Symbol)>>,
    errors: Vec<SemanticError>,
    warnings: Vec<SemanticWarning>,
    suggestions: Vec<SemanticSuggestion>,
    used_import_paths: BTreeSet<String>,
    unused_import_paths: BTreeSet<String>,
    annotated_exprs: Vec<ExprAnnotation>,
    constant_evaluations: Vec<ConstantEvaluation>,
    inline_candidates: Vec<InlineCandidate>,
    enums: HashMap<String, EnumInfo>,
    match_candidates: Vec<MatchCandidate>,
    non_exhaustive_matches: Vec<MatchExhaustivenessIssue>,
    exhaustiveness_checks: usize,
}

fn unwrap_type(ty: &Type) -> TypeKind {
    ty.node.clone()
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            finished_scopes: Vec::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            suggestions: Vec::new(),
            used_import_paths: BTreeSet::new(),
            unused_import_paths: BTreeSet::new(),
            annotated_exprs: Vec::new(),
            constant_evaluations: Vec::new(),
            inline_candidates: Vec::new(),
            enums: HashMap::new(),
            match_candidates: Vec::new(),
            non_exhaustive_matches: Vec::new(),
            exhaustiveness_checks: 0,
        }
    }

    pub fn analyze_program(&mut self, program: &Program) -> SemanticReport {
        self.reset_state();

        // Pass 1: gather top-level declarations and imports.
        for item in &program.items {
            self.declare_top_level_item(item);
        }

        // Pass 2: type checking + usage tracking + initialization checks + annotations.
        for item in &program.items {
            self.type_check_item(item);
        }

        // Pass 3: unused symbol/import analysis.
        self.run_unused_pass();

        // Pass 4: dead code detection (reachability).
        self.run_dead_code_pass(program);

        // Pass 5: optimization hints.
        self.run_inline_candidate_pass(program);
        self.run_exhaustiveness_pass();
        self.run_import_optimization_pass();

        SemanticReport {
            errors: std::mem::take(&mut self.errors),
            warnings: std::mem::take(&mut self.warnings),
            suggestions: std::mem::take(&mut self.suggestions),
            used_imports: std::mem::take(&mut self.used_import_paths)
                .into_iter()
                .collect(),
            unused_imports: std::mem::take(&mut self.unused_import_paths)
                .into_iter()
                .collect(),
            annotated_exprs: std::mem::take(&mut self.annotated_exprs),
            constant_evaluations: std::mem::take(&mut self.constant_evaluations),
            inline_candidates: std::mem::take(&mut self.inline_candidates),
            exhaustiveness_checks: self.exhaustiveness_checks,
            non_exhaustive_matches: std::mem::take(&mut self.non_exhaustive_matches),
        }
    }

    fn reset_state(&mut self) {
        self.scopes.clear();
        self.scopes.push(HashMap::new());
        self.finished_scopes.clear();

        self.errors.clear();
        self.warnings.clear();
        self.suggestions.clear();
        self.used_import_paths.clear();
        self.unused_import_paths.clear();
        self.annotated_exprs.clear();
        self.constant_evaluations.clear();
        self.inline_candidates.clear();
        self.enums.clear();
        self.match_candidates.clear();
        self.non_exhaustive_matches.clear();
        self.exhaustiveness_checks = 0;
    }

    fn declare_top_level_item(&mut self, item: &Item) {
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
                    used: false,
                    initialized: true,
                    is_import: false,
                    import_path: None,
                    const_value: None,
                },
            ),
            ItemKind::Struct { name, .. } | ItemKind::Trait { name, .. } => self.declare(
                name.clone(),
                Symbol {
                    kind: SymbolKind::TypeName,
                    span: item.span,
                    ty: None,
                    params: vec![],
                    used: false,
                    initialized: true,
                    is_import: false,
                    import_path: None,
                    const_value: None,
                },
            ),
            ItemKind::Enum {
                name,
                variants,
                ..
            } => {
                self.declare(
                    name.clone(),
                    Symbol {
                        kind: SymbolKind::TypeName,
                        span: item.span,
                        ty: None,
                        params: vec![],
                        used: false,
                        initialized: true,
                        is_import: false,
                        import_path: None,
                        const_value: None,
                    },
                );
                self.register_enum(name, variants, item.span);
            }
            ItemKind::Import(import_path) => self.declare_import_item(import_path, item.span),
            ItemKind::Impl { .. } => {}
        }
    }

    fn register_enum(&mut self, enum_name: &str, variants: &[EnumVariant], span: Span) {
        let mut map = HashMap::new();

        for variant in variants {
            let arity = variant.payload_types.len();
            if map.insert(variant.name.clone(), arity).is_some() {
                self.push_error(
                    variant.span,
                    format!(
                        "duplicate enum variant '{}' in enum '{}'",
                        variant.name, enum_name
                    ),
                );
            }
        }

        if variants.is_empty() {
            self.push_warning(
                span,
                format!("enum '{}' has no variants", enum_name),
            );
        }

        self.enums.insert(
            enum_name.to_string(),
            EnumInfo { variants: map },
        );
    }

    fn declare_import_item(&mut self, import_path: &ImportPath, span: Span) {
        match &import_path.items {
            ImportItems::Single(name) => {
                let full = Self::build_import_path(&import_path.path, name);
                self.declare_import_binding(name.clone(), full, span);
            }
            ImportItems::Aliased(name, alias) => {
                let full = Self::build_import_path(&import_path.path, name);
                self.declare_import_binding(alias.clone(), full, span);
            }
            ImportItems::Multiple(names) => {
                for name in names {
                    let full = Self::build_import_path(&import_path.path, name);
                    self.declare_import_binding(name.clone(), full, span);
                }
            }
            ImportItems::All => {}
        }
    }

    fn declare_import_binding(&mut self, local_name: String, full_path: String, span: Span) {
        self.declare(
            local_name,
            Symbol {
                kind: SymbolKind::Variable { mutable: false },
                ty: None,
                span,
                params: vec![],
                used: false,
                initialized: true,
                is_import: true,
                import_path: Some(full_path),
                const_value: None,
            },
        );
    }

    fn build_import_path(prefix: &[String], leaf: &str) -> String {
        if prefix.is_empty() {
            leaf.to_string()
        } else {
            format!("{}.{}", prefix.join("."), leaf)
        }
    }

    fn type_check_item(&mut self, item: &Item) {
        match &item.node {
            ItemKind::Fn {
                params,
                return_ty,
                body,
                ..
            } => {
                self.enter_scope();
                for (param_name, param_ty) in params {
                    self.declare(
                        param_name.clone(),
                        Symbol {
                            kind: SymbolKind::Parameter,
                            span: item.span,
                            ty: Some(unwrap_type(param_ty)),
                            params: vec![],
                            used: false,
                            initialized: true,
                            is_import: false,
                            import_path: None,
                            const_value: None,
                        },
                    );
                }

                let expected = unwrap_type(return_ty);
                let _ = self.type_check_block(body, Some(&expected));
                self.exit_scope_collect();
            }
            ItemKind::Struct { .. }
            | ItemKind::Trait { .. }
            | ItemKind::Enum { .. }
            | ItemKind::Import(_) => {}
            ItemKind::Impl { methods, .. } => {
                for method in methods {
                    self.type_check_item(method);
                }
            }
        }
    }

    fn type_check_block(&mut self, block: &Block, expected_return: Option<&TypeKind>) -> bool {
        self.enter_scope();
        let mut reachable = true;
        let mut guaranteed_return = false;

        for stmt in &block.stmts {
            if !reachable {
                // Keep type pass clean: dead-code diagnostics are emitted in dedicated pass.
                continue;
            }

            if self.type_check_stmt(stmt, expected_return) {
                reachable = false;
                guaranteed_return = true;
            }
        }

        self.exit_scope_collect();
        guaranteed_return
    }

    fn type_check_stmt(&mut self, stmt: &Stmt, expected_return: Option<&TypeKind>) -> bool {
        match &stmt.node {
            StmtKind::Var {
                name, value, ty, ..
            } => {
                let value_eval = value
                    .as_ref()
                    .map(|v| self.type_check_expr(v, true))
                    .unwrap_or_default();

                let declared_ty = ty.as_ref().map(|t| t.node.clone());
                if let (Some(ann), Some(val)) = (&declared_ty, &value_eval.ty) {
                    if !self.types_compatible(ann, val) {
                        self.push_error(
                            stmt.span,
                            format!("type mismatch: declared {}, got {}", ann, val),
                        );
                    }
                }

                if let Some(const_val) = &value_eval.const_value {
                    self.push_suggestion(
                        Some(stmt.span),
                        format!(
                            "initializer of '{}' is constant ({}) and can be folded",
                            name, const_val
                        ),
                    );
                }

                self.declare(
                    name.clone(),
                    Symbol {
                        kind: SymbolKind::Variable { mutable: true },
                        span: stmt.span,
                        ty: declared_ty.or(value_eval.ty),
                        params: vec![],
                        used: false,
                        initialized: value.is_some(),
                        is_import: false,
                        import_path: None,
                        const_value: None,
                    },
                );
                false
            }
            StmtKind::Const {
                name, value, ty, ..
            } => {
                let value_eval = self.type_check_expr(value, true);
                let declared_ty = ty.as_ref().map(|t| t.node.clone());

                if let (Some(ann), Some(val)) = (&declared_ty, &value_eval.ty) {
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
                        ty: declared_ty.or(value_eval.ty.clone()),
                        params: vec![],
                        used: false,
                        initialized: true,
                        is_import: false,
                        import_path: None,
                        const_value: value_eval.const_value,
                    },
                );
                false
            }
            StmtKind::Return(expr) => {
                match (expected_return, expr) {
                    (Some(expected), Some(return_expr)) => {
                        let actual = self.type_check_expr(return_expr, true).ty;
                        if let Some(actual) = actual {
                            if !self.types_compatible(expected, &actual) {
                                self.push_error(
                                    stmt.span,
                                    format!(
                                        "return type mismatch: expected {}, got {}",
                                        expected, actual
                                    ),
                                );
                            }
                        }
                    }
                    (Some(expected), None) => {
                        if !matches!(expected, TypeKind::Void) {
                            self.push_error(
                                stmt.span,
                                format!("return type mismatch: expected {}, got void", expected),
                            );
                        }
                    }
                    (None, Some(return_expr)) => {
                        self.type_check_expr(return_expr, true);
                    }
                    (None, None) => {}
                }
                true
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                let condition_eval = self.type_check_expr(condition, true);
                if let Some(condition_ty) = condition_eval.ty {
                    if !matches!(condition_ty, TypeKind::Bool | TypeKind::Any) {
                        self.push_error(
                            condition.span,
                            format!("if condition must be bool, got {}", condition_ty),
                        );
                    }
                }

                let then_returns = self.type_check_block(then_block, expected_return);
                let else_returns = if let Some(else_block) = else_block {
                    self.type_check_block(else_block, expected_return)
                } else {
                    false
                };

                then_returns && else_returns
            }
            StmtKind::While { condition, body } => {
                let condition_eval = self.type_check_expr(condition, true);
                if let Some(condition_ty) = condition_eval.ty {
                    if !matches!(condition_ty, TypeKind::Bool | TypeKind::Any) {
                        self.push_error(
                            condition.span,
                            format!("while condition must be bool, got {}", condition_ty),
                        );
                    }
                }

                let _ = self.type_check_block(body, expected_return);
                false
            }
            StmtKind::ExprStmt(expr) => {
                self.type_check_expr(expr, true);
                false
            }
        }
    }

    fn type_check_expr(&mut self, expr: &Expr, reachable: bool) -> ExprEval {
        let result = match &expr.node {
            ExprKind::Literal(lit) => {
                let ty = match lit {
                    Literal::Int(_) => TypeKind::Int32,
                    Literal::Float(_) => TypeKind::Float64,
                    Literal::String(_) => TypeKind::Str,
                    Literal::Bool(_) => TypeKind::Bool,
                };

                ExprEval {
                    ty: Some(ty),
                    const_value: Self::const_from_literal(lit),
                }
            }
            ExprKind::Ident(name) => match self.resolve_for_read(name) {
                None => {
                    self.push_error(expr.span, format!("unknown identifier '{}'", name));
                    ExprEval::default()
                }
                Some(sym) => {
                    if matches!(sym.kind, SymbolKind::Variable { .. } | SymbolKind::Parameter)
                        && !sym.initialized
                    {
                        self.push_error(
                            expr.span,
                            format!("use of '{}' before initialization", name),
                        );
                        ExprEval {
                            ty: sym.ty,
                            const_value: None,
                        }
                    } else {
                        ExprEval {
                            ty: sym.ty,
                            const_value: sym.const_value,
                        }
                    }
                }
            },
            ExprKind::Group(inner) => self.type_check_expr(inner, reachable),
            ExprKind::Unary { expr: inner, op } => {
                let inner_eval = self.type_check_expr(inner, reachable);

                match (op, &inner_eval.ty) {
                    (UnaryOpKind::Not, Some(t)) if !matches!(t, TypeKind::Bool | TypeKind::Any) => {
                        self.push_error(inner.span, format!("! requires bool, got {}", t));
                    }
                    (UnaryOpKind::Neg, Some(t)) if matches!(t, TypeKind::Str | TypeKind::Bool) => {
                        self.push_error(inner.span, format!("unary - not valid for {}", t));
                    }
                    _ => {}
                }

                ExprEval {
                    ty: inner_eval.ty,
                    const_value: inner_eval
                        .const_value
                        .and_then(|value| Self::const_from_unary(op, value)),
                }
            }
            ExprKind::Binary { left, op, right } => {
                let left_eval = self.type_check_expr(left, reachable);
                let right_eval = self.type_check_expr(right, reachable);

                let ty = self.infer_binary_type(expr.span, op, &left_eval.ty, &right_eval.ty);
                let const_value = match (&left_eval.const_value, &right_eval.const_value) {
                    (Some(lhs), Some(rhs)) => Self::const_from_binary(op, lhs, rhs),
                    _ => None,
                };

                ExprEval { ty, const_value }
            }
            ExprKind::Assign { target, value } => {
                self.analyze_assign_target(target);
                let value_eval = self.type_check_expr(value, reachable);

                if let ExprKind::Ident(name) = &target.node {
                    if let Some(sym) = self.resolve_symbol(name) {
                        if let (Some(var_ty), Some(val_ty)) = (&sym.ty, &value_eval.ty) {
                            if !self.types_compatible(var_ty, val_ty) {
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

                    self.mark_initialized(name);
                    self.set_symbol_const_value(name, value_eval.const_value.clone());
                }

                value_eval
            }
            ExprKind::Call {
                callee,
                type_args: _,
                args,
            } => {
                let arg_tys: Vec<ExprEval> = args
                    .iter()
                    .map(|a| self.type_check_expr(a, reachable))
                    .collect();

                if let ExprKind::Ident(name) = &callee.node {
                    let Some(sym) = self.resolve_for_read(name) else {
                        self.push_error(callee.span, format!("unknown identifier '{}'", name));
                        return ExprEval::default();
                    };

                    if sym.params.len() != args.len() {
                        self.push_error(
                            callee.span,
                            format!("expected {} args, got {}", sym.params.len(), args.len()),
                        );
                    } else {
                        for (i, (param_ty, arg_ty)) in
                            sym.params.iter().zip(arg_tys.iter().map(|e| &e.ty)).enumerate()
                        {
                            if let Some(at) = arg_ty {
                                if !self.types_compatible(param_ty, at) {
                                    self.push_error(
                                        args[i].span,
                                        format!("arg {}: expected {}, got {}", i + 1, param_ty, at),
                                    );
                                }
                            }
                        }
                    }

                    ExprEval {
                        ty: sym.ty,
                        const_value: None,
                    }
                } else {
                    self.type_check_expr(callee, reachable);
                    ExprEval::default()
                }
            }
            ExprKind::MethodCall { object, args, .. } => {
                self.type_check_expr(object, reachable);
                for arg in args {
                    self.type_check_expr(arg, reachable);
                }
                ExprEval::default()
            }
            ExprKind::Field { object, .. } => {
                self.type_check_expr(object, reachable);
                ExprEval::default()
            }
            ExprKind::Match { scrutinee, arms } => {
                let scrutinee_eval = self.type_check_expr(scrutinee, reachable);
                let mut arm_infos = Vec::new();
                let mut result_ty: Option<TypeKind> = None;

                if arms.is_empty() {
                    self.push_error(expr.span, "match expression has no arms".to_string());
                }

                for arm in arms {
                    let (arm_info, bindings) =
                        self.validate_match_pattern(&arm.pattern, &scrutinee_eval.ty);

                    self.enter_scope();
                    for binding in bindings {
                        self.declare(
                            binding,
                            Symbol {
                                kind: SymbolKind::Variable { mutable: false },
                                ty: Some(TypeKind::Any),
                                span: arm.pattern.span,
                                params: vec![],
                                used: false,
                                initialized: true,
                                is_import: false,
                                import_path: None,
                                const_value: None,
                            },
                        );
                    }

                    let arm_eval = self.type_check_expr(&arm.expr, reachable);
                    self.exit_scope_collect();

                    if let Some(arm_ty) = arm_eval.ty {
                        if let Some(current) = &result_ty {
                            if !self.types_compatible(current, &arm_ty) {
                                self.push_error(
                                    arm.span,
                                    format!(
                                        "match arm type mismatch: expected {}, got {}",
                                        current, arm_ty
                                    ),
                                );
                            }
                        } else {
                            result_ty = Some(arm_ty);
                        }
                    }

                    arm_infos.push(arm_info);
                }

                self.match_candidates.push(MatchCandidate {
                    span: expr.span,
                    scrutinee_ty: scrutinee_eval.ty.clone(),
                    arms: arm_infos,
                });

                ExprEval {
                    ty: result_ty,
                    const_value: None,
                }
            }
        };

        self.annotate_expr(expr, &result, reachable);
        result
    }

    fn infer_binary_type(
        &mut self,
        span: Span,
        op: &BinOpKind,
        left: &Option<TypeKind>,
        right: &Option<TypeKind>,
    ) -> Option<TypeKind> {
        match op {
            BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul | BinOpKind::Div | BinOpKind::Mod => {
                match (left, right) {
                    (Some(l), Some(r)) if self.types_compatible(l, r) => Some(l.clone()),
                    (Some(l), Some(r)) => {
                        self.push_error(
                            span,
                            format!("type mismatch in binary op: {} vs {}", l, r),
                        );
                        None
                    }
                    (Some(l), None) => Some(l.clone()),
                    (None, Some(r)) => Some(r.clone()),
                    (None, None) => None,
                }
            }
            BinOpKind::Lt
            | BinOpKind::Gt
            | BinOpKind::LtEq
            | BinOpKind::GtEq
            | BinOpKind::EqEq
            | BinOpKind::NotEq => {
                if let (Some(l), Some(r)) = (left, right) {
                    if !self.types_compatible(l, r) {
                        self.push_error(
                            span,
                            format!("type mismatch in binary op: {} vs {}", l, r),
                        );
                    }
                }
                Some(TypeKind::Bool)
            }
            BinOpKind::AndAnd | BinOpKind::OrOr => {
                if let Some(l) = left {
                    if !matches!(l, TypeKind::Bool | TypeKind::Any) {
                        self.push_error(span, format!("logical op requires bool, got {}", l));
                    }
                }

                if let Some(r) = right {
                    if !matches!(r, TypeKind::Bool | TypeKind::Any) {
                        self.push_error(span, format!("logical op requires bool, got {}", r));
                    }
                }

                Some(TypeKind::Bool)
            }
        }
    }

    fn const_from_literal(lit: &Literal) -> Option<ConstValue> {
        match lit {
            Literal::Int(v) => Some(ConstValue::Int(*v)),
            Literal::Float(v) => Some(ConstValue::Float(*v)),
            Literal::String(v) => Some(ConstValue::String(v.clone())),
            Literal::Bool(v) => Some(ConstValue::Bool(*v)),
        }
    }

    fn const_from_unary(op: &UnaryOpKind, value: ConstValue) -> Option<ConstValue> {
        match (op, value) {
            (UnaryOpKind::Neg, ConstValue::Int(v)) => Some(ConstValue::Int(-v)),
            (UnaryOpKind::Neg, ConstValue::Float(v)) => Some(ConstValue::Float(-v)),
            (UnaryOpKind::Not, ConstValue::Bool(v)) => Some(ConstValue::Bool(!v)),
            _ => None,
        }
    }

    fn const_from_binary(op: &BinOpKind, left: &ConstValue, right: &ConstValue) -> Option<ConstValue> {
        match (op, left, right) {
            (BinOpKind::Add, ConstValue::Int(a), ConstValue::Int(b)) => Some(ConstValue::Int(a + b)),
            (BinOpKind::Sub, ConstValue::Int(a), ConstValue::Int(b)) => Some(ConstValue::Int(a - b)),
            (BinOpKind::Mul, ConstValue::Int(a), ConstValue::Int(b)) => Some(ConstValue::Int(a * b)),
            (BinOpKind::Div, ConstValue::Int(a), ConstValue::Int(b)) if *b != 0 => {
                Some(ConstValue::Int(a / b))
            }
            (BinOpKind::Mod, ConstValue::Int(a), ConstValue::Int(b)) if *b != 0 => {
                Some(ConstValue::Int(a % b))
            }

            (BinOpKind::Add, ConstValue::Float(a), ConstValue::Float(b)) => {
                Some(ConstValue::Float(a + b))
            }
            (BinOpKind::Sub, ConstValue::Float(a), ConstValue::Float(b)) => {
                Some(ConstValue::Float(a - b))
            }
            (BinOpKind::Mul, ConstValue::Float(a), ConstValue::Float(b)) => {
                Some(ConstValue::Float(a * b))
            }
            (BinOpKind::Div, ConstValue::Float(a), ConstValue::Float(b)) if *b != 0.0 => {
                Some(ConstValue::Float(a / b))
            }
            (BinOpKind::Mod, ConstValue::Float(a), ConstValue::Float(b)) if *b != 0.0 => {
                Some(ConstValue::Float(a % b))
            }

            (BinOpKind::Add, ConstValue::String(a), ConstValue::String(b)) => {
                Some(ConstValue::String(format!("{}{}", a, b)))
            }

            (BinOpKind::Lt, ConstValue::Int(a), ConstValue::Int(b)) => Some(ConstValue::Bool(a < b)),
            (BinOpKind::Gt, ConstValue::Int(a), ConstValue::Int(b)) => Some(ConstValue::Bool(a > b)),
            (BinOpKind::LtEq, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Bool(a <= b))
            }
            (BinOpKind::GtEq, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Bool(a >= b))
            }
            (BinOpKind::Lt, ConstValue::Float(a), ConstValue::Float(b)) => {
                Some(ConstValue::Bool(a < b))
            }
            (BinOpKind::Gt, ConstValue::Float(a), ConstValue::Float(b)) => {
                Some(ConstValue::Bool(a > b))
            }
            (BinOpKind::LtEq, ConstValue::Float(a), ConstValue::Float(b)) => {
                Some(ConstValue::Bool(a <= b))
            }
            (BinOpKind::GtEq, ConstValue::Float(a), ConstValue::Float(b)) => {
                Some(ConstValue::Bool(a >= b))
            }

            (BinOpKind::EqEq, a, b) => Some(ConstValue::Bool(a == b)),
            (BinOpKind::NotEq, a, b) => Some(ConstValue::Bool(a != b)),

            (BinOpKind::AndAnd, ConstValue::Bool(a), ConstValue::Bool(b)) => {
                Some(ConstValue::Bool(*a && *b))
            }
            (BinOpKind::OrOr, ConstValue::Bool(a), ConstValue::Bool(b)) => {
                Some(ConstValue::Bool(*a || *b))
            }

            _ => None,
        }
    }

    fn annotate_expr(&mut self, expr: &Expr, eval: &ExprEval, reachable: bool) {
        self.annotated_exprs.push(ExprAnnotation {
            span: expr.span,
            ty: eval.ty.clone(),
            const_value: eval.const_value.clone(),
            reachable,
        });

        if let Some(value) = &eval.const_value {
            self.constant_evaluations.push(ConstantEvaluation {
                span: expr.span,
                value: value.clone(),
            });
        }
    }

    fn validate_match_pattern(
        &mut self,
        pattern: &Pattern,
        scrutinee_ty: &Option<TypeKind>,
    ) -> (MatchArmInfo, Vec<String>) {
        match &pattern.node {
            PatternKind::Wildcard => (
                MatchArmInfo {
                    span: pattern.span,
                    kind: MatchArmKindInfo::Wildcard,
                },
                Vec::new(),
            ),
            PatternKind::Variant {
                enum_name,
                variant,
                bindings,
            } => {
                if let Some(TypeKind::Named { name, .. }) = scrutinee_ty {
                    let target_enum = enum_name.clone().unwrap_or_else(|| name.clone());

                    if target_enum != *name {
                        self.push_error(
                            pattern.span,
                            format!(
                                "pattern enum '{}' does not match scrutinee enum '{}'",
                                target_enum, name
                            ),
                        );
                    }

                    if let Some(info) = self.enums.get(&target_enum) {
                        if let Some(expected_arity) = info.variants.get(variant) {
                            if *expected_arity != bindings.len() {
                                self.push_error(
                                    pattern.span,
                                    format!(
                                        "variant '{}.{}' expects {} binding(s), got {}",
                                        target_enum,
                                        variant,
                                        expected_arity,
                                        bindings.len()
                                    ),
                                );
                            }
                        } else {
                            self.push_error(
                                pattern.span,
                                format!(
                                    "unknown variant '{}.{}' in match pattern",
                                    target_enum, variant
                                ),
                            );
                        }
                    } else {
                        self.push_error(
                            pattern.span,
                            format!("unknown enum '{}' in match pattern", target_enum),
                        );
                    }
                } else if scrutinee_ty.is_some() {
                    self.push_error(
                        pattern.span,
                        "variant pattern requires enum scrutinee".to_string(),
                    );
                }

                (
                    MatchArmInfo {
                        span: pattern.span,
                        kind: MatchArmKindInfo::Variant {
                            enum_name: enum_name.clone(),
                            variant: variant.clone(),
                        },
                    },
                    bindings.clone(),
                )
            }
        }
    }

    fn run_unused_pass(&mut self) {
        let local_scopes = std::mem::take(&mut self.finished_scopes);
        for scope in local_scopes {
            self.emit_unused_warnings(scope, false, false);
        }

        let global_symbols: Vec<(String, Symbol)> = self
            .scopes
            .first()
            .expect("semantic analyzer must always keep global scope")
            .iter()
            .map(|(name, symbol)| (name.clone(), symbol.clone()))
            .collect();

        self.emit_unused_warnings(global_symbols, true, true);
    }

    fn run_dead_code_pass(&mut self, program: &Program) {
        for item in &program.items {
            match &item.node {
                ItemKind::Fn { body, .. } => {
                    let _ = self.dead_code_block(body);
                }
                ItemKind::Impl { methods, .. } => {
                    for method in methods {
                        if let ItemKind::Fn { body, .. } = &method.node {
                            let _ = self.dead_code_block(body);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn dead_code_block(&mut self, block: &Block) -> bool {
        let mut reachable = true;
        let mut guaranteed_return = false;

        for stmt in &block.stmts {
            if !reachable {
                self.push_warning(stmt.span, "unreachable code".to_string());
                self.push_suggestion(
                    Some(stmt.span),
                    "remove or move the unreachable statement".to_string(),
                );
                continue;
            }

            if self.dead_code_stmt(stmt) {
                reachable = false;
                guaranteed_return = true;
            }
        }

        guaranteed_return
    }

    fn dead_code_stmt(&mut self, stmt: &Stmt) -> bool {
        match &stmt.node {
            StmtKind::Return(_) => true,
            StmtKind::If {
                then_block,
                else_block,
                ..
            } => {
                let then_returns = self.dead_code_block(then_block);
                let else_returns = if let Some(else_block) = else_block {
                    self.dead_code_block(else_block)
                } else {
                    false
                };
                then_returns && else_returns
            }
            StmtKind::While { body, .. } => {
                let _ = self.dead_code_block(body);
                false
            }
            StmtKind::Var { .. } | StmtKind::Const { .. } | StmtKind::ExprStmt(_) => false,
        }
    }

    fn run_inline_candidate_pass(&mut self, program: &Program) {
        for item in &program.items {
            match &item.node {
                ItemKind::Fn { name, body, .. } => {
                    self.maybe_add_inline_candidate(name, body, item.span);
                }
                ItemKind::Impl { methods, .. } => {
                    for method in methods {
                        if let ItemKind::Fn { name, body, .. } = &method.node {
                            self.maybe_add_inline_candidate(name, body, method.span);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn maybe_add_inline_candidate(&mut self, name: &str, body: &Block, span: Span) {
        if name == "main" {
            return;
        }

        if body.stmts.len() > 2 {
            return;
        }

        if body
            .stmts
            .iter()
            .any(|stmt| matches!(stmt.node, StmtKind::If { .. } | StmtKind::While { .. }))
        {
            return;
        }

        let candidate = InlineCandidate {
            name: name.to_string(),
            span,
            reason: "small function body".to_string(),
        };
        self.inline_candidates.push(candidate.clone());
        self.push_suggestion(
            Some(span),
            format!(
                "function '{}' is an inline candidate ({})",
                candidate.name, candidate.reason
            ),
        );
    }

    fn run_import_optimization_pass(&mut self) {
        if self.unused_import_paths.is_empty() {
            return;
        }

        let list = self
            .unused_import_paths
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");

        self.push_suggestion(
            None,
            format!("import optimization: remove unused imports [{}]", list),
        );
    }

    fn run_exhaustiveness_pass(&mut self) {
        let candidates = std::mem::take(&mut self.match_candidates);

        for candidate in candidates {
            let Some(TypeKind::Named { name: scrutinee_enum, .. }) = candidate.scrutinee_ty else {
                continue;
            };

            let Some(enum_info) = self.enums.get(&scrutinee_enum).cloned() else {
                continue;
            };

            self.exhaustiveness_checks += 1;

            let mut wildcard_seen = false;
            let mut covered: BTreeSet<String> = BTreeSet::new();

            for arm in &candidate.arms {
                match &arm.kind {
                    MatchArmKindInfo::Wildcard => {
                        if wildcard_seen {
                            self.push_warning(
                                arm.span,
                                "unreachable wildcard match arm".to_string(),
                            );
                        }
                        wildcard_seen = true;
                    }
                    MatchArmKindInfo::Variant { enum_name, variant } => {
                        if wildcard_seen {
                            self.push_warning(
                                arm.span,
                                format!(
                                    "unreachable match arm '{}', wildcard already covers it",
                                    variant
                                ),
                            );
                            continue;
                        }

                        let target_enum =
                            enum_name.clone().unwrap_or_else(|| scrutinee_enum.clone());

                        if target_enum != scrutinee_enum {
                            self.push_error(
                                arm.span,
                                format!(
                                    "match arm enum '{}' does not match '{}'",
                                    target_enum, scrutinee_enum
                                ),
                            );
                            continue;
                        }

                        if !enum_info.variants.contains_key(variant) {
                            self.push_error(
                                arm.span,
                                format!(
                                    "unknown variant '{}.{}' in match arm",
                                    scrutinee_enum, variant
                                ),
                            );
                            continue;
                        }

                        if !covered.insert(variant.clone()) {
                            self.push_warning(
                                arm.span,
                                format!("duplicate/unreachable match arm '{}.{}'", scrutinee_enum, variant),
                            );
                            self.push_suggestion(
                                Some(arm.span),
                                format!(
                                    "remove duplicate match arm for '{}.{}'",
                                    scrutinee_enum, variant
                                ),
                            );
                        }
                    }
                }
            }

            if !wildcard_seen {
                let missing_variants: Vec<String> = enum_info
                    .variants
                    .keys()
                    .filter(|name| !covered.contains(*name))
                    .cloned()
                    .collect();

                if !missing_variants.is_empty() {
                    let missing_joined = missing_variants
                        .iter()
                        .map(|v| format!("{}.{}", scrutinee_enum, v))
                        .collect::<Vec<_>>()
                        .join(", ");

                    self.push_error(
                        candidate.span,
                        format!(
                            "non-exhaustive match for enum '{}': missing [{}]",
                            scrutinee_enum, missing_joined
                        ),
                    );
                    self.push_suggestion(
                        Some(candidate.span),
                        format!(
                            "add missing arms [{}] or add wildcard '_' arm",
                            missing_joined
                        ),
                    );

                    self.non_exhaustive_matches.push(MatchExhaustivenessIssue {
                        span: candidate.span,
                        enum_name: scrutinee_enum,
                        missing_variants,
                    });
                }
            }
        }
    }

    fn types_compatible(&self, a: &TypeKind, b: &TypeKind) -> bool {
        match (a, b) {
            (TypeKind::Any, _) | (_, TypeKind::Any) => true,
            (TypeKind::Named { .. }, _) | (_, TypeKind::Named { .. }) => true,
            _ => std::mem::discriminant(a) == std::mem::discriminant(b),
        }
    }

    fn analyze_assign_target(&mut self, target: &Expr) {
        match &target.node {
            ExprKind::Ident(name) => match self.resolve_symbol(name) {
                None => self.push_error(target.span, format!("unknown identifier '{}'", name)),
                Some(sym) => match sym.kind {
                    SymbolKind::Variable { mutable: false } => {
                        self.push_error(target.span, format!("cannot assign to const '{}'", name));
                    }
                    SymbolKind::Function | SymbolKind::TypeName => {
                        self.push_error(target.span, format!("cannot assign to '{}'", name));
                    }
                    SymbolKind::Parameter | SymbolKind::Variable { mutable: true } => {}
                },
            },
            ExprKind::Field { object, .. } => {
                self.type_check_expr(object, true);
            }
            _ => {
                self.type_check_expr(target, true);
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

    fn resolve_symbol(&self, name: &str) -> Option<Symbol> {
        for scope in self.scopes.iter().rev() {
            if let Some(symbol) = scope.get(name) {
                return Some(symbol.clone());
            }
        }
        None
    }

    fn resolve_for_read(&mut self, name: &str) -> Option<Symbol> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(symbol) = scope.get_mut(name) {
                symbol.used = true;
                if symbol.is_import {
                    if let Some(path) = &symbol.import_path {
                        self.used_import_paths.insert(path.clone());
                    }
                }
                return Some(symbol.clone());
            }
        }
        None
    }

    fn mark_initialized(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(symbol) = scope.get_mut(name) {
                symbol.initialized = true;
                return;
            }
        }
    }

    fn set_symbol_const_value(&mut self, name: &str, value: Option<ConstValue>) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(symbol) = scope.get_mut(name) {
                symbol.const_value = value;
                return;
            }
        }
    }

    fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn exit_scope_collect(&mut self) {
        let scope = self
            .scopes
            .pop()
            .expect("semantic analyzer must always have at least one scope");
        self.finished_scopes.push(scope.into_iter().collect());
    }

    fn emit_unused_warnings(
        &mut self,
        symbols: Vec<(String, Symbol)>,
        include_functions: bool,
        include_imports: bool,
    ) {
        for (name, symbol) in symbols {
            if symbol.used {
                continue;
            }

            if symbol.is_import {
                if include_imports {
                    let full = symbol
                        .import_path
                        .clone()
                        .unwrap_or_else(|| name.clone());
                    self.unused_import_paths.insert(full.clone());
                    self.push_warning(
                        symbol.span,
                        format!("unused import '{}' (path '{}')", name, full),
                    );
                    self.push_suggestion(
                        Some(symbol.span),
                        format!("remove import '{}' if it is not needed", full),
                    );
                }
                continue;
            }

            match symbol.kind {
                SymbolKind::Parameter => {
                    self.push_warning(symbol.span, format!("unused parameter '{}'", name));
                    self.push_suggestion(
                        Some(symbol.span),
                        format!("remove or use parameter '{}'", name),
                    );
                }
                SymbolKind::Variable { mutable } => {
                    let label = if mutable { "variable" } else { "const" };
                    self.push_warning(symbol.span, format!("unused {} '{}'", label, name));
                    self.push_suggestion(
                        Some(symbol.span),
                        format!("remove or use {} '{}'", label, name),
                    );
                }
                SymbolKind::Function => {
                    if include_functions && name != "main" {
                        self.push_warning(symbol.span, format!("unused function '{}'", name));
                        self.push_suggestion(
                            Some(symbol.span),
                            format!("remove function '{}' or call it", name),
                        );
                    }
                }
                SymbolKind::TypeName => {}
            }
        }
    }

    fn push_error(&mut self, span: Span, message: String) {
        self.errors.push(SemanticError { message, span });
    }

    fn push_warning(&mut self, span: Span, message: String) {
        self.warnings.push(SemanticWarning { message, span });
    }

    fn push_suggestion(&mut self, span: Option<Span>, message: String) {
        self.suggestions.push(SemanticSuggestion { message, span });
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

    use super::{Analyzer, ConstValue, SemanticReport};

    fn parse_program(src: &str) -> crate::parser::ast::Program {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        parser.parse().expect("source must parse")
    }

    fn analyze(src: &str) -> SemanticReport {
        let program = parse_program(src);
        let mut analyzer = Analyzer::new();
        analyzer.analyze_program(&program)
    }

    #[test]
    fn reports_type_mismatch_in_const() {
        let report = analyze(
            r#"
fn main() void {
    const x: int32 = "";
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("type mismatch"))
        );
    }

    #[test]
    fn reports_type_mismatch_in_var() {
        let report = analyze(
            r#"
fn main() void {
    var x: bool = 123;
}
    "#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("type mismatch"))
        );
    }

    #[test]
    fn reports_readable_type_names_in_errors() {
        let report = analyze(
            r#"
fn main() void {
    const x: int32 = "";
}
"#,
        );

        let combined = report
            .errors
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
        let report = analyze(
            r#"
fn main() void {
    x = 1;
}
"#,
        );

        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("unknown identifier 'x'"))
        );
    }

    #[test]
    fn reports_duplicate_local_declaration() {
        let report = analyze(
            r#"
fn main() void {
    var x: int32 = 1;
    var x: int32 = 2;
}
"#,
        );

        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("duplicate declaration 'x'"))
        );
    }

    #[test]
    fn warns_for_unused_import_with_dot_path() {
        let report = analyze(
            r#"
import std.io.stdout;

fn main() void {
    const x: int32 = 1;
}
"#,
        );

        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.message.contains("unused import 'stdout'") && w.message.contains("std.io.stdout"))
        );
        assert!(report.unused_imports.contains(&"std.io.stdout".to_string()));
    }

    #[test]
    fn tracks_used_imports_with_dot_path() {
        let report = analyze(
            r#"
import std.io.stdout;

fn main() void {
    stdout.println("ok");
}
"#,
        );

        assert!(report.used_imports.contains(&"std.io.stdout".to_string()));
    }

    #[test]
    fn reports_use_before_initialization() {
        let report = analyze(
            r#"
fn main() void {
    var x: int32;
    const y: int32 = x;
}
"#,
        );

        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("before initialization"))
        );
    }

    #[test]
    fn warns_about_unreachable_code_after_return() {
        let report = analyze(
            r#"
fn main() void {
    return;
    var x: int32 = 1;
}
"#,
        );

        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.message.contains("unreachable code"))
        );
    }

    #[test]
    fn warns_about_unreachable_after_if_else_both_return() {
        let report = analyze(
            r#"
fn main() void {
    if (true) {
        return;
    } else {
        return;
    }
    var x: int32 = 1;
}
"#,
        );

        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.message.contains("unreachable code"))
        );
    }

    #[test]
    fn warns_about_unused_local_variable() {
        let report = analyze(
            r#"
fn main() void {
    var x: int32 = 1;
}
"#,
        );

        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.message.contains("unused variable 'x'"))
        );
    }

    #[test]
    fn warns_about_unused_function() {
        let report = analyze(
            r#"
fn helper() void {
    return;
}

fn main() void {
    return;
}
"#,
        );

        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.message.contains("unused function 'helper'"))
        );
    }

    #[test]
    fn records_expression_annotations_and_const_eval() {
        let report = analyze(
            r#"
fn main() void {
    const x: int32 = 1 + 2;
}
"#,
        );

        assert!(!report.annotated_exprs.is_empty());
        assert!(report.constant_evaluations.iter().any(|entry| entry.value == ConstValue::Int(3)));
    }

    #[test]
    fn detects_inline_candidates() {
        let report = analyze(
            r#"
fn helper(a: int32) int32 {
    return a;
}

fn main() void {
    helper(1);
}
"#,
        );

        assert!(report.inline_candidates.iter().any(|c| c.name == "helper"));
    }

    #[test]
    fn reports_non_exhaustive_match_for_enum() {
        let report = analyze(
            r#"
enum Option[T] {
    Some(T),
    None,
}

fn unwrap_or_fail(x: Option[int32]) int32 {
    return match x {
        Some(v) => v,
    };
}
"#,
        );

        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("non-exhaustive match"))
        );
        assert_eq!(report.exhaustiveness_checks, 1);
        assert_eq!(report.non_exhaustive_matches.len(), 1);
    }

    #[test]
    fn accepts_exhaustive_match_for_enum() {
        let report = analyze(
            r#"
enum Option[T] {
    Some(T),
    None,
}

fn unwrap_or_zero(x: Option[int32]) int32 {
    return match x {
        Some(v) => v,
        None => 0,
    };
}
"#,
        );

        assert!(
            !report
                .errors
                .iter()
                .any(|e| e.message.contains("non-exhaustive match"))
        );
        assert_eq!(report.exhaustiveness_checks, 1);
    }

    #[test]
    fn warns_on_duplicate_match_arm() {
        let report = analyze(
            r#"
enum Color {
    Red,
    Blue,
}

fn color_value(c: Color) int32 {
    return match c {
        Red => 1,
        Red => 2,
        Blue => 3,
    };
}
"#,
        );

        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.message.contains("duplicate/unreachable match arm"))
        );
    }
}
