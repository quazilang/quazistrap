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
                attributes,
                unsafe_fn,
                ..
            } => {
                self.validate_foreign_attributes(attributes);
                let is_foreign = attributes
                    .iter()
                    .any(|a| a.name == "syscall" || a.name == "api" || a.name == "intrinsic");
                if *unsafe_fn {
                    self.unsafe_depth += 1;
                }
                self.current_function.push(name.clone());
                self.enter_scope();
                for p in params {
                    let ty = unwrap_type(&p.ty);
                    let ty = if p.variadic {
                        TypeKind::Slice {
                            elem_ty: Box::new(Spanned::new(ty, p.ty.span)),
                        }
                    } else {
                        ty
                    };
                    // Merge function attributes with parameter attributes
                    let mut param_attrs = extract_attribute_names(&p.attributes);
                    // If function has @syscall or @api, add @ignore to suppress unused warnings
                    if is_foreign {
                        param_attrs.push("ignore".to_string());
                    }
                    self.declare(
                        p.name.clone(),
                        Symbol {
                            kind: SymbolKind::Parameter,
                            span: item.span,
                            ty: Some(ty),
                            params: vec![],
                            used: false,
                            initialized: true,
                            is_import: false,
                            import_path: None,
                            const_value: None,
                            variadic: false,
                            attributes: param_attrs,
                            public: false,
                        },
                    );
                }

                let expected = unwrap_type(return_ty);

                if name == "main"
                    && !matches!(
                        expected,
                        TypeKind::Void | TypeKind::Int32 | TypeKind::Uint32 | TypeKind::Never
                    )
                {
                    self.push_error(
                        item.span,
                        "S01",
                        format!(
                            "main() return type must be void, i32, u32, or !, got {}",
                            expected
                        ),
                    );
                }

                let guaranteed = if let Some(body) = body {
                    self.type_check_block(body, Some(&expected))
                } else {
                    true // bodyless declaration — no return check
                };
                if !guaranteed
                    && !is_foreign
                    && !matches!(expected, TypeKind::Void | TypeKind::Never)
                {
                    self.push_error(
                        item.span,
                        "S03",
                        format!(
                            "function `{}` with return type `{}` does not always return a value",
                            name, expected
                        ),
                    );
                }
                self.exit_scope_collect();
                let _ = self.current_function.pop();
                if *unsafe_fn {
                    self.unsafe_depth -= 1;
                }
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

    fn validate_foreign_attributes(&mut self, attributes: &[Attribute]) {
        let mut syscall_attr: Option<&Attribute> = None;
        let mut api_attr: Option<&Attribute> = None;

        for attr in attributes {
            match attr.name.as_str() {
                "syscall" => syscall_attr = Some(attr),
                "api" => api_attr = Some(attr),
                _ => {}
            }
        }

        if syscall_attr.is_some() && api_attr.is_some() {
            let span = api_attr.unwrap().span;
            self.push_error(
                span,
                "S06",
                "cannot combine @syscall and @api on the same function".to_string(),
            );
        }

        if let Some(attr) = syscall_attr {
            self.validate_syscall_attr(attr);
        }

        if let Some(attr) = api_attr {
            self.validate_api_attr(attr);
        }
    }

    fn validate_syscall_attr(&mut self, attr: &Attribute) {
        let ok = match attr.args.as_slice() {
            [AttrArg::Positional(AttrVal::Str(_))] => true,
            [AttrArg::Positional(AttrVal::Int(n))] => *n >= 0 && *n <= u16::MAX as i64,
            _ => false,
        };

        if !ok {
            self.push_error(
                attr.span,
                "S06",
                "invalid @syscall attribute (use @syscall(\"name\") or @syscall(number))"
                    .to_string(),
            );
        }
    }

    fn validate_api_attr(&mut self, attr: &Attribute) {
        let ok = matches!(attr.args.as_slice(), [AttrArg::Positional(AttrVal::Str(_))]);
        if !ok {
            self.push_error(
                attr.span,
                "S06",
                "invalid @api attribute (use @api(\"FunctionName\"))".to_string(),
            );
        }
    }

    pub(super) fn type_check_block(
        &mut self,
        block: &Block,
        expected_return: Option<&TypeKind>,
    ) -> bool {
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

    pub(super) fn type_check_stmt(
        &mut self,
        stmt: &Stmt,
        expected_return: Option<&TypeKind>,
    ) -> bool {
        match &stmt.node {
            StmtKind::Var {
                name,
                value,
                ty,
                attributes,
                ..
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
                            "S01",
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
                        variadic: false,
                        attributes: extract_attribute_names(attributes),
                        public: false,
                    },
                );
                false
            }
            StmtKind::Const {
                name,
                value,
                ty,
                attributes,
                ..
            } => {
                let value_eval = self.type_check_expr(value, true);
                let declared_ty = ty.as_ref().map(|t| t.node.clone());

                if let (Some(ann), Some(val)) = (&declared_ty, &value_eval.ty) {
                    if !self.types_compatible(ann, val) {
                        self.push_error(
                            stmt.span,
                            "S01",
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
                        variadic: false,
                        attributes: extract_attribute_names(attributes),
                        public: false,
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
                                    "S01",
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
                                "S01",
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
                            "S01",
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
            StmtKind::For { kind, body } => {
                match kind {
                    ForLoop::Cond {
                        condition: Some(cond),
                    } => {
                        let cond_eval = self.type_check_expr(cond, true);
                        if let Some(cond_ty) = cond_eval.ty {
                            if !matches!(cond_ty, TypeKind::Bool | TypeKind::Any) {
                                self.push_error(
                                    cond.span,
                                    "S01",
                                    format!("for condition must be bool, got {}", cond_ty),
                                );
                            }
                        }
                        let _ = self.type_check_block(body, expected_return);
                    }
                    ForLoop::Cond { condition: None } => {
                        let _ = self.type_check_block(body, expected_return);
                    }
                    ForLoop::CStyle {
                        init,
                        condition,
                        update,
                    } => {
                        self.enter_scope();
                        if let Some(init_stmt) = init {
                            self.type_check_stmt(init_stmt, expected_return);
                        }
                        if let Some(cond) = condition {
                            let cond_eval = self.type_check_expr(cond, true);
                            if let Some(cond_ty) = cond_eval.ty {
                                if !matches!(cond_ty, TypeKind::Bool | TypeKind::Any) {
                                    self.push_error(
                                        cond.span,
                                        "S01",
                                        format!("for condition must be bool, got {}", cond_ty),
                                    );
                                }
                            }
                        }
                        if let Some(upd) = update {
                            self.type_check_expr(upd, true);
                        }
                        self.type_check_block(body, expected_return);
                        self.exit_scope_collect();
                    }
                    ForLoop::Each { vars, iter } => {
                        let loop_var_ty = match iter {
                            ForIter::Range { start, end } => {
                                let start_eval = self.type_check_expr(start, true);
                                let end_eval = self.type_check_expr(end, true);
                                if let Some(t) = &start_eval.ty {
                                    if !Self::is_integer(t) {
                                        self.push_error(
                                            start.span,
                                            "S01",
                                            format!(
                                                "for range start must be an integer type, got {}",
                                                t
                                            ),
                                        );
                                    }
                                }
                                if let Some(t) = &end_eval.ty {
                                    if !Self::is_integer(t) {
                                        self.push_error(
                                            end.span,
                                            "S01",
                                            format!(
                                                "for range end must be an integer type, got {}",
                                                t
                                            ),
                                        );
                                    }
                                }
                                start_eval.ty.or(end_eval.ty).unwrap_or(TypeKind::Int32)
                            }
                            ForIter::Iter(expr) => {
                                let iter_eval = self.type_check_expr(expr, true);
                                match &iter_eval.ty {
                                    Some(TypeKind::Array { elem_ty, .. }) => elem_ty.node.clone(),
                                    Some(TypeKind::Slice { elem_ty }) => elem_ty.node.clone(),
                                    _ => TypeKind::Any,
                                }
                            }
                        };
                        self.enter_scope();
                        for var in vars {
                            self.declare(
                                var.clone(),
                                Symbol {
                                    kind: SymbolKind::Variable { mutable: false },
                                    span: stmt.span,
                                    ty: Some(loop_var_ty.clone()),
                                    params: vec![],
                                    used: true,
                                    initialized: true,
                                    is_import: false,
                                    import_path: None,
                                    const_value: None,
                                    variadic: false,
                                    attributes: Vec::new(),
                                    public: false,
                                },
                            );
                        }
                        for s in &body.stmts {
                            self.type_check_stmt(s, expected_return);
                        }
                        self.exit_scope_collect();
                    }
                }
                false
            }
            StmtKind::UnsafeBlock { body } => {
                self.unsafe_depth += 1;
                self.type_check_block(body, expected_return);
                self.unsafe_depth -= 1;
                false
            }
            StmtKind::ExprStmt(expr) => {
                self.type_check_expr(expr, true);
                false
            }
            StmtKind::CfgBlock { body, .. } => {
                self.type_check_block(body, expected_return);
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
                    Literal::String(_) => TypeKind::Ref {
                        inner: Box::new(Spanned::new(TypeKind::Str, expr.span)),
                    },
                    Literal::Bool(_) => TypeKind::Bool,
                };

                ExprEval {
                    ty: Some(ty),
                    const_value: Self::const_from_literal(lit),
                }
            }
            ExprKind::Ident(name) => match self.resolve_for_read(name) {
                None => {
                    self.push_error(expr.span, "S04", format!("unknown identifier '{}'", name));
                    ExprEval::default()
                }
                Some(sym) => {
                    if matches!(
                        sym.kind,
                        SymbolKind::Variable { .. } | SymbolKind::Parameter
                    ) && !sym.initialized
                    {
                        self.push_error(
                            expr.span,
                            "S02",
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

                match op {
                    UnaryOpKind::Ref => {
                        // &expr → type is Ref<inner_type>
                        let ref_ty = inner_eval.ty.map(|t| TypeKind::Ref {
                            inner: Box::new(Spanned::new(t, inner.span)),
                        });
                        let result = ExprEval {
                            ty: ref_ty,
                            const_value: None,
                        };
                        self.annotate_expr(expr, &result, reachable);
                        return result;
                    }
                    UnaryOpKind::Deref => {
                        // *expr → unwrap Ref<T> or RawPtr<T>
                        let result = match &inner_eval.ty {
                            Some(TypeKind::Ref { inner: t }) => ExprEval {
                                ty: Some(t.node.clone()),
                                const_value: None,
                            },
                            Some(TypeKind::RawPtr { inner: t }) => {
                                if self.unsafe_depth == 0 {
                                    self.push_error(
                                        expr.span,
                                        "S11",
                                        "dereference of raw pointer requires unsafe block"
                                            .to_string(),
                                    );
                                }
                                ExprEval {
                                    ty: Some(t.node.clone()),
                                    const_value: None,
                                }
                            }
                            Some(other) => {
                                self.push_error(
                                    expr.span,
                                    "S11",
                                    format!("cannot dereference non-pointer type {}", other),
                                );
                                ExprEval::default()
                            }
                            None => ExprEval::default(),
                        };
                        self.annotate_expr(expr, &result, reachable);
                        return result;
                    }
                    UnaryOpKind::Not => {
                        if let Some(t) = &inner_eval.ty {
                            if !matches!(t, TypeKind::Bool | TypeKind::Any) {
                                self.push_error(
                                    inner.span,
                                    "S06",
                                    format!("! requires bool, got {}", t),
                                );
                            }
                        }
                    }
                    UnaryOpKind::Neg => {
                        if let Some(t) = &inner_eval.ty {
                            if matches!(
                                t,
                                TypeKind::Str
                                    | TypeKind::Bool
                                    | TypeKind::Ref { .. }
                                    | TypeKind::RawPtr { .. }
                            ) {
                                self.push_error(
                                    inner.span,
                                    "S06",
                                    format!("unary - not valid for {}", t),
                                );
                            }
                        }
                    }
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
                                    "S01",
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
                let arg_evals: Vec<ExprEval> = args
                    .iter()
                    .map(|a| self.type_check_expr(a, reachable))
                    .collect();

                if let ExprKind::Ident(name) = &callee.node {
                    let Some(sym) = self.resolve_for_read(name) else {
                        self.push_error(
                            callee.span,
                            "S04",
                            format!("unknown identifier '{}'", name),
                        );
                        return ExprEval::default();
                    };

                    if !matches!(sym.kind, SymbolKind::Function) {
                        self.push_error(
                            callee.span,
                            "S04",
                            format!("function '{}' does not exist", name),
                        );
                        return ExprEval::default();
                    }

                    // Library functions require explicit import-by-name to be called unqualified.
                    if self.library_fn_names.contains(name.as_str())
                        && !self.explicitly_imported_fns.contains(name.as_str())
                    {
                        self.push_error(
                            callee.span,
                            "S04",
                            format!(
                                "'{}' is not in scope; use qualified access or add 'import ...{};'",
                                name, name
                            ),
                        );
                        return ExprEval::default();
                    }

                    self.check_function_call(name, callee.span, args, &arg_evals, &sym)
                } else {
                    self.type_check_expr(callee, reachable);
                    ExprEval::default()
                }
            }
            ExprKind::MethodCall {
                object,
                method,
                type_args,
                args,
            } => {
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
                if self.is_module_import_receiver(object) {
                    self.type_check_expr(object, reachable);
                    let arg_evals: Vec<ExprEval> = args
                        .iter()
                        .map(|a| self.type_check_expr(a, reachable))
                        .collect();
                    if let Some(sym) = self.resolve_symbol(method) {
                        if !matches!(sym.kind, SymbolKind::Function) {
                            self.push_error(
                                expr.span,
                                "S04",
                                format!("function '{}' does not exist", method),
                            );
                            ExprEval::default()
                        } else if self.library_fn_names.contains(method.as_str()) && !sym.public {
                            self.push_error(expr.span, "S04", format!("'{}' is private", method));
                            ExprEval::default()
                        } else {
                            let sym = self
                                .resolve_for_read(method)
                                .expect("symbol should resolve");
                            self.check_function_call(method, expr.span, args, &arg_evals, &sym)
                        }
                    } else {
                        ExprEval::default()
                    }
                } else {
                    let object_eval = self.type_check_expr(object, reachable);
                    for arg in args {
                        self.type_check_expr(arg, reachable);
                    }
                    let ref_str = TypeKind::Ref {
                        inner: Box::new(Spanned::new(TypeKind::Str, expr.span)),
                    };
                    let builtin_ty = match method.as_str() {
                        "len" => Some(TypeKind::Usize),
                        "to_string" | "to_str" => match &object_eval.ty {
                            Some(t)
                                if Self::is_integer(t)
                                    || Self::is_float(t)
                                    || matches!(t, TypeKind::Str | TypeKind::Ref { .. }
                                        | TypeKind::Bool) =>
                            {
                                Some(ref_str.clone())
                            }
                            _ => Some(TypeKind::Named {
                                name: "String".to_string(),
                                type_args: vec![],
                            }),
                        },
                        "as_string" | "as_str" => match &object_eval.ty {
                            Some(t)
                                if matches!(t, TypeKind::Str | TypeKind::Ref { .. })
                                    || matches!(t, TypeKind::Named { name, .. } if name == "String") =>
                            {
                                Some(ref_str.clone())
                            }
                            Some(other) => {
                                self.push_error(expr.span, "S06",
                                        format!("as_str() not valid for {} — use to_str() to convert primitives to string", other));
                                None
                            }
                            None => None,
                        },
                        "parse" => type_args.first().map(|t| t.node.clone()),
                        "abs" => match &object_eval.ty {
                            Some(t) if Self::is_integer(t) || Self::is_float(t) => {
                                object_eval.ty.clone()
                            }
                            _ => None,
                        },
                        "min" | "max" => match &object_eval.ty {
                            Some(t) if Self::is_integer(t) || Self::is_float(t) => {
                                object_eval.ty.clone()
                            }
                            _ => None,
                        },
                        "sqrt" | "floor" | "ceil" | "round" => match &object_eval.ty {
                            Some(t) if Self::is_float(t) => object_eval.ty.clone(),
                            _ => None,
                        },
                        "concat" => match &object_eval.ty {
                            Some(t)
                                if matches!(t, TypeKind::Str | TypeKind::Ref { .. }) =>
                            {
                                Some(ref_str.clone())
                            }
                            _ => None,
                        },
                        _ => None,
                    };
                    ExprEval {
                        ty: builtin_ty,
                        const_value: None,
                    }
                }
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
                    self.push_error(expr.span, "S09", "match expression has no arms".to_string());
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
                                variadic: false,
                                attributes: Vec::new(),
                                public: false,
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
                                    "S01",
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
                let ty =
                    self.infer_binary_type(expr.span, &bin_op, &target_eval.ty, &value_eval.ty);
                ExprEval {
                    ty,
                    const_value: None,
                }
            }
            ExprKind::IncDec { expr: inner, .. } => {
                self.analyze_assign_target(inner);
                let inner_eval = self.type_check_expr(inner, reachable);

                if let Some(ty) = &inner_eval.ty {
                    if matches!(
                        ty,
                        TypeKind::Str
                            | TypeKind::Bool
                            | TypeKind::Void
                            | TypeKind::Ref { .. }
                            | TypeKind::RawPtr { .. }
                    ) {
                        self.push_error(inner.span, "S06", format!("++ / -- not valid for {}", ty));
                    }
                }
                ExprEval {
                    ty: inner_eval.ty,
                    const_value: None,
                }
            }
            ExprKind::ArrayLit(elems) => {
                let mut elem_ty: Option<TypeKind> = None;
                for elem in elems {
                    let eval = self.type_check_expr(elem, reachable);
                    if let (Some(expected), Some(got)) = (&elem_ty, &eval.ty) {
                        if !self.types_compatible(expected, got) {
                            self.push_error(
                                elem.span,
                                "S01",
                                format!(
                                    "array element type mismatch: expected {}, got {}",
                                    expected, got
                                ),
                            );
                        }
                    } else if elem_ty.is_none() {
                        elem_ty = eval.ty;
                    }
                }
                let len = elems.len() as u64;
                let elem_spanned = elem_ty
                    .clone()
                    .map(|k| crate::parser::ast::Spanned::new(k, expr.span));
                let ty = elem_spanned.map(|e| TypeKind::Array {
                    elem_ty: Box::new(e),
                    len,
                });
                ExprEval {
                    ty,
                    const_value: None,
                }
            }
            ExprKind::Index { object, index } => {
                let obj_eval = self.type_check_expr(object, reachable);
                let idx_eval = self.type_check_expr(index, reachable);

                if let Some(idx_ty) = &idx_eval.ty {
                    if !matches!(
                        idx_ty,
                        TypeKind::Int8
                            | TypeKind::Int16
                            | TypeKind::Int32
                            | TypeKind::Int64
                            | TypeKind::Uint8
                            | TypeKind::Uint16
                            | TypeKind::Uint32
                            | TypeKind::Uint64
                            | TypeKind::Isize
                            | TypeKind::Usize
                            | TypeKind::Any
                    ) {
                        self.push_error(
                            index.span,
                            "S06",
                            format!("array index must be an integer, got {}", idx_ty),
                        );
                    }
                }

                let elem_ty = match &obj_eval.ty {
                    Some(TypeKind::Array { elem_ty, .. }) => Some(elem_ty.node.clone()),
                    Some(TypeKind::Slice { elem_ty }) => Some(elem_ty.node.clone()),
                    _ => None,
                };
                ExprEval {
                    ty: elem_ty,
                    const_value: None,
                }
            }
        };

        self.annotate_expr(expr, &result, reachable);
        result
    }

    fn is_module_import_receiver(&self, object: &Expr) -> bool {
        let Some((base, _)) = Self::extract_field_chain(object) else {
            return false;
        };
        self.resolve_symbol(&base)
            .map_or(false, |sym| sym.is_import)
    }

    fn check_function_call(
        &mut self,
        name: &str,
        callee_span: Span,
        args: &[Expr],
        arg_evals: &[ExprEval],
        sym: &Symbol,
    ) -> ExprEval {
        let from = self
            .current_function
            .last()
            .cloned()
            .unwrap_or_else(|| "__program__".to_string());
        *self.call_counts.entry(name.to_string()).or_insert(0) += 1;
        self.add_dependency_edge(DependencyKind::Call, &from, name);

        let is_variadic = sym.variadic;
        let non_variadic_count = if is_variadic {
            sym.params.len().saturating_sub(1)
        } else {
            sym.params.len()
        };
        if !is_variadic && sym.params.len() != args.len() {
            self.push_error(
                callee_span,
                "S08",
                format!("expected {} args, got {}", sym.params.len(), args.len()),
            );
        } else if is_variadic && args.len() < non_variadic_count {
            self.push_error(
                callee_span,
                "S08",
                format!(
                    "expected at least {} args, got {}",
                    non_variadic_count,
                    args.len()
                ),
            );
        } else {
            // Type-check non-variadic args.
            for (i, (param_ty, arg_ty)) in sym.params[..non_variadic_count]
                .iter()
                .zip(arg_evals.iter().map(|e| &e.ty))
                .enumerate()
            {
                if let Some(at) = arg_ty {
                    if !self.types_compatible(param_ty, at) {
                        self.push_error(
                            args[i].span,
                            "S08",
                            format!("arg {}: expected {}, got {}", i + 1, param_ty, at),
                        );
                    }
                }
            }
            // Variadic args checked against the element type.
            if is_variadic {
                if let Some(elem_ty) = sym.params.last() {
                    for (i, arg_ty) in arg_evals[non_variadic_count..]
                        .iter()
                        .map(|e| &e.ty)
                        .enumerate()
                    {
                        if let Some(at) = arg_ty {
                            if !self.types_compatible(elem_ty, at) {
                                self.push_error(
                                    args[non_variadic_count + i].span,
                                    "S08",
                                    format!(
                                        "variadic arg {}: expected {}, got {}",
                                        i + 1,
                                        elem_ty,
                                        at
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }

        ExprEval {
            ty: sym.ty.clone(),
            const_value: None,
        }
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
                            "S01",
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
                            "S01",
                            format!("type mismatch in binary op: {} vs {}", l, r),
                        );
                    }
                }
                Some(TypeKind::Bool)
            }
            BinOpKind::AndAnd | BinOpKind::OrOr => {
                if let Some(l) = left {
                    if !matches!(l, TypeKind::Bool | TypeKind::Any) {
                        self.push_error(
                            span,
                            "S06",
                            format!("logical op requires bool, got {}", l),
                        );
                    }
                }

                if let Some(r) = right {
                    if !matches!(r, TypeKind::Bool | TypeKind::Any) {
                        self.push_error(
                            span,
                            "S06",
                            format!("logical op requires bool, got {}", r),
                        );
                    }
                }

                Some(TypeKind::Bool)
            }
            BinOpKind::Pow => match (left, right) {
                (Some(l), Some(r)) if self.types_compatible(l, r) => Some(l.clone()),
                (Some(l), _) => Some(l.clone()),
                (None, Some(r)) => Some(r.clone()),
                (None, None) => None,
            },
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

    pub(super) fn const_from_binary(
        op: &BinOpKind,
        left: &ConstValue,
        right: &ConstValue,
    ) -> Option<ConstValue> {
        match (op, left, right) {
            (BinOpKind::Add, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(a + b))
            }
            (BinOpKind::Sub, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(a - b))
            }
            (BinOpKind::Mul, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(a * b))
            }
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

            (BinOpKind::Lt, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Bool(a < b))
            }
            (BinOpKind::Gt, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Bool(a > b))
            }
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
                            "S09",
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
                                    "S09",
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
                                "S04",
                                format!(
                                    "unknown variant '{}.{}' in match pattern",
                                    target_enum, variant
                                ),
                            );
                        }
                    } else {
                        self.push_error(
                            pattern.span,
                            "S04",
                            format!("unknown enum '{}' in match pattern", target_enum),
                        );
                    }
                } else if scrutinee_ty.is_some() {
                    self.push_error(
                        pattern.span,
                        "S09",
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
                None => {
                    self.push_error(target.span, "S04", format!("unknown identifier '{}'", name))
                }
                Some(sym) => match sym.kind {
                    SymbolKind::Variable { mutable: false } => {
                        self.push_error(
                            target.span,
                            "S07",
                            format!("cannot assign to const '{}'", name),
                        );
                    }
                    SymbolKind::Function | SymbolKind::TypeName => {
                        self.push_error(target.span, "S07", format!("cannot assign to '{}'", name));
                    }
                    SymbolKind::Parameter | SymbolKind::Variable { mutable: true } => {}
                },
            },
            ExprKind::Field { object, .. } => {
                self.type_check_expr(object, true);
            }
            ExprKind::Unary {
                op: UnaryOpKind::Deref,
                expr: inner,
            } => {
                let inner_eval = self.type_check_expr(inner, true);
                match &inner_eval.ty {
                    Some(TypeKind::RawPtr { .. }) => {
                        if self.unsafe_depth == 0 {
                            self.push_error(
                                target.span,
                                "S11",
                                "store through raw pointer requires unsafe block".to_string(),
                            );
                        }
                    }
                    Some(TypeKind::Ref { .. }) | None => {}
                    Some(other) => {
                        self.push_error(
                            target.span,
                            "S11",
                            format!("cannot assign through non-pointer type {}", other),
                        );
                    }
                }
            }
            ExprKind::Index { object, index } => {
                self.type_check_expr(object, true);
                self.type_check_expr(index, true);
            }
            _ => {
                self.type_check_expr(target, true);
                self.push_error(target.span, "S07", "invalid assignment target".to_string());
            }
        }
    }

    pub(super) fn types_compatible(&self, a: &TypeKind, b: &TypeKind) -> bool {
        match (a, b) {
            (TypeKind::Any, _) | (_, TypeKind::Any) => true,
            (TypeKind::Named { .. }, _) | (_, TypeKind::Named { .. }) => true,
            (a, b) if Self::is_integer(a) && Self::is_integer(b) => true,
            (a, b) if Self::is_float(a) && Self::is_float(b) => true,
            (
                TypeKind::Array {
                    elem_ty: a_e,
                    len: a_l,
                },
                TypeKind::Array {
                    elem_ty: b_e,
                    len: b_l,
                },
            ) => a_l == b_l && self.types_compatible(&a_e.node, &b_e.node),
            (TypeKind::Slice { elem_ty: a_e }, TypeKind::Slice { elem_ty: b_e }) => {
                self.types_compatible(&a_e.node, &b_e.node)
            }
            (TypeKind::Array { elem_ty, .. }, TypeKind::Slice { elem_ty: s_e })
            | (TypeKind::Slice { elem_ty: s_e }, TypeKind::Array { elem_ty, .. }) => {
                self.types_compatible(&elem_ty.node, &s_e.node)
            }
            (TypeKind::Ref { inner: a }, TypeKind::Ref { inner: b }) => {
                self.types_compatible(&a.node, &b.node)
            }
            (TypeKind::RawPtr { inner: a }, TypeKind::RawPtr { inner: b }) => {
                self.types_compatible(&a.node, &b.node)
            }
            (TypeKind::RawPtr { .. }, TypeKind::Ref { .. })
            | (TypeKind::Ref { .. }, TypeKind::RawPtr { .. }) => true,
            // str and &str are interchangeable — both are UTF-8 string views
            (TypeKind::Str, TypeKind::Ref { inner }) | (TypeKind::Ref { inner }, TypeKind::Str)
                if matches!(inner.node, TypeKind::Str) =>
            {
                true
            }
            _ => std::mem::discriminant(a) == std::mem::discriminant(b),
        }
    }

    pub(super) fn is_integer(t: &TypeKind) -> bool {
        matches!(
            t,
            TypeKind::Int8
                | TypeKind::Int16
                | TypeKind::Int32
                | TypeKind::Int64
                | TypeKind::Uint8
                | TypeKind::Uint16
                | TypeKind::Uint32
                | TypeKind::Uint64
                | TypeKind::Isize
                | TypeKind::Usize
        )
    }

    pub(super) fn is_float(t: &TypeKind) -> bool {
        matches!(t, TypeKind::Float16 | TypeKind::Float32 | TypeKind::Float64)
    }
}
