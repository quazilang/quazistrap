// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use crate::parser::ast::*;

use super::*;

impl Analyzer {
    pub(super) fn type_check_item(&mut self, item: &Item) {
        // Skip items disabled by @cfg on this platform.
        let attrs = match &item.node {
            ItemKind::TypeAlias { attributes, .. }
            | ItemKind::Fn { attributes, .. }
            | ItemKind::Struct { attributes, .. }
            | ItemKind::Trait { attributes, .. }
            | ItemKind::Enum { attributes, .. }
            | ItemKind::ForeignGlobal { attributes, .. } => Some(attributes),
            _ => None,
        };
        if let Some(attrs) = attrs
            && !super::item_should_include(attrs)
        {
            return;
        }
        match &item.node {
            ItemKind::Fn {
                name,
                generic_params,
                params,
                return_ty,
                body,
                attributes,
                unsafe_fn,
                pub_fn,
                c_variadic,
                ..
            } => {
                self.validate_foreign_attributes(
                    name,
                    generic_params,
                    params,
                    return_ty,
                    body,
                    attributes,
                    *pub_fn,
                    *c_variadic,
                    item.span,
                );
                let is_foreign = attributes
                    .iter()
                    .any(|a| a.name == "syscall" || a.name == "api" || a.name == "intrinsic");
                // Functions with raw pointer params/return must be declared unsafe fn.
                // @syscall/@api functions are exempt — they are implicitly unsafe via Symbol.unsafe_fn.
                if !unsafe_fn && !is_foreign {
                    let has_raw_ptr = params.iter().any(|p| type_contains_rawptr(&p.ty.node))
                        || type_contains_rawptr(&return_ty.node);
                    if has_raw_ptr {
                        self.push_error(
                            item.span,
                            "S12",
                            format!(
                                "function `{}` with raw pointer types must be declared `unsafe fn`",
                                name
                            ),
                        );
                    }
                }
                // W05: warn when `any` appears in a non-variadic param or return type outside
                // a trait definition (trait methods use `any` as a generic placeholder).
                if self.trait_depth == 0 && !is_foreign {
                    let attr_names = extract_attribute_names(attributes);
                    if !attr_names.contains(&"ignore".to_string()) {
                        for p in params {
                            if !p.variadic && type_contains_any(&p.ty.node) {
                                self.push_warning_with_suggestion(
                                    p.ty.span,
                                    "W05",
                                    format!(
                                        "parameter '{}' has type `any` — consider using a concrete type or generic",
                                        p.name
                                    ),
                                    "replace `any` with a specific type, a generic parameter, or a trait bound".to_string(),
                                );
                            }
                        }
                        if type_contains_any(&return_ty.node) {
                            self.push_warning_with_suggestion(
                                return_ty.span,
                                "W05",
                                format!(
                                    "function `{}` returns `any` — consider using a concrete type or generic",
                                    name
                                ),
                                "replace `any` with a specific return type or generic".to_string(),
                            );
                        }
                    }
                }
                if *unsafe_fn {
                    self.unsafe_depth += 1;
                }
                let fn_name = {
                    let base = self
                        .current_fn_name_override
                        .take()
                        .unwrap_or_else(|| name.clone());
                    // Impl methods already have their mangled name ("TypeName.method") via
                    // the override; don't add a module prefix. Runtime __quazi_* symbols and
                    // ordinary top-level fns in namespaced files get the module prefix to
                    // match the declare pass.
                    if base.contains('.') || base.starts_with("__quazi_") {
                        base
                    } else if let Some(module) = self.module_path_for_span(item.span) {
                        format!("{}.{}", module, base)
                    } else {
                        base
                    }
                };
                let prev_module_path = self.current_module_path.clone();
                self.current_module_path = self.module_path_for_span(item.span);
                self.current_function.push(fn_name);
                self.enter_scope();
                let fn_is_str_variadic = params
                    .last()
                    .map(|p| {
                        p.variadic && matches!(&p.ty.node, TypeKind::Str | TypeKind::Ref { .. })
                    })
                    .unwrap_or(false);
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
                    // str_variadic functions: variadic args are coerced at call sites, not in body
                    if p.variadic && fn_is_str_variadic {
                        param_attrs.push("str_variadic".to_string());
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
                            unsafe_fn: false,
                            generic_params: vec![],
                        },
                    );
                }

                let expected = unwrap_type(return_ty);

                if name == "main" {
                    if !matches!(
                        expected,
                        TypeKind::Void | TypeKind::Int32 | TypeKind::Uint32 | TypeKind::Never
                    ) {
                        self.push_error(
                            item.span,
                            "S01",
                            format!(
                                "main() return type must be void, i32, u32, or !, got {}",
                                expected
                            ),
                        );
                    }

                    let valid_args = match params.as_slice() {
                        [] => true,
                        [p] => {
                            let ty = unwrap_type(&p.ty);
                            matches!(
                                ty,
                                TypeKind::Named {
                                    name,
                                    type_args,
                                    ..
                                } if name == "Array"
                                    && type_args.len() == 1
                                    && matches!(
                                        type_args[0].node,
                                        TypeKind::Str | TypeKind::Ref { .. }
                                    )
                            )
                        }
                        _ => false,
                    };
                    if valid_args && params.len() == 1 {
                        self.main_takes_args = true;
                    } else if !valid_args {
                        self.push_error(
                            item.span,
                            "S01",
                            "main() must take either no parameters or a single `Array[str]` parameter".to_string(),
                        );
                    }
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
                self.current_module_path = prev_module_path;
                if *unsafe_fn {
                    self.unsafe_depth -= 1;
                }
            }
            ItemKind::Struct {
                name,
                generic_params,
                fields,
                bit_widths,
                is_union,
                attributes,
                ..
            } => {
                if attributes.iter().any(|attr| attr.name == "opaque")
                    && (!fields.is_empty() || !generic_params.is_empty())
                {
                    self.push_error(
                        item.span,
                        "S14",
                        format!("@opaque type `{name}` must be a non-generic empty struct"),
                    );
                }
                if let Some(repr) = attributes.iter().find(|attr| attr.name == "repr") {
                    let valid = matches!(
                        repr.args.first(),
                        Some(AttrArg::Positional(AttrVal::Ident(value))) if value == "C"
                    ) && repr.args.iter().skip(1).all(|arg| match arg {
                        AttrArg::Positional(AttrVal::Ident(value)) => value == "packed",
                        AttrArg::KeyValue(key, AttrVal::Int(value)) => {
                            key == "align"
                                && *value > 0
                                && (*value as usize).is_power_of_two()
                                && *value <= 4096
                        }
                        _ => false,
                    });
                    if !valid {
                        self.push_error(
                            repr.span,
                            "S14",
                            "unsupported representation; use @repr(C), @repr(C, packed), or @repr(C, align=N) with a power-of-two N up to 4096".to_string(),
                        );
                    } else {
                        if !generic_params.is_empty() {
                            self.push_error(
                                item.span,
                                "S14",
                                format!("@repr(C) aggregate `{name}` cannot be generic yet"),
                            );
                        }
                        if fields.is_empty() {
                            self.push_error(
                                item.span,
                                "S14",
                                format!(
                                    "@repr(C) aggregate `{name}` cannot be empty because C has no portable empty-aggregate layout"
                                ),
                            );
                        }
                        for (index, ((field_name, field_ty, _), bit_width)) in
                            fields.iter().zip(bit_widths).enumerate()
                        {
                            let resolved = self.resolve_type_aliases(&field_ty.node);
                            let field_supported = ffi_aggregate_field(&resolved);
                            if !field_supported {
                                self.push_error(
                                    field_ty.span,
                                    "S14",
                                    format!(
                                        "@repr(C) field `{field_name}` in `{name}` must be a C scalar or final flexible array member"
                                    ),
                                );
                            }
                            if let Some(width) = bit_width {
                                let Some(storage_bits) = ffi_integer_bits(&resolved) else {
                                    self.push_error(
                                        field_ty.span,
                                        "S14",
                                        format!("bitfield `{field_name}` in `{name}` must use an integer type"),
                                    );
                                    continue;
                                };
                                if usize::from(*width) > storage_bits {
                                    self.push_error(
                                        field_ty.span,
                                        "S14",
                                        format!(
                                            "bitfield `{field_name}` width {} exceeds its {}-bit storage type",
                                            width, storage_bits
                                        ),
                                    );
                                }
                            }
                            if matches!(resolved, TypeKind::FlexibleArray { .. }) {
                                if *is_union {
                                    self.push_error(
                                        field_ty.span,
                                        "S14",
                                        "a union cannot contain a flexible array member"
                                            .to_string(),
                                    );
                                }
                                if index + 1 != fields.len() {
                                    self.push_error(
                                        field_ty.span,
                                        "S14",
                                        format!("flexible array member `{field_name}` must be the final field"),
                                    );
                                }
                                if bit_width.is_some() {
                                    self.push_error(
                                        field_ty.span,
                                        "S14",
                                        "a flexible array member cannot be a bitfield".to_string(),
                                    );
                                }
                            }
                        }
                    }
                } else if *is_union {
                    self.push_error(
                        item.span,
                        "S14",
                        format!("union `{name}` requires @repr(C)"),
                    );
                }
            }
            ItemKind::ForeignGlobal {
                name,
                ty,
                attributes,
                ..
            } => {
                let api_attributes: Vec<&Attribute> = attributes
                    .iter()
                    .filter(|attribute| attribute.name == "api")
                    .collect();
                if api_attributes.len() != 1 {
                    self.push_error(
                        item.span,
                        "S14",
                        format!("foreign global `{name}` requires exactly one @api attribute"),
                    );
                } else {
                    let api = api_attributes[0];
                    let valid_args = matches!(
                        api.args.as_slice(),
                        [] | [AttrArg::Positional(AttrVal::Str(_))]
                    );
                    if !valid_args {
                        self.push_error(
                            api.span,
                            "S14",
                            "@api on a foreign global accepts no argument or one string symbol"
                                .to_string(),
                        );
                    }
                }
                if attributes.iter().any(|attribute| {
                    matches!(attribute.name.as_str(), "export" | "syscall" | "intrinsic")
                }) {
                    self.push_error(
                        item.span,
                        "S14",
                        format!("foreign global `{name}` only supports @api and @cfg attributes"),
                    );
                }
                let resolved = self.resolve_type_aliases(&ty.node);
                if !ffi_primitive(&resolved) || matches!(resolved, TypeKind::Void) {
                    self.push_error(
                        ty.span,
                        "S14",
                        format!(
                            "foreign global `{name}` must use a C scalar, pointer, or C function-pointer type"
                        ),
                    );
                }
            }
            ItemKind::Enum { .. } | ItemKind::Import(_) => {}
            ItemKind::Trait { .. } => {
                // Trait method signatures are abstract — no body to type-check.
                // trait_depth is not incremented here because TraitMethod never
                // reaches ItemKind::Fn; W05 is naturally exempt for them.
            }
            ItemKind::Impl {
                for_ty, methods, ..
            } => {
                let type_name = crate::semantic::declare::type_kind_base_name(&for_ty.node);
                for method in methods {
                    if let ItemKind::Fn { name, .. } = &method.node {
                        self.current_fn_name_override = Some(format!("{}.{}", type_name, name));
                    }
                    self.type_check_item(method);
                }
            }
            // Type aliases are just name bindings — no code to type-check.
            ItemKind::TypeAlias {
                name,
                generic_params,
                aliased_type,
                attributes,
                ..
            } => {
                if let Some(repr) = attributes.iter().find(|attribute| attribute.name == "repr") {
                    let is_c = matches!(
                        repr.args.as_slice(),
                        [AttrArg::Positional(AttrVal::Ident(value))] if value == "C"
                    );
                    if !is_c || !matches!(aliased_type.node, TypeKind::Fn { .. }) {
                        self.push_error(
                            repr.span,
                            "S14",
                            "@repr(C) on a type alias requires `type Name = fn(...) Return`"
                                .to_string(),
                        );
                    }
                    if !generic_params.is_empty() {
                        self.push_error(
                            item.span,
                            "S14",
                            format!("C function pointer alias `{name}` cannot be generic"),
                        );
                    }
                    if let TypeKind::Fn { params, return_ty } = &aliased_type.node {
                        for param in params {
                            let resolved = self.resolve_type_aliases(&param.node);
                            let supported = ffi_primitive(&resolved)
                                || matches!(
                                    &resolved,
                                    TypeKind::Named { name, type_args }
                                        if type_args.is_empty()
                                            && self.repr_c_structs.contains(name)
                                            && !self.flexible_array_structs.contains(name)
                                );
                            if !supported || matches!(resolved, TypeKind::Void) {
                                self.push_error(
                                    param.span,
                                    "S14",
                                    format!(
                                        "C function pointer `{name}` has unsupported parameter type `{}`",
                                        param.node
                                    ),
                                );
                            }
                        }
                        let resolved = self.resolve_type_aliases(&return_ty.node);
                        let supported = ffi_primitive(&resolved)
                            || matches!(
                                &resolved,
                                TypeKind::Named { name, type_args }
                                    if type_args.is_empty()
                                        && self.repr_c_structs.contains(name)
                                        && !self.flexible_array_structs.contains(name)
                            );
                        if !supported {
                            self.push_error(
                                return_ty.span,
                                "S14",
                                format!(
                                    "C function pointer `{name}` has unsupported return type `{}`",
                                    return_ty.node
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_foreign_attributes(
        &mut self,
        name: &str,
        generic_params: &[String],
        params: &[Param],
        return_ty: &Type,
        body: &Option<Block>,
        attributes: &[Attribute],
        is_public: bool,
        c_variadic: bool,
        item_span: Span,
    ) {
        let mut syscall_attr: Option<&Attribute> = None;
        let mut api_attr: Option<&Attribute> = None;
        let mut export_attr: Option<&Attribute> = None;

        for attr in attributes {
            match attr.name.as_str() {
                "syscall" => syscall_attr = Some(attr),
                "api" => api_attr = Some(attr),
                "export" => export_attr = Some(attr),
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
            if body.is_some() {
                self.push_error(
                    item_span,
                    "S06",
                    format!("@api function `{name}` must be a bodyless declaration ending in `;`"),
                );
            }
            self.validate_ffi_signature(
                name,
                generic_params,
                params,
                return_ty,
                item_span,
                c_variadic,
            );
        }

        if let Some(attr) = export_attr {
            self.validate_export_attr(attr);
            if api_attr.is_some() || syscall_attr.is_some() {
                self.push_error(
                    attr.span,
                    "S06",
                    "@export cannot be combined with @api or @syscall".to_string(),
                );
            }
            if body.is_none() {
                self.push_error(
                    item_span,
                    "S06",
                    format!("@export function `{name}` must have a Quazi body"),
                );
            }
            if !is_public {
                self.push_error(
                    item_span,
                    "S06",
                    format!("@export function `{name}` must be declared `pub`"),
                );
            }
            self.validate_ffi_signature(name, generic_params, params, return_ty, item_span, false);
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
        let ok = matches!(
            attr.args.as_slice(),
            [] | [AttrArg::Positional(AttrVal::Str(_))]
        );
        if !ok {
            self.push_error(
                attr.span,
                "S06",
                "invalid @api attribute (use @api or @api(\"symbol\"))".to_string(),
            );
        }
    }

    fn validate_export_attr(&mut self, attr: &Attribute) {
        let ok = match attr.args.as_slice() {
            [] => true,
            [AttrArg::Positional(AttrVal::Str(symbol))] => !symbol.is_empty(),
            _ => false,
        };
        if !ok {
            self.push_error(
                attr.span,
                "S06",
                "invalid @export attribute (use @export or @export(\"symbol\"))".to_string(),
            );
        }
    }

    fn validate_ffi_signature(
        &mut self,
        name: &str,
        generic_params: &[String],
        params: &[Param],
        return_ty: &Type,
        span: Span,
        c_variadic: bool,
    ) {
        if !generic_params.is_empty() {
            self.push_error(
                span,
                "S14",
                format!("FFI function `{name}` cannot be generic"),
            );
        }
        if params.iter().any(|p| p.variadic) {
            self.push_error(
                span,
                "S14",
                format!("Quazi-style variadics are not supported in FFI function `{name}`; use bare `...` for C variadics"),
            );
        }
        for param in params {
            let resolved = self.resolve_type_aliases(&param.ty.node);
            let supported = ffi_primitive(&resolved)
                || matches!(
                    &resolved,
                    TypeKind::Named { name, type_args }
                        if type_args.is_empty()
                            && self.repr_c_structs.contains(name)
                            && !self.flexible_array_structs.contains(name)
                );
            if !supported || matches!(resolved, TypeKind::Void) {
                self.push_error(
                    param.ty.span,
                    "S14",
                    format!(
                        "FFI parameter `{}` in `{name}` has unsupported C ABI type `{}`",
                        param.name, param.ty.node
                    ),
                );
            }
        }
        let resolved_return = self.resolve_type_aliases(&return_ty.node);
        let return_supported = ffi_primitive(&resolved_return)
            || matches!(
                &resolved_return,
                TypeKind::Named { name, type_args }
                    if type_args.is_empty()
                        && self.repr_c_structs.contains(name)
                        && !self.flexible_array_structs.contains(name)
            );
        if !return_supported {
            self.push_error(
                return_ty.span,
                "S14",
                format!(
                    "FFI function `{name}` has unsupported C ABI return type `{}`",
                    return_ty.node
                ),
            );
        }
        let _ = c_variadic;
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
                    if let Some(v) = value.as_ref() {
                        if !self.check_expr_compat(v, ann, val) {
                            self.push_error(
                                stmt.span,
                                "S01",
                                format!("type mismatch: declared {}, got {}", ann, val),
                            );
                        }
                    } else if !self.types_compatible(ann, val) {
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
                        unsafe_fn: false,
                        generic_params: vec![],
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

                if let (Some(ann), Some(val)) = (&declared_ty, &value_eval.ty)
                    && !self.check_expr_compat(value, ann, val)
                {
                    self.push_error(
                        stmt.span,
                        "S01",
                        format!("type mismatch: declared {}, got {}", ann, val),
                    );
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
                        unsafe_fn: false,
                        generic_params: vec![],
                    },
                );
                false
            }
            StmtKind::Return(expr) => {
                match (expected_return, expr) {
                    (Some(expected), Some(return_expr)) => {
                        let actual = self.type_check_expr(return_expr, true).ty;
                        if let Some(actual) = actual
                            && !self.check_expr_compat(return_expr, expected, &actual)
                        {
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
                else_if,
                else_block,
            } => {
                let condition_eval = self.type_check_expr(condition, true);
                if let Some(condition_ty) = condition_eval.ty
                    && !matches!(condition_ty, TypeKind::Bool | TypeKind::Any)
                    && !Self::is_integer(&condition_ty)
                {
                    self.push_error(
                        condition.span,
                        "S01",
                        format!("if condition must be bool or integer, got {}", condition_ty),
                    );
                }

                let mut then_returns = self.type_check_block(then_block, expected_return);
                for (else_if_cond, else_if_block) in else_if {
                    let else_if_eval = self.type_check_expr(else_if_cond, true);
                    if let Some(ty) = else_if_eval.ty
                        && !matches!(ty, TypeKind::Bool | TypeKind::Any)
                        && !Self::is_integer(&ty)
                    {
                        self.push_error(
                            else_if_cond.span,
                            "S01",
                            format!("if condition must be bool or integer, got {}", ty),
                        );
                    }
                    then_returns =
                        self.type_check_block(else_if_block, expected_return) && then_returns;
                }
                let else_returns = if let Some(else_block) = else_block {
                    self.type_check_block(else_block, expected_return)
                } else {
                    false
                };

                then_returns && else_returns
            }
            StmtKind::For { kind, body } => {
                self.enter_scope();
                match kind {
                    ForLoop::Cond {
                        condition: Some(cond),
                    } => {
                        let cond_eval = self.type_check_expr(cond, true);
                        if let Some(cond_ty) = cond_eval.ty
                            && !matches!(cond_ty, TypeKind::Bool | TypeKind::Any)
                            && !Self::is_integer(&cond_ty)
                        {
                            self.push_error(
                                cond.span,
                                "S01",
                                format!("for condition must be bool or integer, got {}", cond_ty),
                            );
                        }
                        self.loop_depth += 1;
                        let _ = self.type_check_block(body, expected_return);
                        self.loop_depth -= 1;
                        self.exit_scope_collect();
                        return false;
                    }
                    ForLoop::Cond { condition: None } => {
                        self.loop_depth += 1;
                        let _ = self.type_check_block(body, expected_return);
                        self.loop_depth -= 1;
                        self.exit_scope_collect();
                        return false;
                    }
                    ForLoop::CStyle {
                        init,
                        condition,
                        update,
                    } => {
                        if let Some(init_stmt) = init {
                            self.type_check_stmt(init_stmt, expected_return);
                        }
                        if let Some(cond) = condition {
                            let cond_eval = self.type_check_expr(cond, true);
                            if let Some(cond_ty) = cond_eval.ty
                                && !matches!(cond_ty, TypeKind::Bool | TypeKind::Any)
                                && !Self::is_integer(&cond_ty)
                            {
                                self.push_error(
                                    cond.span,
                                    "S01",
                                    format!(
                                        "for condition must be bool or integer, got {}",
                                        cond_ty
                                    ),
                                );
                            }
                        }
                        if let Some(upd) = update {
                            self.type_check_expr(upd, true);
                        }
                        self.loop_depth += 1;
                        self.type_check_block(body, expected_return);
                        self.loop_depth -= 1;
                        self.exit_scope_collect();
                        return false;
                    }
                    ForLoop::Each { vars, iter } => {
                        let loop_var_ty = match iter {
                            ForIter::Range { start, end } => {
                                let start_eval = self.type_check_expr(start, true);
                                let end_eval = self.type_check_expr(end, true);
                                if let Some(t) = &start_eval.ty
                                    && !Self::is_integer(t)
                                {
                                    self.push_error(
                                        start.span,
                                        "S01",
                                        format!(
                                            "for range start must be an integer type, got {}",
                                            t
                                        ),
                                    );
                                }
                                if let Some(t) = &end_eval.ty
                                    && !Self::is_integer(t)
                                {
                                    self.push_error(
                                        end.span,
                                        "S01",
                                        format!("for range end must be an integer type, got {}", t),
                                    );
                                }
                                start_eval.ty.or(end_eval.ty).unwrap_or(TypeKind::Int32)
                            }
                            ForIter::Iter(expr) => {
                                let iter_eval = self.type_check_expr(expr, true);
                                let iter_ty = iter_eval.ty.as_ref().map(|t| {
                                    if let TypeKind::Ref { inner } = t {
                                        &inner.node
                                    } else {
                                        t
                                    }
                                });
                                match iter_ty {
                                    Some(TypeKind::Array { elem_ty, .. }) => elem_ty.node.clone(),
                                    Some(TypeKind::Slice { elem_ty }) => elem_ty.node.clone(),
                                    Some(t) => {
                                        self.push_error(
                                            expr.span,
                                            "S01",
                                            format!(
                                                "cannot iterate over type '{}'; expected array or slice",
                                                t
                                            ),
                                        );
                                        TypeKind::Any
                                    }
                                    None => TypeKind::Any,
                                }
                            }
                        };
                        if let Some(name) = vars.first() {
                            self.declare(
                                name.clone(),
                                Symbol {
                                    kind: SymbolKind::Variable { mutable: true },
                                    ty: Some(loop_var_ty.clone()),
                                    span: stmt.span,
                                    params: vec![],
                                    used: false,
                                    initialized: true,
                                    is_import: false,
                                    import_path: None,
                                    const_value: None,
                                    variadic: false,
                                    attributes: Vec::new(),
                                    public: false,
                                    unsafe_fn: false,
                                    generic_params: vec![],
                                },
                            );
                        }
                        if let Some(name) = vars.get(1) {
                            self.declare(
                                name.clone(),
                                Symbol {
                                    kind: SymbolKind::Variable { mutable: true },
                                    ty: Some(TypeKind::Usize),
                                    span: stmt.span,
                                    params: vec![],
                                    used: false,
                                    initialized: true,
                                    is_import: false,
                                    import_path: None,
                                    const_value: None,
                                    variadic: false,
                                    attributes: Vec::new(),
                                    public: false,
                                    unsafe_fn: false,
                                    generic_params: vec![],
                                },
                            );
                        }
                        self.loop_depth += 1;
                        self.type_check_block(body, expected_return);
                        self.loop_depth -= 1;
                        self.exit_scope_collect();
                        return false;
                    }
                }
            }
            StmtKind::Break => {
                if self.loop_depth == 0 {
                    self.push_error(stmt.span, "S11", "break outside of loop".to_string());
                }
                true
            }
            StmtKind::Continue => {
                if self.loop_depth == 0 {
                    self.push_error(stmt.span, "S11", "continue outside of loop".to_string());
                }
                true
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
            StmtKind::CfgBlock { body, condition } => {
                if super::item_should_include(std::slice::from_ref(condition)) {
                    self.type_check_block(body, expected_return);
                }
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
                    Literal::Bytes(_) => TypeKind::Bytes,
                    Literal::Bool(_) => TypeKind::Bool,
                };

                ExprEval {
                    ty: Some(ty),
                    const_value: Self::const_from_literal(lit),
                }
            }
            ExprKind::Ident(name) => {
                if let Some(resolved) = self.resolve_bare_foreign_global_name(name) {
                    if resolved != *name {
                        let _ = self.resolve_for_read(name);
                    }
                    let sym = self
                        .resolve_for_read(&resolved)
                        .expect("resolved foreign global should exist");
                    if self.unsafe_depth == 0 {
                        self.push_error(
                            expr.span,
                            "S11",
                            format!("reading foreign global `{name}` requires unsafe context"),
                        );
                    }
                    let eval = ExprEval {
                        ty: sym.ty,
                        const_value: None,
                    };
                    self.annotate_foreign_global_expr(expr, &eval, reachable, resolved);
                    return eval;
                }
                // Prefer the module-qualified resolution when in a namespaced module.
                if let Some(resolved) = self.resolve_bare_fn_name(name) {
                    let _ = self.resolve_for_read(name);
                    let sym = self
                        .resolve_for_read(&resolved)
                        .expect("resolved function should exist");
                    let return_ty = sym.ty.clone().unwrap_or(TypeKind::Void);
                    let param_types: Vec<Type> = sym
                        .params
                        .iter()
                        .map(|p| Spanned::new(p.clone(), expr.span))
                        .collect();
                    let from = self
                        .current_function
                        .last()
                        .cloned()
                        .unwrap_or_else(|| "__program__".to_string());
                    self.add_dependency_edge(DependencyKind::Call, &from, &resolved);
                    let eval = ExprEval {
                        ty: Some(TypeKind::Fn {
                            params: param_types,
                            return_ty: Box::new(Spanned::new(return_ty, expr.span)),
                        }),
                        const_value: None,
                    };
                    self.annotate_expr(expr, &eval, reachable, Some(resolved));
                    return eval;
                }

                match self.resolve_for_read(name) {
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
                        } else if matches!(sym.kind, SymbolKind::TypeName) {
                            // Type name used as expression (e.g. Array.new(), Array.from(...)).
                            // For type aliases, return the aliased type.
                            // For structs/enums/traits, return Named so static method dispatch works.
                            let resolved = sym.ty.clone().unwrap_or(TypeKind::Named {
                                name: name.clone(),
                                type_args: vec![],
                            });
                            ExprEval {
                                ty: Some(resolved),
                                const_value: None,
                            }
                        } else if matches!(sym.kind, SymbolKind::Function) {
                            // Function name used as value → fn pointer type.
                            let return_ty = sym.ty.clone().unwrap_or(TypeKind::Void);
                            let param_types: Vec<Type> = sym
                                .params
                                .iter()
                                .map(|p| Spanned::new(p.clone(), expr.span))
                                .collect();
                            // Ensure function is reachable even if never directly called.
                            let from = self
                                .current_function
                                .last()
                                .cloned()
                                .unwrap_or_else(|| "__program__".to_string());
                            self.add_dependency_edge(DependencyKind::Call, &from, name);
                            ExprEval {
                                ty: Some(TypeKind::Fn {
                                    params: param_types,
                                    return_ty: Box::new(Spanned::new(return_ty, expr.span)),
                                }),
                                const_value: None,
                            }
                        } else {
                            ExprEval {
                                ty: sym.ty,
                                const_value: sym.const_value,
                            }
                        }
                    }
                }
            }
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
                        self.annotate_expr(expr, &result, reachable, None);
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
                        self.annotate_expr(expr, &result, reachable, None);
                        return result;
                    }
                    UnaryOpKind::Not => {
                        if let Some(t) = &inner_eval.ty
                            && !matches!(t, TypeKind::Bool | TypeKind::Any)
                            && !Self::is_integer(t)
                        {
                            self.push_error(
                                inner.span,
                                "S06",
                                format!("! requires bool or integer, got {}", t),
                            );
                        }
                    }
                    UnaryOpKind::Neg => {
                        if let Some(t) = &inner_eval.ty
                            && matches!(
                                t,
                                TypeKind::Str
                                    | TypeKind::Bool
                                    | TypeKind::Ref { .. }
                                    | TypeKind::RawPtr { .. }
                            )
                        {
                            self.push_error(
                                inner.span,
                                "S06",
                                format!("unary - not valid for {}", t),
                            );
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
            ExprKind::Cast { expr: inner, ty } => {
                let inner_eval = self.type_check_expr(inner, reachable);
                let target_ty = self.resolve_type_aliases(&ty.node);
                let allowed = match inner_eval.ty.as_ref() {
                    Some(src) if Self::is_integer(src) && Self::is_integer(&target_ty) => true,
                    Some(src) if Self::is_float(src) && Self::is_float(&target_ty) => true,
                    Some(src)
                        if std::mem::discriminant(src) == std::mem::discriminant(&target_ty) =>
                    {
                        true
                    }
                    _ => false,
                };
                if !allowed {
                    let src_name = inner_eval
                        .ty
                        .as_ref()
                        .map(|t| format!("{}", t))
                        .unwrap_or_else(|| "<unknown>".to_string());
                    self.push_error(
                        expr.span,
                        "S06",
                        format!("invalid cast from {} to {}", src_name, target_ty),
                    );
                }
                ExprEval {
                    ty: Some(target_ty),
                    const_value: inner_eval.const_value,
                }
            }
            ExprKind::Binary { left, op, right } => {
                let left_eval = self.type_check_expr(left, reachable);
                let right_eval = self.type_check_expr(right, reachable);

                let ty = self.infer_binary_type(expr.span, op, &left_eval.ty, &right_eval.ty);
                self.mark_binary_autoderef(left, &left_eval.ty, right, &right_eval.ty);
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
                    let resolved = self
                        .resolve_bare_foreign_global_name(name)
                        .unwrap_or_else(|| name.clone());
                    if let Some(sym) = self.resolve_symbol(&resolved)
                        && let (Some(var_ty), Some(val_ty)) = (&sym.ty, &value_eval.ty)
                    {
                        if !self.check_expr_compat(value, var_ty, val_ty) {
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

                    self.mark_initialized(name);
                    self.set_symbol_const_value(name, value_eval.const_value.clone());
                }

                // Never propagate const_value from an assign expression — the assign
                // has a side effect that must not be eliminated by const-folding.
                ExprEval {
                    ty: value_eval.ty,
                    const_value: None,
                }
            }
            ExprKind::Call {
                callee,
                type_args,
                args,
                named_args,
            } => {
                let arg_evals: Vec<ExprEval> = args
                    .iter()
                    .chain(named_args.iter().map(|(_, e)| e))
                    .map(|a| self.type_check_expr(a, reachable))
                    .collect();

                if let ExprKind::Ident(name) = &callee.node {
                    // Resolve the name, taking into account namespacing for the current module.
                    let resolved_name = self.resolve_bare_fn_name(name);
                    let _ = self.resolve_for_read(name);

                    if let Some(resolved) = &resolved_name {
                        let sym = self
                            .resolve_for_read(resolved)
                            .expect("resolved function should exist");

                        // Library functions require explicit import-by-name to be called unqualified.
                        // Exception: library code calling other library code is always allowed.
                        let caller_is_library = self
                            .current_function
                            .last()
                            .map(|f| self.library_fn_names.contains(f.as_str()))
                            .unwrap_or(false);
                        if self.library_fn_names.contains(resolved.as_str())
                            && !self.explicitly_imported_fns.contains_key(name.as_str())
                            && !caller_is_library
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

                        if !named_args.is_empty() {
                            self.validate_named_args(
                                callee.span,
                                resolved,
                                named_args,
                                args.len(),
                                &sym,
                            );
                        }
                        let eval = self.check_function_call(
                            resolved,
                            callee.span,
                            args,
                            &arg_evals,
                            &sym,
                            type_args,
                        );
                        self.annotate_expr(expr, &eval, reachable, Some(resolved.clone()));
                        return eval;
                    }

                    // Not resolved as a function — fall through to generic symbol lookup
                    // for variables, parameters, function-pointer values, etc.
                    let Some(sym) = self.resolve_for_read(name) else {
                        self.push_error(
                            callee.span,
                            "S04",
                            format!("unknown identifier '{}'", name),
                        );
                        return ExprEval::default();
                    };

                    // Function pointer variable or other callable expression.
                    if let Some(ref fn_ty) = sym.ty
                        && let resolved @ (TypeKind::Fn { .. } | TypeKind::CFn { .. }) =
                            self.resolve_type_aliases(fn_ty)
                    {
                        self.annotate_expr(
                            callee,
                            &ExprEval {
                                ty: Some(resolved.clone()),
                                const_value: None,
                            },
                            reachable,
                            None,
                        );
                        let (params, return_ty, is_c) = match resolved {
                            TypeKind::Fn { params, return_ty } => (params, return_ty, false),
                            TypeKind::CFn { params, return_ty } => (params, return_ty, true),
                            _ => unreachable!(),
                        };
                        if is_c && self.unsafe_depth == 0 {
                            self.push_error(
                                expr.span,
                                "S11",
                                "calling a C function pointer requires unsafe context".to_string(),
                            );
                        }
                        let expected_count = params.len();
                        let actual_count = arg_evals.len();
                        if actual_count != expected_count {
                            self.push_error(
                                expr.span,
                                "S10",
                                format!(
                                    "expected {} argument(s), got {}",
                                    expected_count, actual_count
                                ),
                            );
                            return ExprEval::default();
                        }
                        for (index, ((param, arg_eval), arg)) in
                            params.iter().zip(&arg_evals).zip(args).enumerate()
                        {
                            if let Some(actual) = &arg_eval.ty
                                && !self.check_expr_compat(arg, &param.node, actual)
                            {
                                self.push_error(
                                    arg.span,
                                    "S08",
                                    format!(
                                        "arg {}: expected {}, got {}",
                                        index + 1,
                                        param.node,
                                        actual
                                    ),
                                );
                            }
                        }
                        let eval = ExprEval {
                            ty: Some(return_ty.node.clone()),
                            const_value: None,
                        };
                        self.annotate_expr(expr, &eval, reachable, None);
                        return eval;
                    }

                    let msg = if let Some(ref path) = sym.import_path {
                        let module_path = path
                            .rsplit_once('.')
                            .map(|(prefix, _)| prefix.to_string())
                            .unwrap_or_else(|| path.clone());
                        format!("'{}' is not exported by '{}'", name, module_path)
                    } else {
                        format!("cannot call '{}': not a function", name)
                    };
                    self.push_error(callee.span, "S04", msg);
                    return ExprEval::default();
                } else {
                    let callee_eval = self.type_check_expr(callee, reachable);
                    match callee_eval.ty.as_ref() {
                        Some(resolved @ (TypeKind::Fn { .. } | TypeKind::CFn { .. })) => {
                            let (params, return_ty, is_c) = match resolved {
                                TypeKind::Fn { params, return_ty } => (params, return_ty, false),
                                TypeKind::CFn { params, return_ty } => (params, return_ty, true),
                                _ => unreachable!(),
                            };
                            if is_c && self.unsafe_depth == 0 {
                                self.push_error(
                                    expr.span,
                                    "S11",
                                    "calling a C function pointer requires unsafe context"
                                        .to_string(),
                                );
                            }
                            let expected_count = params.len();
                            let actual_count = arg_evals.len();
                            if actual_count != expected_count {
                                self.push_error(
                                    expr.span,
                                    "S10",
                                    format!(
                                        "expected {} argument(s), got {}",
                                        expected_count, actual_count
                                    ),
                                );
                                return ExprEval::default();
                            }
                            for (index, ((param, arg_eval), arg)) in
                                params.iter().zip(&arg_evals).zip(args).enumerate()
                            {
                                if let Some(actual) = &arg_eval.ty
                                    && !self.check_expr_compat(arg, &param.node, actual)
                                {
                                    self.push_error(
                                        arg.span,
                                        "S08",
                                        format!(
                                            "arg {}: expected {}, got {}",
                                            index + 1,
                                            param.node,
                                            actual
                                        ),
                                    );
                                }
                            }
                            ExprEval {
                                ty: Some(return_ty.node.clone()),
                                const_value: None,
                            }
                        }
                        Some(other) => {
                            self.push_error(
                                callee.span,
                                "S10",
                                format!("cannot call value of type {}", other),
                            );
                            ExprEval::default()
                        }
                        None => ExprEval::default(),
                    }
                }
            }
            ExprKind::MethodCall {
                object,
                method,
                type_args,
                args,
                named_args,
            } => {
                // For lazy import tracking: record the object chain path (not the method itself)
                if let Some((base, path)) = Self::extract_field_chain(object)
                    && let Some(sym) = self.resolve_for_read(&base)
                    && sym.is_import
                {
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
                if self.is_module_import_receiver(object) {
                    let arg_evals: Vec<ExprEval> = args
                        .iter()
                        .chain(named_args.iter().map(|(_, e)| e))
                        .map(|a| self.type_check_expr(a, reachable))
                        .collect();
                    let resolved_method = self.resolve_module_method(object, method);
                    if let Some(resolved) = &resolved_method {
                        if let Some(sym) = self.resolve_symbol(resolved) {
                            if !matches!(sym.kind, SymbolKind::Function) {
                                let msg = if let Some(ref path) = sym.import_path {
                                    let module_path = path
                                        .rsplit_once('.')
                                        .map(|(prefix, _)| prefix.to_string())
                                        .unwrap_or_else(|| path.clone());
                                    format!("'{}' is not exported by '{}'", method, module_path)
                                } else {
                                    format!("function '{}' does not exist", resolved)
                                };
                                self.push_error(expr.span, "S04", msg);
                                ExprEval::default()
                            } else {
                                let sym = self
                                    .resolve_for_read(resolved)
                                    .expect("symbol should resolve");
                                if !named_args.is_empty() {
                                    self.validate_named_args(
                                        expr.span,
                                        resolved,
                                        named_args,
                                        args.len(),
                                        &sym,
                                    );
                                }
                                let eval = self.check_function_call(
                                    resolved, expr.span, args, &arg_evals, &sym, type_args,
                                );
                                self.annotate_expr(expr, &eval, reachable, Some(resolved.clone()));
                                eval
                            }
                        } else if !self.source_files.is_empty() {
                            // The receiver is an imported module namespace, but the
                            // qualified name does not resolve to any symbol. When source
                            // files are present (real loader run), emit a clear S04 error
                            // instead of silently producing default code. Raw tests that
                            // do not load imports stay lenient.
                            let (base, path) = Self::extract_field_chain(object)
                                .unwrap_or_else(|| (method.to_string(), Vec::new()));
                            let module_name = path.last().map(|s| s.as_str()).unwrap_or(&base);
                            self.push_error(
                                expr.span,
                                "S04",
                                format!("'{}' is not exported by '{}'", method, module_name),
                            );
                            ExprEval::default()
                        } else {
                            ExprEval::default()
                        }
                    } else {
                        ExprEval::default()
                    }
                } else {
                    // Static generic struct namespace call: `Box.new(42)`, `Pair.make(a, b)`.
                    // Object is a struct type name used as namespace, not a value.
                    // Infer concrete type args from actual arg types so the return type
                    // propagates correctly (e.g. `Box.new(42)` → `Box[i32]`).
                    if let ExprKind::Ident(struct_name) = &object.node {
                        let is_struct_ns = self.struct_defs.contains_key(struct_name.as_str())
                            && !matches!(
                                self.resolve_symbol(struct_name).map(|s| s.kind),
                                Some(SymbolKind::Variable { .. }) | Some(SymbolKind::Parameter)
                            );
                        if is_struct_ns {
                            let method_full = format!("{}.{}", struct_name, method);
                            if let Some(sym) = self.resolve_for_read(&method_full) {
                                let arg_evals: Vec<ExprEval> = args
                                    .iter()
                                    .map(|a| self.type_check_expr(a, reachable))
                                    .collect();
                                for (_, a) in named_args {
                                    self.type_check_expr(a, reachable);
                                }
                                let struct_params = self
                                    .struct_generic_params
                                    .get(struct_name.as_str())
                                    .cloned()
                                    .unwrap_or_default();
                                let from = self
                                    .current_function
                                    .last()
                                    .cloned()
                                    .unwrap_or_else(|| "__program__".to_string());
                                let ret_ty = if !struct_params.is_empty() {
                                    let mut subst: std::collections::HashMap<String, TypeKind> =
                                        std::collections::HashMap::new();
                                    for (param_ty, arg_eval) in
                                        sym.params.iter().zip(arg_evals.iter())
                                    {
                                        if let Some(arg_ty) = &arg_eval.ty {
                                            infer_type_subst(
                                                param_ty,
                                                arg_ty,
                                                &struct_params,
                                                &mut subst,
                                            );
                                        }
                                    }
                                    // For variadic methods, also infer from args beyond the fixed params.
                                    if sym.variadic
                                        && arg_evals.len() > sym.params.len()
                                        && let Some(param_ty) = sym.params.last()
                                    {
                                        for arg_eval in &arg_evals[sym.params.len()..] {
                                            if let Some(arg_ty) = &arg_eval.ty {
                                                infer_type_subst(
                                                    param_ty,
                                                    arg_ty,
                                                    &struct_params,
                                                    &mut subst,
                                                );
                                            }
                                        }
                                    }
                                    if !subst.is_empty() {
                                        let type_args: Vec<TypeKind> = struct_params
                                            .iter()
                                            .filter_map(|p| subst.get(p).cloned())
                                            .collect();
                                        if type_args.len() == struct_params.len() {
                                            let mono_name =
                                                mangle_monomorphized(&method_full, &type_args);
                                            if !self
                                                .monomorphizations
                                                .iter()
                                                .any(|m| m.mangled_name == mono_name)
                                            {
                                                self.monomorphizations.push(MonomorphizationInfo {
                                                    fn_name: method_full.clone(),
                                                    type_args,
                                                    mangled_name: mono_name.clone(),
                                                });
                                            }
                                            self.add_dependency_edge(
                                                DependencyKind::Call,
                                                &from,
                                                &mono_name,
                                            );
                                        }
                                    }
                                    sym.ty.as_ref().map(|t| substitute_type_kind(t, &subst))
                                } else {
                                    sym.ty.clone()
                                };
                                self.add_dependency_edge(DependencyKind::Call, &from, &method_full);
                                let eval = ExprEval {
                                    ty: ret_ty,
                                    const_value: None,
                                };
                                self.annotate_expr(expr, &eval, reachable, None);
                                return eval;
                            }
                        }
                    }

                    let object_eval = self.type_check_expr(object, reachable);
                    let arg_evals: Vec<ExprEval> = args
                        .iter()
                        .map(|arg| self.type_check_expr(arg, reachable))
                        .collect();
                    let named_arg_evals: Vec<(String, Expr, ExprEval)> = named_args
                        .iter()
                        .map(|(name, arg)| {
                            (
                                name.clone(),
                                arg.clone(),
                                self.type_check_expr(arg, reachable),
                            )
                        })
                        .collect();

                    // For Named types: impl method resolution takes priority over builtins.
                    // Returns Some(return_ty) when an impl method is found and side-effects recorded.
                    let impl_resolved: Option<Option<TypeKind>> = if let Some(TypeKind::Named {
                        name: type_name,
                        type_args,
                    }) = &object_eval.ty.clone()
                    {
                        let mangled = format!("{}.{}", type_name, method);
                        if let Some(sym) = self.resolve_for_read(&mangled) {
                            let from = self
                                .current_function
                                .last()
                                .cloned()
                                .unwrap_or_else(|| "__program__".to_string());
                            let subst: std::collections::HashMap<String, TypeKind> =
                                if !type_args.is_empty() {
                                    if let Some(struct_params) =
                                        self.struct_generic_params.get(type_name.as_str())
                                    {
                                        struct_params
                                            .iter()
                                            .zip(type_args.iter())
                                            .map(|(p, t)| (p.clone(), t.node.clone()))
                                            .collect()
                                    } else {
                                        std::collections::HashMap::new()
                                    }
                                } else {
                                    std::collections::HashMap::new()
                                };
                            let substituted_params: Vec<TypeKind> = sym
                                .params
                                .iter()
                                .map(|p| substitute_type_kind(p, &subst))
                                .collect();
                            let method_params = substituted_params.get(1..).unwrap_or(&[]);
                            let positional_count = arg_evals.len();
                            let total_arg_count = positional_count + named_arg_evals.len();
                            let is_variadic = sym.variadic;
                            let fixed_count = if is_variadic {
                                method_params.len().saturating_sub(1)
                            } else {
                                method_params.len()
                            };
                            if !is_variadic && total_arg_count != method_params.len() {
                                self.push_error(
                                    expr.span,
                                    "S08",
                                    format!(
                                        "expected {} args, got {}",
                                        method_params.len(),
                                        total_arg_count
                                    ),
                                );
                            } else if is_variadic && total_arg_count < fixed_count {
                                self.push_error(
                                    expr.span,
                                    "S08",
                                    format!(
                                        "expected at least {} args, got {}",
                                        fixed_count, total_arg_count
                                    ),
                                );
                            }
                            for (i, arg_eval) in arg_evals.iter().enumerate() {
                                let param_ty = if is_variadic && i >= fixed_count {
                                    method_params.last()
                                } else {
                                    method_params.get(i)
                                };
                                if let (Some(param_ty), Some(arg_ty)) = (param_ty, &arg_eval.ty) {
                                    let arg_expr = args.get(i).unwrap_or(expr);
                                    if !self.check_expr_compat(arg_expr, param_ty, arg_ty) {
                                        self.push_error(
                                            arg_expr.span,
                                            "S08",
                                            format!(
                                                "arg {}: expected {}, got {}",
                                                i + 1,
                                                param_ty,
                                                arg_ty
                                            ),
                                        );
                                    }
                                }
                            }
                            if !named_arg_evals.is_empty() {
                                let param_names = self
                                    .fn_param_names
                                    .get(&mangled)
                                    .cloned()
                                    .unwrap_or_default();
                                let mut seen: std::collections::HashSet<String> =
                                    std::collections::HashSet::new();
                                for (arg_name, arg_expr, arg_eval) in &named_arg_evals {
                                    if !seen.insert(arg_name.clone()) {
                                        self.push_error(
                                            expr.span,
                                            "S09",
                                            format!("duplicate named argument `{}`", arg_name),
                                        );
                                        continue;
                                    }
                                    let Some(pos) = param_names.iter().position(|p| p == arg_name)
                                    else {
                                        self.push_error(
                                            expr.span,
                                            "S09",
                                            format!(
                                                "unknown parameter name `{}` for method `{}`",
                                                arg_name, mangled
                                            ),
                                        );
                                        continue;
                                    };
                                    if pos < positional_count {
                                        self.push_error(
                                            expr.span,
                                            "S09",
                                            format!(
                                                "named argument `{}` conflicts with positional argument at position {}",
                                                arg_name,
                                                pos + 1
                                            ),
                                        );
                                        continue;
                                    }
                                    if let (Some(param_ty), Some(arg_ty)) =
                                        (method_params.get(pos), &arg_eval.ty)
                                    {
                                        if !self.check_expr_compat(arg_expr, param_ty, arg_ty) {
                                            self.push_error(
                                                expr.span,
                                                "S09",
                                                format!(
                                                    "named argument `{}`: expected {}, got {}",
                                                    arg_name, param_ty, arg_ty
                                                ),
                                            );
                                        }
                                    }
                                }
                            }
                            // Record monomorphization for generic receiver types.
                            if !type_args.is_empty() {
                                let type_kinds: Vec<TypeKind> =
                                    type_args.iter().map(|t| t.node.clone()).collect();
                                let mono_name = mangle_monomorphized(&mangled, &type_kinds);
                                if !self
                                    .monomorphizations
                                    .iter()
                                    .any(|m| m.mangled_name == mono_name)
                                {
                                    self.monomorphizations.push(MonomorphizationInfo {
                                        fn_name: mangled.clone(),
                                        type_args: type_kinds,
                                        mangled_name: mono_name.clone(),
                                    });
                                }
                                self.add_dependency_edge(DependencyKind::Call, &from, &mono_name);
                            }
                            self.add_dependency_edge(DependencyKind::Call, &from, &mangled);
                            if sym.unsafe_fn && self.unsafe_depth == 0 {
                                self.push_error(
                                        expr.span,
                                        "S11",
                                        format!(
                                            "call to unsafe method `{}.{}` requires unsafe block or unsafe fn context",
                                            type_name, method
                                        ),
                                    );
                            }
                            // Apply type substitution to return type for generic receivers.
                            let ret_ty = if !type_args.is_empty() {
                                sym.ty.map(|t| substitute_type_kind(&t, &subst))
                            } else {
                                sym.ty
                            };
                            Some(ret_ty)
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    // For Dyn receivers: validate the method exists on the trait and return Any.
                    let dyn_resolved: Option<Option<TypeKind>> = if impl_resolved.is_none() {
                        if let Some(TypeKind::Dyn { trait_name }) = &object_eval.ty {
                            if let Some(slots) = self.trait_method_slots.get(trait_name.as_str()) {
                                if slots.contains(method) {
                                    Some(Some(TypeKind::Any))
                                } else {
                                    self.push_error(
                                        expr.span,
                                        "S04",
                                        format!(
                                            "trait `{}` has no method `{}`",
                                            trait_name, method
                                        ),
                                    );
                                    Some(None)
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let ty = if let Some(impl_ty) = impl_resolved {
                        impl_ty
                    } else if let Some(dyn_ty) = dyn_resolved {
                        dyn_ty
                    } else {
                        let ref_str = TypeKind::Ref {
                            inner: Box::new(Spanned::new(TypeKind::Str, expr.span)),
                        };
                        match method.as_str() {
                            "len" => Some(TypeKind::Usize),
                            "to_str" => Some(ref_str.clone()),
                            "to_string" => Some(TypeKind::Named {
                                name: "String".to_string(),
                                type_args: vec![],
                            }),
                            "as_string" | "as_str" => match &object_eval.ty {
                                Some(t)
                                    if matches!(t, TypeKind::Str | TypeKind::Ref { .. })
                                        || matches!(t, TypeKind::Named { name, .. } if name == "String") =>
                                {
                                    Some(ref_str.clone())
                                }
                                Some(other) => {
                                    self.push_error(
                                        expr.span,
                                        "S06",
                                        format!("as_str() not valid for {} — use to_str() to convert primitives to string", other),
                                    );
                                    None
                                }
                                None => None,
                            },
                            "as_ptr" => match &object_eval.ty {
                                Some(TypeKind::Bytes) => Some(TypeKind::RawPtr {
                                    inner: Box::new(Spanned::new(TypeKind::Uint8, expr.span)),
                                }),
                                Some(TypeKind::Named { .. }) => None,
                                Some(other) => {
                                    self.push_error(
                                        expr.span,
                                        "S06",
                                        format!("as_ptr() is not available on {}", other),
                                    );
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
                                Some(t) if matches!(t, TypeKind::Str | TypeKind::Ref { .. }) => {
                                    Some(ref_str.clone())
                                }
                                _ => None,
                            },
                            _ => None,
                        }
                    };
                    ExprEval {
                        ty,
                        const_value: None,
                    }
                }
            }
            ExprKind::Field { object, name } => {
                // For lazy import tracking: record the full chain including this field
                if let Some((base, mut path)) = Self::extract_field_chain(object)
                    && let Some(sym) = self.resolve_for_read(&base)
                    && sym.is_import
                {
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
                // Module namespace field referencing a function: `bar.foo` as a value.
                if self.is_module_import_receiver(object) {
                    if let Some(resolved) = self.resolve_module_method(object, name) {
                        if let Some(sym) = self.resolve_for_read(&resolved) {
                            if matches!(sym.kind, SymbolKind::Function) {
                                let return_ty = sym.ty.clone().unwrap_or(TypeKind::Void);
                                let param_types: Vec<Type> = sym
                                    .params
                                    .iter()
                                    .map(|p| Spanned::new(p.clone(), expr.span))
                                    .collect();
                                let from = self
                                    .current_function
                                    .last()
                                    .cloned()
                                    .unwrap_or_else(|| "__program__".to_string());
                                self.add_dependency_edge(DependencyKind::Call, &from, &resolved);
                                let eval = ExprEval {
                                    ty: Some(TypeKind::Fn {
                                        params: param_types,
                                        return_ty: Box::new(Spanned::new(return_ty, expr.span)),
                                    }),
                                    const_value: None,
                                };
                                self.annotate_expr(expr, &eval, reachable, Some(resolved));
                                return eval;
                            }
                        } else if !self.source_files.is_empty() {
                            let (base, path) = Self::extract_field_chain(object)
                                .unwrap_or_else(|| (name.to_string(), Vec::new()));
                            let module_name = path.last().map(|s| s.as_str()).unwrap_or(&base);
                            self.push_error(
                                expr.span,
                                "S04",
                                format!("'{}' is not exported by '{}'", name, module_name),
                            );
                            return ExprEval::default();
                        }
                    }
                }

                let obj_eval = self.type_check_expr(object, reachable);
                if let Some(TypeKind::Named {
                    name: aggregate_name,
                    ..
                }) = &obj_eval.ty
                    && self.repr_c_unions.contains(aggregate_name)
                    && self.unsafe_depth == 0
                {
                    self.push_error(
                        expr.span,
                        "S11",
                        format!("reading union field `{name}` requires unsafe context"),
                    );
                }
                // Resolve field type from struct_defs, substituting generic params when present.
                let field_ty = match &obj_eval.ty {
                    Some(TypeKind::Named {
                        name: struct_name,
                        type_args,
                    }) => {
                        let raw = self
                            .struct_defs
                            .get(struct_name)
                            .and_then(|fields| fields.iter().find(|(fn_, _)| fn_ == name))
                            .map(|(_, ty)| ty.clone());
                        if let Some(raw_ty) = raw {
                            let gp = self
                                .struct_generic_params
                                .get(struct_name)
                                .cloned()
                                .unwrap_or_default();
                            if !gp.is_empty() && !type_args.is_empty() {
                                let subst: std::collections::HashMap<String, TypeKind> = gp
                                    .iter()
                                    .zip(type_args.iter())
                                    .map(|(p, a)| (p.clone(), a.node.clone()))
                                    .collect();
                                Some(substitute_type_kind(&raw_ty, &subst))
                            } else {
                                Some(raw_ty)
                            }
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                ExprEval {
                    ty: field_ty,
                    const_value: None,
                }
            }
            ExprKind::StructInit { name, fields } => {
                if self
                    .resolve_symbol(name)
                    .is_some_and(|symbol| symbol.attributes.iter().any(|attr| attr == "opaque"))
                {
                    self.push_error(
                        expr.span,
                        "S14",
                        format!("opaque FFI type `{name}` cannot be constructed in Quazi"),
                    );
                }
                if self.repr_c_unions.contains(name) && fields.len() != 1 {
                    self.push_error(
                        expr.span,
                        "S14",
                        format!("union `{name}` construction must initialize exactly one field"),
                    );
                }
                if self.flexible_array_structs.contains(name) {
                    self.push_error(
                        expr.span,
                        "S14",
                        format!(
                            "aggregate `{name}` has a flexible array member and cannot be constructed by value"
                        ),
                    );
                }
                if let Some(field_defs) = self.struct_defs.get(name).cloned() {
                    for (fname, fval) in fields {
                        let val_eval = self.type_check_expr(fval, reachable);
                        if let Some((_, expected_ty)) =
                            field_defs.iter().find(|(fn_, _)| fn_ == fname)
                        {
                            if let Some(got_ty) = &val_eval.ty
                                && !self.types_compatible(got_ty, expected_ty)
                            {
                                self.push_error(
                                    fval.span,
                                    "S08",
                                    format!(
                                        "field '{}': expected {}, got {}",
                                        fname, expected_ty, got_ty
                                    ),
                                );
                            }
                        } else {
                            self.push_error(
                                fval.span,
                                "S07",
                                format!("struct '{}' has no field '{}'", name, fname),
                            );
                        }
                    }
                } else {
                    // Unknown struct — type-check all field values anyway
                    for (_, fval) in fields {
                        self.type_check_expr(fval, reachable);
                    }
                }
                ExprEval {
                    ty: Some(TypeKind::Named {
                        name: name.clone(),
                        type_args: vec![],
                    }),
                    const_value: None,
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                let scrutinee_eval = self.type_check_expr(scrutinee, reachable);
                let mut arm_infos = Vec::new();
                let mut result_ty: Option<TypeKind> = None;

                if arms.is_empty() {
                    self.push_error(expr.span, "S09", "match expression has no arms".to_string());
                }

                for arm in arms {
                    let (mut arm_info, bindings) =
                        self.validate_match_pattern(&arm.pattern, &scrutinee_eval.ty);
                    arm_info.has_guard = arm.guard.is_some();

                    self.enter_scope();
                    // Build a map: binding_name → field type for Variant patterns.
                    let binding_types: std::collections::HashMap<String, TypeKind> =
                        if let PatternKind::Variant {
                            enum_name,
                            variant,
                            sub_patterns,
                        } = &arm.pattern.node
                        {
                            let resolved_enum = enum_name.clone().or_else(|| {
                                if let Some(TypeKind::Named { name, .. }) = &scrutinee_eval.ty {
                                    Some(name.clone())
                                } else {
                                    None
                                }
                            });
                            if let Some(ename) = resolved_enum {
                                if let Some(info) = self.enums.get(&ename) {
                                    if let Some(field_tys) = info.variant_fields.get(variant) {
                                        let mut m = std::collections::HashMap::new();
                                        let mut field_idx = 0;
                                        for sub in sub_patterns {
                                            if let PatternKind::Bind(bname) = &sub.node
                                                && let Some(ty) = field_tys.get(field_idx)
                                            {
                                                m.insert(bname.clone(), ty.clone());
                                            }
                                            if !matches!(sub.node, PatternKind::Wildcard) {
                                                field_idx += 1;
                                            }
                                        }
                                        m
                                    } else {
                                        std::collections::HashMap::new()
                                    }
                                } else {
                                    std::collections::HashMap::new()
                                }
                            } else {
                                std::collections::HashMap::new()
                            }
                        } else {
                            std::collections::HashMap::new()
                        };
                    for binding in bindings {
                        let ty = binding_types
                            .get(&binding)
                            .cloned()
                            .unwrap_or(TypeKind::Any);
                        self.declare(
                            binding,
                            Symbol {
                                kind: SymbolKind::Variable { mutable: false },
                                ty: Some(ty),
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
                                unsafe_fn: false,
                                generic_params: vec![],
                            },
                        );
                    }

                    // Type-check the optional guard expression (inside binding scope).
                    if let Some(guard) = &arm.guard {
                        let guard_eval = self.type_check_expr(guard, reachable);
                        if let Some(guard_ty) = guard_eval.ty
                            && !matches!(guard_ty, TypeKind::Bool | TypeKind::Any)
                            && !Self::is_integer(&guard_ty)
                        {
                            self.push_error(
                                guard.span,
                                "S01",
                                format!("match guard must be bool or integer, got {}", guard_ty),
                            );
                        }
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
                self.mark_binary_autoderef(target, &target_eval.ty, value, &value_eval.ty);
                ExprEval {
                    ty,
                    const_value: None,
                }
            }
            ExprKind::IncDec { expr: inner, .. } => {
                self.analyze_assign_target(inner);
                let inner_eval = self.type_check_expr(inner, reachable);

                if let Some(ty) = &inner_eval.ty
                    && matches!(
                        ty,
                        TypeKind::Str
                            | TypeKind::Bool
                            | TypeKind::Void
                            | TypeKind::Ref { .. }
                            | TypeKind::RawPtr { .. }
                    )
                {
                    self.push_error(inner.span, "S06", format!("++ / -- not valid for {}", ty));
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
            ExprKind::Index { object, indices } => {
                let obj_eval = self.type_check_expr(object, reachable);
                let idx_evals: Vec<ExprEval> = indices
                    .iter()
                    .map(|i| self.type_check_expr(i, reachable))
                    .collect();

                // Named type that explicitly implements the Index trait → dispatch to Type.index.
                // Checks trait_impls registry so accidental `fn index` methods don't trigger [].
                let maybe_index_mangled =
                    if let Some(TypeKind::Named { name: tn, .. }) = &obj_eval.ty {
                        let implements_index = self
                            .trait_impls
                            .get(tn.as_str())
                            .map(|ts| ts.contains("Index"))
                            .unwrap_or(false);
                        if implements_index {
                            let m = format!("{}.index", tn);
                            if self.resolve_for_read(&m).is_some() {
                                Some(m)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                let elem_ty = if let Some(mangled) = maybe_index_mangled {
                    let from = self
                        .current_function
                        .last()
                        .cloned()
                        .unwrap_or_else(|| "__program__".to_string());
                    self.add_dependency_edge(DependencyKind::Call, &from, &mangled);
                    if let Some(sym) = self.resolve_symbol(&mangled) {
                        if sym.unsafe_fn && self.unsafe_depth == 0 {
                            self.push_error(
                                expr.span,
                                "S11",
                                format!(
                                    "call to unsafe `[]` operator on `{}` requires unsafe block",
                                    mangled
                                ),
                            );
                        }
                        sym.ty
                    } else {
                        None
                    }
                } else {
                    if let Some(first_eval) = idx_evals.first()
                        && let Some(idx_ty) = &first_eval.ty
                        && !matches!(
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
                        )
                    {
                        self.push_error(
                            indices[0].span,
                            "S06",
                            format!("array index must be an integer, got {}", idx_ty),
                        );
                    }
                    match &obj_eval.ty {
                        Some(TypeKind::Array { elem_ty, .. }) => Some(elem_ty.node.clone()),
                        Some(TypeKind::Slice { elem_ty }) => Some(elem_ty.node.clone()),
                        Some(TypeKind::FlexibleArray { elem_ty }) => {
                            if self.unsafe_depth == 0 {
                                self.push_error(
                                    expr.span,
                                    "S11",
                                    "accessing a flexible array member requires unsafe context"
                                        .to_string(),
                                );
                            }
                            Some(elem_ty.node.clone())
                        }
                        Some(TypeKind::Bytes) => Some(TypeKind::Uint8),
                        _ => None,
                    }
                };
                ExprEval {
                    ty: elem_ty,
                    const_value: None,
                }
            }

            ExprKind::Try { expr: inner } => {
                let inner_eval = self.type_check_expr(inner, reachable);
                // Validate the wrapper and retain its payload type when known.
                let payload_ty = match &inner_eval.ty {
                    Some(TypeKind::Named { name, type_args })
                        if (name == "Result" || name == "Option")
                            && !type_args.is_empty() =>
                    {
                        Some(type_args[0].node.clone())
                    }
                    Some(TypeKind::Named { name, .. }) if name == "Result" || name == "Option" => {
                        Some(TypeKind::Any)
                    }
                    Some(ty) => {
                        self.push_error(
                            expr.span,
                            "S14",
                            format!("`?` operator requires Result or Option, got {}", ty),
                        );
                        Some(TypeKind::Any)
                    }
                    None => None,
                };
                ExprEval {
                    ty: payload_ty,
                    const_value: None,
                }
            }

            ExprKind::Closure { params, body } => {
                // Enter a new scope for the closure's params.
                self.enter_scope();
                let mut param_types = Vec::new();
                for pname in params {
                    // Declare each param as Variable (unknown type for now).
                    self.declare(
                        pname.clone(),
                        Symbol {
                            kind: SymbolKind::Variable { mutable: false },
                            ty: Some(TypeKind::Any),
                            span: expr.span,
                            params: vec![],
                            used: false,
                            initialized: true,
                            is_import: false,
                            import_path: None,
                            const_value: None,
                            variadic: false,
                            attributes: vec![],
                            public: false,
                            unsafe_fn: false,
                            generic_params: vec![],
                        },
                    );
                    param_types.push(Spanned::new(TypeKind::Any, expr.span));
                }
                let body_eval = self.type_check_expr(body, reachable);
                self.exit_scope_collect();
                let return_ty = body_eval.ty.clone().unwrap_or(TypeKind::Void);
                let fn_ty = TypeKind::Fn {
                    params: param_types,
                    return_ty: Box::new(Spanned::new(return_ty, expr.span)),
                };
                ExprEval {
                    ty: Some(fn_ty),
                    const_value: None,
                }
            }
        };

        self.annotate_expr(expr, &result, reachable, None);
        result
    }

    fn is_module_import_receiver(&self, object: &Expr) -> bool {
        let Some((base, _)) = Self::extract_field_chain(object) else {
            return false;
        };
        // An imported type name is a static-method namespace, not a module
        // namespace. Treating it as a module made `Map.new()` and equivalent
        // imported constructors fail before static method resolution.
        if self.struct_defs.contains_key(&base) {
            return false;
        }
        self.resolve_symbol(&base).is_some_and(|sym| sym.is_import)
    }

    /// Resolve a bare function name used in the current function body.
    /// If the current module is namespaced, prefer the module-qualified name
    /// (`bar.foo`) when it exists. Import aliases are followed through to the
    /// actual mangled target so codegen can use the real function name. Returns
    /// `None` if the name is shadowed by a local variable/parameter or does not
    /// name a function.
    fn resolve_bare_fn_name(&self, name: &str) -> Option<String> {
        // Check local scopes (skip the global scope at index 0) for non-function
        // shadowing. If a local variable or parameter uses this name, the identifier
        // refers to that local, not a top-level function.
        for scope in self.scopes.iter().skip(1).rev() {
            if let Some(sym) = scope.get(name)
                && !matches!(sym.kind, SymbolKind::Function)
            {
                return None;
            }
        }

        if let Some(module) = &self.current_module_path {
            let qualified = format!("{}.{}", module, name);
            if let Some(sym) = self.resolve_symbol(&qualified)
                && matches!(sym.kind, SymbolKind::Function)
            {
                return Some(qualified);
            }
        }

        if let Some(sym) = self.resolve_symbol(name)
            && matches!(sym.kind, SymbolKind::Function)
        {
            // Import aliases: follow to the mangled target name.
            if sym.is_import
                && let Some(path) = &sym.import_path
                && let Some(mangled) = super::declare::mangle_import_path(path)
                && self.resolve_symbol(&mangled).is_some()
            {
                return Some(mangled);
            }
            return Some(name.to_string());
        }

        None
    }

    fn resolve_bare_foreign_global_name(&self, name: &str) -> Option<String> {
        for scope in self.scopes.iter().skip(1).rev() {
            if scope.contains_key(name) {
                return None;
            }
        }
        if let Some(module) = &self.current_module_path {
            let qualified = format!("{}.{}", module, name);
            if self.foreign_globals.contains_key(&qualified) {
                return Some(qualified);
            }
        }
        let symbol = self.resolve_symbol(name)?;
        if symbol
            .attributes
            .iter()
            .any(|attribute| attribute == "foreign_global")
        {
            if symbol.is_import
                && let Some(path) = symbol.import_path.as_deref()
                && let Some(mangled) = super::declare::mangle_import_path(path)
                && self.foreign_globals.contains_key(&mangled)
            {
                return Some(mangled);
            }
            if self.foreign_globals.contains_key(name) {
                return Some(name.to_string());
            }
        }
        None
    }

    /// Resolve a module-qualified method call like `bar.foo()` or `std.write()`.
    /// The object chain identifies the imported module namespace; the method is
    /// resolved as `<module_file>.<method>` where `<module_file>` is the last
    /// segment of the chain or the last segment of the import path (i.e. the
    /// actual source file module).
    fn resolve_module_method(&self, object: &Expr, method: &str) -> Option<String> {
        let (base, path) = Self::extract_field_chain(object)?;
        let sym = self.resolve_symbol(&base)?;
        if !sym.is_import {
            return None;
        }
        let import_base = sym.import_path.as_deref().unwrap_or(&base);
        let segments: Vec<&str> = import_base.split('.').collect();
        let module_file = if let Some(last) = path.last() {
            // Multi-level chain like `std.core.write()` → file module is the last
            // object-chain segment (`core`).
            last.as_str()
        } else {
            // Single-level like `bar.foo()` → file module is the last segment of
            // the import path (`bar` from `bar` or `foo.bar`).
            segments.last().copied().unwrap_or(&base)
        };
        Some(format!("{}.{}", module_file, method))
    }

    fn validate_named_args(
        &mut self,
        _span: Span,
        fn_name: &str,
        named_args: &[(String, Expr)],
        positional_count: usize,
        sym: &Symbol,
    ) {
        let param_names = self
            .fn_param_names
            .get(fn_name)
            .cloned()
            .unwrap_or_default();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (arg_name, arg_expr) in named_args {
            if !seen.insert(arg_name.clone()) {
                self.push_error(
                    arg_expr.span,
                    "S09",
                    format!("duplicate named argument `{}`", arg_name),
                );
                continue;
            }
            let Some(pos) = param_names.iter().position(|p| p == arg_name) else {
                self.push_error(
                    arg_expr.span,
                    "S09",
                    format!(
                        "unknown parameter name `{}` for function `{}`",
                        arg_name, fn_name
                    ),
                );
                continue;
            };
            if pos < positional_count {
                self.push_error(
                    arg_expr.span,
                    "S09",
                    format!(
                        "named argument `{}` conflicts with positional argument at position {}",
                        arg_name,
                        pos + 1
                    ),
                );
                continue;
            }
            // Type-check named arg against its param type.
            let eval = self.type_check_expr(arg_expr, true);
            if let (Some(param_ty), Some(arg_ty)) = (sym.params.get(pos), eval.ty) {
                if !self.check_expr_compat(arg_expr, param_ty, &arg_ty) {
                    self.push_error(
                        arg_expr.span,
                        "S09",
                        format!(
                            "named argument `{}`: expected {}, got {}",
                            arg_name, param_ty, arg_ty
                        ),
                    );
                }
            }
        }
    }

    fn check_function_call(
        &mut self,
        name: &str,
        callee_span: Span,
        args: &[Expr],
        arg_evals: &[ExprEval],
        sym: &Symbol,
        type_args: &[Type],
    ) -> ExprEval {
        let from = self
            .current_function
            .last()
            .cloned()
            .unwrap_or_else(|| "__program__".to_string());
        // Resolve alias imports to the original function name for call tracking.
        // `import foo.bar as baz` → record counts and edges under "bar", not "baz".
        let canonical = if sym.is_import {
            sym.import_path
                .as_ref()
                .and_then(|p| p.rsplit('.').next())
                .unwrap_or(name)
        } else {
            name
        };
        *self.call_counts.entry(canonical.to_string()).or_insert(0) += 1;

        // Build substitution map if this is a generic function call with type arguments.
        let subst: std::collections::HashMap<String, TypeKind> =
            if !sym.generic_params.is_empty() && type_args.len() == sym.generic_params.len() {
                let mut map = std::collections::HashMap::new();
                for (param, arg) in sym.generic_params.iter().zip(type_args.iter()) {
                    map.insert(param.clone(), arg.node.clone());
                }
                map
            } else {
                std::collections::HashMap::new()
            };

        // Record monomorphization and add dependency edge.
        if !subst.is_empty() {
            let type_kinds: Vec<TypeKind> = type_args.iter().map(|t| t.node.clone()).collect();
            let mangled = mangle_monomorphized(name, &type_kinds);
            if !self
                .monomorphizations
                .iter()
                .any(|m| m.mangled_name == mangled)
            {
                self.monomorphizations.push(MonomorphizationInfo {
                    fn_name: name.to_string(),
                    type_args: type_kinds.clone(),
                    mangled_name: mangled.clone(),
                });
            }
            self.add_dependency_edge(DependencyKind::Call, &from, &mangled);
        } else {
            self.add_dependency_edge(DependencyKind::Call, &from, canonical);
        }

        // Apply substitution to param types for checking.
        let substituted_params: Vec<TypeKind> = if !subst.is_empty() {
            sym.params
                .iter()
                .map(|p| substitute_type_kind(p, &subst))
                .collect()
        } else {
            sym.params.clone()
        };

        // str_variadic fns with >1 arg cause codegen to inject a call to "format".
        if sym.attributes.contains(&"str_variadic".to_string()) {
            self.add_dependency_edge(DependencyKind::Call, &from, "fmt.format");
            if let Some(expanded) = crate::parser::format::expand_format_call_args(args) {
                for arg in &expanded.args {
                    self.type_check_expr(arg, true);
                }
            }
        }
        if sym.unsafe_fn && self.unsafe_depth == 0 {
            self.push_error(
                callee_span,
                "S11",
                format!(
                    "call to unsafe function `{}` requires unsafe block or unsafe fn context",
                    name
                ),
            );
        }

        let is_variadic = sym.variadic;
        let non_variadic_count = if is_variadic {
            substituted_params.len().saturating_sub(1)
        } else {
            substituted_params.len()
        };
        // Special case: panic() — codegen injects file/line hidden args after msg.
        // User writes panic("msg") or panic("msg", "extra"); we type-check as if
        // the signature were panic(msg: str, ...args: str).
        let is_panic = name.rsplit('.').next() == Some("panic");
        let effective_non_variadic_count = if is_panic { 1 } else { non_variadic_count };
        let effective_variadic_elem = if is_panic {
            substituted_params.first()
        } else {
            substituted_params.last()
        };
        // Use total arg count (positional + named, reflected in arg_evals).
        let total_arg_count = arg_evals.len();
        let is_c_variadic = sym.attributes.contains(&"c_variadic".to_string());

        if is_c_variadic {
            if total_arg_count < substituted_params.len() {
                self.push_error(
                    callee_span,
                    "S08",
                    format!(
                        "expected at least {} args, got {}",
                        substituted_params.len(),
                        total_arg_count
                    ),
                );
            }
            for (i, (param_ty, arg_ty)) in substituted_params
                .iter()
                .zip(arg_evals.iter().map(|eval| eval.ty.as_ref()))
                .enumerate()
            {
                if let (Some(arg_ty), Some(arg_expr)) = (arg_ty, args.get(i))
                    && !self.check_expr_compat(arg_expr, param_ty, arg_ty)
                {
                    self.push_error(
                        arg_expr.span,
                        "S08",
                        format!("arg {}: expected {}, got {}", i + 1, param_ty, arg_ty),
                    );
                }
            }
            for (index, eval) in arg_evals.iter().enumerate().skip(substituted_params.len()) {
                let Some(arg_ty) = &eval.ty else {
                    continue;
                };
                let resolved = self.resolve_type_aliases(arg_ty);
                let supported = (ffi_primitive(&resolved) && !matches!(resolved, TypeKind::Void))
                    || matches!(
                        &resolved,
                        TypeKind::Named { name, type_args }
                            if type_args.is_empty() && self.repr_c_structs.contains(name)
                    );
                if !supported {
                    self.push_error(
                        args.get(index).map_or(callee_span, |arg| arg.span),
                        "S14",
                        format!(
                            "C variadic arg {} has unsupported C ABI type `{}`",
                            index + 1,
                            arg_ty
                        ),
                    );
                }
            }
        } else if !is_variadic && substituted_params.len() != total_arg_count {
            self.push_error(
                callee_span,
                "S08",
                format!(
                    "expected {} args, got {}",
                    substituted_params.len(),
                    total_arg_count
                ),
            );
        } else if is_variadic && total_arg_count < effective_non_variadic_count {
            self.push_error(
                callee_span,
                "S08",
                format!(
                    "expected at least {} args, got {}",
                    effective_non_variadic_count, total_arg_count
                ),
            );
        } else {
            // Type-check non-variadic args.
            let check_count = if is_panic {
                1usize.min(substituted_params.len())
            } else {
                non_variadic_count
            };
            for (i, (param_ty, arg_ty)) in substituted_params[..check_count]
                .iter()
                .zip(arg_evals.iter().map(|e| &e.ty))
                .enumerate()
            {
                if let Some(at) = arg_ty {
                    if let Some(arg_expr) = args.get(i) {
                        if matches!(param_ty, TypeKind::Slice { .. })
                            && matches!(at, TypeKind::Array { .. })
                        {
                            self.push_error(
                                arg_expr.span,
                                "S08",
                                format!(
                                    "arg {}: passing fixed-size array {} to slice parameter {} is not yet supported",
                                    i + 1, at, param_ty
                                ),
                            );
                            continue;
                        }
                        if matches!(param_ty, TypeKind::Array { .. })
                            && matches!(at, TypeKind::Slice { .. })
                        {
                            self.push_error(
                                arg_expr.span,
                                "S08",
                                format!(
                                    "arg {}: passing slice {} to fixed-size array parameter {} is not yet supported",
                                    i + 1, at, param_ty
                                ),
                            );
                            continue;
                        }
                        if !self.check_expr_compat(arg_expr, param_ty, at) {
                            self.push_error(
                                arg_expr.span,
                                "S08",
                                format!("arg {}: expected {}, got {}", i + 1, param_ty, at),
                            );
                        }
                    } else {
                        self.push_error(
                            callee_span,
                            "S08",
                            format!("arg {}: expected {}, got {}", i + 1, param_ty, at),
                        );
                    }
                }
            }
            // Variadic args checked against the element type.
            if is_variadic && let Some(elem_ty) = effective_variadic_elem {
                let var_start = effective_non_variadic_count;
                for (i, arg_ty) in arg_evals[var_start..].iter().map(|e| &e.ty).enumerate() {
                    if let Some(at) = arg_ty {
                        if let Some(arg_expr) = args.get(var_start + i) {
                            if !self.check_expr_compat(arg_expr, elem_ty, at) {
                                self.push_error(
                                    arg_expr.span,
                                    "S08",
                                    format!(
                                        "variadic arg {}: expected {}, got {}",
                                        i + 1,
                                        elem_ty,
                                        at
                                    ),
                                );
                            }
                        } else {
                            self.push_error(
                                callee_span,
                                "S08",
                                format!("variadic arg {}: expected {}, got {}", i + 1, elem_ty, at),
                            );
                        }
                    }
                }
            }
        }

        let return_ty = if !subst.is_empty() {
            sym.ty.as_ref().map(|t| substitute_type_kind(t, &subst))
        } else {
            sym.ty.clone()
        };

        ExprEval {
            ty: return_ty,
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
        // Auto-deref references for binary operations: &T op U becomes T op U.
        let left = left.clone().map(Self::autoderef_type);
        let right = right.clone().map(Self::autoderef_type);
        match op {
            BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul | BinOpKind::Div | BinOpKind::Mod => {
                match (&left, &right) {
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
                if let (Some(l), Some(r)) = (&left, &right)
                    && !self.types_compatible(l, r)
                {
                    self.push_error(
                        span,
                        "S01",
                        format!("type mismatch in binary op: {} vs {}", l, r),
                    );
                }
                Some(TypeKind::Bool)
            }
            BinOpKind::AndAnd | BinOpKind::OrOr => {
                if let Some(l) = &left
                    && !matches!(l, TypeKind::Bool | TypeKind::Any)
                    && !Self::is_integer(l)
                {
                    self.push_error(
                        span,
                        "S06",
                        format!("logical op requires bool or integer, got {}", l),
                    );
                }

                if let Some(r) = &right
                    && !matches!(r, TypeKind::Bool | TypeKind::Any)
                    && !Self::is_integer(r)
                {
                    self.push_error(
                        span,
                        "S06",
                        format!("logical op requires bool or integer, got {}", r),
                    );
                }

                Some(TypeKind::Bool)
            }
            BinOpKind::BitAnd | BinOpKind::BitOr | BinOpKind::BitXor => match (&left, &right) {
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
            },
            BinOpKind::Shl | BinOpKind::Shr => match (&left, &right) {
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
            },
            BinOpKind::Pow => match (&left, &right) {
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
            Literal::Bytes(_) => None,
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

            (BinOpKind::BitAnd, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(a & b))
            }
            (BinOpKind::BitOr, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(a | b))
            }
            (BinOpKind::BitXor, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(a ^ b))
            }
            (BinOpKind::Shl, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(a << b))
            }
            (BinOpKind::Shr, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(a >> b))
            }
            (BinOpKind::BitAnd, ConstValue::Bool(a), ConstValue::Bool(b)) => {
                Some(ConstValue::Bool(*a && *b))
            }
            (BinOpKind::BitOr, ConstValue::Bool(a), ConstValue::Bool(b)) => {
                Some(ConstValue::Bool(*a || *b))
            }
            (BinOpKind::BitXor, ConstValue::Bool(a), ConstValue::Bool(b)) => {
                Some(ConstValue::Bool(*a ^ *b))
            }

            (BinOpKind::AndAnd, ConstValue::Bool(a), ConstValue::Bool(b)) => {
                Some(ConstValue::Bool(*a && *b))
            }
            (BinOpKind::OrOr, ConstValue::Bool(a), ConstValue::Bool(b)) => {
                Some(ConstValue::Bool(*a || *b))
            }

            _ => None,
        }
    }

    pub(super) fn annotate_expr(
        &mut self,
        expr: &Expr,
        eval: &ExprEval,
        reachable: bool,
        resolved_fn: Option<String>,
    ) {
        self.annotated_exprs.push(ExprAnnotation {
            span: expr.span,
            ty: eval.ty.clone(),
            const_value: eval.const_value.clone(),
            reachable,
            resolved_fn,
            resolved_global: None,
            auto_deref: false,
            c_abi_function: false,
        });

        if let Some(value) = &eval.const_value {
            self.constant_evaluations.push(ConstantEvaluation {
                span: expr.span,
                value: value.clone(),
            });
        }
    }

    fn annotate_foreign_global_expr(
        &mut self,
        expr: &Expr,
        eval: &ExprEval,
        reachable: bool,
        resolved_global: String,
    ) {
        self.annotated_exprs.push(ExprAnnotation {
            span: expr.span,
            ty: eval.ty.clone(),
            const_value: None,
            reachable,
            resolved_fn: None,
            resolved_global: Some(resolved_global),
            auto_deref: false,
            c_abi_function: false,
        });
    }

    /// Annotate an expression and mark it for codegen auto-dereference.
    /// The stored type is the dereferenced value type so codegen knows what to load.
    pub(super) fn annotate_expr_auto_deref(
        &mut self,
        expr: &Expr,
        eval: &ExprEval,
        reachable: bool,
        resolved_fn: Option<String>,
    ) {
        let ty = eval.ty.clone().map(Self::autoderef_type);
        self.annotated_exprs.push(ExprAnnotation {
            span: expr.span,
            ty,
            const_value: eval.const_value.clone(),
            reachable,
            resolved_fn,
            resolved_global: None,
            auto_deref: true,
            c_abi_function: false,
        });

        if let Some(value) = &eval.const_value {
            self.constant_evaluations.push(ConstantEvaluation {
                span: expr.span,
                value: value.clone(),
            });
        }
    }

    /// Update the most recent annotation for `expr` to enable codegen auto-deref.
    /// The stored type is collapsed to the value type so codegen loads the right size.
    fn mark_auto_deref(&mut self, expr: &Expr) {
        for ann in self.annotated_exprs.iter_mut().rev() {
            if ann.span.start == expr.span.start && ann.span.end == expr.span.end {
                ann.auto_deref = true;
                if let Some(ty) = ann.ty.clone() {
                    ann.ty = Some(Self::autoderef_type(ty));
                }
                break;
            }
        }
    }

    /// Check type compatibility and mark the expression for auto-deref when a
    /// reference to a value-like type is used where the value type is expected.
    fn check_expr_compat(&mut self, expr: &Expr, expected: &TypeKind, actual: &TypeKind) -> bool {
        let expected_resolved = self.resolve_type_aliases(expected);
        let actual_resolved = self.resolve_type_aliases(actual);
        if let (
            TypeKind::CFn {
                params: expected_params,
                return_ty: expected_return,
            },
            TypeKind::Fn {
                params: actual_params,
                return_ty: actual_return,
            },
        ) = (&expected_resolved, &actual_resolved)
        {
            let signature_matches = expected_params.len() == actual_params.len()
                && expected_params
                    .iter()
                    .zip(actual_params)
                    .all(|(expected, actual)| self.types_compatible(&expected.node, &actual.node))
                && self.types_compatible(&expected_return.node, &actual_return.node);
            let resolved_function = self
                .annotated_exprs
                .iter()
                .rev()
                .find(|annotation| {
                    annotation.span.start == expr.span.start && annotation.span.end == expr.span.end
                })
                .and_then(|annotation| annotation.resolved_fn.clone())
                .or_else(|| match &expr.node {
                    ExprKind::Ident(name) => self.resolve_bare_fn_name(name),
                    _ => None,
                });
            let exported = resolved_function
                .and_then(|resolved| self.resolve_symbol(&resolved))
                .is_some_and(|symbol| symbol.attributes.iter().any(|attr| attr == "export"));
            if signature_matches && exported {
                for annotation in self.annotated_exprs.iter_mut().rev() {
                    if annotation.span.start == expr.span.start
                        && annotation.span.end == expr.span.end
                    {
                        annotation.ty = Some(expected_resolved);
                        annotation.c_abi_function = true;
                        break;
                    }
                }
                return true;
            }
            return false;
        }
        let ok = self.types_compatible(expected, actual);
        if ok {
            if let TypeKind::Ref { inner } = actual {
                if !matches!(expected, TypeKind::Ref { .. })
                    && Self::is_autoderef_value(expected)
                    && self.types_compatible(expected, &inner.node)
                {
                    self.mark_auto_deref(expr);
                }
            }
        }
        ok
    }

    /// Mark binary operands for auto-deref when one side is a reference and the
    /// other side is a value-like type. Both sides are compared after stripping
    /// a single reference level so `&u64 + u64` loads the referenced value.
    fn mark_binary_autoderef(
        &mut self,
        left: &Expr,
        left_ty: &Option<TypeKind>,
        right: &Expr,
        right_ty: &Option<TypeKind>,
    ) {
        let (Some(l), Some(r)) = (left_ty, right_ty) else {
            return;
        };
        let dl = Self::autoderef_type(l.clone());
        let dr = Self::autoderef_type(r.clone());
        if matches!(l, TypeKind::Ref { .. })
            && Self::is_autoderef_value(&dr)
            && self.types_compatible(&dr, &dl)
        {
            self.mark_auto_deref(left);
        }
        if matches!(r, TypeKind::Ref { .. })
            && Self::is_autoderef_value(&dl)
            && self.types_compatible(&dl, &dr)
        {
            self.mark_auto_deref(right);
        }
    }

    pub(super) fn validate_match_pattern(
        &mut self,
        pattern: &Pattern,
        scrutinee_ty: &Option<TypeKind>,
    ) -> (MatchArmInfo, Vec<String>) {
        match &pattern.node {
            PatternKind::Wildcard | PatternKind::Literal(_) => (
                MatchArmInfo {
                    span: pattern.span,
                    kind: MatchArmKindInfo::Wildcard,
                    has_guard: false,
                },
                Vec::new(),
            ),
            PatternKind::Bind(name) => (
                MatchArmInfo {
                    span: pattern.span,
                    kind: MatchArmKindInfo::Wildcard,
                    has_guard: false,
                },
                vec![name.clone()],
            ),
            PatternKind::Variant {
                enum_name,
                variant,
                sub_patterns,
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
                            if *expected_arity != sub_patterns.len() {
                                self.push_error(
                                    pattern.span,
                                    "S09",
                                    format!(
                                        "variant '{}.{}' expects {} binding(s), got {}",
                                        target_enum,
                                        variant,
                                        expected_arity,
                                        sub_patterns.len()
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

                let bindings = crate::parser::ast::collect_pattern_bindings(sub_patterns);
                (
                    MatchArmInfo {
                        span: pattern.span,
                        kind: MatchArmKindInfo::Variant {
                            enum_name: enum_name.clone(),
                            variant: variant.clone(),
                        },
                        has_guard: false,
                    },
                    bindings,
                )
            }
        }
    }

    pub(super) fn analyze_assign_target(&mut self, target: &Expr) {
        // Strip grouping parentheses so e.g. `(arr[0]) += 1` and `(*p)++` are valid lvalues.
        let mut target = target;
        while let ExprKind::Group(inner) = &target.node {
            target = inner;
        }
        match &target.node {
            ExprKind::Ident(name) if self.resolve_bare_foreign_global_name(name).is_some() => {
                let resolved = self
                    .resolve_bare_foreign_global_name(name)
                    .expect("foreign global resolution changed");
                if self.unsafe_depth == 0 {
                    self.push_error(
                        target.span,
                        "S11",
                        format!("writing foreign global `{name}` requires unsafe context"),
                    );
                }
                let ty = self
                    .resolve_symbol(&resolved)
                    .and_then(|symbol| symbol.ty)
                    .map(|ty| ExprEval {
                        ty: Some(ty),
                        const_value: None,
                    })
                    .unwrap_or_default();
                self.annotate_foreign_global_expr(target, &ty, true, resolved);
            }
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
            ExprKind::Field { object, name } => {
                let object_eval = self.type_check_expr(object, true);
                if let Some(TypeKind::Named {
                    name: aggregate_name,
                    ..
                }) = object_eval.ty
                    && self.repr_c_unions.contains(&aggregate_name)
                    && self.unsafe_depth == 0
                {
                    self.push_error(
                        target.span,
                        "S11",
                        format!("writing union field `{name}` requires unsafe context"),
                    );
                }
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
            ExprKind::Index { object, indices } => {
                let object_eval = self.type_check_expr(object, true);
                if matches!(object_eval.ty, Some(TypeKind::Bytes)) {
                    self.push_error(target.span, "S07", "byte strings are immutable".to_string());
                }
                for idx in indices {
                    self.type_check_expr(idx, true);
                }
            }
            _ => {
                self.type_check_expr(target, true);
                self.push_error(target.span, "S07", "invalid assignment target".to_string());
            }
        }
    }

    pub(super) fn types_compatible(&self, a: &TypeKind, b: &TypeKind) -> bool {
        let a = self.resolve_type_aliases(a);
        let b = self.resolve_type_aliases(b);
        match (&a, &b) {
            (TypeKind::Any, _) | (_, TypeKind::Any) => true,
            // Never (!) is compatible with any type — diverging arms unify with anything
            (TypeKind::Never, _) | (_, TypeKind::Never) => true,
            // Named type ↔ dyn Trait: specific check before the broad Named fallback.
            (TypeKind::Named { name, .. }, TypeKind::Dyn { trait_name })
            | (TypeKind::Dyn { trait_name }, TypeKind::Named { name, .. }) => self
                .trait_impls
                .get(name.as_str())
                .map(|ts| ts.contains(trait_name.as_str()))
                .unwrap_or(false),
            // Broad Named fallback — any other Named combination is considered compatible.
            (TypeKind::Named { .. }, _) | (_, TypeKind::Named { .. }) => true,
            (a, b) if Self::is_integer(a) && Self::is_integer(b) => true,
            // Integer ↔ bool: non-zero integers are truthy, bools are stored as 0/1.
            (a, b) if Self::is_integer(a) && matches!(b, TypeKind::Bool) => true,
            (a, b) if matches!(a, TypeKind::Bool) && Self::is_integer(b) => true,
            (a, b) if Self::is_float(a) && Self::is_float(b) => true,
            // Integer literal passed where float expected (e.g. show[f64](2)) — implicit widening.
            (a, b) if Self::is_float(a) && Self::is_integer(b) => true,
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
            // Array ↔ Slice coercion is not yet implemented at codegen time; passing a
            // fixed-size array where a slice is expected (or vice versa) currently
            // produces incorrect code. Reject it explicitly until the backend supports
            // fat-pointer slice values.
            (TypeKind::Array { elem_ty, .. }, TypeKind::Slice { elem_ty: s_e })
            | (TypeKind::Slice { elem_ty: s_e }, TypeKind::Array { elem_ty, .. }) => {
                let _ = (elem_ty, s_e);
                false
            }
            (TypeKind::Ref { inner: a }, TypeKind::Ref { inner: b }) => {
                self.types_compatible(&a.node, &b.node)
            }
            // Auto-deref: &T is compatible with T for value-like types (primitives, pointers).
            // This lets `&u64` be used where `u64` is expected and vice versa.
            (TypeKind::Ref { inner }, t) | (t, TypeKind::Ref { inner })
                if Self::is_autoderef_value(t) && self.types_compatible(&inner.node, t) =>
            {
                true
            }
            // All raw pointers are mutually compatible in unsafe code — C-style void* semantics.
            // Dereference still requires unsafe {}, so the programmer is responsible.
            (TypeKind::RawPtr { .. }, TypeKind::RawPtr { .. }) => true,
            // Integer ↔ *T: enables null pointer constants (var p: *u8 = 0) and
            // address literals (var p: *u8 = some_usize). Dereference still requires
            // unsafe {}, so the programmer is responsible for correctness.
            (a, TypeKind::RawPtr { .. }) | (TypeKind::RawPtr { .. }, a) if Self::is_integer(a) => {
                true
            }
            (a, TypeKind::CFn { .. }) | (TypeKind::CFn { .. }, a) if Self::is_integer(a) => true,
            (TypeKind::RawPtr { .. }, TypeKind::Ref { .. })
            | (TypeKind::Ref { .. }, TypeKind::RawPtr { .. }) => true,
            // str and &str are interchangeable — both are UTF-8 string views
            (TypeKind::Str, TypeKind::Ref { inner }) | (TypeKind::Ref { inner }, TypeKind::Str)
                if matches!(inner.node, TypeKind::Str) =>
            {
                true
            }
            (
                TypeKind::Fn {
                    params: a_params,
                    return_ty: a_ret,
                },
                TypeKind::Fn {
                    params: b_params,
                    return_ty: b_ret,
                },
            ) => {
                a_params.len() == b_params.len()
                    && a_params
                        .iter()
                        .zip(b_params.iter())
                        .all(|(ap, bp)| self.types_compatible(&ap.node, &bp.node))
                    && self.types_compatible(&a_ret.node, &b_ret.node)
            }
            (
                TypeKind::CFn {
                    params: a_params,
                    return_ty: a_ret,
                },
                TypeKind::CFn {
                    params: b_params,
                    return_ty: b_ret,
                },
            ) => {
                a_params.len() == b_params.len()
                    && a_params
                        .iter()
                        .zip(b_params.iter())
                        .all(|(ap, bp)| self.types_compatible(&ap.node, &bp.node))
                    && self.types_compatible(&a_ret.node, &b_ret.node)
            }
            // dyn A ↔ dyn A
            (TypeKind::Dyn { trait_name: a }, TypeKind::Dyn { trait_name: b }) => a == b,
            _ => std::mem::discriminant(&a) == std::mem::discriminant(&b),
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

    /// Types for which `&T` can be implicitly coerced to `T` (and vice versa).
    pub(super) fn is_autoderef_value(t: &TypeKind) -> bool {
        Self::is_integer(t)
            || Self::is_float(t)
            || matches!(
                t,
                TypeKind::Bool
                    | TypeKind::Str
                    | TypeKind::RawPtr { .. }
                    | TypeKind::CFn { .. }
                    | TypeKind::Ref { .. }
                    | TypeKind::Any
            )
    }

    /// If `t` is `&U` where `U` is a value-like type, return `U`; otherwise return `t`.
    pub(super) fn autoderef_type(t: TypeKind) -> TypeKind {
        if let TypeKind::Ref { ref inner } = t {
            if Self::is_autoderef_value(&inner.node) {
                return inner.node.clone();
            }
        }
        t
    }
}

fn type_contains_rawptr(ty: &TypeKind) -> bool {
    match ty {
        TypeKind::RawPtr { .. } => true,
        TypeKind::Ref { inner } => type_contains_rawptr(&inner.node),
        TypeKind::Array { elem_ty, .. } => type_contains_rawptr(&elem_ty.node),
        TypeKind::FlexibleArray { elem_ty } => type_contains_rawptr(&elem_ty.node),
        TypeKind::Slice { elem_ty } => type_contains_rawptr(&elem_ty.node),
        TypeKind::CFn { .. } => true,
        _ => false,
    }
}

/// Primitive types with a direct C ABI representation. `@repr(C)` aggregates
/// are validated separately because they require their recorded field layout.
fn ffi_primitive(ty: &TypeKind) -> bool {
    matches!(
        ty,
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
            | TypeKind::Float32
            | TypeKind::Float64
            | TypeKind::Bool
            | TypeKind::Void
            | TypeKind::RawPtr { .. }
            | TypeKind::CFn { .. }
    )
}

fn ffi_aggregate_field(ty: &TypeKind) -> bool {
    if ffi_primitive(ty) && !matches!(ty, TypeKind::Void) {
        return true;
    }
    match ty {
        TypeKind::FlexibleArray { elem_ty } => {
            ffi_primitive(&elem_ty.node) && !matches!(elem_ty.node, TypeKind::Void)
        }
        _ => false,
    }
}

fn ffi_integer_bits(ty: &TypeKind) -> Option<usize> {
    match ty {
        TypeKind::Int8 | TypeKind::Uint8 | TypeKind::Bool => Some(8),
        TypeKind::Int16 | TypeKind::Uint16 => Some(16),
        TypeKind::Int32 | TypeKind::Uint32 => Some(32),
        TypeKind::Int64 | TypeKind::Uint64 | TypeKind::Isize | TypeKind::Usize => Some(64),
        _ => None,
    }
}

fn type_contains_any(ty: &TypeKind) -> bool {
    match ty {
        TypeKind::Any => true,
        TypeKind::Ref { inner } => type_contains_any(&inner.node),
        TypeKind::RawPtr { inner } => type_contains_any(&inner.node),
        TypeKind::Array { elem_ty, .. } => type_contains_any(&elem_ty.node),
        TypeKind::FlexibleArray { elem_ty } => type_contains_any(&elem_ty.node),
        TypeKind::Slice { elem_ty } => type_contains_any(&elem_ty.node),
        TypeKind::Named { type_args, .. } => type_args.iter().any(|a| type_contains_any(&a.node)),
        TypeKind::Fn { params, return_ty } | TypeKind::CFn { params, return_ty } => {
            params.iter().any(|param| type_contains_any(&param.node))
                || type_contains_any(&return_ty.node)
        }
        _ => false,
    }
}

/// Substitute generic type parameters with concrete types.
/// Replaces `Named(name, [])` when `name` is a key in `subst`.
pub(super) fn substitute_type_kind(
    ty: &TypeKind,
    subst: &std::collections::HashMap<String, TypeKind>,
) -> TypeKind {
    match ty {
        TypeKind::Named { name, type_args } if type_args.is_empty() => {
            if let Some(concrete) = subst.get(name) {
                concrete.clone()
            } else {
                ty.clone()
            }
        }
        TypeKind::Named { name, type_args } => {
            let new_args: Vec<Type> = type_args
                .iter()
                .map(|a| {
                    let new_node = substitute_type_kind(&a.node, subst);
                    Spanned::new(new_node, a.span)
                })
                .collect();
            TypeKind::Named {
                name: name.clone(),
                type_args: new_args,
            }
        }
        TypeKind::Ref { inner } => TypeKind::Ref {
            inner: Box::new(Spanned::new(
                substitute_type_kind(&inner.node, subst),
                inner.span,
            )),
        },
        TypeKind::RawPtr { inner } => TypeKind::RawPtr {
            inner: Box::new(Spanned::new(
                substitute_type_kind(&inner.node, subst),
                inner.span,
            )),
        },
        TypeKind::Array { elem_ty, len } => TypeKind::Array {
            elem_ty: Box::new(Spanned::new(
                substitute_type_kind(&elem_ty.node, subst),
                elem_ty.span,
            )),
            len: *len,
        },
        TypeKind::FlexibleArray { elem_ty } => TypeKind::FlexibleArray {
            elem_ty: Box::new(Spanned::new(
                substitute_type_kind(&elem_ty.node, subst),
                elem_ty.span,
            )),
        },
        TypeKind::Slice { elem_ty } => TypeKind::Slice {
            elem_ty: Box::new(Spanned::new(
                substitute_type_kind(&elem_ty.node, subst),
                elem_ty.span,
            )),
        },
        TypeKind::Fn { params, return_ty } => TypeKind::Fn {
            params: params
                .iter()
                .map(|param| Spanned::new(substitute_type_kind(&param.node, subst), param.span))
                .collect(),
            return_ty: Box::new(Spanned::new(
                substitute_type_kind(&return_ty.node, subst),
                return_ty.span,
            )),
        },
        TypeKind::CFn { params, return_ty } => TypeKind::CFn {
            params: params
                .iter()
                .map(|param| Spanned::new(substitute_type_kind(&param.node, subst), param.span))
                .collect(),
            return_ty: Box::new(Spanned::new(
                substitute_type_kind(&return_ty.node, subst),
                return_ty.span,
            )),
        },
        other => other.clone(),
    }
}

/// Infer a type substitution by matching a param type (possibly containing generic params)
/// against a concrete arg type. Binds generic param names → concrete types in `subst`.
fn infer_type_subst(
    param_ty: &TypeKind,
    arg_ty: &TypeKind,
    generic_params: &[String],
    subst: &mut std::collections::HashMap<String, TypeKind>,
) {
    match param_ty {
        TypeKind::Named { name, type_args } if type_args.is_empty() => {
            if generic_params.contains(name) {
                subst.entry(name.clone()).or_insert_with(|| arg_ty.clone());
            }
        }
        TypeKind::Named {
            name: pname,
            type_args: pargs,
        } => {
            if let TypeKind::Named {
                name: aname,
                type_args: aargs,
            } = arg_ty
                && pname == aname
            {
                for (pa, aa) in pargs.iter().zip(aargs.iter()) {
                    infer_type_subst(&pa.node, &aa.node, generic_params, subst);
                }
            }
        }
        TypeKind::Slice { elem_ty } => {
            // Variadic param: Slice { elem_ty: T } matched against individual arg types.
            infer_type_subst(&elem_ty.node, arg_ty, generic_params, subst);
        }
        TypeKind::Array { elem_ty, .. } => {
            infer_type_subst(&elem_ty.node, arg_ty, generic_params, subst);
        }
        TypeKind::Ref { inner } => {
            if let TypeKind::Ref { inner: arg_inner } = arg_ty {
                infer_type_subst(&inner.node, &arg_inner.node, generic_params, subst);
            }
        }
        _ => {}
    }
}

/// Build a mangled name for a monomorphized function: `fn_name.<type1>.<type2>`
pub(crate) fn mangle_monomorphized(name: &str, type_args: &[TypeKind]) -> String {
    let type_suffix: Vec<String> = type_args
        .iter()
        .map(|t| format!("{}", t).replace(|c: char| !c.is_alphanumeric(), "_"))
        .collect();
    format!("{}<{}>", name, type_suffix.join(","))
}
