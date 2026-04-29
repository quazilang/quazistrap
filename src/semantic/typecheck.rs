// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use crate::parser::ast::*;

use super::*;

impl Analyzer {
    pub(super) fn type_check_item(&mut self, item: &Item) {
        match &item.node {
            ItemKind::Fn {
                name,
                params,
                return_ty,
                body,
                ..
            } => {
                self.current_function.push(name.clone());
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
                let _ = self.current_function.pop();
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

    pub(super) fn type_check_block(&mut self, block: &Block, expected_return: Option<&TypeKind>) -> bool {
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

    pub(super) fn type_check_stmt(&mut self, stmt: &Stmt, expected_return: Option<&TypeKind>) -> bool {
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

    pub(super) fn type_check_expr(&mut self, expr: &Expr, reachable: bool) -> ExprEval {
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
                let const_value = if let (Some(lhs), Some(rhs)) =
                    (&left_eval.const_value, &right_eval.const_value)
                {
                    Self::const_from_binary(op, lhs, rhs)
                } else {
                    self.check_math_identities(expr.span, op, &left_eval, &right_eval)
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

                    if matches!(sym.kind, SymbolKind::Function) {
                        let from = self
                            .current_function
                            .last()
                            .cloned()
                            .unwrap_or_else(|| "__program__".to_string());
                        self.add_dependency_edge(DependencyKind::Call, &from, name);
                    }

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
            ExprKind::MethodCall { object, method: _, args, .. } => {
                // For lazy import tracking: record the object chain path (not the method itself)
                if let Some((base, path)) = Self::extract_field_chain(object) {
                    if let Some(sym) = self.resolve_symbol(&base) {
                        if sym.is_import {
                            let import_base = sym.import_path.as_deref().unwrap_or(&base);
                            let full_access = if path.is_empty() {
                                import_base.to_string()
                            } else {
                                format!("{}.{}", import_base, path.join("."))
                            };
                            self.lazy_import_accesses
                                .entry(base)
                                .or_default()
                                .insert(full_access);
                        }
                    }
                }
                self.type_check_expr(object, reachable);
                for arg in args {
                    self.type_check_expr(arg, reachable);
                }
                ExprEval::default()
            }
            ExprKind::Field { object, name } => {
                // For lazy import tracking: record the full chain including this field
                if let Some((base, mut path)) = Self::extract_field_chain(object) {
                    if let Some(sym) = self.resolve_symbol(&base) {
                        if sym.is_import {
                            path.push(name.clone());
                            let import_base = sym.import_path.as_deref().unwrap_or(&base);
                            let full_access = if path.is_empty() {
                                import_base.to_string()
                            } else {
                                format!("{}.{}", import_base, path.join("."))
                            };
                            self.lazy_import_accesses
                                .entry(base)
                                .or_default()
                                .insert(full_access);
                        }
                    }
                }
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
            ExprKind::CompoundAssign { target, op, value } => {
                self.analyze_assign_target(target);
                let target_eval = self.type_check_expr(target, reachable);
                let value_eval = self.type_check_expr(value, reachable);

                let bin_op = match op {
                    CompoundAssignOp::Add => BinOpKind::Add,
                    CompoundAssignOp::Sub => BinOpKind::Sub,
                    CompoundAssignOp::Mul => BinOpKind::Mul,
                    CompoundAssignOp::Div => BinOpKind::Div,
                    CompoundAssignOp::Mod => BinOpKind::Mod,
                };
                let ty = self.infer_binary_type(expr.span, &bin_op, &target_eval.ty, &value_eval.ty);
                ExprEval { ty, const_value: None }
            }
            ExprKind::IncDec { expr: inner, .. } => {
                self.analyze_assign_target(inner);
                let inner_eval = self.type_check_expr(inner, reachable);

                if let Some(ty) = &inner_eval.ty {
                    if matches!(ty, TypeKind::Str | TypeKind::Bool | TypeKind::Void) {
                        self.push_error(inner.span, format!("++ / -- not valid for {}", ty));
                    }
                }
                ExprEval { ty: inner_eval.ty, const_value: None }
            }
        };

        self.annotate_expr(expr, &result, reachable);
        result
    }

    pub(super) fn infer_binary_type(
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

    pub(super) fn const_from_literal(lit: &Literal) -> Option<ConstValue> {
        match lit {
            Literal::Int(v) => Some(ConstValue::Int(*v)),
            Literal::Float(v) => Some(ConstValue::Float(*v)),
            Literal::String(v) => Some(ConstValue::String(v.clone())),
            Literal::Bool(v) => Some(ConstValue::Bool(*v)),
        }
    }

    pub(super) fn const_from_unary(op: &UnaryOpKind, value: ConstValue) -> Option<ConstValue> {
        match (op, value) {
            (UnaryOpKind::Neg, ConstValue::Int(v)) => Some(ConstValue::Int(-v)),
            (UnaryOpKind::Neg, ConstValue::Float(v)) => Some(ConstValue::Float(-v)),
            (UnaryOpKind::Not, ConstValue::Bool(v)) => Some(ConstValue::Bool(!v)),
            _ => None,
        }
    }

    pub(super) fn const_from_binary(op: &BinOpKind, left: &ConstValue, right: &ConstValue) -> Option<ConstValue> {
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

    pub(super) fn annotate_expr(&mut self, expr: &Expr, eval: &ExprEval, reachable: bool) {
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

    pub(super) fn validate_match_pattern(
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

    pub(super) fn analyze_assign_target(&mut self, target: &Expr) {
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

    pub(super) fn types_compatible(&self, a: &TypeKind, b: &TypeKind) -> bool {
        match (a, b) {
            (TypeKind::Any, _) | (_, TypeKind::Any) => true,
            (TypeKind::Named { .. }, _) | (_, TypeKind::Named { .. }) => true,
            _ => std::mem::discriminant(a) == std::mem::discriminant(b),
        }
    }
}
