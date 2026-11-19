// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD


use super::*;

impl Analyzer {
    /// Record the resolved internal-ABI value layout of a generic
    /// specialization (or, with empty `type_args`, a concrete function) so
    /// code generation can stop assuming one slot per value, then enforce the
    /// current gate: until the multi-slot internal ABI lands, non-single-slot
    /// parameters and results remain S14 errors. Records are keyed by the
    /// canonical resolved type arguments rather than the lossy mangled name.
    pub(super) fn record_fn_value_layout(
        &mut self,
        function: &str,
        type_args: &[TypeKind],
        params: &[TypeKind],
        return_ty: Option<&TypeKind>,
        variadic: bool,
        span: Span,
    ) {
        if !self.validate_specialization_identity(function, type_args, span) {
            return;
        }
        let key = if type_args.is_empty() {
            function.to_string()
        } else {
            let args = type_args
                .iter()
                .map(|arg| {
                    let resolved = self.resolve_type_aliases(arg);
                    format!("{resolved}")
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{function}<{args}>")
        };
        let (fixed_params, variadic_element) = if variadic && !params.is_empty() {
            (&params[..params.len() - 1], Some(&params[params.len() - 1]))
        } else {
            (params, None)
        };
        let record = crate::runtime_layout::FnValueLayout {
            params: fixed_params
                .iter()
                .map(|param| {
                    crate::runtime_layout::runtime_layout_info(&self.resolve_type_aliases(param))
                })
                .collect(),
            variadic_element: variadic_element.map(|element| {
                crate::runtime_layout::runtime_layout_info(&self.resolve_type_aliases(element))
            }),
            result: return_ty
                .map(|ty| {
                    crate::runtime_layout::runtime_layout_info(&self.resolve_type_aliases(ty))
                })
                .unwrap_or(crate::runtime_layout::LayoutInfo {
                    layout: crate::runtime_layout::RuntimeValueLayout::Empty,
                    move_kind: crate::runtime_layout::MoveKind::Plain,
                }),
        };
        self.fn_value_layouts.insert(key, record);

        // Multi-slot register-block parameters and results are now supported
        // by the internal ABI. Variadic elements remain gated because the
        // call-site expansion still assumes one slot per vararg.
        if let Some(element) = variadic_element {
            let physical_ty = self.resolve_type_aliases(element);
            if !crate::runtime_layout::runtime_value_layout(&physical_ty).fits_single_slot() {
                self.push_error(
                    span,
                    "S14",
                    format!(
                        "specialization `{function}` cannot pass variadic `{physical_ty}` through the current one-slot internal ABI"
                    ),
                );
            }
        }
    }

    /// Type-check one declaration while polling at statement boundaries.
    ///
    /// A cancellation result intentionally skips scope cleanup: callers must
    /// discard this partially populated analyzer rather than publish it.
    /// Module-level `const` items (В20А): each value must be a constant
    /// expression of the declared number, `bool` or `str` type. They are
    /// evaluated in source order before any function body, so functions
    /// anywhere can use them; a const may use the consts declared before it.
    pub(super) fn evaluate_module_constants(&mut self, program: &crate::parser::ast::Program) {
        for item in &program.items {
            let ItemKind::Const { name, ty, value, .. } = &item.node else {
                continue;
            };
            let declared = self.resolve_type_aliases(&ty.node);
            let allowed = Self::is_integer(&declared)
                || Self::is_float(&declared)
                || matches!(declared, TypeKind::Bool | TypeKind::Str)
                || matches!(&declared, TypeKind::Ref { inner } if matches!(inner.node, TypeKind::Str));
            if !allowed {
                self.push_error(
                    ty.span,
                    "S01",
                    format!("module-level const `{name}` must be a number, `bool` or `str`, not `{declared}`"),
                );
                continue;
            }
            let eval = self.type_check_expr_expected(value, true, Some(&declared));
            if let Some(actual) = &eval.ty
                && !self.check_expr_compat(value, &declared, actual)
            {
                self.push_error(
                    value.span,
                    "S01",
                    format!("type mismatch: const `{name}` is `{declared}`, got `{actual}`"),
                );
            }
            let Some(constant) = eval.const_value else {
                self.push_error(
                    value.span,
                    "S06",
                    format!(
                        "module-level const `{name}` needs a constant expression (literals, operators and other consts)"
                    ),
                );
                continue;
            };
            let constant = match (constant, &declared) {
                (ConstValue::Int(raw), TypeKind::Float32 | TypeKind::Float64) => {
                    ConstValue::Float(raw as f64)
                }
                (other, _) => other,
            };
            let register_name = if let Some(module) = self.module_path_for_span(item.span) {
                format!("{}.{}", module, name)
            } else {
                name.clone()
            };
            self.set_symbol_const_value(&register_name, Some(constant));
        }
    }

    pub(super) fn type_check_item(
        &mut self,
        item: &Item,
        checkpoint: &mut dyn FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<(), crate::cancel::Cancelled> {
        checkpoint()?;
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
            return Ok(());
        }
        if let Some(attributes) = attrs {
            for attribute in attributes {
                let field = match attribute.name.as_str() {
                    "no_std" => Some("package.std = false"),
                    "no_crash" => Some("package.crash_handler = false"),
                    "no_mangle" | "no_mangling" => Some("package.mangling = false"),
                    _ => None,
                };
                if let Some(field) = field {
                    self.push_error(
                        attribute.span,
                        "S06",
                        format!(
                            "@{} was removed; configure `{field}` in quazi.toml",
                            attribute.name
                        ),
                    );
                }
                if attribute.name == "test" && !matches!(item.node, ItemKind::Fn { .. }) {
                    self.push_error(
                        attribute.span,
                        "S06",
                        "@test can only annotate a function".to_string(),
                    );
                }
            }
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
                self.current_generic_params.push(generic_params.clone());
                let erased_format_variadic = attributes.iter().any(|a| a.name == "format")
                    && params.last().is_some_and(|param| {
                        param.variadic && matches!(param.ty.node, TypeKind::Any)
                    });
                for (index, param) in params.iter().enumerate() {
                    let allowed_erased_param =
                        erased_format_variadic && param.variadic && index + 1 == params.len();
                    if type_contains_any(&param.ty.node) && !allowed_erased_param {
                        self.push_error(
                            param.ty.span,
                            "S14",
                            format!(
                                "parameter `{}` uses runtime `any`, but Quazi has no tagged dynamic-value representation; use a concrete type, generic parameter, or `dyn Trait`",
                                param.name
                            ),
                        );
                    }
                    if contains_nested_owned_function_value(
                        &self.resolve_type_aliases(&self.source_type(&param.ty)),
                    ) {
                        self.push_error(
                            param.ty.span,
                            "S10",
                            format!(
                                "parameter `{}` cannot contain a Quazi function value before recursive closure cleanup is implemented",
                                param.name
                            ),
                        );
                    }
                }
                if type_contains_any(&return_ty.node) {
                    self.push_error(
                        return_ty.span,
                        "S14",
                        format!(
                            "function `{name}` returns runtime `any`, but Quazi has no tagged dynamic-value representation; use a concrete type, generic parameter, or `dyn Trait`"
                        ),
                    );
                }
                let returned = self.resolve_type_aliases(&self.source_type(return_ty));
                // A plain `&T` result is allowed when every return derives it
                // from a reference parameter (checked at each `ret`, D-014).
                let parameter_reference = matches!(&returned, TypeKind::Ref { inner }
                    if !contains_non_string_reference(&inner.node));
                if contains_non_string_reference(&returned) && !parameter_reference {
                    self.push_error(
                        return_ty.span,
                        "S10",
                        format!(
                            "function `{name}` may return only a plain shared reference `&T` derived from a reference parameter; return an owned value instead"
                        ),
                    );
                }
                if contains_nested_owned_function_value(
                    &self.resolve_type_aliases(&self.source_type(return_ty)),
                ) {
                    self.push_error(
                        return_ty.span,
                        "S10",
                        format!(
                            "function `{name}` cannot return an aggregate containing a Quazi function value before recursive closure cleanup is implemented"
                        ),
                    );
                }
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
                if !is_foreign {
                    // Fixed parameters and results may use the multi-slot
                    // register-block ABI. Variadic parameters remain limited to
                    // one slot until vararg packing is generalized.
                    for param in params {
                        if !param.variadic {
                            continue;
                        }
                        let resolved = self.resolve_type_aliases(&self.source_type(&param.ty));
                        let erased_format_param =
                            erased_format_variadic && matches!(resolved, TypeKind::Any);
                        if !erased_format_param
                            && !crate::runtime_layout::runtime_value_layout(&resolved)
                                .fits_single_slot()
                        {
                            self.push_error(
                                param.ty.span,
                                "S14",
                                format!(
                                    "{} cannot be passed by value in variadic parameter `{}` through the current one-slot internal ABI",
                                    resolved, param.name
                                ),
                            );
                        }
                    }
                }
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
                // Record the resolved internal-ABI layout of ordinary concrete
                // functions. Foreign functions follow the C ABI, and generic
                // templates are recorded per specialization at call sites.
                if !is_foreign && generic_params.is_empty() {
                    let (fixed_params, variadic_element) = match params.last() {
                        Some(last) if last.variadic => {
                            let fixed = &params[..params.len() - 1];
                            if erased_format_variadic {
                                (fixed, None)
                            } else {
                                (fixed, Some(&last.ty.node))
                            }
                        }
                        _ => (&params[..], None),
                    };
                    let record = crate::runtime_layout::FnValueLayout {
                        params: fixed_params
                            .iter()
                            .map(|param| {
                                crate::runtime_layout::runtime_layout_info(
                                    &self.resolve_type_aliases(&self.source_type(&param.ty)),
                                )
                            })
                            .collect(),
                        variadic_element: variadic_element.map(|element| {
                            crate::runtime_layout::runtime_layout_info(
                                &self
                                    .resolve_type_aliases(&self.source_type_at(element, item.span)),
                            )
                        }),
                        result: crate::runtime_layout::runtime_layout_info(
                            &self.resolve_type_aliases(&self.source_type(return_ty)),
                        ),
                    };
                    self.fn_value_layouts.insert(fn_name.clone(), record);
                }
                let prev_module_path = self.current_module_path.clone();
                self.current_module_path = self.module_path_for_span(item.span);
                self.current_function.push(fn_name);
                self.enter_scope();
                let ignores: Vec<String> = extract_attribute_names(attributes)
                    .into_iter()
                    .filter(|name| name == "ignore" || name.starts_with("ignore("))
                    .collect();
                if !ignores.is_empty() {
                    self.warning_ignore_regions.push((item.span, ignores));
                }
                let fn_is_str_variadic = crate::parser::format::is_format_variadic(params, attributes);
                for p in params {
                    if erased_format_variadic && p.variadic && matches!(p.ty.node, TypeKind::Any) {
                        // `@format ...args: any` is an erased call-site convention.
                        // The compiler formats each argument before the call, and the
                        // pseudo-parameter is intentionally unavailable to the body.
                        continue;
                    }
                    let ty = self.source_type(&p.ty);
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
                            span: p.name_span,
                            ty: Some(ty),
                            params: vec![],
                            used: false,
                            initialized: true,
                            is_import: false,
                            import_path: None,
                            const_value: None,
                            variadic: p.variadic,
                            attributes: param_attrs,
                            public: false,
                            unsafe_fn: false,
                            generic_params: vec![],
                        },
                    );
                }

                let expected = self.source_type(return_ty);

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
                            let ty = self.source_type(&p.ty);
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
                                        TypeKind::Str | TypeKind::Ref { .. } | TypeKind::MutRef { .. }
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
                    let previous_return = self.current_return_type.replace(expected.clone());
                    let checked = self.type_check_block(body, Some(&expected), checkpoint);
                    self.current_return_type = previous_return;
                    checked?
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
                let _ = self.current_generic_params.pop();
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
                is_union,
                attributes,
                ..
            } => {
                self.current_generic_params.push(generic_params.clone());
                self.validate_contiguous_elements_attribute(
                    name,
                    generic_params,
                    fields,
                    *is_union,
                    attributes,
                    item.span,
                );
                for field in fields {
                    let field_name = &field.name;
                    let field_ty = &field.ty;
                    if type_contains_any(&field_ty.node) {
                        self.push_error(
                            field_ty.span,
                            "S14",
                            format!(
                                "field `{field_name}` in `{name}` uses runtime `any`, which is unsupported without a tagged representation"
                            ),
                        );
                    }
                    if contains_non_string_reference(&self.resolve_type_aliases(&field_ty.node)) {
                        self.push_error(
                            field_ty.span,
                            "S10",
                            format!(
                                "field `{field_name}` in `{name}` cannot store a shared reference before Quazi has lifetime parameters"
                            ),
                        );
                    }
                    if contains_owned_function_value(&self.resolve_type_aliases(&field_ty.node)) {
                        self.push_error(
                            field_ty.span,
                            "S10",
                            format!(
                                "field `{field_name}` in `{name}` cannot own a Quazi function value before closure environment destruction is recursive"
                            ),
                        );
                    }
                    // Plain Quazi aggregates store one slot per field; C
                    // aggregates have a real layout solver and are exempt.
                    if !self.repr_c_structs.contains(name) {
                        let resolved = self.resolve_type_aliases(&field_ty.node);
                        if !matches!(resolved, TypeKind::Error)
                            && !crate::runtime_layout::runtime_value_layout(&resolved)
                                .fits_single_slot()
                        {
                            self.push_error(
                                field_ty.span,
                                "S14",
                                format!(
                                    "field `{field_name}` in `{name}` cannot store `{resolved}` through the current one-slot aggregate representation"
                                ),
                            );
                        }
                    }
                }
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
                        for (index, field) in fields.iter().enumerate() {
                            let field_name = &field.name;
                            let field_ty = &field.ty;
                            let bit_width = &field.bit_width;
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
                self.current_generic_params.pop();
            }
            // Evaluated before every function by `evaluate_module_constants`.
            ItemKind::Const { .. } => {}
            ItemKind::ForeignGlobal {
                name,
                ty,
                attributes,
                ..
            } => {
                if type_contains_any(&ty.node) {
                    self.push_error(
                        ty.span,
                        "S14",
                        format!(
                            "foreign global `{name}` uses runtime `any`, which has no C ABI representation"
                        ),
                    );
                }
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
            ItemKind::Enum {
                name,
                generic_params,
                variants,
                ..
            } => {
                self.current_generic_params.push(generic_params.clone());
                for variant in variants {
                    for payload in &variant.payload_types {
                        if type_contains_any(&payload.node) {
                            self.push_error(
                                payload.span,
                                "S14",
                                format!(
                                    "payload `{name}.{}` uses runtime `any`, which is unsupported without a tagged representation",
                                    variant.name
                                ),
                            );
                        }
                        if contains_non_string_reference(&self.resolve_type_aliases(&payload.node))
                        {
                            self.push_error(
                                payload.span,
                                "S10",
                                format!(
                                    "payload `{name}.{}` cannot store a shared reference before Quazi has lifetime parameters",
                                    variant.name
                                ),
                            );
                        }
                        if contains_owned_function_value(&self.resolve_type_aliases(&payload.node))
                        {
                            self.push_error(
                                payload.span,
                                "S10",
                                format!(
                                    "payload `{name}.{}` cannot own a Quazi function value before closure environment destruction is recursive",
                                    variant.name
                                ),
                            );
                        }
                        // Enum storage reserves one slot per payload value.
                        let resolved = self.resolve_type_aliases(&payload.node);
                        if !matches!(resolved, TypeKind::Error)
                            && !crate::runtime_layout::runtime_value_layout(&resolved)
                                .fits_single_slot()
                        {
                            self.push_error(
                                payload.span,
                                "S14",
                                format!(
                                    "payload `{name}.{}` cannot store `{resolved}` through the current one-slot enum representation",
                                    variant.name
                                ),
                            );
                        }
                    }
                }
                self.current_generic_params.pop();
            }
            ItemKind::Import(_) => {}
            ItemKind::Trait {
                name,
                generic_params,
                methods,
                ..
            } => {
                self.current_generic_params.push(generic_params.clone());
                for method in methods {
                    self.current_generic_params
                        .push(method.generic_params.clone());
                    for param in &method.params {
                        if type_contains_any(&param.node) {
                            self.push_error(
                                param.span,
                                "S14",
                                format!(
                                    "trait method `{name}.{}` uses runtime `any`; use `Self`, a trait generic, or a concrete type",
                                    method.name
                                ),
                            );
                        }
                        if contains_nested_owned_function_value(
                            &self.resolve_type_aliases(&param.node),
                        ) {
                            self.push_error(
                                param.span,
                                "S10",
                                format!(
                                    "trait method `{name}.{}` cannot store an owned function value inside a parameter aggregate before recursive cleanup is implemented",
                                    method.name
                                ),
                            );
                        }
                    }
                    if type_contains_any(&method.return_ty.node) {
                        self.push_error(
                            method.return_ty.span,
                            "S14",
                            format!(
                                "trait method `{name}.{}` returns runtime `any`; use `Self`, a trait generic, or a concrete type",
                                method.name
                            ),
                        );
                    }
                    // A method with an explicit receiver may return a plain `&T`
                    // derived from it (each implementation's `ret` is checked).
                    let returned = self.resolve_type_aliases(&method.return_ty.node);
                    let receiver_reference = method.param_names.first().is_some_and(|name| name == "self")
                        && matches!(&returned, TypeKind::Ref { inner } if !contains_non_string_reference(&inner.node));
                    if contains_non_string_reference(&returned) && !receiver_reference {
                        self.push_error(
                            method.return_ty.span,
                            "S10",
                            format!(
                                "trait method `{name}.{}` may return only a plain `&T` derived from its `self` receiver",
                                method.name
                            ),
                        );
                    }
                    if contains_nested_owned_function_value(
                        &self.resolve_type_aliases(&method.return_ty.node),
                    ) {
                        self.push_error(
                            method.return_ty.span,
                            "S10",
                            format!(
                                "trait method `{name}.{}` cannot return an aggregate containing an owned function value before recursive cleanup is implemented",
                                method.name
                            ),
                        );
                    }
                    self.current_generic_params.pop();
                }
                self.current_generic_params.pop();
            }
            ItemKind::Impl {
                for_ty,
                trait_ty,
                methods,
                ..
            } => {
                if type_contains_any(&for_ty.node)
                    || trait_ty
                        .as_ref()
                        .is_some_and(|trait_ty| type_contains_any(&trait_ty.node))
                {
                    self.push_error(
                        item.span,
                        "S14",
                        "`any` cannot be used as an implementation target".to_string(),
                    );
                }
                let type_name = crate::semantic::declare::type_kind_base_name(&for_ty.node);
                let impl_generic_params = self.resolved_impl_generic_parameters(&for_ty.node);
                self.current_generic_params.push(impl_generic_params);
                if let Some(trait_ty) = trait_ty {
                    self.validate_trait_impl_conformance(trait_ty, for_ty, methods, item.span);
                }
                for method in methods {
                    if let ItemKind::Fn {
                        name,
                        params,
                        return_ty,
                        attributes,
                        unsafe_fn,
                        c_variadic,
                        ..
                    } = &method.node
                    {
                        self.validate_impl_receiver(for_ty, name, params);
                        if name == "free"
                            && matches!(
                                self.resolve_type_aliases(&for_ty.node),
                                TypeKind::Named { .. }
                            )
                            && params.first().is_some_and(|param| {
                                param.name == "self"
                                    && !matches!(
                                        param.ty.node,
                                        TypeKind::Ref { .. } | TypeKind::MutRef { .. }
                                    )
                            })
                            && !matches!(self.resolve_type_aliases(&return_ty.node), TypeKind::Void)
                        {
                            self.push_error(
                                method.span,
                                "S14",
                                "a consuming `free(self)` destructor must return void; use another method name for ownership transfer or fallible cleanup".to_string(),
                            );
                        }
                        let replacement_attrs = attributes
                            .iter()
                            .filter(|attribute| attribute.name == "contiguous_element_replace")
                            .collect::<Vec<_>>();
                        if !replacement_attrs.is_empty() {
                            let element = match (
                                &for_ty.node,
                                self.contiguous_element_contracts.get(&type_name),
                            ) {
                                (TypeKind::Named { type_args, .. }, Some(contract)) => {
                                    type_args.get(contract.element_param_index)
                                }
                                _ => None,
                            };
                            let valid = replacement_attrs.len() == 1
                                && replacement_attrs[0].args.is_empty()
                                && !unsafe_fn
                                && !c_variadic
                                && matches!(return_ty.node, TypeKind::Void)
                                && params.len() == 3
                                && matches!(params.first().map(|param| &param.ty.node), Some(TypeKind::MutRef { inner }) if self.resolve_type_aliases(&inner.node).to_string() == self.resolve_type_aliases(&for_ty.node).to_string())
                                && matches!(
                                    params.get(1).map(|param| &param.ty.node),
                                    Some(TypeKind::Usize)
                                )
                                && element.is_some_and(|element| {
                                    self.resolve_type_aliases(&params[2].ty.node).to_string()
                                        == self.resolve_type_aliases(&element.node).to_string()
                                });
                            if !valid {
                                self.push_error(
                                    method.span,
                                    "S14",
                                    "@contiguous_element_replace requires one safe, non-variadic method with `self: &Container[...]!`, usize index, declared element value, and void result".to_string(),
                                );
                            }
                        }
                        let removal_attrs = attributes
                            .iter()
                            .filter(|attribute| attribute.name == "contiguous_element_remove")
                            .collect::<Vec<_>>();
                        if !removal_attrs.is_empty() {
                            let element = match (
                                &for_ty.node,
                                self.contiguous_element_contracts.get(&type_name),
                            ) {
                                (TypeKind::Named { type_args, .. }, Some(contract)) => {
                                    type_args.get(contract.element_param_index)
                                }
                                _ => None,
                            };
                            let valid = removal_attrs.len() == 1
                                && removal_attrs[0].args.is_empty()
                                && !unsafe_fn
                                && !c_variadic
                                && params.len() == 2
                                && matches!(params.first().map(|param| &param.ty.node), Some(TypeKind::MutRef { inner }) if self.resolve_type_aliases(&inner.node).to_string() == self.resolve_type_aliases(&for_ty.node).to_string())
                                && matches!(
                                    params.get(1).map(|param| &param.ty.node),
                                    Some(TypeKind::Usize)
                                )
                                && element.is_some_and(|element| {
                                    self.resolve_type_aliases(&return_ty.node).to_string()
                                        == self.resolve_type_aliases(&element.node).to_string()
                                });
                            if !valid {
                                self.push_error(
                                    method.span,
                                    "S14",
                                    "@contiguous_element_remove requires one safe, non-variadic method with `self: &Container[...]!`, usize index, and the declared element result".to_string(),
                                );
                            }
                        }
                        let borrow_attrs = attributes
                            .iter()
                            .filter(|attribute| attribute.name == "contiguous_element_borrow")
                            .collect::<Vec<_>>();
                        if !borrow_attrs.is_empty() {
                            let element = match (
                                &for_ty.node,
                                self.contiguous_element_contracts.get(&type_name),
                            ) {
                                (TypeKind::Named { type_args, .. }, Some(contract)) => {
                                    type_args.get(contract.element_param_index)
                                }
                                _ => None,
                            };
                            let valid = borrow_attrs.len() == 1
                                && borrow_attrs[0].args.is_empty()
                                && !unsafe_fn
                                && !c_variadic
                                && params.len() == 2
                                && matches!(params.first().map(|param| &param.ty.node), Some(TypeKind::Ref { inner }) if self.resolve_type_aliases(&inner.node).to_string() == self.resolve_type_aliases(&for_ty.node).to_string())
                                && matches!(
                                    params.get(1).map(|param| &param.ty.node),
                                    Some(TypeKind::Usize)
                                )
                                && element.is_some_and(|element| {
                                    matches!(&return_ty.node, TypeKind::Ref { inner }
                                        if self.resolve_type_aliases(&inner.node).to_string()
                                            == self.resolve_type_aliases(&element.node).to_string())
                                });
                            if !valid {
                                self.push_error(
                                    method.span,
                                    "S14",
                                    "@contiguous_element_borrow requires one safe, non-variadic method with `self: &Container[...]`, usize index, and a `&Element` result".to_string(),
                                );
                            }
                        }
                        self.current_fn_name_override = Some(format!("{}.{}", type_name, name));
                    }
                    self.type_check_item(method, checkpoint)?;
                }
                let _ = self.current_generic_params.pop();
            }
            // Type aliases are just name bindings — no code to type-check.
            ItemKind::TypeAlias {
                name,
                generic_params,
                aliased_type,
                attributes,
                ..
            } => {
                self.current_generic_params.push(generic_params.clone());
                if type_contains_any(&aliased_type.node) {
                    self.push_error(
                        aliased_type.span,
                        "S14",
                        format!(
                            "type alias `{name}` contains runtime `any`, which is unsupported without a tagged representation"
                        ),
                    );
                }
                if contains_nested_owned_function_value(
                    &self.resolve_type_aliases(&aliased_type.node),
                ) {
                    self.push_error(
                        aliased_type.span,
                        "S10",
                        format!(
                            "type alias `{name}` cannot contain an owned function value before recursive cleanup is implemented"
                        ),
                    );
                }
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
                self.current_generic_params.pop();
            }
        }
        Ok(())
    }

    fn validate_contiguous_elements_attribute(
        &mut self,
        name: &str,
        generic_params: &[String],
        fields: &[AggregateField],
        is_union: bool,
        attributes: &[Attribute],
        item_span: Span,
    ) {
        let attrs = attributes
            .iter()
            .filter(|attribute| attribute.name == "contiguous_elements")
            .collect::<Vec<_>>();
        if attrs.is_empty() {
            return;
        }
        if attrs.len() != 1 {
            self.push_error(
                attrs[1].span,
                "S14",
                format!("@contiguous_elements may appear at most once on `{name}`"),
            );
            return;
        }
        let attr = attrs[0];
        if is_union || self.repr_c_structs.contains(name) {
            self.push_error(
                attr.span,
                "S14",
                format!("@contiguous_elements requires an ordinary non-C struct, not `{name}`"),
            );
            return;
        }

        let mut element = None;
        let mut pointer = None;
        let mut length = None;
        for argument in &attr.args {
            let AttrArg::KeyValue(key, AttrVal::Ident(value)) = argument else {
                self.push_error(
                    attr.span,
                    "S14",
                    format!("@contiguous_elements on `{name}` requires named identifier arguments"),
                );
                return;
            };
            let slot = match key.as_str() {
                "element" => &mut element,
                "pointer" => &mut pointer,
                "length" => &mut length,
                _ => {
                    self.push_error(
                        attr.span,
                        "S14",
                        format!("@contiguous_elements on `{name}` has unknown argument `{key}`"),
                    );
                    return;
                }
            };
            if slot.replace(value.as_str()).is_some() {
                self.push_error(
                    attr.span,
                    "S14",
                    format!("@contiguous_elements on `{name}` repeats `{key}`"),
                );
                return;
            }
        }
        let Some(element) = element else {
            self.push_error(
                attr.span,
                "S14",
                format!("@contiguous_elements on `{name}` requires `element=...`"),
            );
            return;
        };
        let Some(pointer) = pointer else {
            self.push_error(
                attr.span,
                "S14",
                format!("@contiguous_elements on `{name}` requires `pointer=...`"),
            );
            return;
        };
        let Some(length) = length else {
            self.push_error(
                attr.span,
                "S14",
                format!("@contiguous_elements on `{name}` requires `length=...`"),
            );
            return;
        };
        let element_param_index = generic_params
            .iter()
            .position(|parameter| parameter == element);
        if element_param_index.is_none() {
            self.push_error(
                attr.span,
                "S14",
                format!(
                    "@contiguous_elements on `{name}` names `{element}`, which is not a generic parameter"
                ),
            );
        }
        let pointer_valid = matches!(
            fields.iter().find(|field| field.name == pointer),
            Some(field) if matches!(field.ty.node, TypeKind::RawPtr { .. })
        );
        match fields.iter().find(|field| field.name == pointer) {
            Some(field) if matches!(field.ty.node, TypeKind::RawPtr { .. }) => {}
            Some(_) => self.push_error(
                attr.span,
                "S14",
                format!(
                    "@contiguous_elements pointer field `{name}.{pointer}` must have a raw-pointer type"
                ),
            ),
            None => self.push_error(
                attr.span,
                "S14",
                format!(
                    "@contiguous_elements on `{name}` names missing pointer field `{pointer}`"
                ),
            ),
        }
        let length_valid = matches!(
            fields.iter().find(|field| field.name == length),
            Some(field) if matches!(field.ty.node, TypeKind::Usize)
        );
        match fields.iter().find(|field| field.name == length) {
            Some(field) if matches!(field.ty.node, TypeKind::Usize) => {}
            Some(_) => self.push_error(
                attr.span,
                "S14",
                format!("@contiguous_elements length field `{name}.{length}` must have type usize"),
            ),
            None => self.push_error(
                attr.span,
                "S14",
                format!("@contiguous_elements on `{name}` names missing length field `{length}`"),
            ),
        }
        let destructor = format!("{name}.free");
        let destructor_valid = self.consuming_receiver_methods.contains(&destructor);
        if !destructor_valid {
            self.push_error(
                item_span,
                "S14",
                format!(
                    "@contiguous_elements on `{name}` requires consuming `fn free(self: {name}[...])`"
                ),
            );
        }
        if let Some(element_param_index) = element_param_index
            && pointer_valid
            && length_valid
            && destructor_valid
        {
            self.contiguous_element_contracts.insert(
                name.to_string(),
                ContiguousElementContract {
                    element_param_index,
                    pointer_field: pointer.to_string(),
                    length_field: length.to_string(),
                },
            );
        }
    }

    fn validate_trait_impl_conformance(
        &mut self,
        trait_ty: &Type,
        for_ty: &Type,
        methods: &[Item],
        impl_span: Span,
    ) {
        let canonical_trait = Spanned::new(self.source_type(trait_ty), trait_ty.span);
        let trait_ty = &canonical_trait;
        let canonical_target = Spanned::new(self.source_type(for_ty), for_ty.span);
        let for_ty = &canonical_target;
        let trait_name = crate::semantic::declare::type_kind_base_name(&trait_ty.node);
        let Some(signatures) = self.trait_method_signatures.get(&trait_name).cloned() else {
            self.push_error(
                trait_ty.span,
                "S04",
                format!("unknown trait `{trait_name}` in implementation"),
            );
            return;
        };

        let trait_generic_params = self
            .resolve_symbol(&trait_name)
            .map(|symbol| symbol.generic_params)
            .unwrap_or_default();
        let trait_type_args: &[Type] = match &trait_ty.node {
            TypeKind::Named { type_args, .. } => type_args.as_slice(),
            _ => &[],
        };
        if trait_generic_params.len() != trait_type_args.len() {
            self.push_error(
                trait_ty.span,
                "S14",
                format!(
                    "trait `{trait_name}` expects {} type argument(s), got {}",
                    trait_generic_params.len(),
                    trait_type_args.len()
                ),
            );
            return;
        }
        let substitution: std::collections::HashMap<String, TypeKind> = trait_generic_params
            .iter()
            .zip(trait_type_args)
            .map(|(name, ty)| (name.clone(), ty.node.clone()))
            .collect();

        for (method_name, signature) in signatures {
            let implementation = methods.iter().find(|method| {
                matches!(
                    &method.node,
                    ItemKind::Fn { name, attributes, .. }
                        if name == &method_name && super::item_should_include(attributes)
                )
            });
            let Some(implementation) = implementation else {
                self.push_error(
                    impl_span,
                    "S14",
                    format!("implementation of `{trait_name}` is missing method `{method_name}`"),
                );
                continue;
            };
            let ItemKind::Fn {
                params,
                return_ty,
                generic_params,
                attributes,
                unsafe_fn,
                ..
            } = &implementation.node
            else {
                unreachable!();
            };
            self.current_generic_params.push(generic_params.clone());

            if *unsafe_fn {
                self.push_error(
                    implementation.span,
                    "S14",
                    format!(
                        "method `{trait_name}.{method_name}` is safe in the trait declaration and cannot be implemented as unsafe"
                    ),
                );
            }

            if *generic_params != signature.generic_params {
                self.push_error(
                    implementation.span,
                    "S14",
                    format!(
                        "method `{trait_name}.{method_name}` generic parameters do not match the trait declaration"
                    ),
                );
            }

            let mut expected_params = Vec::new();
            if !signature.has_explicit_receiver && !signature.associated {
                expected_params.push(for_ty.node.clone());
            }
            if signature.associated {
                // Its result must not borrow from its arguments (checked after
                // result retention), so bounded `T.method(...)` calls are fresh.
                self.associated_impl_methods.push((
                    super::declare::type_kind_base_name(&for_ty.node),
                    method_name.clone(),
                    implementation.span,
                    substitute_self_type(&return_ty.node, &for_ty.node),
                ));
            }
            expected_params.extend(signature.params.iter().map(|param| {
                let substituted = substitute_type_kind(param, &substitution);
                substitute_self_type(&substituted, &for_ty.node)
            }));
            let erased_format_tail = attributes
                .iter()
                .any(|attribute| attribute.name == "format")
                && params
                    .last()
                    .is_some_and(|param| param.variadic && matches!(param.ty.node, TypeKind::Any));
            let runtime_param_count = params.len() - usize::from(erased_format_tail);
            let method_binders = self
                .current_generic_params
                .iter()
                .flatten()
                .chain(generic_params)
                .cloned()
                .collect::<Vec<_>>();
            let actual_params: Vec<TypeKind> = params[..runtime_param_count]
                .iter()
                .map(|param| {
                    self.canonical_source_type(&param.ty.node, &method_binders, param.ty.span)
                })
                .collect();
            if expected_params.len() != actual_params.len() {
                self.push_error(
                    implementation.span,
                    "S14",
                    format!(
                        "method `{trait_name}.{method_name}` expects {} parameter(s) including its receiver, implementation declares {}",
                        expected_params.len(),
                        actual_params.len()
                    ),
                );
            } else {
                for (index, (expected, actual)) in
                    expected_params.iter().zip(&actual_params).enumerate()
                {
                    if !self.types_have_same_runtime_shape(expected, actual) {
                        self.push_error(
                            params[index].ty.span,
                            "S14",
                            format!(
                                "method `{trait_name}.{method_name}` parameter {} must be `{expected}`, got `{actual}`",
                                index + 1
                            ),
                        );
                    }
                }
            }

            let expected_return = substitute_self_type(
                &substitute_type_kind(&signature.return_ty, &substitution),
                &for_ty.node,
            );
            let actual_return =
                self.canonical_source_type(&return_ty.node, &method_binders, return_ty.span);
            if !self.types_have_same_runtime_shape(&expected_return, &actual_return) {
                self.push_error(
                    return_ty.span,
                    "S14",
                    format!(
                        "method `{trait_name}.{method_name}` must return `{expected_return}`, got `{}`",
                        return_ty.node
                    ),
                );
            }
            self.current_generic_params.pop();
        }
    }

    fn validate_impl_receiver(&mut self, for_ty: &Type, method_name: &str, params: &[Param]) {
        let Some(receiver) = params.first().filter(|param| param.name == "self") else {
            return;
        };
        let receiver_ty = match &receiver.ty.node {
            TypeKind::Ref { inner } | TypeKind::MutRef { inner } => &inner.node,
            ty => ty,
        };
        if !self.types_compatible(&for_ty.node, receiver_ty) {
            self.push_error(
                receiver.ty.span,
                "S14",
                format!(
                    "receiver `self` of method `{method_name}` must be `{}`, `&{}`, or `&{}!`, got {}",
                    for_ty.node, for_ty.node, for_ty.node, receiver.ty.node
                ),
            );
        }
    }

    /// Returns the explicit receiver capabilities of an inherent method for a
    /// concrete receiver type. Imports can retain a qualified display name
    /// while impl symbols use the declaring leaf name, so both canonical keys
    /// are considered here rather than in each method-call check.
    fn explicit_method_receiver_capabilities(
        &self,
        receiver_ty: &TypeKind,
        method: &str,
    ) -> (bool, bool) {
        if let Some(capabilities) = self.bounded_receiver_capabilities(receiver_ty, method) {
            return capabilities;
        }
        let resolved = self.resolve_type_aliases(receiver_ty);
        let type_name = super::declare::type_kind_base_name(&resolved);
        let declared_type_name = type_name.rsplit('.').next().unwrap_or(type_name.as_str());
        let method_names = [
            format!("{}.{}", type_name, method),
            format!("{}.{}", declared_type_name, method),
        ];
        (
            method_names
                .iter()
                .any(|name| self.explicit_shared_receiver_methods.contains(name)),
            method_names
                .iter()
                .any(|name| self.explicit_exclusive_receiver_methods.contains(name)),
        )
    }

    /// A method reached through a generic bound has the receiver its trait
    /// declares: `self: &Self` is shared, `self: &mut Self` exclusive, and an
    /// implicit receiver is the value itself.
    fn bounded_receiver_capabilities(
        &self,
        receiver_ty: &TypeKind,
        method: &str,
    ) -> Option<(bool, bool)> {
        let receiver = match self.resolve_type_aliases(receiver_ty) {
            TypeKind::Ref { inner } | TypeKind::MutRef { inner } => inner.node.clone(),
            other => other,
        };
        let TypeKind::Named { name: param, type_args } = &receiver else {
            return None;
        };
        if !type_args.is_empty()
            || !self.current_generic_params.iter().any(|scope| scope.contains(param))
        {
            return None;
        }
        let signature = self
            .current_generic_bounds()
            .into_iter()
            .filter(|(bounded, _)| bounded == param)
            .find_map(|(_, bound)| {
                self.bound_trait_signatures(&bound)
                    .and_then(|methods| methods.get(method).cloned())
            })?;
        if !signature.has_explicit_receiver {
            return Some((false, false));
        }
        Some(match signature.params.first().map(|ty| self.resolve_type_aliases(ty)) {
            Some(TypeKind::Ref { .. }) => (true, false),
            Some(TypeKind::MutRef { .. }) => (false, true),
            _ => (false, false),
        })
    }

    fn require_exclusive_receiver(
        &mut self,
        object: &Expr,
        object_ty: Option<&TypeKind>,
        method: &str,
    ) {
        let Some(object_ty) = object_ty else {
            return;
        };
        let (_, exclusive_receiver_is_explicit) =
            self.explicit_method_receiver_capabilities(object_ty, method);
        let object_is_exclusive_reference = matches!(
            self.resolve_type_aliases(object_ty),
            TypeKind::MutRef { .. }
        );
        if exclusive_receiver_is_explicit
            && !object_is_exclusive_reference
            && !self.is_exclusive_receiver_place(object)
        {
            self.push_error(
                object.span,
                "S07",
                "exclusive receiver requires a mutable local variable or parameter".to_string(),
            );
        }
        if exclusive_receiver_is_explicit
            && !object_is_exclusive_reference
            && !self.receiver_has_slot_storage(object)
        {
            self.push_error(
                object.span,
                "S14",
                "exclusive receiver requires value-slot storage; narrow C fields and bitfields cannot back safe references".to_string(),
            );
        }
    }

    fn receiver_has_slot_storage(&self, expr: &Expr) -> bool {
        let ExprKind::Field { object, name } = &expr.node else {
            return match &expr.node {
                ExprKind::Group(inner) => self.receiver_has_slot_storage(inner),
                _ => true,
            };
        };
        let Some(aggregate) = self.receiver_place_type(object) else {
            return true;
        };
        let aggregate = self.resolve_type_aliases(&aggregate);
        let aggregate_name = super::declare::type_kind_base_name(&aggregate);
        if !self.repr_c_structs.contains(&aggregate_name) {
            return true;
        }
        if self
            .struct_field_bit_widths
            .get(&aggregate_name)
            .is_some_and(|fields| {
                fields
                    .iter()
                    .any(|(field, width)| field == name && width.is_some())
            })
        {
            return false;
        }
        self.receiver_place_type(expr).is_some_and(|ty| {
            matches!(
                self.resolve_type_aliases(&ty),
                TypeKind::Int64
                    | TypeKind::Uint64
                    | TypeKind::Isize
                    | TypeKind::Usize
                    | TypeKind::Float64
                    | TypeKind::RawPtr { .. }
                    | TypeKind::CFn { .. }
            )
        })
    }

    fn is_exclusive_receiver_place(&self, expr: &Expr) -> bool {
        match &expr.node {
            ExprKind::Group(inner) => self.is_exclusive_receiver_place(inner),
            ExprKind::Field { object, name } => {
                self.is_exclusive_receiver_place(object)
                    && !self.receiver_field_is_const(object, name)
            }
            ExprKind::Ident(name) => self.resolve_symbol(name).is_some_and(|symbol| {
                matches!(
                    symbol.kind,
                    SymbolKind::Variable { mutable: true } | SymbolKind::Parameter
                )
            }),
            _ => false,
        }
    }

    fn receiver_field_is_const(&self, object: &Expr, field: &str) -> bool {
        let Some(object_ty) = self.receiver_place_type(object) else {
            return false;
        };
        let resolved = self.resolve_type_aliases(&object_ty);
        let type_name = super::declare::type_kind_base_name(&resolved);
        self.struct_const_fields
            .get(&type_name)
            .is_some_and(|fields| fields.contains(field))
    }

    fn receiver_place_type(&self, expr: &Expr) -> Option<TypeKind> {
        match &expr.node {
            ExprKind::Ident(name) => self.resolve_symbol(name).and_then(|symbol| symbol.ty),
            ExprKind::Group(inner) => self.receiver_place_type(inner),
            ExprKind::Field { object, name } => {
                let object_ty = self.receiver_place_type(object)?;
                let resolved = self.resolve_type_aliases(&object_ty);
                let aggregate_ty = match resolved {
                    TypeKind::Ref { inner } | TypeKind::MutRef { inner } => {
                        self.resolve_type_aliases(&inner.node)
                    }
                    other => other,
                };
                let TypeKind::Named {
                    name: struct_name,
                    type_args,
                } = aggregate_ty
                else {
                    return None;
                };
                let field_ty = self
                    .struct_defs
                    .get(&struct_name)?
                    .iter()
                    .find(|(field_name, _)| field_name == name)
                    .map(|(_, field_ty)| field_ty.clone())?;
                let generic_params = self
                    .struct_generic_params
                    .get(&struct_name)
                    .cloned()
                    .unwrap_or_default();
                if generic_params.is_empty() || type_args.is_empty() {
                    Some(field_ty)
                } else {
                    let substitution = generic_params
                        .iter()
                        .zip(type_args.iter())
                        .map(|(parameter, argument)| (parameter.clone(), argument.node.clone()))
                        .collect();
                    Some(substitute_type_kind(&field_ty, &substitution))
                }
            }
            _ => None,
        }
    }

    fn types_have_same_runtime_shape(&self, expected: &TypeKind, actual: &TypeKind) -> bool {
        let expected = self.resolve_type_aliases(expected);
        let actual = self.resolve_type_aliases(actual);
        match (&expected, &actual) {
            (TypeKind::Str, TypeKind::Ref { inner }) | (TypeKind::Ref { inner }, TypeKind::Str)
                if matches!(inner.node, TypeKind::Str) =>
            {
                true
            }
            (
                TypeKind::Named {
                    name: expected_name,
                    type_args: expected_args,
                },
                TypeKind::Named {
                    name: actual_name,
                    type_args: actual_args,
                },
            ) => {
                expected_name == actual_name
                    && expected_args.len() == actual_args.len()
                    && expected_args
                        .iter()
                        .zip(actual_args)
                        .all(|(expected, actual)| {
                            self.types_have_same_runtime_shape(&expected.node, &actual.node)
                        })
            }
            (TypeKind::Ref { inner: expected }, TypeKind::Ref { inner: actual })
            | (TypeKind::Ref { inner: expected }, TypeKind::MutRef { inner: actual })
            | (TypeKind::MutRef { inner: expected }, TypeKind::MutRef { inner: actual })
            | (TypeKind::RawPtr { inner: expected }, TypeKind::RawPtr { inner: actual })
            | (TypeKind::Slice { elem_ty: expected }, TypeKind::Slice { elem_ty: actual })
            | (
                TypeKind::FlexibleArray { elem_ty: expected },
                TypeKind::FlexibleArray { elem_ty: actual },
            ) => self.types_have_same_runtime_shape(&expected.node, &actual.node),
            (
                TypeKind::Array {
                    elem_ty: expected,
                    len: expected_len,
                },
                TypeKind::Array {
                    elem_ty: actual,
                    len: actual_len,
                },
            ) => {
                expected_len == actual_len
                    && self.types_have_same_runtime_shape(&expected.node, &actual.node)
            }
            (
                TypeKind::Fn {
                    params: expected_params,
                    return_ty: expected_return,
                },
                TypeKind::Fn {
                    params: actual_params,
                    return_ty: actual_return,
                },
            )
            | (
                TypeKind::CFn {
                    params: expected_params,
                    return_ty: expected_return,
                },
                TypeKind::CFn {
                    params: actual_params,
                    return_ty: actual_return,
                },
            ) => {
                expected_params.len() == actual_params.len()
                    && expected_params
                        .iter()
                        .zip(actual_params)
                        .all(|(expected, actual)| {
                            self.types_have_same_runtime_shape(&expected.node, &actual.node)
                        })
                    && self
                        .types_have_same_runtime_shape(&expected_return.node, &actual_return.node)
            }
            (
                TypeKind::Dyn {
                    trait_name: expected,
                },
                TypeKind::Dyn { trait_name: actual },
            ) => expected == actual,
            _ => std::mem::discriminant(&expected) == std::mem::discriminant(&actual),
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
        let mut test_attr: Option<&Attribute> = None;

        for attr in attributes {
            match attr.name.as_str() {
                "syscall" => syscall_attr = Some(attr),
                "api" => api_attr = Some(attr),
                "export" => export_attr = Some(attr),
                "test" => test_attr = Some(attr),
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
            if params.len() > 6 {
                self.push_error(
                    item_span,
                    "S14",
                    format!(
                        "@syscall function `{name}` has {} parameters; x86_64 syscalls support at most 6",
                        params.len()
                    ),
                );
            }
            if !generic_params.is_empty() {
                self.push_error(
                    item_span,
                    "S14",
                    format!("@syscall function `{name}` cannot be generic"),
                );
            }
            for param in params {
                if !syscall_abi_type(&param.ty.node, false) {
                    self.push_error(
                        param.ty.span,
                        "S14",
                        format!(
                            "@syscall function `{name}` has unsupported parameter type `{}`",
                            param.ty.node
                        ),
                    );
                }
            }
            if !syscall_abi_type(&return_ty.node, true) {
                self.push_error(
                    return_ty.span,
                    "S14",
                    format!(
                        "@syscall function `{name}` has unsupported return type `{}`",
                        return_ty.node
                    ),
                );
            }
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

        if let Some(attr) = test_attr {
            if !attr.args.is_empty() {
                self.push_error(attr.span, "S06", "@test takes no arguments".to_string());
            }
            if body.is_none() {
                self.push_error(
                    item_span,
                    "S06",
                    format!("@test function `{name}` must have a body"),
                );
            }
            if name == "main" {
                self.push_error(
                    item_span,
                    "S06",
                    "@test function cannot be named `main`; that name is reserved for the test harness"
                        .to_string(),
                );
            }
            if !generic_params.is_empty()
                || !params.is_empty()
                || !matches!(return_ty.node, TypeKind::Void)
            {
                self.push_error(
                    item_span,
                    "S06",
                    format!("@test function `{name}` must have signature `fn {name}() void`"),
                );
            }
            if syscall_attr.is_some() || api_attr.is_some() || export_attr.is_some() {
                self.push_error(
                    attr.span,
                    "S06",
                    "@test cannot be combined with FFI attributes".to_string(),
                );
            }
        }
    }

    fn validate_syscall_attr(&mut self, attr: &Attribute) {
        let ok = match attr.args.as_slice() {
            [AttrArg::Positional(AttrVal::Str(_))] => true,
            [AttrArg::Positional(AttrVal::Int(n))] => *n <= u16::MAX as u64,
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
        checkpoint: &mut dyn FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<bool, crate::cancel::Cancelled> {
        self.enter_scope();
        let mut reachable = true;
        let mut guaranteed_return = false;

        for stmt in &block.stmts {
            if !reachable {
                // Keep type pass clean: dead-code diagnostics are emitted in dedicated pass.
                continue;
            }

            if self.type_check_stmt(stmt, expected_return, checkpoint)? {
                reachable = false;
                guaranteed_return = true;
            }
        }

        self.exit_scope_collect();
        Ok(guaranteed_return)
    }

    pub(super) fn type_check_stmt(
        &mut self,
        stmt: &Stmt,
        expected_return: Option<&TypeKind>,
        checkpoint: &mut dyn FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<bool, crate::cancel::Cancelled> {
        let terminal = self.type_check_stmt_inner(stmt, expected_return, checkpoint)?;
        if terminal {
            self.terminal_statements
                .insert((stmt.span.start, stmt.span.end));
        }
        Ok(terminal)
    }

    fn type_check_stmt_inner(
        &mut self,
        stmt: &Stmt,
        expected_return: Option<&TypeKind>,
        checkpoint: &mut dyn FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<bool, crate::cancel::Cancelled> {
        checkpoint()?;
        Ok(match &stmt.node {
            StmtKind::Var {
                name,
                value,
                ty,
                attributes,
                ..
            } => {
                let declared_ty = ty.as_ref().map(|t| t.node.clone());
                let value_eval = value
                    .as_ref()
                    .map(|v| self.type_check_expr_expected(v, true, declared_ty.as_ref()))
                    .unwrap_or_default();

                if value_eval.ty.as_ref().is_some_and(|value_ty| {
                    matches!(
                        self.resolve_type_aliases(value_ty),
                        TypeKind::Ref { ref inner } | TypeKind::MutRef { ref inner }
                            if !matches!(inner.node, TypeKind::Str)
                    )
                }) && value
                    .as_ref()
                    .is_some_and(|value| !is_lexical_reference_expr(value))
                {
                    self.push_error(
                        stmt.span,
                        "S10",
                        format!(
                            "shared reference `{name}` must be initialized directly from a local, parameter, or existing reference"
                        ),
                    );
                }

                if let Some(annotation) = ty
                    && type_contains_any(&annotation.node)
                {
                    self.push_error(
                        annotation.span,
                        "S14",
                        format!(
                            "variable `{name}` uses runtime `any`, but Quazi has no tagged dynamic-value representation"
                        ),
                    );
                }
                if declared_ty
                    .as_ref()
                    .or(value_eval.ty.as_ref())
                    .is_some_and(contains_nested_non_string_reference)
                {
                    self.push_error(
                        stmt.span,
                        "S10",
                        format!(
                            "variable `{name}` cannot own a value containing shared references before lifetimes are tracked"
                        ),
                    );
                }
                if declared_ty
                    .as_ref()
                    .or(value_eval.ty.as_ref())
                    .is_some_and(contains_nested_owned_function_value)
                {
                    self.push_error(
                        stmt.span,
                        "S10",
                        format!(
                            "variable `{name}` cannot store a Quazi function value inside an aggregate before recursive closure cleanup is implemented"
                        ),
                    );
                }
                if let (Some(ann), Some(val)) = (&declared_ty, &value_eval.ty) {
                    self.request_dyn_vtable_specializations(ann, val);
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
                let declared_ty = ty.as_ref().map(|t| t.node.clone());
                let value_eval = self.type_check_expr_expected(value, true, declared_ty.as_ref());
                if let (Some(declared), Some(actual)) = (&declared_ty, &value_eval.ty) {
                    self.request_dyn_vtable_specializations(declared, actual);
                }
                if value_eval.ty.as_ref().is_some_and(|value_ty| {
                    matches!(
                        self.resolve_type_aliases(value_ty),
                        TypeKind::Ref { ref inner } | TypeKind::MutRef { ref inner }
                            if !matches!(inner.node, TypeKind::Str)
                    )
                }) && !is_lexical_reference_expr(value)
                {
                    self.push_error(
                        stmt.span,
                        "S10",
                        format!(
                            "shared reference `{name}` must be initialized directly from a local, parameter, or existing reference"
                        ),
                    );
                }
                if let Some(annotation) = ty
                    && type_contains_any(&annotation.node)
                {
                    self.push_error(
                        annotation.span,
                        "S14",
                        format!(
                            "constant `{name}` uses runtime `any`, but Quazi has no tagged dynamic-value representation"
                        ),
                    );
                }
                if declared_ty
                    .as_ref()
                    .or(value_eval.ty.as_ref())
                    .is_some_and(contains_nested_non_string_reference)
                {
                    self.push_error(
                        stmt.span,
                        "S10",
                        format!(
                            "constant `{name}` cannot own a value containing shared references before lifetimes are tracked"
                        ),
                    );
                }
                if declared_ty
                    .as_ref()
                    .or(value_eval.ty.as_ref())
                    .is_some_and(contains_nested_owned_function_value)
                {
                    self.push_error(
                        stmt.span,
                        "S10",
                        format!(
                            "constant `{name}` cannot store a Quazi function value inside an aggregate before recursive closure cleanup is implemented"
                        ),
                    );
                }

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
                        let returns_reference = matches!(self.resolve_type_aliases(expected), TypeKind::Ref { ref inner }
                            if !matches!(inner.node, TypeKind::Str));
                        if !returns_reference && address_of_ident(return_expr).is_some() {
                            self.push_error(
                                return_expr.span,
                                "S10",
                                "cannot return the address of a stack local; return an owned value"
                                    .to_string(),
                            );
                        }
                        let actual = self
                            .type_check_expr_expected(return_expr, true, Some(expected))
                            .ty;
                        // Checked after typing, so calls' argument types are known.
                        if returns_reference && !self.is_parameter_reference(return_expr) {
                            self.push_error(
                                return_expr.span,
                                "S10",
                                "a returned reference must be a reference parameter, `&parameter.field` reached through one, or a reference-returning call whose reference arguments are such; its caller then keeps the borrowed argument alive"
                                    .to_string(),
                            );
                        }
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
            } => self.type_check_if_initialization(
                condition,
                then_block,
                else_if,
                else_block.as_ref(),
                expected_return,
                checkpoint,
            )?,
            StmtKind::For { kind, body } => {
                self.enter_scope();
                match kind {
                    ForLoop::Cond { condition } => {
                        let terminal = self.type_check_loop_initialization(
                            body,
                            condition.as_ref(),
                            None,
                            false,
                            expected_return,
                            checkpoint,
                        )?;
                        self.exit_scope_collect();
                        return Ok(terminal);
                    }
                    ForLoop::CStyle {
                        init,
                        condition,
                        update,
                    } => {
                        if let Some(init_stmt) = init {
                            self.type_check_stmt(init_stmt, expected_return, checkpoint)?;
                        }
                        let terminal = self.type_check_loop_initialization(
                            body,
                            condition.as_ref(),
                            update.as_ref(),
                            false,
                            expected_return,
                            checkpoint,
                        )?;
                        self.exit_scope_collect();
                        return Ok(terminal);
                    }
                    ForLoop::Each {
                        vars,
                        var_spans,
                        iter,
                    } => {
                        let loop_var_ty = match iter {
                            ForIter::Range { start, end, .. } => {
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
                                // `0..n` counts in the type of `n`: a literal
                                // bound takes the other bound's type.
                                if crate::evaluation::is_integer_literal_expression(start) {
                                    end_eval.ty.or(start_eval.ty).unwrap_or(TypeKind::Int32)
                                } else {
                                    start_eval.ty.or(end_eval.ty).unwrap_or(TypeKind::Int32)
                                }
                            }
                            ForIter::Iter(expr) => {
                                let iter_eval = self.type_check_expr(expr, true);
                                let iter_ty = iter_eval.ty.as_ref().map(|t| {
                                    if let TypeKind::Ref { inner } | TypeKind::MutRef { inner } = t
                                    {
                                        &inner.node
                                    } else {
                                        t
                                    }
                                });
                                let iterator_item = iter_ty.and_then(|ty| self.iterator_item_type(ty));
                                if iterator_item.is_some()
                                    && let Some(TypeKind::Named { name, type_args }) =
                                        iter_ty.map(|ty| self.resolve_type_aliases(ty))
                                {
                                    let from = self
                                        .current_function
                                        .last()
                                        .cloned()
                                        .unwrap_or_else(|| "__program__".to_string());
                                    for method in ["has_next", "next"] {
                                        let base = format!("{name}.{method}");
                                        let target = if type_args.is_empty() {
                                            base
                                        } else {
                                            let arguments: Vec<TypeKind> =
                                                type_args.iter().map(|arg| arg.node.clone()).collect();
                                            mangle_monomorphized(&base, &arguments)
                                        };
                                        self.add_dependency_edge(DependencyKind::Call, &from, &target);
                                    }
                                }
                                let prelude_array = iter_ty
                                    .map(|ty| self.resolve_type_aliases(ty))
                                    .and_then(|ty| match ty {
                                        TypeKind::Named { name, type_args }
                                            if name == "Array" && type_args.len() == 1 =>
                                        {
                                            Some(type_args[0].node.clone())
                                        }
                                        _ => None,
                                    });
                                if let Some(element) = &prelude_array {
                                    self.record_array_loop(expr, iter_eval.ty.as_ref(), element);
                                }
                                match iter_ty {
                                    Some(TypeKind::Array { elem_ty, .. }) => elem_ty.node.clone(),
                                    Some(TypeKind::Slice { elem_ty }) => elem_ty.node.clone(),
                                    Some(_) if prelude_array.is_some() => {
                                        prelude_array.clone().expect("array element")
                                    }
                                    Some(_) if iterator_item.is_some() => {
                                        iterator_item.expect("iterator item")
                                    }
                                    Some(t) => {
                                        self.push_error(
                                            expr.span,
                                            "S01",
                                            format!(
                                                "cannot iterate over type '{}'; expected array or slice",
                                                t
                                            ),
                                        );
                                        TypeKind::Error
                                    }
                                    None => TypeKind::Error,
                                }
                            }
                        };
                        // `for value : items` or `for index, value : items`.
                        let (value_position, index_position) =
                            if vars.len() >= 2 { (1, Some(0)) } else { (0, None) };
                        if let Some(name) = vars.get(value_position) {
                            self.declare(
                                name.clone(),
                                Symbol {
                                    kind: SymbolKind::Variable { mutable: true },
                                    ty: Some(loop_var_ty.clone()),
                                    span: var_spans
                                        .get(value_position)
                                        .copied()
                                        .unwrap_or(stmt.span),
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
                        if let Some(name) = index_position.and_then(|position| vars.get(position)) {
                            self.declare(
                                name.clone(),
                                Symbol {
                                    kind: SymbolKind::Variable { mutable: true },
                                    ty: Some(TypeKind::Usize),
                                    span: index_position
                                        .and_then(|position| var_spans.get(position))
                                        .copied()
                                        .unwrap_or(stmt.span),
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
                        self.type_check_loop_initialization(
                            body,
                            None,
                            None,
                            true,
                            expected_return,
                            checkpoint,
                        )?;
                        self.exit_scope_collect();
                        return Ok(false);
                    }
                }
            }
            StmtKind::Break => {
                self.record_initialization_loop_exit(true);
                if self.loop_depth == 0 {
                    self.push_error(stmt.span, "S11", "break outside of loop".to_string());
                }
                true
            }
            StmtKind::Continue => {
                self.record_initialization_loop_exit(false);
                if self.loop_depth == 0 {
                    self.push_error(stmt.span, "S11", "continue outside of loop".to_string());
                }
                true
            }
            StmtKind::UnsafeBlock { body } => {
                self.unsafe_depth += 1;
                let terminal = self.type_check_block(body, expected_return, checkpoint)?;
                self.unsafe_depth -= 1;
                terminal
            }
            StmtKind::ExprStmt(expr) => {
                // A call that never returns (`panic(...)`, any `!` function)
                // ends the path like `ret`.
                let eval = self.type_check_expr(expr, true);
                matches!(eval.ty, Some(TypeKind::Never))
            }
            StmtKind::CfgBlock { body, condition } => {
                if super::item_should_include(std::slice::from_ref(condition)) {
                    self.type_check_block(body, expected_return, checkpoint)?
                } else {
                    false
                }
            }
        })
    }

    fn type_check_expr_expected(
        &mut self,
        expr: &Expr,
        reachable: bool,
        expected: Option<&TypeKind>,
    ) -> ExprEval {
        // `Error` marks a parameter whose type still mentions an uninferred
        // binder: the argument is checked on its own and inference follows.
        let expected = expected.filter(|ty| !matches!(ty, TypeKind::Error));
        if let Some(eval) = self.type_check_contextual_none_value(expr, reachable, expected) {
            self.reject_nested_owned_function_expression(expr, &eval);
            return eval;
        }
        if let Some(eval) = self.type_check_contextual_sum_constructor(expr, reachable, expected) {
            self.reject_nested_owned_function_expression(expr, &eval);
            return eval;
        }
        if let ExprKind::Closure { params, body, .. } = &expr.node {
            let eval = self.type_check_closure_expr(expr, params, body, reachable, expected);
            self.reject_nested_owned_function_expression(expr, &eval);
            return eval;
        }
        if let Some(expected) = expected {
            self.contextual_expected_types.push(expected.clone());
        }
        let eval = self.type_check_expr(expr, reachable);
        if expected.is_some() {
            self.contextual_expected_types.pop();
        }
        eval
    }

    fn reject_nested_owned_function_expression(&mut self, expr: &Expr, eval: &ExprEval) {
        let constructs_qualified_sum = match &expr.node {
            ExprKind::MethodCall {
                object,
                method,
                args,
                named_args,
                ..
            } if matches!(&object.node, ExprKind::Ident(name) if name == "Option" || name == "Result")
                && matches!(method.as_str(), "Some" | "Ok" | "Err") =>
            {
                args.iter()
                    .chain(named_args.iter().map(|(_, argument)| argument))
                    .any(|argument| self.expression_has_owned_function_type(argument))
            }
            _ => false,
        };
        if constructs_qualified_sum
            || eval.ty.as_ref().is_some_and(|ty| {
                contains_nested_owned_function_value(&self.resolve_type_aliases(ty))
            })
        {
            self.push_error(
                expr.span,
                "S10",
                "expressions cannot construct aggregates containing owned function values before recursive cleanup is implemented"
                    .to_string(),
            );
        }
    }

    fn expression_has_owned_function_type(&self, expr: &Expr) -> bool {
        self.annotated_exprs
            .iter()
            .rev()
            .find(|annotation| {
                annotation.span.start == expr.span.start && annotation.span.end == expr.span.end
            })
            .and_then(|annotation| annotation.ty.as_ref())
            .is_some_and(|ty| matches!(self.resolve_type_aliases(ty), TypeKind::Fn { .. }))
            || match &expr.node {
                ExprKind::Ident(name) => self.resolve_symbol(name).is_some_and(|symbol| {
                    symbol.ty.as_ref().is_some_and(|ty| {
                        matches!(self.resolve_type_aliases(ty), TypeKind::Fn { .. })
                    })
                }),
                ExprKind::Group(inner) | ExprKind::Cast { expr: inner, .. } => {
                    self.expression_has_owned_function_type(inner)
                }
                _ => false,
            }
    }

    fn type_check_contextual_none_value(
        &mut self,
        expr: &Expr,
        reachable: bool,
        expected: Option<&TypeKind>,
    ) -> Option<ExprEval> {
        let (ExprKind::Ident(name), Some(expected)) = (&expr.node, expected) else {
            return None;
        };
        let resolved = self.resolve_bare_fn_name(name)?;
        let symbol = self.resolve_symbol(&resolved)?;
        if builtin_constructor_kind(&resolved, &symbol) != Some("None") {
            return None;
        }
        let expected = self.resolve_type_aliases(expected);
        if !matches!(
            &expected,
            TypeKind::Named { name, type_args }
                if name.rsplit('.').next() == Some("Option") && type_args.len() == 1
        ) {
            return None;
        }
        let _ = self.resolve_for_read(name);
        let eval = ExprEval {
            ty: Some(expected),
            const_value: None,
        };
        self.annotate_expr(expr, &eval, reachable, Some(resolved));
        Some(eval)
    }

    fn type_check_contextual_sum_constructor(
        &mut self,
        expr: &Expr,
        reachable: bool,
        expected: Option<&TypeKind>,
    ) -> Option<ExprEval> {
        let Some(expected) = expected else {
            return None;
        };
        let ExprKind::Call {
            callee,
            type_args,
            args,
            named_args,
        } = &expr.node
        else {
            return None;
        };
        let ExprKind::Ident(callee_name) = &callee.node else {
            return None;
        };
        let resolved = self.resolve_bare_fn_name(callee_name)?;
        let symbol = self.resolve_symbol(&resolved)?;
        let constructor = builtin_constructor_kind(&resolved, &symbol)?;

        let expected = self.resolve_type_aliases(expected);
        let (expected_name, expected_args) = match &expected {
            TypeKind::Named { name, type_args } => {
                (name.rsplit('.').next().unwrap_or(name), type_args)
            }
            _ => return None,
        };

        let payload_index = match (constructor, expected_name) {
            ("None", "Option") if expected_args.len() == 1 => None,
            ("Some", "Option") if expected_args.len() == 1 => Some(0),
            ("Ok", "Result") if expected_args.len() == 2 => Some(0),
            ("Err", "Result") if expected_args.len() == 2 => Some(1),
            _ => return None,
        };

        let expected_arg_count = usize::from(payload_index.is_some());
        if !type_args.is_empty() {
            self.push_error(
                expr.span,
                "S14",
                format!("{constructor} gets its type arguments from the surrounding type"),
            );
        }
        if !named_args.is_empty() {
            self.push_error(
                expr.span,
                "S08",
                format!("{constructor} does not accept named arguments"),
            );
        }
        if args.len() != expected_arg_count {
            self.push_error(
                expr.span,
                "S08",
                format!("{constructor} expects exactly {expected_arg_count} argument(s)"),
            );
        }

        let mut inferred_payload = None;
        if let Some(index) = payload_index {
            let parameter = &expected_args[index].node;
            let open = matches!(parameter, TypeKind::Error);
            if let Some(payload) = args.first() {
                let eval = self.type_check_expr_expected(payload, reachable, Some(parameter));
                if let Some(actual) = eval.ty {
                    if open {
                        inferred_payload = Some((index, actual));
                    } else if !self.check_expr_compat(payload, parameter, &actual) {
                        self.push_error(
                            payload.span,
                            "S08",
                            format!("{constructor} payload: expected {parameter}, got {actual}"),
                        );
                    }
                }
            }
            let layout_type = inferred_payload.as_ref().map_or(parameter, |(_, actual)| actual);
            let resolved = self.resolve_type_aliases(layout_type);
            if !matches!(resolved, TypeKind::Error)
                && !crate::runtime_layout::runtime_value_layout(&resolved).fits_single_slot()
            {
                self.push_error(
                    expr.span,
                    "S14",
                    format!(
                        "{constructor} payload `{resolved}` cannot be stored through the current one-slot enum representation"
                    ),
                );
            }
        }
        for arg in args.iter().skip(expected_arg_count) {
            self.type_check_expr(arg, reachable);
        }
        let mut expected = expected;
        if let Some((index, inferred)) = inferred_payload
            && let TypeKind::Named { type_args, .. } = &mut expected
        {
            type_args[index].node = inferred;
        }

        let _ = self.resolve_for_read(callee_name);
        *self.call_counts.entry(constructor.to_string()).or_insert(0) += 1;
        let eval = ExprEval {
            ty: Some(expected),
            const_value: None,
        };
        self.annotate_expr(expr, &eval, reachable, Some(resolved));
        Some(eval)
    }

    /// Fill the unused variant's type once the other arguments have fixed
    /// the callee's generic signature. The payload was already checked.
    fn complete_sum_argument(&mut self, argument: &Expr, actual: &TypeKind, parameter: &TypeKind) {
        let TypeKind::Named { name, type_args } = actual else {
            return;
        };
        if !matches!(
            (name.rsplit('.').next(), type_args.len()),
            (Some("Option"), 1) | (Some("Result"), 2)
        ) || !type_args.iter().any(|arg| matches!(arg.node, TypeKind::Error))
        {
            return;
        }
        let parameter = self.resolve_type_aliases(parameter);
        let TypeKind::Named { name: expected_name, type_args: expected_args } = &parameter else {
            return;
        };
        if name != expected_name || type_args.len() != expected_args.len() {
            return;
        }
        let mut completed = actual.clone();
        if let TypeKind::Named { type_args, .. } = &mut completed {
            for (arg, expected) in type_args.iter_mut().zip(expected_args) {
                if matches!(arg.node, TypeKind::Error) {
                    arg.node = expected.node.clone();
                }
            }
        }
        if let Some(annotation) = self.annotated_exprs.iter_mut().rev().find(|ann| ann.span == argument.span) {
            annotation.ty = Some(completed);
        }
    }

    fn type_check_closure_expr(
        &mut self,
        expr: &Expr,
        params: &[String],
        body: &Expr,
        reachable: bool,
        expected: Option<&TypeKind>,
    ) -> ExprEval {
        let once = matches!(expr.node, ExprKind::Closure { once: true, .. });
        let mut captured_names = Vec::new();
        collect_expr_identifiers(body, &mut captured_names);
        captured_names.sort();
        captured_names.dedup();
        for name in captured_names {
            if params.contains(&name) {
                continue;
            }
            if let Some(symbol) = self.resolve_symbol(&name)
                && matches!(
                    symbol.kind,
                    SymbolKind::Variable { .. } | SymbolKind::Parameter
                )
                && let Some(ty) = symbol.ty.as_ref()
            {
                let resolved = self.resolve_type_aliases(ty);
                // Owned single-slot values (structs, enums, String, Array)
                // move into the closure environment (D-023 В14Г). A `qo`
                // closure may consume them; others borrow them.
                let _ = once;
                let moved_owner = matches!(resolved, TypeKind::Named { .. });
                if !closure_capture_is_plain_copy(&resolved) && !moved_owner {
                    self.push_error(
                        expr.span,
                        "S10",
                        format!(
                            "closure cannot capture `{name}` of type `{resolved}` until owned and borrowed capture lifetimes are implemented"
                        ),
                    );
                }
                // A method call mutates a capture only through an exclusive
                // `&T!` receiver; consuming calls are moves (borrow pass).
                let type_name = super::declare::type_kind_base_name(&resolved);
                let exclusive = |method: &str| {
                    self.explicit_exclusive_receiver_methods
                        .contains(&format!("{type_name}.{method}"))
                };
                if expr_mutates_ident_with(body, &name, &exclusive) {
                    self.push_error(
                        expr.span,
                        "S07",
                        format!(
                            "closure cannot mutate captured variable `{name}` until mutable capture environments are implemented"
                        ),
                    );
                }
            }
        }
        let expected = expected.map(|ty| self.resolve_type_aliases(ty));
        let expected_signature = match expected.as_ref() {
            Some(TypeKind::Fn { params, return_ty }) => {
                Some((params.clone(), return_ty.node.clone()))
            }
            Some(other) => {
                self.push_error(
                    expr.span,
                    "S14",
                    format!("closure requires a Quazi `fn` type, got {other}"),
                );
                None
            }
            None => None,
        };

        if !params.is_empty() && expected_signature.is_none() {
            self.push_error(
                expr.span,
                "S14",
                "closure parameter types cannot be inferred here; assign the closure to an explicit `fn(...) Return` type"
                    .to_string(),
            );
        }

        let (parameter_types, expected_return) = expected_signature.unwrap_or_else(|| {
            (
                params
                    .iter()
                    .map(|_| Spanned::new(TypeKind::Error, expr.span))
                    .collect(),
                TypeKind::Error,
            )
        });
        for parameter in &parameter_types {
            let resolved = self.resolve_type_aliases(&parameter.node);
            if !closure_capture_is_plain_copy(&resolved) {
                self.push_error(
                    parameter.span,
                    "S10",
                    format!(
                        "closure parameters of type `{resolved}` are unavailable until closure argument ownership is implemented"
                    ),
                );
            }
        }
        // A `qo` closure's single result moves into its qoroutine's result
        // slot and out through `join`, so owned named results are allowed.
        let owned_once_result =
            |ty: &TypeKind| once && matches!(ty, TypeKind::Named { .. });
        if !matches!(
            expected_return,
            TypeKind::Error | TypeKind::Void | TypeKind::Never
        ) && !closure_capture_is_plain_copy(&self.resolve_type_aliases(&expected_return))
            && !owned_once_result(&self.resolve_type_aliases(&expected_return))
        {
            self.push_error(
                expr.span,
                "S10",
                format!(
                    "closure return type `{expected_return}` is unavailable until closure result ownership is implemented"
                ),
            );
        }
        if parameter_types.len() != params.len() {
            self.push_error(
                expr.span,
                "S08",
                format!(
                    "closure expects {} parameter(s), but its target type declares {}",
                    params.len(),
                    parameter_types.len()
                ),
            );
        }

        let outer_initialization = self.initialization_snapshot();
        self.enter_scope();
        for (index, name) in params.iter().enumerate() {
            let ty = parameter_types
                .get(index)
                .map(|ty| ty.node.clone())
                .unwrap_or(TypeKind::Error);
            self.declare(
                name.clone(),
                Symbol {
                    kind: SymbolKind::Parameter,
                    ty: Some(ty),
                    span: expr.span,
                    params: Vec::new(),
                    used: false,
                    initialized: true,
                    is_import: false,
                    import_path: None,
                    const_value: None,
                    variadic: false,
                    attributes: Vec::new(),
                    public: false,
                    unsafe_fn: false,
                    generic_params: Vec::new(),
                },
            );
        }
        let previous_return = self.current_return_type.replace(expected_return.clone());
        let body_eval = self.type_check_expr_expected(body, reachable, Some(&expected_return));
        self.current_return_type = previous_return;
        self.exit_scope_collect();
        self.restore_initialization(&outer_initialization);
        self.initialization_condition = None;

        if let Some(actual) = &body_eval.ty
            && !matches!(expected_return, TypeKind::Error)
            && !self.types_have_same_runtime_shape(&expected_return, actual)
        {
            self.push_error(
                body.span,
                "S01",
                format!(
                    "closure return type mismatch: expected {}, got {}",
                    expected_return, actual
                ),
            );
        }

        let return_ty = if matches!(expected_return, TypeKind::Error) {
            body_eval.ty.unwrap_or(TypeKind::Void)
        } else {
            expected_return
        };
        let resolved_return = self.resolve_type_aliases(&return_ty);
        if !matches!(
            resolved_return,
            TypeKind::Void | TypeKind::Never | TypeKind::Error
        ) && !closure_capture_is_plain_copy(&resolved_return)
            && !owned_once_result(&resolved_return)
        {
            self.push_error(
                body.span,
                "S10",
                format!(
                    "closure return type `{return_ty}` is unavailable until closure result ownership is implemented"
                ),
            );
        }
        let eval = ExprEval {
            ty: Some(TypeKind::Fn {
                params: parameter_types,
                return_ty: Box::new(Spanned::new(return_ty, expr.span)),
            }),
            const_value: None,
        };
        self.annotate_expr(expr, &eval, reachable, None);
        eval
    }

    pub(super) fn type_check_expr(&mut self, expr: &Expr, reachable: bool) -> ExprEval {
        self.initialization_condition = None;
        let mut initialization_condition = None;
        let mut resolved_method: Option<String> = None;
        // Generic arguments inferred for a module-qualified call (`mod.f(x)`).
        let mut inferred_type_args: Option<Vec<TypeKind>> = None;
        let result = match &expr.node {
            ExprKind::Literal(Literal::String(text))
                if !self
                    .format_template_literals
                    .contains(&(expr.span.start, expr.span.end)) =>
            {
                self.type_check_string_literal(expr, text, reachable)
            }
            ExprKind::Literal(lit) => {
                let ty = match lit {
                    // A literal above i64::MAX can only be a u64 value; typing
                    // it as i32 made constant folding and codegen treat it as a
                    // negative signed operand.
                    Literal::Int(value) if *value > i64::MAX as u64 => TypeKind::Uint64,
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
                if let Some(resolved) = self.resolve_module_const_name(name) {
                    if resolved != *name {
                        let _ = self.resolve_for_read(name);
                    }
                    let sym = self.resolve_for_read(&resolved).expect("resolved const exists");
                    let eval = ExprEval {
                        ty: sym.ty,
                        const_value: sym.const_value,
                    };
                    self.annotate_expr(expr, &eval, reachable, None);
                    return eval;
                }
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
                    self.reject_nested_owned_function_expression(expr, &eval);
                    return eval;
                }
                // A zero-payload enum constructor may be used as a value (`None`) as
                // well as called (`None()`). Keep this narrow: locals still shadow the
                // builtin, and payload-bearing constructors remain function values.
                if name == "None"
                    && self.resolve_bare_fn_name(name).as_deref() == Some("None")
                    && self
                        .enums
                        .get("Option")
                        .and_then(|info| info.variants.get(name))
                        .is_some_and(|arity| *arity == 0)
                {
                    let _ = self.resolve_for_read(name);
                    let eval = ExprEval {
                        ty: Some(TypeKind::Named {
                            name: "Option".to_string(),
                            type_args: vec![Spanned::new(TypeKind::Error, expr.span)],
                        }),
                        const_value: None,
                    };
                    self.annotate_expr(expr, &eval, reachable, Some(name.clone()));
                    self.reject_nested_owned_function_expression(expr, &eval);
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
                    self.reject_nested_owned_function_expression(expr, &eval);
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
                            && self.initialization_reachable
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
                            // Alias symbol bodies retain display parameter names;
                            // expansion must use the hygienic authoritative table.
                            // Bare generic aliases must also satisfy their arity.
                            let nominal = TypeKind::Named {
                                name: name.clone(),
                                type_args: vec![],
                            };
                            let resolved = if sym.ty.is_some() {
                                self.source_type_at(&nominal, expr.span)
                            } else {
                                nominal
                            };
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
            ExprKind::Group(inner) => {
                let eval = self.type_check_expr(inner, reachable);
                initialization_condition = Some(self.take_initialization_condition(inner, &eval));
                eval
            }
            // `...values` forwards a variadic parameter of the enclosing
            // function; its value is the element type (calls check the rest).
            ExprKind::Spread(inner) => {
                let eval = self.type_check_expr(inner, reachable);
                let forwards_variadic = matches!(&inner.node, ExprKind::Ident(name)
                    if self.resolve_symbol(name).is_some_and(|symbol| {
                        symbol.kind == SymbolKind::Parameter && symbol.variadic
                    }));
                if !forwards_variadic {
                    self.push_error(
                        expr.span,
                        "S08",
                        "`...` forwards only a variadic parameter of the enclosing function".to_string(),
                    );
                }
                // The parameter is a slice of its elements; each forwarded
                // element is checked against the callee's variadic type.
                match eval.ty {
                    Some(TypeKind::Slice { elem_ty }) => ExprEval {
                        ty: Some(elem_ty.node),
                        ..ExprEval::default()
                    },
                    _ => eval,
                }
            }
            ExprKind::Unary { expr: inner, op } => {
                let inner_eval = self.type_check_expr(inner, reachable);
                if matches!(op, UnaryOpKind::Not) {
                    let mut condition = self.take_initialization_condition(inner, &inner_eval);
                    std::mem::swap(&mut condition.truthy, &mut condition.falsy);
                    initialization_condition = Some(condition);
                }

                match op {
                    UnaryOpKind::Ref | UnaryOpKind::RefMut => {
                        let addressable = addressable_place_root(inner).and_then(|name| {
                            self.resolve_symbol(name).filter(|symbol| {
                                matches!(
                                    symbol.kind,
                                    SymbolKind::Variable { .. } | SymbolKind::Parameter
                                )
                            })
                        });
                        if addressable.is_none() {
                            self.push_error(
                                inner.span,
                                "S14",
                                "address-of currently requires a local variable, a parameter, or a field of one; indexes, dereferences, calls, and temporaries are not addressable yet"
                                    .to_string(),
                            );
                        }
                        // A field reached through a shared `&T` root is read-only:
                        // `&p.field!` must not mint exclusive access from it.
                        let through_shared_reference = matches!(&inner.node, ExprKind::Field { .. })
                            && addressable.as_ref().is_some_and(|symbol| {
                                symbol.ty.as_ref().is_some_and(|ty| {
                                    matches!(self.resolve_type_aliases(ty), TypeKind::Ref { .. })
                                })
                            });
                        if matches!(op, UnaryOpKind::RefMut) && through_shared_reference {
                            self.push_error(
                                inner.span,
                                "S07",
                                "exclusive address-of cannot reach a field through a shared reference `&T`".to_string(),
                            );
                        } else if matches!(op, UnaryOpKind::RefMut)
                            && !addressable.is_some_and(|symbol| {
                                matches!(
                                    symbol.kind,
                                    SymbolKind::Variable { mutable: true } | SymbolKind::Parameter
                                )
                            })
                        {
                            self.push_error(
                                inner.span,
                                "S07",
                                "exclusive address-of requires a mutable local variable or parameter".to_string(),
                            );
                        }
                        // &expr → type is Ref<inner_type>
                        let ref_ty = inner_eval.ty.map(|t| {
                            let inner = Box::new(Spanned::new(t, inner.span));
                            if matches!(op, UnaryOpKind::RefMut) {
                                TypeKind::MutRef { inner }
                            } else {
                                TypeKind::Ref { inner }
                            }
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
                            Some(TypeKind::Ref { inner: t })
                            | Some(TypeKind::MutRef { inner: t }) => {
                                let pointee = self.resolve_type_aliases(&t.node);
                                if !Self::is_autoderef_value(&pointee)
                                    && self.place_write_target_span
                                        != Some((expr.span.start, expr.span.end))
                                {
                                    self.push_error(
                                        expr.span,
                                        "S10",
                                        format!(
                                            "cannot materialize `{pointee}` through a shared reference before aggregate reference semantics are implemented"
                                        ),
                                    );
                                }
                                ExprEval {
                                    ty: Some(t.node.clone()),
                                    const_value: None,
                                }
                            }
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
                            && !matches!(t, TypeKind::Bool | TypeKind::Error)
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
                        self.check_arithmetic_op(
                            expr.span,
                            ArithmeticOp::Negate,
                            inner_eval.ty.as_ref(),
                            None,
                        );
                        if inner_eval
                            .ty
                            .as_ref()
                            .is_some_and(|ty| Self::is_integer(&self.resolve_type_aliases(ty)))
                        {
                            self.require_language_panic();
                        }
                    }
                }

                if matches!(op, UnaryOpKind::Neg)
                    && !crate::evaluation::is_integer_literal_expression(inner)
                    && let (Some(value), Some(ty)) = (
                        inner_eval.const_value.as_ref(),
                        inner_eval.ty.as_ref().map(|ty| self.resolve_type_aliases(ty)),
                    )
                {
                    let exact = Self::exact_integer_constant(value, &ty).map(|value| -value);
                    self.check_constant_overflow(expr.span, "unary `-`", exact, Some(&ty));
                }
                let const_value = inner_eval
                    .const_value
                    .and_then(|value| Self::const_from_unary(op, value))
                    .map(|value| {
                        if crate::evaluation::is_integer_literal_expression(inner) {
                            value
                        } else {
                            self.wrap_const_integer(value, inner_eval.ty.as_ref())
                        }
                    });
                ExprEval {
                    ty: inner_eval.ty,
                    const_value,
                }
            }
            ExprKind::Cast { expr: inner, ty } => {
                let inner_eval = self.type_check_expr(inner, reachable);
                let target_ty = self.resolve_type_aliases(&ty.node);
                if matches!(inner_eval.ty, Some(TypeKind::Bool))
                    && matches!(target_ty, TypeKind::Bool)
                {
                    initialization_condition =
                        Some(self.take_initialization_condition(inner, &inner_eval));
                }
                if type_contains_any(&target_ty)
                    || inner_eval.ty.as_ref().is_some_and(type_contains_any)
                {
                    self.push_error(
                        expr.span,
                        "S14",
                        "casts to or from runtime `any` are unsupported because Quazi has no tagged dynamic-value representation".to_string(),
                    );
                }
                let allowed = match inner_eval.ty.as_ref() {
                    Some(src) if Self::is_integer(src) && Self::is_integer(&target_ty) => true,
                    Some(src) if Self::is_float(src) && Self::is_float(&target_ty) => true,
                    // Cross-family casts round (to float) or truncate and
                    // saturate (to integer); f16 has no conversion lowering.
                    Some(src)
                        if (Self::is_integer(src)
                            && matches!(target_ty, TypeKind::Float32 | TypeKind::Float64))
                            || (matches!(src, TypeKind::Float32 | TypeKind::Float64)
                                && Self::is_integer(&target_ty)) =>
                    {
                        true
                    }
                    Some(src)
                        if (Self::is_integer(src) || matches!(src, TypeKind::RawPtr { .. }))
                            && matches!(&target_ty, TypeKind::CFn { .. }) =>
                    {
                        true
                    }
                    Some(TypeKind::CFn { .. })
                        if Self::is_integer(&target_ty)
                            || matches!(&target_ty, TypeKind::RawPtr { .. }) =>
                    {
                        true
                    }
                    Some(src @ (TypeKind::Ref { .. } | TypeKind::MutRef { .. }))
                        if matches!(target_ty, TypeKind::Ref { .. } | TypeKind::MutRef { .. }) =>
                    {
                        self.types_have_same_runtime_shape(
                            &self.resolve_type_aliases(src),
                            &target_ty,
                        )
                    }
                    Some(src @ TypeKind::Fn { .. }) if matches!(target_ty, TypeKind::Fn { .. }) => {
                        self.types_have_same_runtime_shape(
                            &self.resolve_type_aliases(src),
                            &target_ty,
                        )
                    }
                    Some(src @ TypeKind::CFn { .. })
                        if matches!(target_ty, TypeKind::CFn { .. }) =>
                    {
                        self.types_have_same_runtime_shape(
                            &self.resolve_type_aliases(src),
                            &target_ty,
                        )
                    }
                    Some(src)
                        if !matches!(
                            src,
                            TypeKind::Ref { .. }
                                | TypeKind::MutRef { .. }
                                | TypeKind::Fn { .. }
                                | TypeKind::CFn { .. }
                        ) && std::mem::discriminant(src)
                            == std::mem::discriminant(&target_ty) =>
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
                if (matches!(&target_ty, TypeKind::CFn { .. })
                    || matches!(inner_eval.ty.as_ref(), Some(TypeKind::CFn { .. })))
                    && self.unsafe_depth == 0
                {
                    self.push_error(
                        expr.span,
                        "S11",
                        "casting a raw C function pointer requires unsafe context".to_string(),
                    );
                }
                let const_value = match (inner_eval.const_value, inner_eval.ty.as_ref()) {
                    (Some(value), Some(source)) => {
                        Self::const_numeric_cast(value, &self.resolve_type_aliases(source), &target_ty)
                    }
                    (value, _) => value,
                };
                ExprEval {
                    ty: Some(target_ty),
                    const_value,
                }
            }
            ExprKind::Binary { left, op, right } => {
                let left_eval = self.type_check_expr(left, reachable);
                let right_eval = if matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr) {
                    let left_condition = self.take_initialization_condition(left, &left_eval);
                    let fallback = self.initialization_snapshot();
                    let input = if matches!(op, BinOpKind::AndAnd) {
                        left_condition.truthy.as_ref()
                    } else {
                        left_condition.falsy.as_ref()
                    };
                    self.restore_initialization(input.unwrap_or(&fallback));
                    let previous_reachable = self.initialization_reachable;
                    self.initialization_reachable &= input.is_some();
                    let eval = self.type_check_expr(right, reachable);
                    let right_condition = if input.is_some() {
                        self.take_initialization_condition(right, &eval)
                    } else {
                        InitializationCondition::default()
                    };
                    self.initialization_reachable = previous_reachable;
                    let condition = if matches!(op, BinOpKind::AndAnd) {
                        let mut falsy = left_condition.falsy;
                        join_initialization(&mut falsy, right_condition.falsy);
                        InitializationCondition {
                            truthy: right_condition.truthy,
                            falsy,
                        }
                    } else {
                        let mut truthy = left_condition.truthy;
                        join_initialization(&mut truthy, right_condition.truthy);
                        InitializationCondition {
                            truthy,
                            falsy: right_condition.falsy,
                        }
                    };
                    self.restore_initialization(condition.normal().as_ref().unwrap_or(&fallback));
                    initialization_condition = Some(condition);
                    eval
                } else {
                    self.type_check_expr(right, reachable)
                };

                let requirement = match op {
                    BinOpKind::Add
                    | BinOpKind::Sub
                    | BinOpKind::Mul
                    | BinOpKind::Div
                    | BinOpKind::Mod
                    | BinOpKind::BitAnd
                    | BinOpKind::BitOr
                    | BinOpKind::BitXor
                    | BinOpKind::Shl
                    | BinOpKind::Shr => Some(ArithmeticOp::Binary(op.clone())),
                    BinOpKind::Lt | BinOpKind::LtEq | BinOpKind::Gt | BinOpKind::GtEq => {
                        Some(ArithmeticOp::Ordering(op.clone()))
                    }
                    _ => None,
                };
                if let Some(requirement) = requirement {
                    self.check_arithmetic_op(
                        expr.span,
                        requirement,
                        left_eval.ty.as_ref(),
                        right_eval.ty.as_ref(),
                    );
                    self.mark_arithmetic_operand_autoderef(left, left_eval.ty.as_ref());
                    self.mark_arithmetic_operand_autoderef(right, right_eval.ty.as_ref());
                }
                if matches!(op, BinOpKind::Pow) {
                    self.mark_arithmetic_operand_autoderef(left, left_eval.ty.as_ref());
                    self.mark_arithmetic_operand_autoderef(right, right_eval.ty.as_ref());
                    self.check_power_expression(expr.span, left, right, &left_eval, &right_eval)
                } else {
                    let ty = self.infer_binary_type(expr.span, op, &left_eval.ty, &right_eval.ty);
                    let width_operand = crate::evaluation::integer_width_operand(op, left, right);
                    // `1 + x` has the type of `x`: integer literals are `i32`
                    // only for want of a typed operand.
                    let ty = match (&ty, &right_eval.ty) {
                        (Some(result), Some(right_ty))
                            if width_operand.is_some_and(|operand| {
                                std::ptr::eq(operand, right.as_ref())
                            }) && Self::is_integer(&self.resolve_type_aliases(result))
                                && Self::is_integer(&self.resolve_type_aliases(right_ty)) =>
                        {
                            Some(right_ty.clone())
                        }
                        // `0.0 - value` has the float type of `value`.
                        (Some(result), Some(right_ty))
                            if crate::evaluation::is_float_literal_expression(left)
                                && Self::is_float(&self.resolve_type_aliases(result))
                                && Self::is_float(&self.resolve_type_aliases(right_ty)) =>
                        {
                            Some(right_ty.clone())
                        }
                        _ => ty,
                    };
                    self.mark_binary_autoderef(left, &left_eval.ty, right, &right_eval.ty);
                    let const_value = if let (Some(lhs), Some(rhs)) =
                        (&left_eval.const_value, &right_eval.const_value)
                    {
                        let signedness = width_operand.and_then(|operand| {
                            if std::ptr::eq(operand, left.as_ref()) {
                                left_eval.ty.as_ref()
                            } else {
                                right_eval.ty.as_ref()
                            }
                        });
                        if width_operand.is_some()
                            && let Some(result_ty) =
                                ty.as_ref().map(|ty| self.resolve_type_aliases(ty))
                            && let (Some(a), Some(b)) = (
                                Self::exact_integer_constant(lhs, &result_ty),
                                Self::exact_integer_constant(rhs, &result_ty),
                            )
                        {
                            let (name, exact) = match op {
                                BinOpKind::Add => ("`+`", a.checked_add(b)),
                                BinOpKind::Sub => ("`-`", a.checked_sub(b)),
                                BinOpKind::Mul => ("`*`", a.checked_mul(b)),
                                BinOpKind::Div if b != 0 => ("`/`", a.checked_div(b)),
                                _ => ("", None),
                            };
                            self.check_constant_overflow(expr.span, name, exact, Some(&result_ty));
                        }
                        Self::const_from_binary(op, lhs, rhs, signedness.or(left_eval.ty.as_ref()))
                            .map(|value| match width_operand {
                                Some(_) => self.wrap_const_integer(value, ty.as_ref()),
                                None => value,
                            })
                    } else {
                        self.check_math_identities(expr.span, op, &left_eval, &right_eval)
                    };
                    ExprEval { ty, const_value }
                }
            }
            ExprKind::Assign { target, value } => {
                let target_eval = if assignment_ident(target).is_none() {
                    let mut place = target.as_ref();
                    while let ExprKind::Group(inner) = &place.node {
                        place = inner;
                    }
                    let previous = self
                        .place_write_target_span
                        .replace((place.span.start, place.span.end));
                    let target_eval = self.type_check_expr(target, reachable);
                    self.place_write_target_span = previous;
                    Some(target_eval)
                } else {
                    None
                };
                let assignment_expected = target_eval
                    .as_ref()
                    .and_then(|eval| eval.ty.clone())
                    .or_else(|| {
                        if let Some(name) = assignment_ident(target) {
                            let resolved = self
                                .resolve_bare_foreign_global_name(name)
                                .unwrap_or_else(|| name.to_string());
                            self.resolve_symbol(&resolved).and_then(|symbol| symbol.ty)
                        } else {
                            None
                        }
                    });
                self.analyze_assign_target(target);
                let value_eval =
                    self.type_check_expr_expected(value, reachable, assignment_expected.as_ref());
                let mut value_condition = self.take_initialization_condition(value, &value_eval);

                if let (Some(target_eval), Some(value_ty)) = (&target_eval, &value_eval.ty)
                    && let Some(target_ty) = &target_eval.ty
                    && !self.check_expr_compat(value, target_ty, value_ty)
                {
                    self.push_error(
                        target.span,
                        "S01",
                        format!(
                            "type mismatch in {}: expected {}, got {}",
                            if matches!(target.node, ExprKind::Index { .. }) {
                                "indexed assignment"
                            } else {
                                "assignment"
                            },
                            target_ty,
                            value_ty
                        ),
                    );
                }

                if let Some(name) = assignment_ident(target) {
                    let resolved = self
                        .resolve_bare_foreign_global_name(name)
                        .unwrap_or_else(|| name.to_string());
                    if let Some(sym) = self.resolve_symbol(&resolved)
                        && let (Some(var_ty), Some(val_ty)) = (&sym.ty, &value_eval.ty)
                    {
                        if matches!(self.resolve_type_aliases(var_ty), TypeKind::Fn { .. })
                            && expression_is_ident(value, name)
                        {
                            self.push_error(
                                value.span,
                                "S10",
                                format!(
                                    "self-assignment of owned function value `{name}` would destroy its environment"
                                ),
                            );
                        }
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
                    self.initialize_condition_binding(name, &mut value_condition);
                    // Mutable assignments may occur on only one control-flow
                    // path (or in a loop that executes zero times). Treating
                    // the assigned literal as globally constant makes later
                    // branches unsound. The bytecode dataflow pass still folds
                    // values where reaching definitions prove constancy.
                    self.set_symbol_const_value(name, None);
                }

                initialization_condition = Some(value_condition);
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
                self.record_ownership_call_site(expr.span, None);
                for type_arg in type_args {
                    if type_contains_any(&type_arg.node) {
                        self.push_error(
                            type_arg.span,
                            "S14",
                            "runtime `any` cannot be used as a generic type argument".to_string(),
                        );
                    }
                    if contains_non_string_reference(&self.resolve_type_aliases(&type_arg.node)) {
                        self.push_error(
                            type_arg.span,
                            "S10",
                            "shared references cannot be generic type arguments before generic lifetimes are tracked"
                                .to_string(),
                        );
                    }
                    if contains_owned_function_value(&self.resolve_type_aliases(&type_arg.node)) {
                        self.push_error(
                            type_arg.span,
                            "S10",
                            "owned function values cannot be generic type arguments before generic ownership is tracked"
                                .to_string(),
                        );
                    }
                }
                if let ExprKind::Ident(name) = &callee.node
                    && let Some(symbol) = self.resolve_symbol(name)
                {
                    self.require_initialized(name, &symbol, callee.span);
                }
                let indirect_callee = (!matches!(callee.node, ExprKind::Ident(_)))
                    .then(|| self.type_check_expr(callee, reachable));
                let contextual_signature = if let ExprKind::Ident(name) = &callee.node {
                    if let Some(resolved) = self.resolve_bare_fn_name(name) {
                        self.resolve_symbol(&resolved).map(|symbol| {
                            let substitution: std::collections::HashMap<String, TypeKind> = symbol
                                .generic_params
                                .iter()
                                .zip(type_args.iter())
                                .map(|(param, arg)| (param.clone(), arg.node.clone()))
                                .collect();
                            let params = symbol
                                .params
                                .iter()
                                .map(|param| {
                                    contextual_call_parameter(
                                        param,
                                        &symbol.generic_params,
                                        &substitution,
                                    )
                                })
                                .collect::<Vec<_>>();
                            let names = self
                                .fn_param_names
                                .get(&resolved)
                                .cloned()
                                .unwrap_or_default();
                            (params, names)
                        })
                    } else {
                        self.resolve_symbol(name).and_then(|symbol| {
                            let fn_ty = symbol.ty.as_ref()?;
                            let TypeKind::Fn { params, .. } = self.resolve_type_aliases(fn_ty)
                            else {
                                return None;
                            };
                            Some((
                                params.into_iter().map(|param| param.node).collect(),
                                Vec::new(),
                            ))
                        })
                    }
                } else {
                    None
                };
                let template_callee = match &callee.node {
                    ExprKind::Ident(name) => self.resolve_bare_fn_name(name),
                    _ => None,
                };
                self.note_format_template(template_callee.as_deref(), args);
                let arg_evals = self.type_check_call_arguments(
                    args,
                    named_args,
                    reachable,
                    contextual_signature
                        .as_ref()
                        .map(|(params, names)| (params.as_slice(), names.as_slice())),
                );

                if let ExprKind::Ident(name) = &callee.node {
                    // Resolve the name, taking into account namespacing for the current module.
                    let resolved_name = self.resolve_bare_fn_name(name);
                    let _ = self.resolve_for_read(name);

                    if let Some(resolved) = &resolved_name {
                        let sym = self
                            .resolve_for_read(resolved)
                            .expect("resolved function should exist");

                        // Generic source bodies are checked against their declared
                        // template. Their concrete artifact effects remain outside
                        // D-014; this retains the pre-existing source-only
                        // reference-capability rule without trusting QZI.
                        self.record_source_call_effect(OwnershipCallEffect::Direct(
                            resolved.clone(),
                        ));

                        // Library functions require explicit import-by-name to be called unqualified.
                        // Exception: library code calling other library code is always allowed.
                        let caller_is_library = self
                            .current_function
                            .last()
                            .map(|f| self.library_fn_names.contains(f.as_str()))
                            .unwrap_or(false);
                        // Any function of a module may call its own siblings,
                        // whether or not the caller itself is public.
                        let same_module = self
                            .current_module_path
                            .as_ref()
                            .is_some_and(|module| *resolved == format!("{module}.{name}"));
                        if self.library_fn_names.contains(resolved.as_str())
                            && !self.explicitly_imported_fns.contains_key(name.as_str())
                            && !caller_is_library
                            && !same_module
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
                            );
                        }
                        let inferred_type_args = if type_args.is_empty() {
                            let ordered = self.slot_ordered_evals(
                                resolved,
                                &sym,
                                args.len(),
                                named_args,
                                &arg_evals,
                            );
                            self.infer_call_type_args(&sym, &ordered, expr.span)
                        } else {
                            None
                        };
                        let eval = self.check_function_call(
                            resolved,
                            callee.span,
                            expr.span,
                            args,
                            named_args,
                            &arg_evals,
                            &sym,
                            inferred_type_args.as_deref().unwrap_or(type_args),
                        );
                        self.annotate_expr(expr, &eval, reachable, Some(resolved.clone()));
                        if let Some(inferred) = inferred_type_args
                            && let Some(annotation) = self.annotated_exprs.last_mut()
                        {
                            annotation.inferred_type_args =
                                inferred.into_iter().map(|arg| arg.node).collect();
                        }
                        self.reject_nested_owned_function_expression(expr, &eval);
                        return eval;
                    }

                    // Not resolved as a function — fall through to generic symbol lookup
                    // for variables, parameters, function-pointer values, etc.
                    let Some(sym) = self.resolve_for_read(name) else {
                        self.record_source_call_effect(OwnershipCallEffect::Opaque);
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
                        self.record_source_call_effect(OwnershipCallEffect::Opaque);
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
                        self.reject_nested_owned_function_expression(expr, &eval);
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
                    self.record_source_call_effect(OwnershipCallEffect::Opaque);
                    self.push_error(callee.span, "S04", msg);
                    return ExprEval::default();
                } else {
                    self.record_source_call_effect(OwnershipCallEffect::Opaque);
                    let callee_eval = indirect_callee.expect("checked indirect callee");
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
                self.record_ownership_call_site(expr.span, None);
                // Receiver dispatch, including an inherent method, remains
                // outside the unqualified direct-call ownership checkpoint.
                self.record_source_call_effect(OwnershipCallEffect::Opaque);
                for type_arg in type_args {
                    if type_contains_any(&type_arg.node) {
                        self.push_error(
                            type_arg.span,
                            "S14",
                            "runtime `any` cannot be used as a generic type argument".to_string(),
                        );
                    }
                    if contains_non_string_reference(&self.resolve_type_aliases(&type_arg.node)) {
                        self.push_error(
                            type_arg.span,
                            "S10",
                            "shared references cannot be generic type arguments before generic lifetimes are tracked"
                                .to_string(),
                        );
                    }
                    if contains_owned_function_value(&self.resolve_type_aliases(&type_arg.node)) {
                        self.push_error(
                            type_arg.span,
                            "S10",
                            "owned function values cannot be generic type arguments before generic ownership is tracked"
                                .to_string(),
                        );
                    }
                }
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
                    let module_method = self.resolve_module_method(object, method);
                    let contextual_signature = module_method.as_ref().and_then(|resolved| {
                        self.resolve_symbol(resolved).map(|symbol| {
                            let substitution: std::collections::HashMap<String, TypeKind> = symbol
                                .generic_params
                                .iter()
                                .zip(type_args.iter())
                                .map(|(param, arg)| (param.clone(), arg.node.clone()))
                                .collect();
                            let params = symbol
                                .params
                                .iter()
                                .map(|param| {
                                    contextual_call_parameter(
                                        param,
                                        &symbol.generic_params,
                                        &substitution,
                                    )
                                })
                                .collect::<Vec<_>>();
                            let names = self
                                .fn_param_names
                                .get(resolved)
                                .cloned()
                                .unwrap_or_default();
                            (params, names)
                        })
                    });
                    self.note_format_template(module_method.as_deref(), args);
                    let arg_evals = self.type_check_call_arguments(
                        args,
                        named_args,
                        reachable,
                        contextual_signature
                            .as_ref()
                            .map(|(params, names)| (params.as_slice(), names.as_slice())),
                    );
                    if let Some(resolved) = &module_method {
                        if let Some(sym) = self.resolve_symbol(resolved) {
                            self.check_module_function_visibility(&sym, method, expr.span);
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
                                let mut sym = self
                                    .resolve_for_read(resolved)
                                    .expect("symbol should resolve");
                                // `module.Type.function(...)` on a generic type infers the
                                // impl's parameters too, like the unqualified `Type.function`;
                                // explicit type arguments keep naming the function's own.
                                let impl_params = self.generic_params_for_function(resolved);
                                if type_args.is_empty() && impl_params.len() > sym.generic_params.len() {
                                    sym.generic_params = impl_params;
                                }
                                if !named_args.is_empty() {
                                    self.validate_named_args(
                                        expr.span,
                                        resolved,
                                        named_args,
                                        args.len(),
                                    );
                                }
                                let inferred = if type_args.is_empty() {
                                    let ordered = self.slot_ordered_evals(
                                        resolved,
                                        &sym,
                                        args.len(),
                                        named_args,
                                        &arg_evals,
                                    );
                                    self.infer_call_type_args(&sym, &ordered, expr.span)
                                } else {
                                    None
                                };
                                let eval = self.check_function_call(
                                    resolved,
                                    expr.span,
                                    expr.span,
                                    args,
                                    named_args,
                                    &arg_evals,
                                    &sym,
                                    inferred.as_deref().unwrap_or(type_args),
                                );
                                inferred_type_args =
                                    inferred.map(|args| args.into_iter().map(|arg| arg.node).collect());
                                resolved_method = Some(resolved.clone());
                                eval
                            }
                        } else if !self.source_files.is_empty() {
                            // The receiver is an imported module namespace, but the
                            // qualified name does not resolve to any symbol. When source
                            // files are present (real loader run), emit a clear S04 error
                            // instead of silently producing default code. Raw tests that
                            // do not load imports stay lenient.
                            let message = self.missing_module_member_message(object, method);
                            self.push_error(expr.span, "S04", message);
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
                    if let ExprKind::Ident(source_name) = &object.node {
                        if self
                            .current_generic_params
                            .iter()
                            .any(|scope| scope.contains(source_name))
                            && !self.resolve_symbol(source_name).is_some_and(|symbol| {
                                matches!(
                                    symbol.kind,
                                    SymbolKind::Variable { .. } | SymbolKind::Parameter
                                )
                            })
                        {
                            // `T.make(args)` with `T: Trait` calls the trait's
                            // receiverless method; each specialization calls the
                            // concrete impl (`P.make`).
                            let param = source_name.clone();
                            let bounds: Vec<String> = self
                                .current_generic_bounds()
                                .into_iter()
                                .filter(|(bounded, _)| *bounded == param)
                                .map(|(_, bound)| bound)
                                .collect();
                            let signature = bounds.iter().find_map(|bound| {
                                self.bound_trait_signatures(bound)
                                    .and_then(|methods| methods.get(method).cloned())
                                    .filter(|signature| signature.associated)
                            });
                            let Some(signature) = signature else {
                                self.push_error(object.span, "S14", format!("`{param}.{method}` needs a bound `{param}: Trait` whose trait declares `@associated fn {method}`"));
                                return ExprEval::default();
                            };
                            let self_type = TypeKind::Named { name: param.clone(), type_args: Vec::new() };
                            let evals: Vec<ExprEval> = args
                                .iter()
                                .map(|argument| self.type_check_expr(argument, reachable))
                                .collect();
                            if args.len() != signature.params.len() {
                                self.push_error(expr.span, "S08", format!("`{param}.{method}` expects {} argument(s), got {}", signature.params.len(), args.len()));
                            }
                            for ((argument, eval), expected) in args.iter().zip(&evals).zip(&signature.params) {
                                let expected = substitute_self_type(expected, &self_type);
                                if let Some(actual) = &eval.ty
                                    && !self.check_expr_compat(argument, &expected, actual)
                                {
                                    self.push_error(argument.span, "S08", format!("argument of `{param}.{method}`: expected {expected}, got {actual}"));
                                }
                            }
                            if let Some(function) = self.current_function.last().cloned() {
                                self.bound_method_calls
                                    .entry(function)
                                    .or_default()
                                    .push((param.clone(), method.to_string()));
                            }
                            self.associated_trait_calls.insert((expr.span.start, expr.span.end));
                            let result = ExprEval {
                                ty: Some(substitute_self_type(&signature.return_ty, &self_type)),
                                const_value: None,
                            };
                            self.annotate_expr(expr, &result, reachable, None);
                            return result;
                        }
                        let canonical = self.resolve_type_aliases(&TypeKind::Named {
                            name: source_name.clone(),
                            type_args: vec![],
                        });
                        let struct_name = match &canonical {
                            TypeKind::Named { name, .. } => name,
                            _ => source_name,
                        };
                        let method_full = format!("{struct_name}.{method}");
                        let is_enum_method = self.enums.get(struct_name.as_str())
                            .is_some_and(|info| !info.variants.contains_key(method))
                            && self.resolve_symbol(&method_full).is_some();
                        let is_type_namespace = (self.struct_defs.contains_key(struct_name.as_str())
                            || is_enum_method)
                            && !self
                                .current_generic_params
                                .iter()
                                .any(|scope| scope.contains(source_name))
                            && !matches!(
                                self.resolve_symbol(source_name).map(|s| s.kind),
                                Some(SymbolKind::Variable { .. }) | Some(SymbolKind::Parameter)
                            );
                        if is_type_namespace {
                            if let Some(sym) = self.resolve_for_read(&method_full) {
                                // Retain declaration identity independently of whether
                                // this call's generic arguments have been inferred.
                                self.record_source_call_declaration(expr.span, &method_full);
                                let param_names = self
                                    .fn_param_names
                                    .get(&method_full)
                                    .cloned()
                                    .unwrap_or_default();
                                let all_evals = self.type_check_call_arguments(
                                    args,
                                    named_args,
                                    reachable,
                                    Some((&sym.params, &param_names)),
                                );
                                let arg_evals = &all_evals[..args.len()];
                                // Named arguments infer through their declared slots.
                                let slot_evals = self.slot_ordered_evals(
                                    &method_full,
                                    &sym,
                                    args.len(),
                                    named_args,
                                    &all_evals,
                                );
                                let struct_params = self.generic_params_for_function(&method_full);
                                let mut inferred_static_args = None;
                                let from = self
                                    .current_function
                                    .last()
                                    .cloned()
                                    .unwrap_or_else(|| "__program__".to_string());
                                // Parameter types to check the arguments against; generic
                                // ones only once every type parameter is inferred.
                                let mut checked_params: Option<Vec<TypeKind>> =
                                    struct_params.is_empty().then(|| sym.params.clone());
                                // A concrete call should reach its specialization without
                                // also keeping the declaration's symbolic dependencies live.
                                let mut dependency = method_full.clone();
                                let ret_ty = if !struct_params.is_empty() {
                                    let mut subst: std::collections::HashMap<String, TypeKind> =
                                        std::collections::HashMap::new();
                                    if let Some(expected) =
                                        self.contextual_expected_types.last().cloned()
                                    {
                                        let expected = self.resolve_type_aliases(&expected);
                                        if let TypeKind::Named {
                                            name: expected_name,
                                            type_args: expected_args,
                                        } = expected
                                            && expected_name.rsplit('.').next()
                                                == Some(struct_name.as_str())
                                            && expected_args.len() == struct_params.len()
                                        {
                                            subst.extend(
                                                struct_params.iter().zip(expected_args).map(
                                                    |(param, argument)| {
                                                        (param.clone(), argument.node)
                                                    },
                                                ),
                                            );
                                        }
                                    }
                                    for (param_ty, arg_eval) in
                                        sym.params.iter().zip(slot_evals.iter())
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
                                            inferred_static_args = Some(type_args.clone());
                                            self.record_ownership_call_site(
                                                expr.span,
                                                Some((&method_full, &type_args)),
                                            );
                                            let specialized_params: Vec<TypeKind> = sym
                                                .params
                                                .iter()
                                                .map(|param| substitute_type_kind(param, &subst))
                                                .collect();
                                            checked_params = Some(specialized_params.clone());
                                            let specialized_return = sym
                                                .ty
                                                .as_ref()
                                                .map(|ty| substitute_type_kind(ty, &subst));
                                            self.record_fn_value_layout(
                                                &method_full,
                                                &type_args,
                                                &specialized_params,
                                                specialized_return.as_ref(),
                                                sym.variadic,
                                                expr.span,
                                            );
                                            let mono_name =
                                                mangle_monomorphized(&method_full, &type_args);
                                            if !self.has_monomorphization(
                                                &method_full,
                                                &type_args,
                                                &mono_name,
                                            ) {
                                                self.monomorphizations.push(MonomorphizationInfo {
                                                    fn_name: method_full.clone(),
                                                    type_args,
                                                    mangled_name: mono_name.clone(),
                                                });
                                            }
                                            dependency = mono_name;
                                        }
                                    }
                                    sym.ty.as_ref().map(|t| substitute_type_kind(t, &subst))
                                } else {
                                    self.record_ownership_call_site(
                                        expr.span,
                                        Some((&method_full, &[])),
                                    );
                                    sym.ty.clone()
                                };
                                self.add_dependency_edge(DependencyKind::Call, &from, &dependency);
                                if let Some(params) = checked_params {
                                    self.check_associated_call_arguments(
                                        expr.span,
                                        &sym,
                                        &params,
                                        args,
                                        arg_evals,
                                        named_args.len(),
                                    );
                                }
                                let eval = ExprEval {
                                    ty: ret_ty,
                                    const_value: None,
                                };
                                self.annotate_expr(expr, &eval, reachable, None);
                                // Codegen names the specialization from these, in the
                                // function's type-parameter order.
                                if let Some(inferred) = inferred_static_args
                                    && let Some(annotation) = self.annotated_exprs.last_mut()
                                {
                                    annotation.inferred_type_args = inferred;
                                }
                                self.reject_nested_owned_function_expression(expr, &eval);
                                return eval;
                            }

                            for arg in args {
                                self.type_check_expr(arg, reachable);
                            }
                            for (_, arg) in named_args {
                                self.type_check_expr(arg, reachable);
                            }
                            self.push_error(
                                expr.span,
                                "S04",
                                format!(
                                    "associated function `{method_full}` does not exist; define it in an `impl {struct_name}` block"
                                ),
                            );
                            return ExprEval::default();
                        }
                    }

                    // Qualified enum constructor calls (`Option.Some(x)`,
                    // `Shape.Circle(5.0)`) are lowered structurally by codegen
                    // but have no semantic constructor resolution. Validate the
                    // variant, its arity, and the one-slot payload
                    // representation here; associated functions on the enum
                    // type keep their normal resolution below.
                    if let ExprKind::Ident(enum_name) = &object.node
                        && self.enums.contains_key(enum_name.as_str())
                        && matches!(
                            self.resolve_symbol(enum_name).map(|symbol| symbol.kind),
                            Some(SymbolKind::TypeName)
                        )
                    {
                        let variant_arity = self
                            .enums
                            .get(enum_name.as_str())
                            .and_then(|info| info.variants.get(method).copied());
                        let has_associated_fn = self
                            .resolve_symbol(&format!("{enum_name}.{method}"))
                            .is_some();
                        if variant_arity.is_some() || !has_associated_fn {
                            match variant_arity {
                                None => {
                                    self.push_error(
                                        expr.span,
                                        "S04",
                                        format!(
                                            "enum `{enum_name}` has no variant or associated function `{method}`"
                                        ),
                                    );
                                }
                                Some(arity) => {
                                    if arity != args.len() {
                                        self.push_error(
                                            expr.span,
                                            "S08",
                                            format!(
                                                "{enum_name}.{method} expects exactly {arity} argument(s)"
                                            ),
                                        );
                                    }
                                }
                            }
                            let enum_params = self
                                .declared_type_parameters(enum_name)
                                .cloned()
                                .unwrap_or_default();
                            let param = |index: usize| -> Vec<TypeKind> {
                                enum_params
                                    .get(index)
                                    .map(|name| TypeKind::Named {
                                        name: name.clone(),
                                        type_args: Vec::new(),
                                    })
                                    .into_iter()
                                    .collect()
                            };
                            let declared_fields = match (enum_name.as_str(), method.as_str()) {
                                ("Option", "Some") | ("Result", "Ok") => param(0),
                                ("Result", "Err") => param(1),
                                _ => self
                                    .enums
                                    .get(enum_name.as_str())
                                    .and_then(|info| info.variant_fields.get(method))
                                    .cloned()
                                    .unwrap_or_default(),
                            };
                            let mut substitution = std::collections::HashMap::new();
                            if !enum_params.is_empty()
                                && let Some(TypeKind::Named { name: expected_name, type_args }) = self
                                    .contextual_expected_types
                                    .last()
                                    .map(|expected| self.resolve_type_aliases(expected))
                                && expected_name == *enum_name
                                && type_args.len() == enum_params.len()
                            {
                                substitution.extend(
                                    enum_params
                                        .iter()
                                        .cloned()
                                        .zip(type_args.into_iter().map(|arg| arg.node)),
                                );
                            }
                            for (index, arg) in args.iter().enumerate() {
                                let arg_eval = self.type_check_expr(arg, reachable);
                                if variant_arity.is_none() {
                                    continue;
                                }
                                let Some(payload_ty) = &arg_eval.ty else {
                                    continue;
                                };
                                if let Some(field_ty) = declared_fields.get(index) {
                                    infer_struct_type_subst(
                                        field_ty,
                                        payload_ty,
                                        &enum_params,
                                        &mut substitution,
                                    );
                                }
                                let resolved = self.resolve_type_aliases(payload_ty);
                                if !matches!(resolved, TypeKind::Error)
                                    && !crate::runtime_layout::runtime_value_layout(&resolved)
                                        .fits_single_slot()
                                {
                                    self.push_error(
                                        arg.span,
                                        "S14",
                                        format!(
                                            "{enum_name}.{method} payload `{resolved}` cannot be stored through the current one-slot enum representation"
                                        ),
                                    );
                                }
                            }
                            // A valid constructor has its enum type once every
                            // type parameter is known (D-021 В10А: its owner
                            // must be destroyed); otherwise it stays untyped
                            // and codegen resolves it structurally.
                            let eval = if variant_arity.is_some()
                                && enum_params.iter().all(|param| substitution.contains_key(param))
                            {
                                ExprEval {
                                    ty: Some(TypeKind::Named {
                                        name: enum_name.clone(),
                                        type_args: enum_params
                                            .iter()
                                            .map(|param| {
                                                Spanned::new(substitution[param].clone(), expr.span)
                                            })
                                            .collect(),
                                    }),
                                    ..ExprEval::default()
                                }
                            } else {
                                ExprEval::default()
                            };
                            self.annotate_expr(expr, &eval, reachable, None);
                            self.reject_nested_owned_function_expression(expr, &eval);
                            return eval;
                        }
                    }

                    let object_eval = self.type_check_expr(object, reachable);
                    // Only builtin-style parse[T] falls back to parse_<suffix>.
                    // A declared consuming or legacy parse method has neither
                    // explicit borrow capability, but must still take priority.
                    let has_declared_parse = object_eval.ty.as_ref().is_some_and(|ty| {
                        let resolved = self.resolve_type_aliases(ty);
                        let type_name = super::declare::type_kind_base_name(&resolved);
                        let leaf = type_name.rsplit('.').next().unwrap_or(&type_name);
                        [format!("{type_name}.parse"), format!("{leaf}.parse")]
                            .iter()
                            .any(|name| self.resolve_symbol(name).is_some())
                    });
                    let capability_method = if method == "parse" && !has_declared_parse {
                        type_args
                            .first()
                            .and_then(|ty| parse_method_suffix(&ty.node))
                            .map(|suffix| format!("parse_{suffix}"))
                            .unwrap_or_else(|| method.clone())
                    } else {
                        method.clone()
                    };
                    let (shared_receiver_is_explicit, exclusive_receiver_is_explicit) = object_eval
                        .ty
                        .as_ref()
                        .map(|ty| {
                            self.explicit_method_receiver_capabilities(ty, &capability_method)
                        })
                        .unwrap_or((false, false));
                    self.require_exclusive_receiver(
                        object,
                        object_eval.ty.as_ref(),
                        &capability_method,
                    );
                    let reference_receiver_is_compatible =
                        object_eval.ty.as_ref().is_some_and(|ty| {
                            match self.resolve_type_aliases(ty) {
                                TypeKind::Ref { inner } => {
                                    shared_receiver_is_explicit
                                        || matches!(
                                            self.resolve_type_aliases(&inner.node),
                                            TypeKind::Str
                                        )
                                }
                                TypeKind::MutRef { .. } => {
                                    shared_receiver_is_explicit || exclusive_receiver_is_explicit
                                }
                                other => !contains_non_string_reference(&other),
                            }
                        });
                    if !reference_receiver_is_compatible
                        || self.expr_dereferences_shared_reference(object)
                    {
                        self.push_error(
                            object.span,
                            "S07",
                            "method calls through shared references are unavailable until methods distinguish shared and mutable receivers"
                                .to_string(),
                        );
                    }
                    let contextual_method_signature =
                        object_eval.ty.as_ref().and_then(|object_ty| {
                            if let TypeKind::Dyn { trait_name } = object_ty {
                                return self
                                    .trait_method_signatures
                                    .get(trait_name.as_str())
                                    .and_then(|methods| methods.get(method))
                                    .map(|signature| {
                                        if signature.has_explicit_receiver {
                                            signature.params[1..].to_vec()
                                        } else {
                                            signature.params.clone()
                                        }
                                    });
                            }
                            let resolved_object_ty = self.resolve_type_aliases(object_ty);
                            let type_name =
                                super::declare::type_kind_base_name(&resolved_object_ty);
                            let mangled = format!("{}.{}", type_name, method);
                            let leaf = format!(
                                "{}.{method}",
                                type_name.rsplit('.').next().unwrap_or(&type_name)
                            );
                            let symbol = self
                                .resolve_symbol(&mangled)
                                .or_else(|| self.resolve_symbol(&leaf))?;
                            let mut substitution: std::collections::HashMap<String, TypeKind> =
                                match receiver_named_type_args(&resolved_object_ty).as_slice() {
                                    type_args if !type_args.is_empty() => self
                                        .struct_generic_params
                                        .get(type_name.as_str())
                                        .map(|params| {
                                            params
                                                .iter()
                                                .zip(type_args.iter())
                                                .map(|(param, ty)| (param.clone(), ty.node.clone()))
                                                .collect()
                                        })
                                        .unwrap_or_default(),
                                    _ => std::collections::HashMap::new(),
                                };
                            for (index, binder) in symbol.generic_params.iter().enumerate() {
                                substitution.remove(binder);
                                if let Some(argument) = type_args.get(index) {
                                    substitution.insert(binder.clone(), argument.node.clone());
                                }
                            }
                            let params = symbol
                                .params
                                .iter()
                                .map(|param| {
                                    contextual_call_parameter(
                                        param,
                                        &symbol.generic_params,
                                        &substitution,
                                    )
                                })
                                .collect::<Vec<_>>();
                            Some(params.get(1..).unwrap_or(&[]).to_vec())
                        });
                    let parameter_names = object_eval
                        .ty
                        .as_ref()
                        .and_then(|ty| {
                            let ty = self.resolve_type_aliases(ty);
                            let type_name = super::declare::type_kind_base_name(&ty);
                            let qualified = format!("{type_name}.{method}");
                            let leaf = format!(
                                "{}.{method}",
                                type_name.rsplit('.').next().unwrap_or(&type_name)
                            );
                            self.fn_param_names
                                .get(&qualified)
                                .or_else(|| self.fn_param_names.get(&leaf))
                                .cloned()
                        })
                        .unwrap_or_default();
                    if let Some(object_ty) = object_eval.ty.as_ref() {
                        let resolved = self.resolve_type_aliases(object_ty);
                        let type_name = super::declare::type_kind_base_name(&resolved);
                        let mangled = format!("{type_name}.{method}");
                        let leaf = format!(
                            "{}.{method}",
                            type_name.rsplit('.').next().unwrap_or(&type_name)
                        );
                        let name = if self.resolve_symbol(&mangled).is_some() { mangled } else { leaf };
                        self.note_format_template(Some(&name), args);
                    }
                    let all_evals = self.type_check_call_arguments(
                        args,
                        named_args,
                        reachable,
                        contextual_method_signature
                            .as_ref()
                            .map(|params| (params.as_slice(), parameter_names.as_slice())),
                    );
                    let arg_evals = all_evals[..args.len()].to_vec();
                    let named_arg_evals: Vec<(String, Expr, ExprEval)> = named_args
                        .iter()
                        .zip(&all_evals[args.len()..])
                        .map(|((name, arg), eval)| (name.clone(), arg.clone(), eval.clone()))
                        .collect();

                    // Library-defined impl methods take priority over compiler builtins.
                    // Primitive types use the same mangled namespace (`str.foo`,
                    // `i64.foo`, and so on) as named types, which keeps their API in
                    // Quazi instead of hardcoding every convenience method here.
                    // Returns Some(return_ty) when an impl method is found and side-effects recorded.
                    let impl_resolved: Option<(Option<TypeKind>, String)> = if let Some(object_ty) =
                        &object_eval.ty.clone()
                    {
                        let resolved_object_ty = self.resolve_type_aliases(object_ty);
                        // These operations are representation-level compiler builtins on
                        // primitive receivers. Prelude Display impls intentionally express
                        // their defaults in terms of them (for example i32.to_str calls
                        // i32.to_string), so resolving that inner call back to the impl
                        // would recurse forever. Named types still get normal impl lookup.
                        let prefer_primitive_builtin = !receiver_is_named_type(&resolved_object_ty)
                            && matches!(
                                method.as_str(),
                                "len" | "as_string" | "as_str" | "as_ptr" | "parse"
                            );
                        if prefer_primitive_builtin {
                            None
                        } else {
                            let type_name =
                                super::declare::type_kind_base_name(&resolved_object_ty);
                            // Explicit `value.method[T](...)` arguments of a generic method.
                            let explicit_method_type_args: Vec<TypeKind> =
                                type_args.iter().map(|arg| arg.node.clone()).collect();
                            let type_args = receiver_named_type_args(&resolved_object_ty);
                            let mangled = format!("{}.{}", type_name, method);
                            // Impl chunks are indexed by their declared type name. An
                            // imported type may carry a module-qualified display name
                            // (`fs.File`) in expression annotations, but its impl body
                            // is still indexed as `File.close`. Keep lookup in the
                            // qualified semantic namespace, then record the canonical
                            // codegen key for reachability and direct dispatch.
                            let declared_type_name =
                                type_name.rsplit('.').next().unwrap_or(type_name.as_str());
                            let codegen_mangled = format!("{}.{}", declared_type_name, method);
                            let resolved_mangled = if self.resolve_symbol(&mangled).is_some() {
                                mangled.clone()
                            } else {
                                codegen_mangled.clone()
                            };
                            if let Some(sym) = self.resolve_for_read(&resolved_mangled) {
                                let from = self
                                    .current_function
                                    .last()
                                    .cloned()
                                    .unwrap_or_else(|| "__program__".to_string());
                                let impl_params = self
                                    .impl_generic_params
                                    .get(&resolved_mangled)
                                    .cloned()
                                    .unwrap_or_default();
                                let mut subst = std::collections::HashMap::new();
                                let receiver_value_ty = match &resolved_object_ty {
                                    TypeKind::Ref { inner } | TypeKind::MutRef { inner } => {
                                        &inner.node
                                    }
                                    other => other,
                                };
                                if let Some(receiver_ty) = sym.params.first() {
                                    let expected_receiver = match receiver_ty {
                                        TypeKind::Ref { inner } | TypeKind::MutRef { inner } => {
                                            &inner.node
                                        }
                                        other => other,
                                    };
                                    infer_type_subst(
                                        expected_receiver,
                                        receiver_value_ty,
                                        &impl_params,
                                        &mut subst,
                                    );
                                    let expected_receiver = self.resolve_type_aliases(
                                        &substitute_type_kind(expected_receiver, &subst),
                                    );
                                    if !self.types_compatible(&expected_receiver, receiver_value_ty)
                                    {
                                        self.push_error(
                                            expr.span,
                                            "S08",
                                            format!(
                                                "receiver `{receiver_value_ty}` does not match implementation target `{expected_receiver}`"
                                            ),
                                        );
                                        return ExprEval::default();
                                    }
                                }
                                // A generic method's own parameters come from explicit
                                // type arguments or from its arguments.
                                let method_generics = sym.generic_params.clone();
                                if !method_generics.is_empty() {
                                    if !explicit_method_type_args.is_empty() {
                                        if explicit_method_type_args.len() != method_generics.len() {
                                            self.push_error(
                                                expr.span,
                                                "S08",
                                                format!(
                                                    "method `{codegen_mangled}` takes {} type argument(s), got {}",
                                                    method_generics.len(),
                                                    explicit_method_type_args.len()
                                                ),
                                            );
                                        }
                                        for (param, argument) in
                                            method_generics.iter().zip(&explicit_method_type_args)
                                        {
                                            subst.insert(param.clone(), argument.clone());
                                        }
                                    } else {
                                        for (param, eval) in sym.params.iter().skip(1).zip(arg_evals.iter()) {
                                            if let Some(actual) = &eval.ty {
                                                infer_type_subst(param, actual, &method_generics, &mut subst);
                                            }
                                        }
                                        // Named arguments infer through their declared slots.
                                        let names = self
                                            .fn_param_names
                                            .get(&codegen_mangled)
                                            .cloned()
                                            .unwrap_or_default();
                                        for (name, _, eval) in &named_arg_evals {
                                            if let (Some(position), Some(actual)) = (
                                                names
                                                    .iter()
                                                    .position(|parameter| parameter == name),
                                                &eval.ty,
                                            ) && let Some(param) = sym.params.get(position + 1)
                                            {
                                                infer_type_subst(
                                                    param,
                                                    actual,
                                                    &method_generics,
                                                    &mut subst,
                                                );
                                            }
                                        }
                                    }
                                    if let Some(missing) =
                                        method_generics.iter().find(|param| !subst.contains_key(*param))
                                    {
                                        self.push_error(
                                            expr.span,
                                            "S14",
                                            format!(
                                                "cannot infer type parameter `{missing}` of `{codegen_mangled}`; write `.{method}[Type](...)`"
                                            ),
                                        );
                                    }
                                }
                                let ownership_type_args = impl_params
                                    .iter()
                                    .chain(&method_generics)
                                    .filter_map(|param| subst.get(param).cloned())
                                    .collect::<Vec<_>>();
                                self.record_ownership_call_site(
                                    expr.span,
                                    Some((&resolved_mangled, &ownership_type_args)),
                                );
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
                                    if is_variadic
                                        && i >= fixed_count
                                        && sym
                                            .attributes
                                            .iter()
                                            .any(|attribute| attribute == "str_variadic")
                                    {
                                        continue;
                                    }
                                    let param_ty = if is_variadic && i >= fixed_count {
                                        method_params.last()
                                    } else {
                                        method_params.get(i)
                                    };
                                    if let (Some(param_ty), Some(arg_ty)) = (param_ty, &arg_eval.ty)
                                    {
                                        let arg_expr = args.get(i).unwrap_or(expr);
                                        self.complete_sum_argument(arg_expr, arg_ty, param_ty);
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
                                        .get(&codegen_mangled)
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
                                        let Some(pos) =
                                            param_names.iter().position(|p| p == arg_name)
                                        else {
                                            self.push_error(
                                                expr.span,
                                                "S09",
                                                format!(
                                                    "unknown parameter name `{}` for method `{}`",
                                                    arg_name, codegen_mangled
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
                                // Apply type substitution to the result and validate the
                                // physical ABI before recording a specialization.
                                let generic_call = !type_args.is_empty() || !method_generics.is_empty();
                                let ret_ty = if generic_call {
                                    sym.ty.as_ref().map(|ty| substitute_type_kind(ty, &subst))
                                } else {
                                    sym.ty.clone()
                                };
                                let mut resolved_target = codegen_mangled.clone();
                                if generic_call {
                                    // Receiver type arguments, then the method's own.
                                    let mut type_kinds: Vec<TypeKind> =
                                        type_args.iter().map(|t| t.node.clone()).collect();
                                    type_kinds.extend(
                                        method_generics
                                            .iter()
                                            .filter_map(|param| subst.get(param).cloned()),
                                    );
                                    self.record_fn_value_layout(
                                        &codegen_mangled,
                                        &type_kinds,
                                        &substituted_params,
                                        ret_ty.as_ref(),
                                        is_variadic,
                                        expr.span,
                                    );
                                    let type_kinds_for_codegen = type_kinds.clone();
                                    let mono_name =
                                        mangle_monomorphized(&codegen_mangled, &type_kinds);
                                    if !self.has_monomorphization(
                                        &codegen_mangled,
                                        &type_kinds,
                                        &mono_name,
                                    ) {
                                        self.monomorphizations.push(MonomorphizationInfo {
                                            fn_name: codegen_mangled.clone(),
                                            type_args: type_kinds,
                                            mangled_name: mono_name.clone(),
                                        });
                                    }
                                    self.add_dependency_edge(
                                        DependencyKind::Call,
                                        &from,
                                        &mono_name,
                                    );
                                    if !method_generics.is_empty() {
                                        resolved_target = mono_name.clone();
                                        // Inside a generic body these name its
                                        // parameters; each specialization of the
                                        // caller substitutes its own.
                                        inferred_type_args = Some(type_kinds_for_codegen.clone());
                                    }
                                } else {
                                    self.add_dependency_edge(
                                        DependencyKind::Call,
                                        &from,
                                        &codegen_mangled,
                                    );
                                }
                                if sym
                                    .attributes
                                    .iter()
                                    .any(|attribute| attribute == "str_variadic")
                                {
                                    self.add_dependency_edge(
                                        DependencyKind::Call,
                                        &from,
                                        "fmt.format",
                                    );
                                    if let Some(expanded) =
                                        crate::parser::format::expand_format_call_args(args)
                                    {
                                        for argument in &expanded.args {
                                            self.type_check_expr(argument, reachable);
                                        }
                                    }
                                }
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
                                Some((ret_ty, resolved_target))
                            } else {
                                None
                            }
                        }
                    } else {
                        None
                    };

                    // Dynamic dispatch retains the declaration's concrete signature.
                    // Methods mentioning `Self` in their return type are not object-safe:
                    // the erased receiver cannot determine a concrete result layout.
                    let dyn_resolved: Option<Option<TypeKind>> = if impl_resolved.is_none() {
                        if let Some(TypeKind::Dyn { trait_name }) = &object_eval.ty {
                            let signature = self
                                .trait_method_signatures
                                .get(trait_name.as_str())
                                .and_then(|methods| methods.get(method))
                                .cloned();
                            if let Some(signature) = signature {
                                let trait_is_generic = self
                                    .resolve_symbol(trait_name)
                                    .is_some_and(|symbol| !symbol.generic_params.is_empty());
                                let method_params = if signature.has_explicit_receiver {
                                    &signature.params[1..]
                                } else {
                                    &signature.params[..]
                                };
                                let object_unsafe_reason = if trait_is_generic {
                                    Some("the trait has unresolved generic parameters")
                                } else if !signature.generic_params.is_empty() {
                                    Some("the method is generic")
                                } else if method_params.iter().any(type_contains_self) {
                                    Some("a non-receiver parameter contains `Self`")
                                } else if type_contains_self(&signature.return_ty) {
                                    Some("the return type contains `Self`")
                                } else {
                                    None
                                };
                                if let Some(reason) = object_unsafe_reason {
                                    self.push_error(
                                        expr.span,
                                        "S14",
                                        format!(
                                            "dynamic call to `{}.{}` is not object-safe because {}",
                                            trait_name, method, reason
                                        ),
                                    );
                                    Some(None)
                                } else {
                                    if !type_args.is_empty() {
                                        self.push_error(
                                            expr.span,
                                            "S14",
                                            "dynamic trait methods do not accept explicit type arguments"
                                                .to_string(),
                                        );
                                    }
                                    if !named_args.is_empty() {
                                        self.push_error(
                                            expr.span,
                                            "S09",
                                            "named arguments are not supported for dynamic trait methods"
                                                .to_string(),
                                        );
                                    }
                                    if args.len() != method_params.len() {
                                        self.push_error(
                                            expr.span,
                                            "S08",
                                            format!(
                                                "expected {} args, got {}",
                                                method_params.len(),
                                                args.len()
                                            ),
                                        );
                                    }
                                    for (index, ((arg, eval), expected)) in args
                                        .iter()
                                        .zip(arg_evals.iter())
                                        .zip(method_params.iter())
                                        .enumerate()
                                    {
                                        if let Some(actual) = &eval.ty
                                            && !self.check_expr_compat(arg, expected, actual)
                                        {
                                            self.push_error(
                                                arg.span,
                                                "S08",
                                                format!(
                                                    "arg {}: expected {}, got {}",
                                                    index + 1,
                                                    expected,
                                                    actual
                                                ),
                                            );
                                        }
                                    }
                                    Some(Some(signature.return_ty))
                                }
                            } else if self.trait_method_slots.contains_key(trait_name.as_str()) {
                                self.push_error(
                                    expr.span,
                                    "S04",
                                    format!("trait `{}` has no method `{}`", trait_name, method),
                                );
                                Some(None)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let (ty, resolved_impl_method) = if let Some((impl_ty, resolved_method)) =
                        impl_resolved
                    {
                        (impl_ty, Some(resolved_method))
                    } else if let Some(dyn_ty) = dyn_resolved {
                        (dyn_ty, None)
                    } else {
                        let ref_str = TypeKind::Ref {
                            inner: Box::new(Spanned::new(TypeKind::Str, expr.span)),
                        };
                        let builtin_ty = match method.as_str() {
                            "len" => Some(TypeKind::Usize),
                            "to_string" => {
                                self.push_error(expr.span, "S04", "to_string() requires an owned Display implementation; enable the prelude or provide one".to_string());
                                None
                            }
                            "as_string" | "as_str" => match &object_eval.ty {
                                Some(t) if is_string_view_type(&self.resolve_type_aliases(t)) => {
                                    Some(ref_str.clone())
                                }
                                Some(other) => {
                                    self.push_error(
                                        expr.span,
                                        "S06",
                                        format!("as_str() not valid for {} — use to_string() to convert numbers and bools to text", other),
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
                            "parse" => self.resolve_checked_parse(
                                expr.span,
                                &object_eval.ty,
                                type_args,
                                args.len(),
                            ),
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
                            _ if let Some(bounded) = self.bounded_generic_method(
                                object_eval.ty.as_ref(),
                                method,
                                expr.span,
                            ) =>
                            {
                                bounded
                            }
                            _ => {
                                // A struct or scalar receiver with no such method
                                // anywhere: report it instead of an unknown result.
                                let receiver = object_eval.ty.as_ref().map(|ty| {
                                    match self.resolve_type_aliases(ty) {
                                        TypeKind::Ref { inner } | TypeKind::MutRef { inner } => {
                                            self.resolve_type_aliases(&inner.node)
                                        }
                                        other => other,
                                    }
                                });
                                let concrete = match &receiver {
                                    Some(TypeKind::Named { name, .. }) => {
                                        self.struct_defs.contains_key(name)
                                            && !self
                                                .current_generic_params
                                                .iter()
                                                .any(|scope| scope.contains(name))
                                    }
                                    Some(ty) => {
                                        Self::is_integer(ty)
                                            || Self::is_float(ty)
                                            || matches!(ty, TypeKind::Bool | TypeKind::Void)
                                    }
                                    None => false,
                                };
                                if concrete && let Some(receiver) = &receiver {
                                    self.push_error(
                                        expr.span,
                                        "S04",
                                        format!("no method `{method}` on type `{receiver}`"),
                                    );
                                }
                                None
                            }
                        };
                        (builtin_ty, None)
                    };
                    resolved_method = resolved_impl_method;
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
                        if let Some(sym) = self.resolve_for_read(&resolved)
                            && sym.attributes.iter().any(|a| a == "module_const")
                        {
                            if !sym.public {
                                self.push_error(
                                    expr.span,
                                    "S04",
                                    format!("const `{name}` is private to its module"),
                                );
                            }
                            let eval = ExprEval {
                                ty: sym.ty,
                                const_value: sym.const_value,
                            };
                            self.annotate_expr(expr, &eval, reachable, None);
                            return eval;
                        }
                        if let Some(sym) = self.resolve_for_read(&resolved) {
                            self.check_module_function_visibility(&sym, name, expr.span);
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
                                self.reject_nested_owned_function_expression(expr, &eval);
                                return eval;
                            }
                        } else if !self.source_files.is_empty() {
                            let message = self.missing_module_member_message(object, name);
                            self.push_error(expr.span, "S04", message);
                            return ExprEval::default();
                        }
                    }
                }

                let obj_eval = self.type_check_expr(object, reachable);
                let aggregate_ty = obj_eval.ty.as_ref().map(|ty| {
                    let resolved = self.resolve_type_aliases(ty);
                    match resolved {
                        TypeKind::Ref { inner } | TypeKind::MutRef { inner } => {
                            self.resolve_type_aliases(&inner.node)
                        }
                        _ => resolved,
                    }
                });
                if let Some(TypeKind::Named {
                    name: aggregate_name,
                    ..
                }) = aggregate_ty.as_ref()
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
                let field_ty = match aggregate_ty {
                    Some(TypeKind::Named {
                        name: struct_name,
                        type_args,
                    }) => {
                        let raw = self
                            .struct_defs
                            .get(&struct_name)
                            .and_then(|fields| fields.iter().find(|(fn_, _)| fn_ == name))
                            .map(|(_, ty)| ty.clone());
                        if let Some(raw_ty) = raw {
                            let gp = self
                                .struct_generic_params
                                .get(&struct_name)
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
                // `C.B` names a unit variant of a non-generic enum: its type
                // is the enum (`const c = C.B;` must not be untyped, or match
                // exhaustiveness and assignment checks are skipped).
                let field_ty = field_ty.or_else(|| self.unit_variant_type(object, name));
                ExprEval {
                    ty: field_ty,
                    const_value: None,
                }
            }
            ExprKind::StructInit { name, fields } => {
                let canonical = self
                    .type_name_bindings
                    .get(name)
                    .map(|binding| binding.name.clone())
                    .unwrap_or_else(|| name.clone());
                let name = &canonical;
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
                // The runtime String owns its text; a literal outside its
                // declaring file could wrap a borrowed view (`String { data: v
                // }`) and free it later. Its constructors copy instead.
                let literal_type = self.resolve_type_aliases(&TypeKind::Named {
                    name: name.clone(),
                    type_args: Vec::new(),
                });
                if let Some(abi) = &self.checked_string_abi
                    && (abi.declaration.name == *name || abi.matches_type(&literal_type))
                    && !self.same_source_file(abi.declaration.span, expr.span)
                {
                    self.push_error(
                        expr.span,
                        "S14",
                        format!(
                            "`{name}` cannot be built with a struct literal outside its declaration; use `{name}.new()` or `{name}.from(text)`"
                        ),
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
                let mut provided = std::collections::HashSet::new();
                for (field, value) in fields {
                    if !provided.insert(field) {
                        self.push_error(
                            value.span,
                            "S07",
                            format!("duplicate field `{field}` in `{name}` initializer"),
                        );
                    }
                }
                let generic_params = self
                    .struct_generic_params
                    .get(name)
                    .cloned()
                    .unwrap_or_default();
                let mut substitution = std::collections::HashMap::new();
                // `const h: Holder[i64] = Holder { item: 5 }` takes its type
                // arguments from the declared type before field inference.
                if !generic_params.is_empty()
                    && let Some(TypeKind::Named { name: expected_name, type_args }) = self
                        .contextual_expected_types
                        .last()
                        .map(|expected| self.resolve_type_aliases(expected))
                    && expected_name == *name
                    && type_args.len() == generic_params.len()
                {
                    substitution.extend(
                        generic_params
                            .iter()
                            .cloned()
                            .zip(type_args.into_iter().map(|arg| arg.node)),
                    );
                }
                if let Some(field_defs) = self.struct_defs.get(name).cloned() {
                    let mut scheduled_fields: Vec<_> = fields.iter().collect();
                    scheduled_fields.sort_by_key(|(name, _)| {
                        field_defs
                            .iter()
                            .position(|(declared, _)| declared == name)
                            .unwrap_or(usize::MAX)
                    });
                    for (fname, fval) in scheduled_fields {
                        // A field value sees its own field type as context,
                        // never the enclosing literal's (`Box2 { item: Box2 {..} }`).
                        let field_context = field_defs
                            .iter()
                            .find(|(declared, _)| declared == fname)
                            .map(|(_, ty)| substitute_type_kind(ty, &substitution))
                            .filter(|ty| !type_mentions_params(ty, &generic_params))
                            .unwrap_or(TypeKind::Void);
                        let val_eval = if matches!(field_context, TypeKind::Void) {
                            self.contextual_expected_types.push(field_context);
                            let eval = self.type_check_expr(fval, reachable);
                            self.contextual_expected_types.pop();
                            eval
                        } else {
                            self.type_check_expr_expected(fval, reachable, Some(&field_context))
                        };
                        if let Some((_, expected_ty)) =
                            field_defs.iter().find(|(fn_, _)| fn_ == fname)
                        {
                            if let Some(got_ty) = &val_eval.ty {
                                infer_struct_type_subst(
                                    expected_ty,
                                    got_ty,
                                    &generic_params,
                                    &mut substitution,
                                );
                                let concrete_expected =
                                    substitute_type_kind(expected_ty, &substitution);
                                // Generic struct instantiation can produce a
                                // field shape the declaration check could not
                                // see; concrete structs are gated at declaration.
                                if !generic_params.is_empty() {
                                    let resolved_expected =
                                        self.resolve_type_aliases(&concrete_expected);
                                    if !matches!(resolved_expected, TypeKind::Error)
                                        && !crate::runtime_layout::runtime_value_layout(
                                            &resolved_expected,
                                        )
                                        .fits_single_slot()
                                    {
                                        self.push_error(
                                            fval.span,
                                            "S14",
                                            format!(
                                                "field '{fname}' cannot store `{resolved_expected}` through the current one-slot aggregate representation"
                                            ),
                                        );
                                    }
                                }
                                // Field values follow the assignment rules:
                                // literals take the field type, numbers convert
                                // only by lossless widening or `as`.
                                if !self.check_expr_compat(fval, &concrete_expected, got_ty) {
                                    self.push_error(
                                        fval.span,
                                        "S08",
                                        format!(
                                            "field '{}': expected {}, got {}",
                                            fname, concrete_expected, got_ty
                                        ),
                                    );
                                }
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
                let missing_fields: Vec<String> = if self.repr_c_unions.contains(name) {
                    Vec::new()
                } else {
                    self.struct_defs
                        .get(name)
                        .into_iter()
                        .flatten()
                        .filter(|(field_name, _)| {
                            !fields.iter().any(|(provided, _)| provided == field_name)
                        })
                        .map(|(field_name, _)| field_name.clone())
                        .collect()
                };
                for field_name in missing_fields {
                    self.push_error(
                        expr.span,
                        "S08",
                        format!("missing field '{}' in struct '{}'", field_name, name),
                    );
                }
                ExprEval {
                    ty: Some(TypeKind::Named {
                        name: name.clone(),
                        type_args: if generic_params.iter().all(|param| {
                            substitution.contains_key(param)
                                || self
                                    .current_generic_params
                                    .iter()
                                    .rev()
                                    .any(|params| params.contains(param))
                        }) {
                            generic_params
                                .iter()
                                .map(|param| {
                                    Spanned::new(
                                        substitution.get(param).cloned().unwrap_or_else(|| {
                                            TypeKind::Named {
                                                name: param.clone(),
                                                type_args: Vec::new(),
                                            }
                                        }),
                                        expr.span,
                                    )
                                })
                                .collect()
                        } else {
                            Vec::new()
                        },
                    }),
                    const_value: None,
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                let scrutinee_eval = self.type_check_expr(scrutinee, reachable);
                let scrutinee_ty = scrutinee_eval
                    .ty
                    .as_ref()
                    .map(|ty| self.resolve_type_aliases(ty));
                // Enum discriminants are readable through either kind of safe
                // reference. Payload projection remains unavailable: a payload
                // binding would require materializing an aggregate or taking a
                // reference to inline scalar storage, neither of which has a
                // stable ownership/provenance model yet.
                let match_scrutinee_ty = match &scrutinee_ty {
                    Some(TypeKind::Ref { inner }) | Some(TypeKind::MutRef { inner }) => {
                        Some(inner.node.clone())
                    }
                    ty => ty.clone(),
                };
                let borrowed_enum_scrutinee = matches!(
                    &scrutinee_ty,
                    Some(TypeKind::Ref { .. } | TypeKind::MutRef { .. })
                ) && match_scrutinee_ty.as_ref().is_some_and(|ty| {
                    let TypeKind::Named { name, .. } = self.resolve_type_aliases(ty) else {
                        return false;
                    };
                    self.enums.contains_key(&name)
                });
                let mut initialization_match =
                    self.initialization_match(match_scrutinee_ty.as_ref(), &scrutinee_eval);
                let mut arm_infos = Vec::new();
                let mut result_ty: Option<TypeKind> = None;

                if arms.is_empty() {
                    self.push_error(expr.span, "S09", "match expression has no arms".to_string());
                }

                for arm in arms {
                    let (coverage, input) = initialization_match.arm(&arm.pattern);
                    self.restore_initialization(
                        input.as_ref().unwrap_or(&initialization_match.entry),
                    );
                    let previous_reachable = self.initialization_reachable;
                    self.initialization_reachable &= input.is_some();
                    let (mut arm_info, bindings) =
                        self.validate_match_pattern(&arm.pattern, &match_scrutinee_ty);
                    arm_info.span = arm.span;
                    arm_info.has_guard = arm.guard.is_some();

                    if borrowed_enum_scrutinee
                        && !Self::is_borrowed_enum_discriminant_pattern(&arm.pattern)
                    {
                        self.push_error(
                            arm.pattern.span,
                            "S10",
                            "matching a borrowed enum may inspect only its discriminant before aggregate projection semantics are implemented"
                                .to_string(),
                        );
                    }

                    self.enter_scope();
                    // Payload slots include wildcards; substitute the actual enum
                    // arguments before assigning direct binding types.
                    let mut binding_types = std::collections::HashMap::new();
                    if matches!(arm.pattern.node, PatternKind::Struct { .. })
                        && let Some(scrutinee) = match_scrutinee_ty.as_ref()
                    {
                        self.struct_pattern_binding_types(&arm.pattern, scrutinee, &mut binding_types);
                    }
                    if matches!(arm.pattern.node, PatternKind::Variant { .. })
                        && let Some(scrutinee) = match_scrutinee_ty.as_ref()
                    {
                        self.variant_pattern_binding_types(&arm.pattern, scrutinee, &mut binding_types);
                    }
                    for binding in bindings {
                        let ty = binding_types
                            .get(&binding)
                            .cloned()
                            .unwrap_or(TypeKind::Error);
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

                    // Guard effects flow to this body or the next matching arm,
                    // while pattern mismatches retain their original facts.
                    let fallback = self.initialization_snapshot();
                    let mut guard_condition = InitializationCondition {
                        truthy: input.clone(),
                        falsy: None,
                    };
                    if let Some(guard) = &arm.guard {
                        let guard_eval = self.type_check_expr(guard, reachable);
                        guard_condition = if input.is_some() {
                            self.take_initialization_condition(guard, &guard_eval)
                        } else {
                            InitializationCondition::default()
                        };
                        if let Some(guard_ty) = guard_eval.ty
                            && !matches!(guard_ty, TypeKind::Bool | TypeKind::Error)
                            && !Self::is_integer(&guard_ty)
                        {
                            self.push_error(
                                guard.span,
                                "S01",
                                format!("match guard must be bool or integer, got {}", guard_ty),
                            );
                        }
                    }

                    self.restore_initialization(
                        guard_condition.truthy.as_ref().unwrap_or(&fallback),
                    );
                    self.initialization_reachable &= guard_condition.truthy.is_some();
                    let arm_eval = self.type_check_expr(&arm.expr, reachable);
                    let output = if guard_condition.truthy.is_some() {
                        self.take_initialization_condition(&arm.expr, &arm_eval)
                    } else {
                        InitializationCondition::default()
                    };
                    self.exit_scope_collect();
                    self.initialization_reachable = previous_reachable;
                    initialization_match.finish_arm(&coverage, guard_condition.falsy, output);

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

                let (condition, exhaustive, exhausted) = initialization_match.finish();
                if exhausted {
                    self.push_error(
                        expr.span,
                        "S14",
                        "match initialization analysis exceeded its 1,000,000-step fact budget"
                            .to_string(),
                    );
                }
                self.restore_initialization(
                    condition
                        .normal()
                        .as_ref()
                        .unwrap_or(&self.initialization_snapshot()),
                );
                if !exhausted
                    && !exhaustive
                    && result_ty
                        .as_ref()
                        .is_some_and(|ty| !matches!(ty, TypeKind::Void | TypeKind::Error))
                    && match_scrutinee_ty.as_ref().is_some_and(|ty| {
                        matches!(ty, TypeKind::Bool | TypeKind::Str | TypeKind::Ref { .. })
                            || Self::is_integer(ty)
                            || Self::is_float(ty)
                    })
                {
                    self.push_error(expr.span, "S09", "non-exhaustive match may leave its result uninitialized; add a catch-all arm".to_string());
                }
                initialization_condition = Some(condition);
                self.match_candidates.push(MatchCandidate {
                    span: expr.span,
                    scrutinee_ty: match_scrutinee_ty,
                    arms: arm_infos,
                });

                if result_ty
                    .as_ref()
                    .is_some_and(contains_non_string_reference)
                {
                    self.push_error(
                        expr.span,
                        "S10",
                        "match expressions cannot produce shared references before branch lifetimes are tracked"
                            .to_string(),
                    );
                }
                if result_ty
                    .as_ref()
                    .is_some_and(|ty| matches!(self.resolve_type_aliases(ty), TypeKind::Fn { .. }))
                {
                    self.push_error(
                        expr.span,
                        "S10",
                        "match expressions cannot produce owned function values before path-sensitive ownership is implemented"
                            .to_string(),
                    );
                }

                ExprEval {
                    ty: result_ty,
                    const_value: None,
                }
            }
            ExprKind::CompoundAssign { target, op, value } => {
                let target_eval = self.type_check_expr(target, reachable);
                self.analyze_assign_target(target);
                let value_eval = self.type_check_expr(value, reachable);

                let bin_op = match op {
                    CompoundAssignOp::Add => BinOpKind::Add,
                    CompoundAssignOp::Sub => BinOpKind::Sub,
                    CompoundAssignOp::Mul => BinOpKind::Mul,
                    CompoundAssignOp::Div => BinOpKind::Div,
                    CompoundAssignOp::Mod => BinOpKind::Mod,
                };
                self.check_arithmetic_op(
                    expr.span,
                    ArithmeticOp::Compound(op.clone()),
                    target_eval.ty.as_ref(),
                    value_eval.ty.as_ref(),
                );
                let ty =
                    self.infer_binary_type(expr.span, &bin_op, &target_eval.ty, &value_eval.ty);
                self.mark_arithmetic_operand_autoderef(value, value_eval.ty.as_ref());
                self.mark_binary_autoderef(target, &target_eval.ty, value, &value_eval.ty);
                ExprEval {
                    ty,
                    const_value: None,
                }
            }
            ExprKind::IncDec { expr: inner, .. } => {
                let inner_eval = self.type_check_expr(inner, reachable);
                self.analyze_assign_target(inner);

                self.check_arithmetic_op(
                    expr.span,
                    ArithmeticOp::Increment,
                    inner_eval.ty.as_ref(),
                    None,
                );
                if inner_eval
                    .ty
                    .as_ref()
                    .is_some_and(|ty| Self::is_integer(&self.resolve_type_aliases(ty)))
                {
                    self.require_language_panic();
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
                if elem_ty.as_ref().is_some_and(contains_non_string_reference) {
                    self.push_error(
                        expr.span,
                        "S10",
                        "fixed arrays cannot store shared references before element lifetimes are tracked"
                            .to_string(),
                    );
                }
                // Fixed arrays are contiguous register blocks whose elements
                // must each fit one slot; nested multi-slot elements are
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
                let object_value_ty = obj_eval
                    .ty
                    .as_ref()
                    .map(|ty| self.resolve_type_aliases(ty))
                    .map(|ty| match ty {
                        TypeKind::Ref { inner } | TypeKind::MutRef { inner } => {
                            self.resolve_type_aliases(&inner.node)
                        }
                        other => other,
                    });
                let idx_evals: Vec<ExprEval> = indices
                    .iter()
                    .map(|i| self.type_check_expr(i, reachable))
                    .collect();

                // Named type that explicitly implements the Index trait → dispatch to Type.index.
                // Checks trait_impls registry so accidental `fn index` methods don't trigger [].
                let maybe_index_mangled = if let Some(object_ty) = &object_value_ty {
                    let tn = super::declare::type_kind_base_name(object_ty);
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

                let index_reads_value =
                    self.place_write_target_span != Some((expr.span.start, expr.span.end));
                let elem_ty = if let Some(mangled) = maybe_index_mangled {
                    if index_reads_value {
                        resolved_method = Some(mangled.clone());
                    }
                    let from = self
                        .current_function
                        .last()
                        .cloned()
                        .unwrap_or_else(|| "__program__".to_string());
                    if index_reads_value {
                        self.add_dependency_edge(DependencyKind::Call, &from, &mangled);
                    }
                    if let Some(sym) = self.resolve_symbol(&mangled) {
                        if index_reads_value && sym.unsafe_fn && self.unsafe_depth == 0 {
                            self.push_error(
                                expr.span,
                                "S11",
                                format!(
                                    "call to unsafe `[]` operator on `{}` requires unsafe block",
                                    mangled
                                ),
                            );
                        }
                        let (generic_params, substitution) = match object_value_ty.as_ref() {
                            Some(object_ty @ TypeKind::Named { name, .. }) => {
                                let generic_params = self
                                    .struct_generic_params
                                    .get(name.as_str())
                                    .cloned()
                                    .unwrap_or_default();
                                let mut substitution = std::collections::HashMap::new();
                                if let Some(receiver_ty) = sym.params.first() {
                                    let receiver_ty = match receiver_ty {
                                        TypeKind::Ref { inner } | TypeKind::MutRef { inner } => {
                                            &inner.node
                                        }
                                        other => other,
                                    };
                                    infer_type_subst(
                                        receiver_ty,
                                        object_ty,
                                        &generic_params,
                                        &mut substitution,
                                    );
                                }
                                (generic_params, substitution)
                            }
                            _ => (Vec::new(), std::collections::HashMap::new()),
                        };
                        let substituted_params: Vec<TypeKind> = sym
                            .params
                            .iter()
                            .map(|param| substitute_type_kind(param, &substitution))
                            .collect();
                        let return_ty = sym
                            .ty
                            .as_ref()
                            .map(|ty| substitute_type_kind(ty, &substitution));
                        if index_reads_value && !substitution.is_empty() {
                            let type_args: Vec<TypeKind> = generic_params
                                .iter()
                                .filter_map(|param| substitution.get(param))
                                .map(|argument| self.resolve_type_aliases(argument))
                                .collect();
                            if !self
                                .validate_specialization_identity(&mangled, &type_args, expr.span)
                            {
                                return ExprEval::default();
                            }
                            self.record_fn_value_layout(
                                &mangled,
                                &type_args,
                                &substituted_params,
                                return_ty.as_ref(),
                                sym.variadic,
                                expr.span,
                            );
                            if type_args.len() == generic_params.len() {
                                let mono_name = mangle_monomorphized(&mangled, &type_args);
                                if !self.has_monomorphization(&mangled, &type_args, &mono_name) {
                                    self.monomorphizations.push(MonomorphizationInfo {
                                        fn_name: mangled.clone(),
                                        type_args,
                                        mangled_name: mono_name.clone(),
                                    });
                                }
                                self.add_dependency_edge(DependencyKind::Call, &from, &mono_name);
                            }
                        }
                        return_ty
                    } else {
                        None
                    }
                } else {
                    let builtin_index = matches!(
                        &object_value_ty,
                        Some(
                            TypeKind::Array { .. }
                                | TypeKind::Slice { .. }
                                | TypeKind::FlexibleArray { .. }
                                | TypeKind::Bytes
                        )
                    );
                    if builtin_index && indices.len() != 1 {
                        self.push_error(
                            expr.span,
                            "S06",
                            format!(
                                "indexing this value requires exactly one index, got {}",
                                indices.len()
                            ),
                        );
                    }

                    if let (Some(TypeKind::Array { len, .. }), Some(first_eval)) =
                        (&object_value_ty, idx_evals.first())
                        && let Some(ConstValue::Int(index)) = first_eval.const_value
                        && (index < 0 || index as u64 >= *len)
                    {
                        self.push_error(
                            indices[0].span,
                            "S06",
                            format!("fixed-array index {index} is out of bounds for length {len}"),
                        );
                    }
                    if let (ExprKind::Literal(Literal::Bytes(bytes)), Some(first_eval)) =
                        (&object.node, idx_evals.first())
                        && let Some(ConstValue::Int(index)) = first_eval.const_value
                        && (index < 0 || index as usize >= bytes.len())
                    {
                        self.push_error(
                            indices[0].span,
                            "S06",
                            format!(
                                "bytes index {index} is out of bounds for length {}",
                                bytes.len()
                            ),
                        );
                    }

                    if matches!(
                        &object_value_ty,
                        Some(TypeKind::Array { .. } | TypeKind::Slice { .. } | TypeKind::Bytes)
                    ) {
                        self.require_language_panic();
                    }

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
                                | TypeKind::Error
                        )
                    {
                        self.push_error(
                            indices[0].span,
                            "S06",
                            format!("array index must be an integer, got {}", idx_ty),
                        );
                    }
                    match &object_value_ty {
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

            ExprKind::Try { expr: inner } => self.type_check_try_expr(expr, inner, reachable),

            ExprKind::Closure { params, body, .. } => {
                return self.type_check_closure_expr(expr, params, body, reachable, None);
            }
        };

        self.initialization_condition =
            initialization_condition.map(|condition| (expr.span, condition));
        self.reject_nested_owned_function_expression(expr, &result);
        self.annotate_expr(expr, &result, reachable, resolved_method);
        if let Some(inferred) = inferred_type_args
            && let Some(annotation) = self.annotated_exprs.last_mut()
        {
            annotation.inferred_type_args = inferred;
        }
        result
    }

    fn type_check_try_expr(&mut self, expr: &Expr, inner: &Expr, reachable: bool) -> ExprEval {
        let inner_eval = self.type_check_expr(inner, reachable);
        let operand = inner_eval
            .ty
            .as_ref()
            .map(|ty| self.resolve_type_aliases(ty));
        let (wrapper, type_args) = match &operand {
            Some(TypeKind::Named { name, type_args })
                if (name == "Result" && type_args.len() == 2)
                    || (name == "Option" && type_args.len() == 1) =>
            {
                (name, type_args)
            }
            Some(TypeKind::Error) => return inner_eval,
            _ => {
                self.push_error(
                    expr.span,
                    "S14",
                    format!(
                        "`?` operator requires a known Result[T, E] or Option[T] operand, got {}",
                        operand
                            .as_ref()
                            .map_or_else(|| "an unknown type".to_string(), ToString::to_string)
                    ),
                );
                return ExprEval {
                    ty: Some(TypeKind::Error),
                    const_value: None,
                };
            }
        };
        let target = self
            .current_return_type
            .as_ref()
            .map(|ty| self.resolve_type_aliases(ty));
        match &target {
            Some(TypeKind::Named { name, type_args: target_args })
                if name == wrapper && target_args.len() == type_args.len() =>
            {
                // Failure lowering copies the payload unchanged. Assignment
                // compatibility permits conversions which are not emitted here.
                if wrapper == "Result"
                    && !(self.types_have_same_runtime_shape(&target_args[1].node, &type_args[1].node)
                        && self.types_have_same_runtime_shape(&type_args[1].node, &target_args[1].node))
                {
                    self.push_error(
                        expr.span,
                        "S14",
                        format!(
                            "`?` cannot propagate error type {} into {}; error types must match without conversion",
                            type_args[1].node, target_args[1].node
                        ),
                    );
                }
            }
            _ => self.push_error(
                expr.span,
                "S14",
                format!(
                    "`?` on {wrapper} requires an enclosing function or explicitly typed closure returning {wrapper}; current return type is {}",
                    target.as_ref().map_or_else(|| "unavailable".to_string(), ToString::to_string)
                ),
            ),
        }
        ExprEval {
            ty: Some(type_args[0].node.clone()),
            const_value: None,
        }
    }

    /// Whether `==` on `ty` would compare aggregate handle addresses: declared
    /// struct/enum types (including Option/Result), but not checked strings
    /// or generic parameters, which are validated per specialization.
    fn equality_compares_handles(&self, ty: &TypeKind) -> bool {
        let resolved = self.resolve_type_aliases(ty);
        let TypeKind::Named { name, .. } = &resolved else {
            return false;
        };
        !self.is_checked_string(&resolved)
            && self.is_declared_type_name(name)
            && (self.struct_defs.contains_key(name) || self.enums.contains_key(name))
    }

    pub(super) fn is_module_import_receiver(&self, object: &Expr) -> bool {
        let Some((base, _)) = Self::extract_field_chain(object) else {
            return false;
        };
        // An imported type name is a static-method namespace, not a module
        // namespace. Treating it as a module made `Map.new()` and equivalent
        // imported constructors fail before static method resolution.
        if self.struct_defs.contains_key(&base) {
            return false;
        }
        // A namespaced source file may call its own top-level functions through
        // its module name (`thread.thread_spawn()`). This is not an import, but
        // it resolves to the same mangled symbol and must not be rejected merely
        // because a module never imports itself.
        if self.is_local_binding(&base) {
            return false;
        }
        if self.current_module_path.as_deref() == Some(base.as_str()) {
            return true;
        }
        self.resolve_symbol(&base).is_some_and(|sym| sym.is_import)
    }

    /// Locals and parameters shadow module namespaces, including the current
    /// module's own name (`const time = ...; time.method()` inside `time`).
    fn is_local_binding(&self, name: &str) -> bool {
        self.scopes.iter().skip(1).any(|scope| scope.contains_key(name))
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

    /// The declared name of the module-level const that `name` refers to:
    /// a local shadows it; inside a module its own consts come first; an
    /// imported const resolves to its module's declaration.
    pub(super) fn resolve_module_const_name(&self, name: &str) -> Option<String> {
        for scope in self.scopes.iter().skip(1).rev() {
            if scope.contains_key(name) {
                return None;
            }
        }
        let is_const = |symbol: &Symbol| symbol.attributes.iter().any(|a| a == "module_const");
        if let Some(module) = &self.current_module_path {
            let qualified = format!("{}.{}", module, name);
            if self.resolve_symbol(&qualified).is_some_and(|symbol| is_const(&symbol)) {
                return Some(qualified);
            }
        }
        let symbol = self.resolve_symbol(name)?;
        if is_const(&symbol) {
            return Some(name.to_string());
        }
        if symbol.is_import
            && let Some(path) = symbol.import_path.as_deref()
            && let Some(mangled) = super::declare::mangle_import_path(path)
            && self.resolve_symbol(&mangled).is_some_and(|target| is_const(&target))
        {
            return Some(mangled);
        }
        None
    }

    pub(super) fn resolve_bare_foreign_global_name(&self, name: &str) -> Option<String> {
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
    /// A function without `pub` is private to its file, also when reached
    /// through a module path (`store.hidden()`), as for imports.
    fn check_module_function_visibility(&mut self, sym: &Symbol, name: &str, span: Span) {
        if matches!(sym.kind, SymbolKind::Function)
            && !sym.public
            && !self.same_source_file(sym.span, span)
        {
            self.push_error(
                span,
                "S04",
                format!("function `{name}` is private to its module"),
            );
        }
    }

    pub(super) fn resolve_module_method(&self, object: &Expr, method: &str) -> Option<String> {
        let (base, path) = Self::extract_field_chain(object)?;
        if self.is_local_binding(&base) {
            return None;
        }
        if self.current_module_path.as_deref() == Some(base.as_str()) && path.is_empty() {
            return Some(format!("{}.{}", base, method));
        }
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
        let resolved = format!("{}.{}", module_file, method);
        if let Some(symbol) = self.resolve_symbol(&resolved) {
            // A function the module re-exports with `pub import` is called
            // where it is defined.
            if symbol.is_import
                && matches!(symbol.kind, SymbolKind::Function)
                && let Some(real) = symbol.import_path
            {
                return Some(super::declare::mangle_import_path(&real).unwrap_or(real));
            }
            return Some(resolved);
        }

        // A package's entry file may be imported through its file path from
        // within the package (for example `std.core`), while its declarations
        // use the package-root namespace (`std`). Fall back to that root when
        // the file-qualified name is absent; external modules keep the
        // file-qualified name above.
        let package_root = segments.first().copied().unwrap_or(&base);
        let package_resolved = format!("{}.{}", package_root, method);
        if self.resolve_symbol(&package_resolved).is_some() {
            return Some(package_resolved);
        }
        Some(resolved)
    }

    /// Evaluate source arguments in the declared-slot schedule used by codegen,
    /// retaining lexical positions for existing signature/ownership validation.
    fn type_check_call_arguments(
        &mut self,
        args: &[Expr],
        named_args: &[(String, Expr)],
        reachable: bool,
        signature: Option<(&[TypeKind], &[String])>,
    ) -> Vec<ExprEval> {
        let mut schedule = Vec::with_capacity(args.len() + named_args.len());
        for (index, arg) in args.iter().enumerate() {
            let expected =
                signature.and_then(|(params, _)| params.get(index).or_else(|| params.last()));
            schedule.push((index, index, arg, expected));
        }
        for (index, (name, arg)) in named_args.iter().enumerate() {
            let slot = signature
                .and_then(|(_, names)| names.iter().position(|parameter| parameter == name));
            let expected = slot.and_then(|slot| signature?.0.get(slot));
            schedule.push((
                slot.unwrap_or(usize::MAX),
                args.len() + index,
                arg,
                expected,
            ));
        }
        schedule.sort_by_key(|(slot, _, _, _)| *slot);
        let mut result = vec![ExprEval::default(); schedule.len()];
        for (_, index, arg, expected) in schedule {
            result[index] = self.type_check_expr_expected(arg, reachable, expected);
        }
        result
    }

    fn validate_named_args(
        &mut self,
        _span: Span,
        fn_name: &str,
        named_args: &[(String, Expr)],
        positional_count: usize,
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
            // Types are checked by `check_function_call` against the
            // signature after generic substitution.
        }
    }

    /// Argument evaluations in declaration-slot order, for generic inference:
    /// positional arguments keep their slots and each named argument moves to
    /// its parameter's slot. Unsupplied slots have no type.
    fn slot_ordered_evals(
        &self,
        fn_name: &str,
        sym: &Symbol,
        positional_count: usize,
        named_args: &[(String, Expr)],
        arg_evals: &[ExprEval],
    ) -> Vec<ExprEval> {
        let positional = positional_count.min(arg_evals.len());
        let mut ordered = arg_evals[..positional].to_vec();
        let names = self
            .fn_param_names
            .get(fn_name)
            .cloned()
            .unwrap_or_default();
        let receiver_offset = sym.params.len().saturating_sub(names.len());
        for (index, (name, _)) in named_args.iter().enumerate() {
            let (Some(position), Some(eval)) = (
                names.iter().position(|parameter| parameter == name),
                arg_evals.get(positional + index),
            ) else {
                continue;
            };
            let slot = position + receiver_offset;
            if ordered.len() <= slot {
                ordered.resize(slot + 1, ExprEval::default());
            }
            ordered[slot] = eval.clone();
        }
        ordered
    }

    /// The item type `T` of an iterator: a named type whose methods
    /// `has_next(self) bool` and `next(self) Option[T]` drive `for x : it`.
    fn iterator_item_type(&self, ty: &TypeKind) -> Option<TypeKind> {
        let TypeKind::Named { name, type_args } = self.resolve_type_aliases(ty) else {
            return None;
        };
        let has_next = self.resolve_symbol(&format!("{name}.has_next"))?;
        if !matches!(has_next.ty, Some(TypeKind::Bool)) {
            return None;
        }
        let next = self.resolve_symbol(&format!("{name}.next"))?;
        let TypeKind::Named { name: option, type_args: item } = next.ty.as_ref()? else {
            return None;
        };
        if option != "Option" || item.len() != 1 {
            return None;
        }
        let item = item[0].node.clone();
        if type_args.is_empty() {
            return Some(item);
        }
        let parameters = self.generic_params_for_function(&format!("{name}.next"));
        let substitution: std::collections::HashMap<String, TypeKind> = parameters
            .into_iter()
            .zip(type_args.iter().map(|arg| arg.node.clone()))
            .collect();
        Some(substitute_type_kind(&item, &substitution))
    }

    /// Arity and argument types of a type-qualified call `Type.function(...)`,
    /// checked like a free function call. Named arguments count towards the
    /// arity; their types were checked against their declared slots already.
    /// Display-converted format tails are left to the format rules.
    fn check_associated_call_arguments(
        &mut self,
        span: Span,
        sym: &Symbol,
        params: &[TypeKind],
        args: &[Expr],
        arg_evals: &[ExprEval],
        named_count: usize,
    ) {
        let fixed = if sym.variadic { params.len().saturating_sub(1) } else { params.len() };
        let supplied = args.len() + named_count;
        if (!sym.variadic && supplied != fixed) || (sym.variadic && supplied < fixed) {
            self.push_error(
                span,
                "S10",
                format!("expected {} argument(s), got {}", fixed, supplied),
            );
            return;
        }
        let format_tail = sym.attributes.iter().any(|attribute| attribute == "str_variadic");
        for (index, (arg, eval)) in args.iter().zip(arg_evals).enumerate() {
            let expected = if index < fixed {
                &params[index]
            } else if format_tail {
                continue;
            } else {
                match params.last() {
                    Some(element) => element,
                    None => continue,
                }
            };
            if let Some(actual) = &eval.ty
                && !self.check_expr_compat(arg, expected, actual)
            {
                self.push_error(
                    arg.span,
                    "S08",
                    format!("arg {}: expected {}, got {}", index + 1, expected, actual),
                );
            }
        }
    }

    /// Mark `args[0]` as a format template when `callee` is a format function
    /// (`@format`/`str_variadic`): its braces follow the format-call rules.
    fn note_format_template(&mut self, callee: Option<&str>, args: &[Expr]) {
        let Some(first) = args.first() else { return };
        let ExprKind::Literal(Literal::String(text)) = &first.node else {
            return;
        };
        // Without placeholders the literal is plain text: its `{{`/`}}` read
        // as single braces like any other literal.
        if matches!(
            crate::parser::format::classify_string_literal(text),
            crate::parser::format::StringLiteralForm::Plain(_)
        ) {
            return;
        }
        if callee
            .and_then(|name| self.resolve_symbol(name))
            .is_some_and(|symbol| symbol.attributes.iter().any(|attribute| attribute == "str_variadic"))
        {
            self.format_template_literals
                .insert((first.span.start, first.span.end));
        }
    }

    /// D-020: an ordinary string literal is a format string. Without
    /// placeholders it is a `str` whose `{{`/`}}` are single braces; with
    /// `{expr}` placeholders it formats to an owned `String`.
    fn type_check_string_literal(&mut self, expr: &Expr, text: &str, reachable: bool) -> ExprEval {
        let view = TypeKind::Ref {
            inner: Box::new(Spanned::new(TypeKind::Str, expr.span)),
        };
        match crate::parser::format::classify_string_literal(text) {
            crate::parser::format::StringLiteralForm::Plain(plain) => ExprEval {
                ty: Some(view),
                const_value: Some(ConstValue::String(plain)),
            },
            crate::parser::format::StringLiteralForm::Invalid(message) => {
                self.push_error(expr.span, "S01", message);
                ExprEval {
                    ty: Some(view),
                    const_value: None,
                }
            }
            crate::parser::format::StringLiteralForm::Interpolated => {
                let from = self
                    .current_function
                    .last()
                    .cloned()
                    .unwrap_or_else(|| "__program__".to_string());
                self.add_dependency_edge(DependencyKind::Call, &from, "fmt.format");
                if let Some(expanded) =
                    crate::parser::format::expand_format_call_args(std::slice::from_ref(expr))
                {
                    for argument in &expanded.args {
                        self.type_check_expr(argument, reachable);
                    }
                }
                let string = self
                    .resolve_symbol("fmt.format")
                    .and_then(|symbol| symbol.ty.clone())
                    .unwrap_or(TypeKind::Named {
                        name: "String".to_string(),
                        type_args: Vec::new(),
                    });
                ExprEval {
                    ty: Some(string),
                    const_value: None,
                }
            }
        }
    }

    /// The S04 message for `path.member` when `path` is a module namespace
    /// spelling without such a member. Enum variants are not reachable
    /// through module paths yet (`lib.Shape.Circle`); name the remedy.
    fn missing_module_member_message(&self, object: &Expr, member: &str) -> String {
        let Some((base, path)) = Self::extract_field_chain(object) else {
            return format!("'{member}' is not exported by '{member}'");
        };
        let owner = path.last().map_or(base.as_str(), String::as_str);
        if let Some((module, _)) = std::iter::once(base.as_str())
            .chain(path.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(".")
            .rsplit_once('.')
            .map(|(module, enum_name)| (module.to_string(), enum_name.to_string()))
            && self
                .enums
                .get(owner)
                .is_some_and(|info| info.variants.contains_key(member))
        {
            return format!(
                "enum variants cannot be reached through a module path yet: add `import {module}.{owner};` and write `{owner}.{member}`"
            );
        }
        format!("'{member}' is not exported by '{owner}'")
    }

    /// Infer every generic argument of a call without explicit ones from its
    /// positional argument types; `None` when some binder stays unknown.
    fn infer_call_type_args(
        &self,
        sym: &Symbol,
        arg_evals: &[ExprEval],
        span: Span,
    ) -> Option<Vec<Type>> {
        // Generic intrinsics/imports dispatch on their arguments directly.
        if sym.generic_params.is_empty()
            || sym
                .attributes
                .iter()
                .any(|attribute| matches!(attribute.as_str(), "intrinsic" | "syscall" | "api"))
        {
            return None;
        }
        let mut subst = std::collections::HashMap::new();
        for (param, eval) in sym.params.iter().zip(arg_evals) {
            if let Some(actual) = &eval.ty {
                infer_type_subst(param, actual, &sym.generic_params, &mut subst);
            }
        }
        sym.generic_params
            .iter()
            .map(|param| {
                subst
                    .get(param)
                    .filter(|ty| !matches!(ty, TypeKind::Error))
                    .map(|ty| Spanned::new(ty.clone(), span))
            })
            .collect()
    }

    fn check_function_call(
        &mut self,
        name: &str,
        callee_span: Span,
        call_span: Span,
        args: &[Expr],
        named_args: &[(String, Expr)],
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

        // Built-in sum-type constructors are polymorphic, but they are not
        // runtime `any` functions. Preserve every type known at the call site
        // and use the internal error sentinel only for the opposite payload,
        // which is supplied by surrounding Option/Result context.
        if let Some(constructor) = builtin_constructor_kind(name, sym) {
            let expected_args = usize::from(constructor != "None");
            if arg_evals.len() != expected_args {
                self.push_error(
                    callee_span,
                    "S08",
                    format!("{constructor} expects exactly {expected_args} argument(s)"),
                );
                return ExprEval::default();
            }
            if constructor == "None" {
                return ExprEval {
                    ty: Some(TypeKind::Named {
                        name: "Option".to_string(),
                        type_args: vec![Spanned::new(TypeKind::Error, callee_span)],
                    }),
                    const_value: None,
                };
            }
            let payload = arg_evals[0].ty.clone().unwrap_or(TypeKind::Error);
            let span = args.first().map_or(callee_span, |arg| arg.span);
            let resolved_payload = self.resolve_type_aliases(&payload);
            if !matches!(resolved_payload, TypeKind::Error)
                && !crate::runtime_layout::runtime_value_layout(&resolved_payload)
                    .fits_single_slot()
            {
                self.push_error(
                    span,
                    "S14",
                    format!(
                        "{constructor} payload `{resolved_payload}` cannot be stored through the current one-slot enum representation"
                    ),
                );
            }
            let type_args = match constructor {
                "Some" => vec![Spanned::new(payload, span)],
                "Ok" => vec![
                    Spanned::new(payload, span),
                    Spanned::new(TypeKind::Error, span),
                ],
                "Err" => vec![
                    Spanned::new(TypeKind::Error, span),
                    Spanned::new(payload, span),
                ],
                _ => unreachable!(),
            };
            let enum_name = if constructor == "Some" {
                "Option"
            } else {
                "Result"
            };
            return ExprEval {
                ty: Some(TypeKind::Named {
                    name: enum_name.to_string(),
                    type_args,
                }),
                const_value: None,
            };
        }

        let ownership_type_args = type_args
            .iter()
            .map(|ty| ty.node.clone())
            .collect::<Vec<_>>();
        self.record_ownership_call_site(call_span, Some((name, &ownership_type_args)));
        if !type_args.is_empty() && type_args.len() != sym.generic_params.len() {
            self.push_error(
                callee_span,
                "S08",
                format!(
                    "function `{name}` expects {} type argument(s), got {}",
                    sym.generic_params.len(),
                    type_args.len()
                ),
            );
            return ExprEval::default();
        }

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
            if !self.has_monomorphization(name, &type_kinds, &mangled) {
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
                // Explicit arguments were already checked as call arguments;
                // only placeholder expressions from the template are new.
                for arg in &expanded.args {
                    if !args.iter().any(|explicit| explicit.span == arg.span) {
                        self.type_check_expr(arg, true);
                    }
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
        let is_panic = self.is_runtime_panic(&sym);
        let effective_non_variadic_count = if is_panic { 1 } else { non_variadic_count };
        if sym
            .attributes
            .iter()
            .any(|attribute| attribute == "str_variadic")
        {
            for (index, value) in arg_evals
                .iter()
                .enumerate()
                .skip(effective_non_variadic_count)
            {
                if let Some(argument) = args.get(index) {
                    self.check_display_argument(value.ty.as_ref(), argument.span);
                }
            }
        }
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
            for (i, (arg_expr, arg_eval)) in args.iter().zip(arg_evals.iter()).enumerate() {
                let Some(param_ty) = substituted_params.get(i).filter(|_| i < check_count) else {
                    continue;
                };
                let Some(at) = &arg_eval.ty else {
                    continue;
                };
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
                self.complete_sum_argument(arg_expr, at, param_ty);
                if !self.check_expr_compat(arg_expr, param_ty, at) {
                    self.push_error(
                        arg_expr.span,
                        "S08",
                        format!("arg {}: expected {}, got {}", i + 1, param_ty, at),
                    );
                }
            }
            // `arg_evals` stores positional evaluations first and named
            // evaluations afterwards. Validate each named argument against
            // its declared parameter rather than treating it as a missing
            // positional slot.
            let parameter_names = self.fn_param_names.get(name).cloned().unwrap_or_default();
            let receiver_offset = substituted_params
                .len()
                .saturating_sub(parameter_names.len());
            for (named_index, (argument_name, argument)) in named_args.iter().enumerate() {
                let Some(parameter_index) = parameter_names
                    .iter()
                    .position(|parameter| parameter == argument_name)
                else {
                    continue;
                };
                let Some(param_ty) = substituted_params.get(parameter_index + receiver_offset)
                else {
                    continue;
                };
                let Some(Some(actual_ty)) = arg_evals
                    .get(args.len().saturating_add(named_index))
                    .map(|eval| eval.ty.as_ref())
                else {
                    continue;
                };
                if !self.check_expr_compat(argument, param_ty, actual_ty) {
                    self.push_error(
                        argument.span,
                        "S08",
                        format!(
                            "named argument `{argument_name}`: expected {param_ty}, got {actual_ty}"
                        ),
                    );
                }
            }
            // Variadic args checked against the element type.
            if is_variadic
                && !sym
                    .attributes
                    .iter()
                    .any(|attribute| attribute == "str_variadic")
                && let Some(elem_ty) = effective_variadic_elem
            {
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
                        } else if !self.types_compatible(at, elem_ty) {
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

        if !subst.is_empty() {
            let type_kinds: Vec<TypeKind> = type_args.iter().map(|t| t.node.clone()).collect();
            self.record_fn_value_layout(
                name,
                &type_kinds,
                &substituted_params,
                return_ty.as_ref(),
                is_variadic,
                callee_span,
            );
        }

        ExprEval {
            ty: return_ty,
            const_value: None,
        }
    }

    fn resolve_checked_parse(
        &mut self,
        span: Span,
        receiver: &Option<TypeKind>,
        type_args: &[Type],
        argument_count: usize,
    ) -> Option<TypeKind> {
        if argument_count != 0 || type_args.len() != 1 {
            self.push_error(
                span,
                "S08",
                "parse expects one type argument and no value arguments: parse[T]()".to_string(),
            );
            return None;
        }
        let Some(suffix) = parse_method_suffix(&type_args[0].node) else {
            self.push_error(
                type_args[0].span,
                "S06",
                format!(
                    "parse does not support {}; expected an integer or floating-point type",
                    type_args[0].node
                ),
            );
            return None;
        };
        let receiver_value_ty = receiver
            .as_ref()
            .map(|ty| self.resolve_type_aliases(ty))
            .map(|ty| match ty {
                TypeKind::Ref { inner } | TypeKind::MutRef { inner } => {
                    self.resolve_type_aliases(&inner.node)
                }
                other => other,
            });
        let prefix = match &receiver_value_ty {
            Some(ty) if self.is_checked_owned_string(ty) => self
                .checked_string_abi
                .as_ref()
                .unwrap()
                .declaration
                .name
                .as_str(),
            Some(ty) if is_string_view_type(ty) => "str",
            Some(ty) => {
                self.push_error(span, "S06", format!("parse is not available on {}", ty));
                return None;
            }
            None => return None,
        };
        let target = format!("{prefix}.parse_{suffix}");
        let Some(symbol) = self.resolve_for_read(&target) else {
            self.push_error(
                span,
                "S07",
                format!("checked parser `{target}` is unavailable"),
            );
            return None;
        };
        let from = self
            .current_function
            .last()
            .cloned()
            .unwrap_or_else(|| "__program__".to_string());
        self.add_dependency_edge(DependencyKind::Call, &from, &target);
        symbol.ty
    }

    pub(super) fn infer_binary_type(
        &mut self,
        span: Span,
        op: &BinOpKind,
        left: &Option<TypeKind>,
        right: &Option<TypeKind>,
    ) -> Option<TypeKind> {
        // Resolve aliases before interpreting reference representation.
        let mut operand_type = |ty: &TypeKind| {
            let ty = self.resolve_type_aliases(ty);
            if matches!(
                op,
                BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul | BinOpKind::Div | BinOpKind::Mod
            ) && Self::arithmetic_string_load(&ty).is_some()
            {
                TypeKind::Str
            } else {
                self.checked_string_operand_type(ty)
            }
        };
        let left = left.as_ref().map(&mut operand_type);
        let right = right.as_ref().map(operand_type);
        match op {
            BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul | BinOpKind::Div | BinOpKind::Mod => {
                match (&left, &right) {
                    (Some(l), Some(r)) if self.types_compatible(l, r) => {
                        // Integer division and remainder lower to an implicit panic guard.
                        // Record that hidden call so whole-program dead-code elimination keeps
                        // the prelude panic path reachable from the current function.
                        // A symbolic numeric parameter can specialize to an integer.
                        // Checked integer + - * panic on overflow too.
                        if (matches!(op, BinOpKind::Div | BinOpKind::Mod) && !Self::is_float(l))
                            || (matches!(op, BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul)
                                && Self::is_integer(l))
                        {
                            self.require_language_panic();
                        }
                        Some(l.clone())
                    }
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
            BinOpKind::EqEq | BinOpKind::NotEq => {
                if let (Some(l), Some(r)) = (&left, &right)
                    && ((!self.types_compatible(l, r))
                        || (self.is_checked_string(l) != self.is_checked_string(r)))
                {
                    self.push_error(
                        span,
                        "S01",
                        format!("type mismatch in binary op: {} vs {}", l, r),
                    );
                } else if let Some(handle) = [&left, &right]
                    .into_iter()
                    .flatten()
                    .find(|ty| self.equality_compares_handles(ty))
                {
                    // Struct and enum values are heap handles; `==` would compare
                    // addresses, so even `Kind.A == Kind.A` was false.
                    self.push_error(
                        span,
                        "S06",
                        format!(
                            "`==`/`!=` cannot compare {handle} values; use `match` or a comparison method"
                        ),
                    );
                }
                Some(TypeKind::Bool)
            }
            BinOpKind::Lt | BinOpKind::Gt | BinOpKind::LtEq | BinOpKind::GtEq => {
                if let (Some(l), Some(r)) = (&left, &right) {
                    let ordered = (Self::is_integer(l) && Self::is_integer(r))
                        || (Self::is_float(l) && Self::is_float(r))
                        || (self.is_checked_string(l) && self.is_checked_string(r))
                        || self.arithmetic_op_is_deferred(span);
                    if !ordered {
                        self.push_error(
                            span,
                            "S06",
                            format!("ordering requires numbers or strings, got {} and {}", l, r),
                        );
                    }
                }
                Some(TypeKind::Bool)
            }
            BinOpKind::AndAnd | BinOpKind::OrOr => {
                if let Some(l) = &left
                    && !matches!(l, TypeKind::Bool | TypeKind::Error)
                    && !Self::is_integer(l)
                {
                    self.push_error(
                        span,
                        "S06",
                        format!("logical op requires bool or integer, got {}", l),
                    );
                }

                if let Some(r) = &right
                    && !matches!(r, TypeKind::Bool | TypeKind::Error)
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
                (Some(base), Some(exponent)) => power_result_type(base, exponent),
                _ => None,
            },
        }
    }

    pub(super) fn const_from_literal(lit: &Literal) -> Option<ConstValue> {
        match lit {
            Literal::Int(v) => Some(ConstValue::Int(*v as i64)),
            Literal::Float(v) => Some(ConstValue::Float(*v)),
            Literal::String(v) => Some(ConstValue::String(v.clone())),
            Literal::Bytes(_) => None,
            Literal::Bool(v) => Some(ConstValue::Bool(*v)),
        }
    }

    /// Wrap an integer constant to the width of its narrow integer type, as
    /// runtime arithmetic does (codegen `wrap_narrow_integer`).
    fn wrap_const_integer(&self, value: ConstValue, ty: Option<&TypeKind>) -> ConstValue {
        match (&value, ty.map(|ty| self.resolve_type_aliases(ty))) {
            (ConstValue::Int(_), Some(ty)) if Self::is_integer(&ty) => {
                Self::const_numeric_cast(value.clone(), &ty, &ty).unwrap_or(value)
            }
            _ => value,
        }
    }

    pub(super) fn const_from_unary(op: &UnaryOpKind, value: ConstValue) -> Option<ConstValue> {
        match (op, value) {
            (UnaryOpKind::Neg, ConstValue::Int(v)) => Some(ConstValue::Int(v.wrapping_neg())),
            (UnaryOpKind::Neg, ConstValue::Float(v)) => Some(ConstValue::Float(-v)),
            (UnaryOpKind::Not, ConstValue::Bool(v)) => Some(ConstValue::Bool(!v)),
            _ => None,
        }
    }

    pub(super) fn const_from_binary(
        op: &BinOpKind,
        left: &ConstValue,
        right: &ConstValue,
        left_ty: Option<&TypeKind>,
    ) -> Option<ConstValue> {
        let unsigned = left_ty.is_some_and(|ty| {
            matches!(
                ty,
                TypeKind::Uint8
                    | TypeKind::Uint16
                    | TypeKind::Uint32
                    | TypeKind::Uint64
                    | TypeKind::Usize
            )
        });
        match (op, left, right) {
            (BinOpKind::Add, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(a.wrapping_add(*b)))
            }
            (BinOpKind::Sub, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(a.wrapping_sub(*b)))
            }
            (BinOpKind::Mul, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(a.wrapping_mul(*b)))
            }
            (BinOpKind::Div, ConstValue::Int(a), ConstValue::Int(b)) if *b != 0 => {
                if unsigned {
                    Some(ConstValue::Int(((*a as u64) / (*b as u64)) as i64))
                } else {
                    Some(ConstValue::Int(a.wrapping_div(*b)))
                }
            }
            (BinOpKind::Mod, ConstValue::Int(a), ConstValue::Int(b)) if *b != 0 => {
                if unsigned {
                    Some(ConstValue::Int(((*a as u64) % (*b as u64)) as i64))
                } else {
                    Some(ConstValue::Int(a.wrapping_rem(*b)))
                }
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
                Some(ConstValue::Bool(if unsigned {
                    (*a as u64) < (*b as u64)
                } else {
                    a < b
                }))
            }
            (BinOpKind::Gt, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Bool(if unsigned {
                    (*a as u64) > (*b as u64)
                } else {
                    a > b
                }))
            }
            (BinOpKind::LtEq, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Bool(if unsigned {
                    (*a as u64) <= (*b as u64)
                } else {
                    a <= b
                }))
            }
            (BinOpKind::GtEq, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Bool(if unsigned {
                    (*a as u64) >= (*b as u64)
                } else {
                    a >= b
                }))
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
                Some(ConstValue::Int(a.wrapping_shl((*b as u64 & 63) as u32)))
            }
            (BinOpKind::Shr, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(if unsigned {
                    (*a as u64).wrapping_shr((*b as u64 & 63) as u32) as i64
                } else {
                    a.wrapping_shr((*b as u64 & 63) as u32)
                }))
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
        let resolved_binding = resolved_fn
            .as_deref()
            .and_then(|name| self.resolved_binding_for_name(name))
            .or_else(|| match &expr.node {
                ExprKind::Ident(name) => self.resolved_binding_for_name(name),
                _ => None,
            });
        let foldable = self
            .can_elide_expression(expr)
            .then(|| eval.const_value.clone())
            .flatten();
        if self.current_generic_params.iter().any(|scope| !scope.is_empty()) {
            let binders = self.current_generic_params.iter().flatten().cloned().collect();
            self.annotation_binders
                .insert((expr.span.start, expr.span.end), binders);
        }
        self.annotated_exprs.push(ExprAnnotation {
            span: expr.span,
            binding_span: binding_occurrence_span(expr, resolved_binding.as_ref()),
            ty: eval.ty.clone(),
            const_value: foldable.clone(),
            reachable,
            resolved_fn,
            resolved_global: None,
            resolved_binding,
            auto_deref: false,
            c_abi_function: false,
            inferred_type_args: Vec::new(),
        });

        if let Some(value) = &foldable {
            self.constant_evaluations.push(ConstantEvaluation {
                span: expr.span,
                value: value.clone(),
            });
        }
    }

    fn resolved_binding_for_name(&self, name: &str) -> Option<ResolvedBinding> {
        let symbol = self.resolve_symbol(name)?;
        (symbol.span.end > symbol.span.start).then(|| ResolvedBinding {
            name: name.to_string(),
            span: symbol.span,
            kind: symbol.kind,
        })
    }

    fn annotate_foreign_global_expr(
        &mut self,
        expr: &Expr,
        eval: &ExprEval,
        reachable: bool,
        resolved_global: String,
    ) {
        let resolved_binding = self.resolved_binding_for_name(&resolved_global);
        self.annotated_exprs.push(ExprAnnotation {
            span: expr.span,
            binding_span: binding_occurrence_span(expr, resolved_binding.as_ref()),
            ty: eval.ty.clone(),
            const_value: None,
            reachable,
            resolved_fn: None,
            resolved_global: Some(resolved_global),
            resolved_binding,
            auto_deref: false,
            c_abi_function: false,
            inferred_type_args: Vec::new(),
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
        let ty = eval.ty.clone();
        let resolved_binding = resolved_fn
            .as_deref()
            .and_then(|name| self.resolved_binding_for_name(name))
            .or_else(|| match &expr.node {
                ExprKind::Ident(name) => self.resolved_binding_for_name(name),
                _ => None,
            });
        let foldable = self
            .can_elide_expression(expr)
            .then(|| eval.const_value.clone())
            .flatten();
        self.annotated_exprs.push(ExprAnnotation {
            span: expr.span,
            binding_span: binding_occurrence_span(expr, resolved_binding.as_ref()),
            ty,
            const_value: foldable.clone(),
            reachable,
            resolved_fn,
            resolved_global: None,
            resolved_binding,
            auto_deref: false,
            c_abi_function: false,
            inferred_type_args: Vec::new(),
        });

        self.mark_auto_deref(expr);

        if let Some(value) = &foldable {
            self.constant_evaluations.push(ConstantEvaluation {
                span: expr.span,
                value: value.clone(),
            });
        }
    }

    /// Known arithmetic reference operands load independently of a symbolic peer.
    fn mark_arithmetic_operand_autoderef(&mut self, expr: &Expr, ty: Option<&TypeKind>) {
        if let Some(ty) = ty {
            let resolved = self.resolve_type_aliases(ty);
            if matches!(resolved, TypeKind::Ref { .. } | TypeKind::MutRef { .. }) {
                let loaded = self.checked_string_operand_type(resolved.clone());
                if Self::is_integer(&loaded)
                    || Self::is_float(&loaded)
                    || matches!(
                        loaded,
                        TypeKind::Str | TypeKind::Bool | TypeKind::RawPtr { .. }
                    )
                    || self.is_checked_owned_string(&loaded)
                    || Self::arithmetic_string_load(&resolved).is_some()
                {
                    self.mark_auto_deref(expr);
                }
            }
        }
    }

    /// Update the most recent annotation for `expr` to enable codegen auto-deref.
    /// The stored type is collapsed to the value type so codegen loads the right size.
    fn mark_auto_deref(&mut self, expr: &Expr) {
        if let Some(index) = self
            .annotated_exprs
            .iter()
            .rposition(|ann| ann.span == expr.span)
        {
            if self.annotated_exprs[index].auto_deref {
                return;
            }
            if let Some(ty) = self.annotated_exprs[index].ty.clone() {
                let resolved = self.resolve_type_aliases(&ty);
                let (ty, load) = if let Some(load) = Self::arithmetic_string_load(&resolved) {
                    (TypeKind::Str, load)
                } else {
                    let load = matches!(resolved, TypeKind::Ref { .. } | TypeKind::MutRef { .. });
                    (self.checked_string_operand_type(resolved), load)
                };
                self.annotated_exprs[index].ty = Some(ty);
                self.annotated_exprs[index].auto_deref = load;
            }
        }
    }

    /// Check type compatibility and mark the expression for auto-deref when a
    /// reference to a value-like type is used where the value type is expected.
    /// `for value : array` over a prelude `Array[T]` calls `len` with `get`
    /// (copyable elements) or `remove` (owning elements, moved out one by
    /// one). A borrowed array cannot give up owning elements.
    fn record_array_loop(&mut self, iterable: &Expr, iterable_ty: Option<&TypeKind>, element: &TypeKind) {
        let borrowed = matches!(
            iterable_ty.map(|ty| self.resolve_type_aliases(ty)),
            Some(TypeKind::Ref { .. } | TypeKind::MutRef { .. })
        );
        let owning = self.bc_is_move_type(&self.resolve_type_aliases(element));
        if borrowed && owning {
            self.push_error(
                iterable.span,
                "S10",
                format!(
                    "cannot iterate over borrowed `Array[{element}]`: its elements own resources; iterate the array by value (consuming it) or by index"
                ),
            );
        }
        let from = self
            .current_function
            .last()
            .cloned()
            .unwrap_or_else(|| "__program__".to_string());
        let arguments = vec![element.clone()];
        for method in ["len", "get", "remove"] {
            let generic = format!("Array.{method}");
            let target = mangle_monomorphized(&generic, &arguments);
            let generic_params = self.generic_params_for_function(&generic);
            self.record_specialization_layout(&generic, &generic_params, &arguments);
            if !self.has_monomorphization(&generic, &arguments, &target) {
                self.monomorphizations.push(MonomorphizationInfo {
                    fn_name: generic,
                    type_args: arguments.clone(),
                    mangled_name: target.clone(),
                });
            }
            self.add_dependency_edge(DependencyKind::Call, &from, &target);
        }
    }

    /// Keep the prelude panic path reachable from the current function for
    /// operations that may panic implicitly (checked integer arithmetic).
    pub(super) fn require_language_panic(&mut self) {
        let Some(panic) = self.runtime_panic_name.clone() else {
            // Without a selected runtime panic, language panics trap.
            return;
        };
        let from = self
            .current_function
            .last()
            .cloned()
            .unwrap_or_else(|| "__program__".to_string());
        self.add_dependency_edge(DependencyKind::Call, &from, &panic);
    }

    /// Trait bounds visible in the current function: its own and, in an impl
    /// method, those of the generic type it belongs to.
    fn current_generic_bounds(&self) -> Vec<(String, String)> {
        let Some(function) = self.current_function.last() else {
            return Vec::new();
        };
        let mut bounds = self.generic_bounds.get(function).cloned().unwrap_or_default();
        if let Some((owner, _)) = function.rsplit_once('.')
            && let Some(owner_bounds) = self
                .generic_bounds
                .get(owner)
                .or_else(|| self.generic_bounds.get(owner.rsplit('.').next().unwrap_or(owner)))
        {
            bounds.extend(owner_bounds.iter().cloned());
        }
        bounds
    }

    /// `x.method()` where `x: T` and `T` has trait bounds: the method comes
    /// from a bound trait (its return type), or it is an error. `None` when
    /// the receiver is not a bounded generic parameter.
    fn bounded_generic_method(
        &mut self,
        receiver: Option<&TypeKind>,
        method: &str,
        span: Span,
    ) -> Option<Option<TypeKind>> {
        let receiver = receiver.map(|ty| match self.resolve_type_aliases(ty) {
            TypeKind::Ref { inner } | TypeKind::MutRef { inner } => inner.node.clone(),
            other => other,
        })?;
        let TypeKind::Named { name: param, type_args } = &receiver else {
            return None;
        };
        if !type_args.is_empty()
            || !self.current_generic_params.iter().any(|scope| scope.contains(param))
        {
            return None;
        }
        let bounds: Vec<String> = self
            .current_generic_bounds()
            .into_iter()
            .filter(|(bounded, _)| bounded == param)
            .map(|(_, bound)| bound)
            .collect();
        if bounds.is_empty() {
            self.push_error(
                span,
                "S04",
                format!(
                    "no method `{method}` on `{param}`: add a trait bound such as `{param}: Trait` that declares it"
                ),
            );
            return Some(None);
        }
        for bound in &bounds {
            if let Some(signature) = self
                .bound_trait_signatures(bound)
                .and_then(|methods| methods.get(method).cloned())
            {
                let result = signature.return_ty.clone();
                let by_value = !signature.has_explicit_receiver
                    || !signature.params.first().is_some_and(|ty| {
                        matches!(
                            self.resolve_type_aliases(ty),
                            TypeKind::Ref { .. } | TypeKind::MutRef { .. }
                        )
                    });
                if by_value {
                    self.consuming_bound_calls.insert((span.start, span.end));
                }
                self.bounded_trait_calls.insert((span.start, span.end));
                if let Some(function) = self.current_function.last().cloned() {
                    self.bound_method_calls
                        .entry(function)
                        .or_default()
                        .push((param.clone(), method.to_string()));
                }
                return Some(Some(result));
            }
        }
        self.push_error(
            span,
            "S04",
            format!(
                "no method `{method}` on `{param}`: its bounds ({}) do not declare it",
                bounds.join(" + ")
            ),
        );
        Some(None)
    }

    /// Every instantiation of a bounded generic function or type uses types
    /// that implement the bounds, and keeps the concrete trait methods its
    /// body calls (dependency edges). Runs before the specialization closure
    /// (diagnostics, used methods) and again after it, where it returns the
    /// specializations of generic impl methods that bounded calls need
    /// (`Holder.hash_code<P>` for `T = Holder[P]`) so the closure can run again.
    pub(super) fn check_generic_bound_instantiations(&mut self) -> bool {
        let known: HashSet<String> = self
            .monomorphizations
            .iter()
            .map(|mono| mono.mangled_name.clone())
            .collect();
        let mut added = Vec::new();
        for mono in self.monomorphizations.clone() {
            // Arguments naming a generic body's own parameters are checked
            // against that body's bounds; a global type of the same spelling
            // (a user `struct W`) must not be mistaken for them.
            if self.symbolic_specialization_names.contains(&mono.mangled_name) {
                continue;
            }
            let first_visit = self.bound_checked_monos.insert(mono.mangled_name.clone());
            let mut bounds = self.generic_bounds.get(&mono.fn_name).cloned().unwrap_or_default();
            if let Some((owner, _)) = mono.fn_name.rsplit_once('.')
                && let Some(owner_bounds) = self.generic_bounds.get(owner)
            {
                bounds.extend(owner_bounds.iter().cloned());
            }
            if bounds.is_empty() {
                continue;
            }
            let params = self.generic_params_for_function(&mono.fn_name);
            let concrete = |param: &str| {
                params
                    .iter()
                    .position(|candidate| candidate == param)
                    .and_then(|index| mono.type_args.get(index))
                    .cloned()
            };
            if first_visit {
                let span = self
                    .resolve_symbol(&mono.fn_name)
                    .map(|symbol| symbol.span)
                    .unwrap_or(Span::new(0, 0, 0, 0));
                let substitution: std::collections::HashMap<String, TypeKind> = params
                    .iter()
                    .cloned()
                    .zip(mono.type_args.iter().cloned())
                    .collect();
                for (param, bound) in &bounds {
                    if let Some(argument) = concrete(param) {
                        let bound = self.instantiate_bound(bound, &substitution);
                        self.check_bound(&argument, param, &bound, &mono.fn_name, span);
                    }
                }
            }
            for (param, method) in self.bound_method_calls.get(&mono.fn_name).cloned().unwrap_or_default() {
                let Some(argument) = concrete(&param) else {
                    continue;
                };
                let argument = self.resolve_type_aliases(&argument);
                if self.is_unresolved_type_parameter(&argument) {
                    continue;
                }
                let base = format!("{}.{method}", super::declare::type_kind_base_name(&argument));
                let target = match &argument {
                    TypeKind::Named { type_args, .. }
                        if !type_args.is_empty()
                            && !self.generic_params_for_function(&base).is_empty() =>
                    {
                        let type_args: Vec<TypeKind> =
                            type_args.iter().map(|arg| arg.node.clone()).collect();
                        let mangled = mangle_monomorphized(&base, &type_args);
                        if !known.contains(&mangled)
                            && !added.iter().any(|mono: &MonomorphizationInfo| mono.mangled_name == mangled)
                        {
                            added.push(MonomorphizationInfo {
                                fn_name: base.clone(),
                                type_args,
                                mangled_name: mangled.clone(),
                            });
                        }
                        mangled
                    }
                    _ => base.clone(),
                };
                self.resolve_for_read(&base);
                self.add_dependency_edge(DependencyKind::Call, &mono.mangled_name, &target);
            }
        }
        let grew = !added.is_empty();
        self.monomorphizations.extend(added);
        grew
    }

    /// A type parameter not yet substituted (a caller's own `U`): its bounds
    /// are checked where the caller is instantiated.
    fn is_unresolved_type_parameter(&self, ty: &TypeKind) -> bool {
        matches!(ty, TypeKind::Named { name, type_args }
            if type_args.is_empty()
                && !self.resolve_symbol(name).is_some_and(|symbol| symbol.kind == SymbolKind::TypeName))
    }

    /// The method signatures of a bound's trait, named plainly or through
    /// its module (`marks.Tag`).
    /// A bound's trait name and type arguments: `AsRef[String]` is
    /// (`AsRef`, [String]).
    fn bound_parts(&self, bound: &str) -> (String, Vec<TypeKind>) {
        let Some(open) = bound.find('[') else {
            return (bound.to_string(), Vec::new());
        };
        let tokens = crate::lexer::Lexer::new(bound).tokenize();
        let parsed = crate::parser::Parser::new(tokens).parse_type_text();
        let arguments = match parsed {
            Some(TypeKind::Named { type_args, .. }) => type_args.into_iter().map(|argument| argument.node).collect(),
            _ => Vec::new(),
        };
        (bound[..open].to_string(), arguments)
    }

    /// The method signatures of a bound's trait with the bound's type
    /// arguments substituted for the trait's parameters.
    fn bound_trait_signatures(
        &self,
        bound: &str,
    ) -> Option<HashMap<String, super::TraitMethodSignature>> {
        let (base, arguments) = self.bound_parts(bound);
        let methods = self
            .trait_method_signatures
            .get(&base)
            .or_else(|| self.trait_method_signatures.get(base.rsplit('.').next()?))?;
        if arguments.is_empty() {
            return Some(methods.clone());
        }
        let params = self
            .resolve_symbol(&base)
            .map(|symbol| symbol.generic_params.clone())
            .unwrap_or_default();
        let substitution: std::collections::HashMap<String, TypeKind> =
            params.into_iter().zip(arguments).collect();
        Some(
            methods
                .iter()
                .map(|(name, signature)| {
                    let mut signature = signature.clone();
                    signature.params = signature
                        .params
                        .iter()
                        .map(|param| substitute_type_kind(param, &substitution))
                        .collect();
                    signature.return_ty = substitute_type_kind(&signature.return_ty, &substitution);
                    (name.clone(), signature)
                })
                .collect(),
        )
    }

    /// Whether `argument` implements the instantiated generic bound
    /// `Trait[Args]`: some impl's target unifies with `argument` and its trait
    /// arguments, under that unification, equal the bound's.
    fn implements_generic_bound(&self, argument: &TypeKind, bound: &str) -> bool {
        let (base, bound_args) = self.bound_parts(bound);
        let base = base.rsplit('.').next().unwrap_or(&base).to_string();
        let type_name = super::declare::type_kind_base_name(argument);
        let rendered: Vec<String> = bound_args
            .iter()
            .map(|argument| self.resolve_type_aliases(argument).to_string())
            .collect();
        self.generic_trait_impls.iter().any(|(implemented, for_ty, trait_ty)| {
            if *implemented != type_name {
                return false;
            }
            let TypeKind::Named { name, type_args } = trait_ty else {
                return false;
            };
            if name.rsplit('.').next() != Some(base.as_str()) || type_args.len() != rendered.len() {
                return false;
            }
            // The impl's own parameters: bare names that are not types.
            let mut params = Vec::new();
            collect_bare_names(for_ty, &mut params);
            params.retain(|name| {
                !self.resolve_symbol(name).is_some_and(|symbol| symbol.kind == SymbolKind::TypeName)
            });
            let mut substitution = std::collections::HashMap::new();
            infer_type_subst(for_ty, argument, &params, &mut substitution);
            type_args.iter().zip(&rendered).all(|(implemented, wanted)| {
                let concrete = substitute_type_kind(&implemented.node, &substitution);
                self.resolve_type_aliases(&concrete).to_string() == *wanted
            })
        })
    }

    /// `bound` with the enclosing generic's parameters replaced by their
    /// concrete arguments (`AsRef[Array[T]]` with T = i64).
    pub(super) fn instantiate_bound(&self, bound: &str, substitution: &std::collections::HashMap<String, TypeKind>) -> String {
        let (base, arguments) = self.bound_parts(bound);
        if arguments.is_empty() {
            return bound.to_string();
        }
        let rendered: Vec<String> = arguments
            .iter()
            .map(|argument| substitute_type_kind(argument, substitution).to_string())
            .collect();
        format!("{base}[{}]", rendered.join(", "))
    }

    /// Whether values of `ty` are duplicated rather than moved (`Copy`).
    pub(super) fn is_copy_type(&self, ty: &TypeKind) -> bool {
        match self.resolve_type_aliases(ty) {
            TypeKind::Bool
            | TypeKind::Int8
            | TypeKind::Int16
            | TypeKind::Int32
            | TypeKind::Int64
            | TypeKind::Uint8
            | TypeKind::Uint16
            | TypeKind::Uint32
            | TypeKind::Uint64
            | TypeKind::Isize
            | TypeKind::Usize
            | TypeKind::Float16
            | TypeKind::Float32
            | TypeKind::Float64
            | TypeKind::Str
            | TypeKind::Bytes
            | TypeKind::RawPtr { .. }
            | TypeKind::CFn { .. }
            | TypeKind::Ref { .. } => true,
            TypeKind::Named { name, type_args } if type_args.is_empty() => {
                // A generic parameter is Copy only under a `Copy` bound.
                if self.current_generic_params.iter().any(|scope| scope.contains(&name)) {
                    return self.current_generic_bounds().iter().any(|(param, bound)| {
                        *param == name && bound.rsplit('.').next() == Some("Copy")
                    });
                }
                self.trait_impls.get(&name).is_some_and(|traits| traits.contains("Copy"))
            }
            // `P[T]` with `impl Copy for P[T]` is Copy when its arguments are.
            TypeKind::Named { name, type_args } => {
                self.trait_impls.get(&name).is_some_and(|traits| traits.contains("Copy"))
                    && type_args.iter().all(|argument| self.is_copy_type(&argument.node))
            }
            _ => false,
        }
    }

    fn check_bound(&mut self, argument: &TypeKind, param: &str, bound: &str, owner: &str, span: Span) {
        let argument = self.resolve_type_aliases(argument);
        if self.is_unresolved_type_parameter(&argument) {
            return;
        }
        if bound.contains('[') {
            if !self.implements_generic_bound(&argument, bound) {
                let type_name = super::declare::type_kind_base_name(&argument);
                self.push_error(
                    span,
                    "S04",
                    format!("`{type_name}` does not implement `{bound}`, required by `{param}` of `{owner}`"),
                );
            }
            return;
        }
        let trait_name = bound.rsplit('.').next().unwrap_or(bound);
        let type_name = super::declare::type_kind_base_name(&argument);
        if trait_name == "Copy" {
            if !self.is_copy_type(&argument) {
                self.push_error(
                    span,
                    "S04",
                    format!("`{argument}` is not `Copy`, required by `{param}` of `{owner}`"),
                );
            }
            return;
        }
        if !self
            .trait_impls
            .get(&type_name)
            .is_some_and(|traits| traits.contains(trait_name) || traits.contains(bound))
        {
            self.push_error(
                span,
                "S04",
                format!("`{type_name}` does not implement `{bound}`, required by `{param}` of `{owner}`"),
            );
        }
    }

    /// Bounds name declared traits, and a bounded type used anywhere (not
    /// only through its methods) satisfies them.
    pub(super) fn check_bounded_declarations_and_types(&mut self) {
        let mut declared: Vec<(String, Vec<(String, String)>)> = self
            .generic_bounds
            .iter()
            .map(|(owner, bounds)| (owner.clone(), bounds.clone()))
            .collect();
        declared.sort();
        for (owner, bounds) in &declared {
            let span = self
                .resolve_symbol(owner)
                .map(|symbol| symbol.span)
                .unwrap_or(Span::new(0, 0, 0, 0));
            for (param, bound) in bounds {
                let base = bound.split('[').next().unwrap_or(bound);
                let trait_name = base.rsplit('.').next().unwrap_or(base);
                // `Copy` is a compiler-known marker trait (В23А).
                if trait_name != "Copy"
                    && !self.trait_method_slots.contains_key(trait_name)
                    && !self.trait_method_slots.contains_key(bound.as_str())
                    && self.bound_trait_signatures(bound).is_none()
                {
                    self.push_error(
                        span,
                        "S04",
                        format!("bound `{param}: {bound}` of `{owner}` does not name a trait"),
                    );
                }
            }
        }
        let mut seen = HashSet::new();
        let mut pending: Vec<(TypeKind, Span)> = self
            .annotated_exprs
            .iter()
            .filter_map(|annotation| annotation.ty.clone().map(|ty| (ty, annotation.span)))
            .collect();
        while let Some((ty, span)) = pending.pop() {
            match ty {
                TypeKind::Named { name, type_args } if !type_args.is_empty() => {
                    for arg in &type_args {
                        pending.push((arg.node.clone(), span));
                    }
                    let key = format!("{name}{type_args:?}");
                    if !seen.insert(key) {
                        continue;
                    }
                    let Some(bounds) = self.generic_bounds.get(&name).cloned() else {
                        continue;
                    };
                    let params = self.declared_type_parameters(&name).cloned().unwrap_or_default();
                    let substitution: std::collections::HashMap<String, TypeKind> = params
                        .iter()
                        .cloned()
                        .zip(type_args.iter().map(|argument| argument.node.clone()))
                        .collect();
                    let binders = self
                        .annotation_binders
                        .get(&(span.start, span.end))
                        .cloned()
                        .unwrap_or_default();
                    for (param, bound) in &bounds {
                        if let Some(index) = params.iter().position(|candidate| candidate == param)
                            && let Some(argument) = type_args.get(index)
                            && !type_mentions_params(&argument.node, &binders)
                        {
                            let bound = self.instantiate_bound(bound, &substitution);
                            self.check_bound(&argument.node, param, &bound, &name, span);
                        }
                    }
                }
                TypeKind::Ref { inner } | TypeKind::MutRef { inner } => {
                    pending.push((inner.node, span));
                }
                _ => {}
            }
        }
    }

    /// A generic value coerced to `dyn Trait` dispatches to its specialized
    /// methods (`Holder.hash_code<P>`): request them and keep them reachable.
    fn request_dyn_vtable_specializations(&mut self, declared: &TypeKind, value: &TypeKind) {
        let TypeKind::Dyn { trait_name } = self.resolve_type_aliases(declared) else {
            return;
        };
        let TypeKind::Named { name, type_args } = self.resolve_type_aliases(value) else {
            return;
        };
        if type_args.is_empty() {
            return;
        }
        let type_args: Vec<TypeKind> = type_args.into_iter().map(|arg| arg.node).collect();
        let methods = self.trait_method_slots.get(&trait_name).cloned().unwrap_or_default();
        let from = self
            .current_function
            .last()
            .cloned()
            .unwrap_or_else(|| "__program__".to_string());
        for method in methods {
            let fn_name = format!("{name}.{method}");
            let mangled = mangle_monomorphized(&fn_name, &type_args);
            if !self.monomorphizations.iter().any(|mono| mono.mangled_name == mangled) {
                let params = self.generic_params_for_function(&fn_name);
                self.record_specialization_layout(&fn_name, &params, &type_args);
                self.monomorphizations.push(MonomorphizationInfo {
                    fn_name: fn_name.clone(),
                    type_args: type_args.clone(),
                    mangled_name: mangled.clone(),
                });
            }
            self.resolve_for_read(&fn_name);
            self.add_dependency_edge(DependencyKind::Call, &from, &mangled);
        }
    }

    pub(super) fn unit_variant_type(&self, object: &Expr, variant: &str) -> Option<TypeKind> {
        let ExprKind::Ident(enum_name) = &object.node else {
            return None;
        };
        let symbol = self.resolve_symbol(enum_name)?;
        if !matches!(symbol.kind, SymbolKind::TypeName) || !symbol.generic_params.is_empty() {
            return None;
        }
        let canonical = self
            .type_name_bindings
            .get(enum_name)
            .map(|binding| binding.name.clone())
            .unwrap_or_else(|| enum_name.clone());
        if self
            .struct_generic_params
            .get(&canonical)
            .is_some_and(|params| !params.is_empty())
        {
            return None;
        }
        let info = self.enums.get(&canonical)?;
        (info.variants.get(variant) == Some(&0)).then(|| TypeKind::Named {
            name: canonical,
            type_args: Vec::new(),
        })
    }

    /// An integer literal (or literal-only expression) must fit the integer
    /// type it initializes: `var y: u8 = 300;` is an error, not a wrap.
    /// `if c { 29 } else { 28 }` and similar: every branch is a literal that
    /// the expected type can hold, so no conversion is implied.
    fn literal_branches_fit(&self, expr: &Expr, expected: &TypeKind) -> bool {
        let fits = |branch: &Expr| {
            if let Some(value) = crate::evaluation::integer_literal_value(branch) {
                Self::integer_range(expected).is_some_and(|(min, max)| (min..=max).contains(&value))
                    || Self::is_float(expected)
            } else {
                Self::is_float(expected) && crate::evaluation::is_float_literal_expression(branch)
            }
        };
        let typed_as_expected = |branch: &Expr| {
            self.annotated_exprs
                .iter()
                .rev()
                .find(|annotation| annotation.span == branch.span)
                .and_then(|annotation| annotation.ty.as_ref())
                .is_some_and(|ty| {
                    std::mem::discriminant(&self.resolve_type_aliases(ty))
                        == std::mem::discriminant(expected)
                })
        };
        match &expr.node {
            ExprKind::Group(inner) => self.literal_branches_fit(inner, expected),
            ExprKind::Match { arms, .. } => arms.iter().all(|arm| {
                fits(&arm.expr)
                    || typed_as_expected(&arm.expr)
                    || matches!(arm.expr.node, ExprKind::Call { .. } | ExprKind::MethodCall { .. })
                        && self
                            .annotated_exprs
                            .iter()
                            .rev()
                            .find(|annotation| annotation.span == arm.expr.span)
                            .is_some_and(|annotation| matches!(annotation.ty, Some(TypeKind::Never)))
                    || self.literal_branches_fit(&arm.expr, expected)
            }),
            _ => false,
        }
    }

    /// Every value of integer type `from` is a value of `to` (`i32` -> `i64`,
    /// `u8` -> `u32`, `u32` -> `i64`): the canonical slot needs no change.
    fn integer_widens(from: &TypeKind, to: &TypeKind) -> bool {
        match (Self::integer_range(from), Self::integer_range(to)) {
            (Some((from_min, from_max)), Some((to_min, to_max))) => {
                to_min <= from_min && from_max <= to_max
            }
            _ => false,
        }
    }

    pub(super) fn integer_range(ty: &TypeKind) -> Option<(i128, i128)> {
        Some(match ty {
            TypeKind::Int8 => (i8::MIN.into(), i8::MAX.into()),
            TypeKind::Int16 => (i16::MIN.into(), i16::MAX.into()),
            TypeKind::Int32 => (i32::MIN.into(), i32::MAX.into()),
            TypeKind::Int64 | TypeKind::Isize => (i64::MIN.into(), i64::MAX.into()),
            TypeKind::Uint8 => (0, u8::MAX.into()),
            TypeKind::Uint16 => (0, u16::MAX.into()),
            TypeKind::Uint32 => (0, u32::MAX.into()),
            TypeKind::Uint64 | TypeKind::Usize => (0, u64::MAX.into()),
            _ => return None,
        })
    }

    /// The exact value of a folded constant of integer type `ty`.
    fn exact_integer_constant(value: &ConstValue, ty: &TypeKind) -> Option<i128> {
        let ConstValue::Int(raw) = value else {
            return None;
        };
        let unsigned = matches!(
            ty,
            TypeKind::Uint8 | TypeKind::Uint16 | TypeKind::Uint32 | TypeKind::Uint64 | TypeKind::Usize
        );
        Some(if unsigned { i128::from(*raw as u64) } else { i128::from(*raw) })
    }

    /// Constant `+ - *` (and unary minus, `MIN / -1`) of a typed operand must
    /// not overflow: the same operation panics at run time (checked integer
    /// arithmetic), so a constant one is a compile error.
    fn check_constant_overflow(
        &mut self,
        span: Span,
        op: &str,
        exact: Option<i128>,
        ty: Option<&TypeKind>,
    ) {
        let Some(ty) = ty.map(|ty| self.resolve_type_aliases(ty)) else {
            return;
        };
        let (Some(exact), Some((min, max))) = (exact, Self::integer_range(&ty)) else {
            return;
        };
        if exact < min || exact > max {
            self.push_error(
                span,
                "S06",
                format!(
                    "this arithmetic operation will overflow: {op} gives {exact}, outside `{ty}` ({min}..={max})"
                ),
            );
        }
    }

    fn check_integer_literal_range(&mut self, expr: &Expr, value: i128, ty: &TypeKind) {
        let Some((min, max)) = Self::integer_range(ty) else {
            return;
        };
        if value < min || value > max {
            self.push_error(
                expr.span,
                "S06",
                format!(
                    "integer literal {value} does not fit in `{ty}` (range {min}..={max}); use a value in range or an explicit `as` cast"
                ),
            );
        }
    }

    fn check_expr_compat(&mut self, expr: &Expr, expected: &TypeKind, actual: &TypeKind) -> bool {
        let expected_resolved = self.resolve_type_aliases(expected);
        let actual_resolved = self.resolve_type_aliases(actual);
        if Self::is_integer(&expected_resolved)
            && Self::is_integer(&actual_resolved)
            && let Some(value) = crate::evaluation::integer_literal_value(expr)
        {
            self.check_integer_literal_range(expr, value, &expected_resolved);
            return true;
        }
        // Numbers and bools convert only with `as`: no implicit change of
        // width, signedness, family or truthiness (the slot is not converted).
        let numeric_kind = |ty: &TypeKind| {
            if Self::is_integer(ty) || Self::is_float(ty) || matches!(ty, TypeKind::Bool) {
                Some(std::mem::discriminant(ty))
            } else {
                None
            }
        };
        if let (Some(expected_kind), Some(actual_kind)) =
            (numeric_kind(&expected_resolved), numeric_kind(&actual_resolved))
            && expected_kind != actual_kind
            && !(Self::is_float(&expected_resolved)
                && (crate::evaluation::integer_literal_value(expr).is_some()
                    || crate::evaluation::is_float_literal_expression(expr)))
            && !self.literal_branches_fit(expr, &expected_resolved)
            && !Self::integer_widens(&actual_resolved, &expected_resolved)
            && !(Self::is_float(&expected_resolved) && Self::is_float(&actual_resolved)
                && !matches!(expected_resolved, TypeKind::Float32 | TypeKind::Float16))
        {
            let hint = if matches!(expected_resolved, TypeKind::Bool) {
                "compare explicitly, for example `value != 0`".to_string()
            } else if matches!(actual_resolved, TypeKind::Bool) {
                format!("convert explicitly with `if flag {{ 1 }} else {{ 0 }}`")
            } else {
                format!("convert explicitly with `as {expected_resolved}`")
            };
            self.push_error(
                expr.span,
                "S01",
                format!("expected `{expected_resolved}`, found `{actual_resolved}`; {hint}"),
            );
            return true;
        }
        if let (
            TypeKind::RawPtr {
                inner: expected_inner,
            },
            TypeKind::Ref {
                inner: actual_inner,
            },
        ) = (&expected_resolved, &actual_resolved)
        {
            return address_of_ident(expr).is_some()
                && self.types_have_same_runtime_shape(&expected_inner.node, &actual_inner.node);
        }
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
                    .all(|(expected, actual)| {
                        self.types_have_same_runtime_shape(&expected.node, &actual.node)
                    })
                && self.types_have_same_runtime_shape(&expected_return.node, &actual_return.node);
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
        if let TypeKind::Ref { inner } | TypeKind::MutRef { inner } = &actual_resolved
            && !matches!(
                expected_resolved,
                TypeKind::Ref { .. } | TypeKind::RawPtr { .. }
            )
            && Self::is_autoderef_value(&expected_resolved)
            && self.types_have_same_runtime_shape(&expected_resolved, &inner.node)
        {
            self.mark_auto_deref(expr);
            return true;
        }
        match (&expected_resolved, &actual_resolved) {
            (
                TypeKind::Ref {
                    inner: expected_inner,
                },
                TypeKind::Ref {
                    inner: actual_inner,
                }
                | TypeKind::MutRef {
                    inner: actual_inner,
                },
            ) => {
                return self
                    .types_have_same_runtime_shape(&expected_inner.node, &actual_inner.node);
            }
            (
                TypeKind::MutRef {
                    inner: expected_inner,
                },
                TypeKind::MutRef {
                    inner: actual_inner,
                },
            ) => {
                return self
                    .types_have_same_runtime_shape(&expected_inner.node, &actual_inner.node);
            }
            _ => {}
        }
        if matches!(
            expected_resolved,
            TypeKind::Ref { .. } | TypeKind::MutRef { .. }
        ) && !matches!(
            actual_resolved,
            TypeKind::Ref { .. } | TypeKind::MutRef { .. } | TypeKind::Str
        ) {
            return false;
        }
        self.types_compatible(&expected_resolved, &actual_resolved)
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
        if matches!(l, TypeKind::Ref { .. } | TypeKind::MutRef { .. })
            && Self::is_autoderef_value(&dr)
            && self.types_compatible(&dr, &dl)
        {
            self.mark_auto_deref(left);
        }
        if matches!(r, TypeKind::Ref { .. } | TypeKind::MutRef { .. })
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
        if let PatternKind::Literal(literal) = &pattern.node {
            let valid = scrutinee_ty.as_ref().is_some_and(|ty| {
                matches!(ty, TypeKind::Error)
                    || match literal {
                        LiteralValue::Int(_) | LiteralValue::Bool(_) => {
                            Self::is_integer(ty) || matches!(ty, TypeKind::Bool)
                        }
                        LiteralValue::Float(_) => Self::is_float(ty),
                        LiteralValue::Str(_) => is_string_view_type(ty),
                    }
            });
            if !valid {
                self.push_error(
                    pattern.span,
                    "S01",
                    "literal pattern is incompatible with the scrutinee type".to_string(),
                );
            }
        }
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

                // Lowering consumes this checked discriminant instead of searching
                // unrelated enums with the same variant spelling.
                let target = match scrutinee_ty {
                    Some(TypeKind::Named { name, .. }) => Some(name.as_str()),
                    _ => enum_name.as_deref(),
                };
                let tags: std::collections::BTreeSet<usize> = if let Some(target) = target {
                    self.enums
                        .get(target)
                        .and_then(|info| info.order.iter().position(|name| name == variant))
                        .into_iter()
                        .collect()
                } else {
                    self.enums
                        .values()
                        .filter_map(|info| info.order.iter().position(|name| name == variant))
                        .collect()
                };
                if tags.len() == 1 {
                    self.match_pattern_tags.insert(
                        (pattern.span.start, pattern.span.end),
                        *tags.first().unwrap(),
                    );
                } else if scrutinee_ty.is_none() {
                    self.push_error(pattern.span, "S14", format!("variant `{variant}` has no unique discriminant; annotate the scrutinee enum type"));
                }

                let payloads = self.match_pattern_payload_types(scrutinee_ty.as_ref(), variant);
                for (index, sub_pattern) in sub_patterns.iter().enumerate() {
                    if matches!(
                        sub_pattern.node,
                        PatternKind::Wildcard | PatternKind::Bind(_)
                    ) {
                        continue;
                    }
                    if let Some(payload) =
                        payloads.as_ref().and_then(|payloads| payloads.get(index))
                    {
                        let payload = self.resolve_type_aliases(payload);
                        self.validate_match_pattern(sub_pattern, &Some(payload));
                    } else {
                        self.push_error(sub_pattern.span, "S14", "nested refutable pattern requires a proven enum payload type; annotate the scrutinee".to_string());
                    }
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
            PatternKind::Struct { name, fields, rest } => {
                let canonical = self
                    .type_name_bindings
                    .get(name)
                    .map(|binding| binding.name.clone())
                    .unwrap_or_else(|| name.clone());
                match scrutinee_ty {
                    Some(owner @ TypeKind::Named { name: owner_name, .. })
                        if *owner_name == canonical && self.struct_defs.contains_key(owner_name) =>
                    {
                        if self.repr_c_structs.contains(owner_name) {
                            self.push_error(pattern.span, "S14", format!("destructuring the C-layout struct `{name}` is not supported; read its fields instead"));
                        }
                        let declared: Vec<String> = self.struct_defs[owner_name]
                            .iter()
                            .map(|(field, _)| field.clone())
                            .collect();
                        for (field, sub_pattern) in fields {
                            let Some(field_ty) = self.struct_pattern_field_type(owner, field) else {
                                self.push_error(sub_pattern.span, "S04", format!("struct `{name}` has no field `{field}`"));
                                continue;
                            };
                            if !matches!(sub_pattern.node, PatternKind::Bind(_) | PatternKind::Wildcard) {
                                self.validate_match_pattern(sub_pattern, &Some(field_ty));
                            }
                        }
                        if !rest {
                            let missing: Vec<&String> = declared
                                .iter()
                                .filter(|field| !fields.iter().any(|(written, _)| written == *field))
                                .collect();
                            if !missing.is_empty() {
                                self.push_error(pattern.span, "S09", format!(
                                    "struct pattern `{name}` does not mention field(s) {}; list them or add `..`",
                                    missing.iter().map(|field| format!("`{field}`")).collect::<Vec<_>>().join(", ")
                                ));
                            }
                        }
                        // Lowering reads field offsets from the struct type.
                        self.annotated_exprs.push(ExprAnnotation {
                            span: pattern.span,
                            binding_span: None,
                            ty: Some(owner.clone()),
                            const_value: None,
                            reachable: true,
                            resolved_fn: None,
                            resolved_global: None,
                            resolved_binding: None,
                            auto_deref: false,
                            c_abi_function: false,
                            inferred_type_args: Vec::new(),
                        });
                    }
                    Some(other) => {
                        self.push_error(pattern.span, "S09", format!("struct pattern `{name}` does not match the scrutinee type `{other}`"));
                    }
                    None => {
                        self.push_error(pattern.span, "S14", format!("struct pattern `{name}` requires a scrutinee of a known struct type"));
                    }
                }
                let bindings = crate::parser::ast::collect_pattern_bindings(std::slice::from_ref(pattern));
                (
                    MatchArmInfo {
                        span: pattern.span,
                        // Like literal arms, refutable fields are checked at run
                        // time; an unmatched value traps.
                        kind: MatchArmKindInfo::Wildcard,
                        has_guard: false,
                    },
                    bindings,
                )
            }
        }
    }

    /// Binding types of a variant pattern matched against `ty`, through nested
    /// variant and struct patterns (`Some(Status.Code(code))`).
    fn variant_pattern_binding_types(
        &self,
        pattern: &Pattern,
        ty: &TypeKind,
        binding_types: &mut std::collections::HashMap<String, TypeKind>,
    ) {
        let PatternKind::Variant {
            variant,
            sub_patterns,
            ..
        } = &pattern.node
        else {
            return;
        };
        let ty = self.resolve_type_aliases(ty);
        let Some(payloads) = self.match_pattern_payload_types(Some(&ty), variant) else {
            return;
        };
        for (sub, payload) in sub_patterns.iter().zip(payloads) {
            match &sub.node {
                PatternKind::Bind(name) => {
                    binding_types.insert(name.clone(), payload);
                }
                PatternKind::Struct { .. } => {
                    let payload = self.resolve_type_aliases(&payload);
                    self.struct_pattern_binding_types(sub, &payload, binding_types);
                }
                PatternKind::Variant { .. } => {
                    self.variant_pattern_binding_types(sub, &payload, binding_types);
                }
                _ => {}
            }
        }
    }

    /// Resolve payload representation from the actual scrutinee, never merely
    /// from a pattern's spelling. Qualified constructors can still be untyped.
    fn match_pattern_payload_types(
        &self,
        scrutinee_ty: Option<&TypeKind>,
        variant: &str,
    ) -> Option<Vec<TypeKind>> {
        let TypeKind::Named { name, type_args } = scrutinee_ty? else {
            return None;
        };
        let info = self.enums.get(name)?;
        let Some(fields) = info.variant_fields.get(variant) else {
            // These are already caller actuals. Substituting the builtin's
            // binder again could capture a caller type also named T or E.
            return match (name.as_str(), variant) {
                ("Option", "Some") | ("Result", "Ok") => {
                    type_args.first().map(|ty| vec![ty.node.clone()])
                }
                ("Result", "Err") => type_args.get(1).map(|ty| vec![ty.node.clone()]),
                _ => None,
            };
        };
        let subst: std::collections::HashMap<_, _> = self
            .declared_type_parameters(name)
            .into_iter()
            .flatten()
            .cloned()
            .zip(type_args.iter().map(|ty| ty.node.clone()))
            .collect();
        Some(
            fields
                .iter()
                .map(|ty| substitute_type_kind(ty, &subst))
                .collect(),
        )
    }

    /// A borrowed enum can expose its discriminant, but no payload storage. In
    /// particular, a direct wildcard is safe while bindings, literals, and
    /// nested patterns could observe or materialize a payload.
    fn is_borrowed_enum_discriminant_pattern(pattern: &Pattern) -> bool {
        match &pattern.node {
            PatternKind::Wildcard => true,
            PatternKind::Variant { sub_patterns, .. } => sub_patterns
                .iter()
                .all(|sub_pattern| matches!(sub_pattern.node, PatternKind::Wildcard)),
            PatternKind::Bind(_) | PatternKind::Literal(_) | PatternKind::Struct { .. } => false,
        }
    }

    /// The type of `field` in the struct type `owner`, with the owner's
    /// generic arguments substituted.
    fn struct_pattern_field_type(&self, owner: &TypeKind, field: &str) -> Option<TypeKind> {
        let TypeKind::Named { name, type_args } = owner else {
            return None;
        };
        let raw = self
            .struct_defs
            .get(name)?
            .iter()
            .find(|(field_name, _)| field_name == field)
            .map(|(_, ty)| ty.clone())?;
        let parameters = self
            .struct_generic_params
            .get(name)
            .cloned()
            .unwrap_or_default();
        let subst: std::collections::HashMap<String, TypeKind> = parameters
            .iter()
            .zip(type_args.iter())
            .map(|(parameter, argument)| (parameter.clone(), argument.node.clone()))
            .collect();
        Some(self.resolve_type_aliases(&substitute_type_kind(&raw, &subst)))
    }

    /// Binding types of a struct pattern's fields, recursively.
    fn struct_pattern_binding_types(
        &self,
        pattern: &Pattern,
        ty: &TypeKind,
        out: &mut std::collections::HashMap<String, TypeKind>,
    ) {
        match &pattern.node {
            PatternKind::Bind(name) => {
                out.insert(name.clone(), ty.clone());
            }
            PatternKind::Struct { fields, .. } => {
                for (field, sub_pattern) in fields {
                    if let Some(field_ty) = self.struct_pattern_field_type(ty, field) {
                        self.struct_pattern_binding_types(sub_pattern, &field_ty, out);
                    }
                }
            }
            _ => {}
        }
    }

    /// Assignment validation must not execute target operands a second time.
    fn assignment_operand_type(&self, expr: &Expr) -> Option<TypeKind> {
        self.annotated_exprs
            .iter()
            .rev()
            .find(|annotation| annotation.span == expr.span)
            .and_then(|annotation| annotation.ty.clone())
    }

    /// The binding whose own storage a field place `a.b.c` lies in: reached
    /// through field accesses (and grouping) only, never through a value of
    /// reference or pointer type (a write there goes through the pointer).
    fn field_place_root<'e>(&self, expr: &'e Expr) -> Option<&'e str> {
        if self.assignment_operand_type(expr).is_some_and(|ty| {
            matches!(
                self.resolve_type_aliases(&ty),
                TypeKind::Ref { .. } | TypeKind::MutRef { .. } | TypeKind::RawPtr { .. }
            )
        }) {
            return None;
        }
        match &expr.node {
            ExprKind::Ident(name) => Some(name.as_str()),
            ExprKind::Group(inner) => self.field_place_root(inner),
            ExprKind::Field { object, .. } => self.field_place_root(object),
            _ => None,
        }
    }

    pub(super) fn analyze_assign_target(&mut self, target: &Expr) {
        if self.lvalue_contains_shared_deref(target) {
            self.push_error(
                target.span,
                "S07",
                "cannot assign through a shared reference".to_string(),
            );
            return;
        }
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
                    SymbolKind::Parameter | SymbolKind::Variable { mutable: true } => {
                        if sym.ty.as_ref().is_some_and(|ty| {
                            contains_non_string_reference(&self.resolve_type_aliases(ty))
                        }) {
                            self.push_error(
                                target.span,
                                "S07",
                                format!(
                                    "shared reference `{name}` cannot be rebound; create a new lexical reference instead"
                                ),
                            );
                        }
                    }
                },
            },
            ExprKind::Field { object, name } => {
                let object_ty = self.assignment_operand_type(object);
                // A field of an immutable (`const`) binding is immutable too,
                // as `&T!` method calls on it already are. Paths through a
                // dereference (`(*p).f`) are checked by their pointer instead.
                if let Some(root) = self.field_place_root(object)
                    && let Some(symbol) = self.resolve_symbol(root)
                    && matches!(symbol.kind, SymbolKind::Variable { mutable: false })
                {
                    self.push_error(
                        target.span,
                        "S07",
                        format!("cannot assign to a field of const '{root}'"),
                    );
                }
                // A `const` field is immutable after construction, also
                // through `&T!` and in the type's own methods.
                let aggregate_ty = object_ty.as_ref().map(|ty| match self.resolve_type_aliases(ty) {
                    TypeKind::Ref { inner } | TypeKind::MutRef { inner } => {
                        self.resolve_type_aliases(&inner.node)
                    }
                    other => other,
                });
                if let Some(aggregate_ty) = &aggregate_ty {
                    let type_name = super::declare::type_kind_base_name(aggregate_ty);
                    if self
                        .struct_const_fields
                        .get(&type_name)
                        .is_some_and(|fields| fields.contains(name))
                    {
                        self.push_error(
                            target.span,
                            "S07",
                            format!("cannot assign to const field `{name}`"),
                        );
                    }
                }
                if let Some(TypeKind::Named {
                    name: aggregate_name,
                    ..
                }) = object_ty
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
                let inner_ty = self.assignment_operand_type(inner);
                match &inner_ty {
                    Some(TypeKind::RawPtr { .. }) => {
                        if self.unsafe_depth == 0 {
                            self.push_error(
                                target.span,
                                "S11",
                                "store through raw pointer requires unsafe block".to_string(),
                            );
                        }
                    }
                    Some(TypeKind::Ref { .. }) => self.push_error(
                        target.span,
                        "S07",
                        "cannot assign through a shared reference".to_string(),
                    ),
                    Some(TypeKind::MutRef { .. }) => {}
                    None => {}
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
                let object_ty = self.assignment_operand_type(object);
                self.require_exclusive_receiver(object, object_ty.as_ref(), "set");
                let object_value_ty = object_ty
                    .as_ref()
                    .map(|ty| self.resolve_type_aliases(ty))
                    .map(|ty| match ty {
                        TypeKind::Ref { inner } | TypeKind::MutRef { inner } => {
                            self.resolve_type_aliases(&inner.node)
                        }
                        other => other,
                    });
                if matches!(&object_value_ty, Some(TypeKind::Bytes)) {
                    self.push_error(target.span, "S07", "byte strings are immutable".to_string());
                }
                if let Some(TypeKind::Named { name, type_args }) = &object_value_ty
                    && name == "Array"
                {
                    let setter_name = format!("{name}.set");
                    if self
                        .resolve_symbol(&setter_name)
                        .is_some_and(|symbol| symbol.unsafe_fn)
                        && self.unsafe_depth == 0
                    {
                        self.push_error(
                            target.span,
                            "S11",
                            "indexed assignment requires unsafe context while its setter is unsafe"
                                .to_string(),
                        );
                    }
                    let from = self
                        .current_function
                        .last()
                        .cloned()
                        .unwrap_or_else(|| "__program__".to_string());
                    let type_kinds: Vec<TypeKind> =
                        type_args.iter().map(|arg| arg.node.clone()).collect();
                    if type_kinds.is_empty() {
                        self.add_dependency_edge(DependencyKind::Call, &from, "Array.set");
                    } else {
                        // Record the real `Array.set` signature with the
                        // receiver's generics substituted, not only the
                        // element type, so the layout record is complete.
                        let set_signature = self.resolve_symbol("Array.set").map(|symbol| {
                            (symbol.params.clone(), symbol.ty.clone(), symbol.variadic)
                        });
                        let array_generic_params = self
                            .struct_generic_params
                            .get("Array")
                            .cloned()
                            .unwrap_or_default();
                        if let Some((params, return_ty, variadic)) = set_signature {
                            let substitution: std::collections::HashMap<String, TypeKind> =
                                array_generic_params
                                    .iter()
                                    .cloned()
                                    .zip(type_kinds.iter().cloned())
                                    .collect();
                            let substituted_params: Vec<TypeKind> = params
                                .iter()
                                .map(|param| substitute_type_kind(param, &substitution))
                                .collect();
                            let substituted_return =
                                return_ty.map(|ty| substitute_type_kind(&ty, &substitution));
                            self.record_fn_value_layout(
                                "Array.set",
                                &type_kinds,
                                &substituted_params,
                                substituted_return.as_ref(),
                                variadic,
                                target.span,
                            );
                        } else {
                            self.record_fn_value_layout(
                                "Array.set",
                                &type_kinds,
                                &type_kinds,
                                None,
                                false,
                                target.span,
                            );
                        }
                        let mangled = mangle_monomorphized("Array.set", &type_kinds);
                        if !self.has_monomorphization("Array.set", &type_kinds, &mangled) {
                            self.monomorphizations.push(MonomorphizationInfo {
                                fn_name: "Array.set".to_string(),
                                type_args: type_kinds,
                                mangled_name: mangled.clone(),
                            });
                        }
                        self.add_dependency_edge(DependencyKind::Call, &from, &mangled);
                    }
                }
                let _ = indices;
            }
            _ => {
                self.push_error(target.span, "S07", "invalid assignment target".to_string());
            }
        }
    }

    fn lvalue_contains_shared_deref(&self, target: &Expr) -> bool {
        match &target.node {
            ExprKind::Group(inner) => self.lvalue_contains_shared_deref(inner),
            ExprKind::Field { object, .. } | ExprKind::Index { object, .. } => {
                matches!(
                    self.assignment_operand_type(object),
                    Some(TypeKind::Ref { .. })
                ) || self.lvalue_contains_shared_deref(object)
            }
            ExprKind::Unary {
                op: UnaryOpKind::Deref,
                expr: inner,
            } => matches!(
                self.assignment_operand_type(inner),
                Some(TypeKind::Ref { .. })
            ),
            _ => false,
        }
    }

    fn expr_dereferences_shared_reference(&self, expr: &Expr) -> bool {
        match &expr.node {
            ExprKind::Group(inner) => self.expr_dereferences_shared_reference(inner),
            ExprKind::Field { object, .. } | ExprKind::Index { object, .. } => {
                self.expr_dereferences_shared_reference(object)
            }
            ExprKind::Unary {
                op: UnaryOpKind::Deref,
                expr: inner,
            } => self
                .annotated_exprs
                .iter()
                .rev()
                .find(|annotation| {
                    annotation.span.start == inner.span.start
                        && annotation.span.end == inner.span.end
                })
                .and_then(|annotation| annotation.ty.as_ref())
                .is_some_and(|ty| contains_non_string_reference(&self.resolve_type_aliases(ty))),
            _ => false,
        }
    }

    pub(super) fn types_compatible(&self, a: &TypeKind, b: &TypeKind) -> bool {
        let a = self.resolve_type_aliases(a);
        let b = self.resolve_type_aliases(b);
        match (&a, &b) {
            (TypeKind::Error, _) | (_, TypeKind::Error) => true,
            (TypeKind::Any, TypeKind::Any) => true,
            // Never (!) is compatible with any type — diverging arms unify with anything
            (TypeKind::Never, _) | (_, TypeKind::Never) => true,
            // Named type ↔ dyn Trait: specific check before the broad Named fallback.
            (TypeKind::Named { name, .. }, TypeKind::Dyn { trait_name })
            | (TypeKind::Dyn { trait_name }, TypeKind::Named { name, .. }) => self
                .trait_impls
                .get(name.as_str())
                .map(|ts| ts.contains(trait_name.as_str()))
                .unwrap_or(false),
            (
                TypeKind::Named {
                    name: a_name,
                    type_args: a_args,
                },
                TypeKind::Named {
                    name: b_name,
                    type_args: b_args,
                },
            ) => {
                a_name == b_name
                    && ((a_args.is_empty() && b_args.is_empty())
                        || (a_args.len() == b_args.len()
                            && a_args
                                .iter()
                                .zip(b_args.iter())
                                .all(|(a, b)| self.generic_type_args_compatible(&a.node, &b.node))))
            }
            (TypeKind::Named { name, type_args }, _) | (_, TypeKind::Named { name, type_args })
                if type_args.is_empty()
                    && self
                        .current_generic_params
                        .iter()
                        .rev()
                        .any(|params| params.contains(name)) =>
            {
                true
            }
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
            (TypeKind::Ref { inner: a }, TypeKind::Ref { inner: b })
            | (TypeKind::MutRef { inner: a }, TypeKind::MutRef { inner: b })
            | (TypeKind::Ref { inner: a }, TypeKind::MutRef { inner: b }) => {
                self.types_have_same_runtime_shape(&a.node, &b.node)
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
            (TypeKind::RawPtr { .. }, TypeKind::Ref { .. } | TypeKind::MutRef { .. })
            | (TypeKind::Ref { .. } | TypeKind::MutRef { .. }, TypeKind::RawPtr { .. }) => false,
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
                        .all(|(ap, bp)| self.types_have_same_runtime_shape(&ap.node, &bp.node))
                    && self.types_have_same_runtime_shape(&a_ret.node, &b_ret.node)
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
                        .all(|(ap, bp)| self.types_have_same_runtime_shape(&ap.node, &bp.node))
                    && self.types_have_same_runtime_shape(&a_ret.node, &b_ret.node)
            }
            // dyn A ↔ dyn A
            (TypeKind::Dyn { trait_name: a }, TypeKind::Dyn { trait_name: b }) => a == b,
            _ => std::mem::discriminant(&a) == std::mem::discriminant(&b),
        }
    }

    fn generic_type_args_compatible(&self, a: &TypeKind, b: &TypeKind) -> bool {
        let a = self.resolve_type_aliases(a);
        let b = self.resolve_type_aliases(b);
        if matches!(a, TypeKind::Error) || matches!(b, TypeKind::Error) {
            return true;
        }
        let is_scoped_generic = |ty: &TypeKind| {
            matches!(
                ty,
                TypeKind::Named { name, type_args }
                    if type_args.is_empty()
                        && self
                            .current_generic_params
                            .iter()
                            .rev()
                            .any(|params| params.contains(name))
            )
        };
        if is_scoped_generic(&a) || is_scoped_generic(&b) {
            return true;
        }
        match (&a, &b) {
            (
                TypeKind::Named {
                    name: a_name,
                    type_args: a_args,
                },
                TypeKind::Named {
                    name: b_name,
                    type_args: b_args,
                },
            ) => {
                a_name == b_name
                    && a_args.len() == b_args.len()
                    && a_args
                        .iter()
                        .zip(b_args)
                        .all(|(a, b)| self.generic_type_args_compatible(&a.node, &b.node))
            }
            (TypeKind::Ref { inner: a }, TypeKind::Ref { inner: b })
            | (TypeKind::MutRef { inner: a }, TypeKind::MutRef { inner: b })
            | (TypeKind::RawPtr { inner: a }, TypeKind::RawPtr { inner: b })
            | (TypeKind::Slice { elem_ty: a }, TypeKind::Slice { elem_ty: b })
            | (TypeKind::FlexibleArray { elem_ty: a }, TypeKind::FlexibleArray { elem_ty: b }) => {
                self.generic_type_args_compatible(&a.node, &b.node)
            }
            (
                TypeKind::Array {
                    elem_ty: a,
                    len: a_len,
                },
                TypeKind::Array {
                    elem_ty: b,
                    len: b_len,
                },
            ) => a_len == b_len && self.generic_type_args_compatible(&a.node, &b.node),
            (
                TypeKind::Fn {
                    params: a_params,
                    return_ty: a_ret,
                }
                | TypeKind::CFn {
                    params: a_params,
                    return_ty: a_ret,
                },
                TypeKind::Fn {
                    params: b_params,
                    return_ty: b_ret,
                }
                | TypeKind::CFn {
                    params: b_params,
                    return_ty: b_ret,
                },
            ) => {
                a_params.len() == b_params.len()
                    && a_params
                        .iter()
                        .zip(b_params)
                        .all(|(a, b)| self.generic_type_args_compatible(&a.node, &b.node))
                    && self.generic_type_args_compatible(&a_ret.node, &b_ret.node)
            }
            (TypeKind::Dyn { trait_name: a }, TypeKind::Dyn { trait_name: b }) => a == b,
            (TypeKind::Str, TypeKind::Ref { inner }) | (TypeKind::Ref { inner }, TypeKind::Str)
                if matches!(inner.node, TypeKind::Str) =>
            {
                true
            }
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

    /// The constant result of `value as target`, matching the runtime cast
    /// lowering: integers wrap to the target width, integer to float rounds
    /// to nearest, float to integer truncates and saturates (NaN is 0), and
    /// f64 to f32 rounds. Constants of other casts pass through unchanged.
    pub(super) fn const_numeric_cast(
        value: ConstValue,
        source: &TypeKind,
        target: &TypeKind,
    ) -> Option<ConstValue> {
        let unsigned_source = matches!(
            source,
            TypeKind::Uint8 | TypeKind::Uint16 | TypeKind::Uint32 | TypeKind::Uint64 | TypeKind::Usize
        );
        let integer = |wide: i128| -> Option<ConstValue> {
            let narrowed = match target {
                TypeKind::Int8 => wide as i8 as i64,
                TypeKind::Int16 => wide as i16 as i64,
                TypeKind::Int32 => wide as i32 as i64,
                TypeKind::Uint8 => wide as u8 as i64,
                TypeKind::Uint16 => wide as u16 as i64,
                TypeKind::Uint32 => wide as u32 as i64,
                _ => wide as i64,
            };
            Some(ConstValue::Int(narrowed))
        };
        match value {
            ConstValue::Int(raw) if Self::is_integer(target) => {
                integer(if unsigned_source { raw as u64 as i128 } else { raw as i128 })
            }
            ConstValue::Int(raw) if matches!(target, TypeKind::Float32 | TypeKind::Float64) => {
                let single = matches!(target, TypeKind::Float32);
                let float = match (unsigned_source, single) {
                    (true, true) => raw as u64 as f32 as f64,
                    (true, false) => raw as u64 as f64,
                    (false, true) => raw as f32 as f64,
                    (false, false) => raw as f64,
                };
                Some(ConstValue::Float(float))
            }
            ConstValue::Float(float) if Self::is_integer(target) => {
                let saturated = match target {
                    TypeKind::Int8 => float as i8 as i64,
                    TypeKind::Int16 => float as i16 as i64,
                    TypeKind::Int32 => float as i32 as i64,
                    TypeKind::Uint8 => float as u8 as i64,
                    TypeKind::Uint16 => float as u16 as i64,
                    TypeKind::Uint32 => float as u32 as i64,
                    TypeKind::Uint64 | TypeKind::Usize => float as u64 as i64,
                    _ => float as i64,
                };
                Some(ConstValue::Int(saturated))
            }
            ConstValue::Float(float) if matches!(target, TypeKind::Float32) => {
                Some(ConstValue::Float(float as f32 as f64))
            }
            other => Some(other),
        }
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
                    | TypeKind::MutRef { .. }
                    | TypeKind::Any
            )
    }

    /// If `t` is `&U` where `U` is a value-like type, return `U`; otherwise return `t`.
    pub(super) fn autoderef_type(t: TypeKind) -> TypeKind {
        if let TypeKind::Ref { ref inner } | TypeKind::MutRef { ref inner } = t {
            if Self::is_autoderef_value(&inner.node) {
                return inner.node.clone();
            }
        }
        t
    }
}

/// Return the spelling that introduced a resolved binding in an expression.
/// The semantic annotation still covers the whole expression for type and
/// code-generation consumers; this narrower span is reserved for tooling that
/// must never replace an argument list or another surrounding construct.
fn binding_occurrence_span(expr: &Expr, binding: Option<&ResolvedBinding>) -> Option<Span> {
    binding?;
    match &expr.node {
        ExprKind::Ident(_) => Some(expr.span),
        ExprKind::Call { callee, .. } => binding_occurrence_span(callee, binding),
        ExprKind::Group(inner) => binding_occurrence_span(inner, binding),
        _ => None,
    }
}

fn type_contains_rawptr(ty: &TypeKind) -> bool {
    match ty {
        TypeKind::RawPtr { .. } => true,
        TypeKind::Ref { inner } | TypeKind::MutRef { inner } => type_contains_rawptr(&inner.node),
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

fn syscall_abi_type(ty: &TypeKind, allow_void: bool) -> bool {
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
            | TypeKind::Bool
            | TypeKind::Str
            | TypeKind::RawPtr { .. }
    ) || (allow_void && matches!(ty, TypeKind::Void))
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
        TypeKind::Ref { inner } | TypeKind::MutRef { inner } => type_contains_any(&inner.node),
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

fn builtin_constructor_kind(name: &str, symbol: &Symbol) -> Option<&'static str> {
    if symbol.is_import || symbol.span.start != 0 || symbol.span.end != 0 {
        return None;
    }
    match name {
        "Some" => Some("Some"),
        "None" => Some("None"),
        "Ok" => Some("Ok"),
        "Err" => Some("Err"),
        _ => None,
    }
}

pub(super) fn type_contains_error(ty: &TypeKind) -> bool {
    match ty {
        TypeKind::Error => true,
        TypeKind::Ref { inner } | TypeKind::MutRef { inner } | TypeKind::RawPtr { inner } => {
            type_contains_error(&inner.node)
        }
        TypeKind::Array { elem_ty, .. }
        | TypeKind::FlexibleArray { elem_ty }
        | TypeKind::Slice { elem_ty } => type_contains_error(&elem_ty.node),
        TypeKind::Named { type_args, .. } => {
            type_args.iter().any(|arg| type_contains_error(&arg.node))
        }
        TypeKind::Fn { params, return_ty } | TypeKind::CFn { params, return_ty } => {
            params.iter().any(|param| type_contains_error(&param.node))
                || type_contains_error(&return_ty.node)
        }
        _ => false,
    }
}

fn type_contains_self(ty: &TypeKind) -> bool {
    match ty {
        TypeKind::Named { name, type_args } => {
            (name == "Self" && type_args.is_empty())
                || type_args.iter().any(|arg| type_contains_self(&arg.node))
        }
        TypeKind::Ref { inner } | TypeKind::MutRef { inner } | TypeKind::RawPtr { inner } => {
            type_contains_self(&inner.node)
        }
        TypeKind::Array { elem_ty, .. }
        | TypeKind::FlexibleArray { elem_ty }
        | TypeKind::Slice { elem_ty } => type_contains_self(&elem_ty.node),
        TypeKind::Fn { params, return_ty } | TypeKind::CFn { params, return_ty } => {
            params.iter().any(|param| type_contains_self(&param.node))
                || type_contains_self(&return_ty.node)
        }
        _ => false,
    }
}

fn substitute_self_type(ty: &TypeKind, concrete: &TypeKind) -> TypeKind {
    match ty {
        TypeKind::Named { name, type_args } if name == "Self" && type_args.is_empty() => {
            concrete.clone()
        }
        TypeKind::Named { name, type_args } => TypeKind::Named {
            name: name.clone(),
            type_args: type_args
                .iter()
                .map(|arg| Spanned::new(substitute_self_type(&arg.node, concrete), arg.span))
                .collect(),
        },
        TypeKind::Ref { inner } => TypeKind::Ref {
            inner: Box::new(Spanned::new(
                substitute_self_type(&inner.node, concrete),
                inner.span,
            )),
        },
        TypeKind::MutRef { inner } => TypeKind::MutRef {
            inner: Box::new(Spanned::new(
                substitute_self_type(&inner.node, concrete),
                inner.span,
            )),
        },
        TypeKind::RawPtr { inner } => TypeKind::RawPtr {
            inner: Box::new(Spanned::new(
                substitute_self_type(&inner.node, concrete),
                inner.span,
            )),
        },
        TypeKind::Array { elem_ty, len } => TypeKind::Array {
            elem_ty: Box::new(Spanned::new(
                substitute_self_type(&elem_ty.node, concrete),
                elem_ty.span,
            )),
            len: *len,
        },
        TypeKind::FlexibleArray { elem_ty } => TypeKind::FlexibleArray {
            elem_ty: Box::new(Spanned::new(
                substitute_self_type(&elem_ty.node, concrete),
                elem_ty.span,
            )),
        },
        TypeKind::Slice { elem_ty } => TypeKind::Slice {
            elem_ty: Box::new(Spanned::new(
                substitute_self_type(&elem_ty.node, concrete),
                elem_ty.span,
            )),
        },
        TypeKind::Fn { params, return_ty } => TypeKind::Fn {
            params: params
                .iter()
                .map(|param| Spanned::new(substitute_self_type(&param.node, concrete), param.span))
                .collect(),
            return_ty: Box::new(Spanned::new(
                substitute_self_type(&return_ty.node, concrete),
                return_ty.span,
            )),
        },
        TypeKind::CFn { params, return_ty } => TypeKind::CFn {
            params: params
                .iter()
                .map(|param| Spanned::new(substitute_self_type(&param.node, concrete), param.span))
                .collect(),
            return_ty: Box::new(Spanned::new(
                substitute_self_type(&return_ty.node, concrete),
                return_ty.span,
            )),
        },
        _ => ty.clone(),
    }
}

/// Keep enough context for closures and sum constructors to infer an open
/// callee parameter without mistaking a caller's types for callee binders.
fn contextual_call_parameter(
    param: &TypeKind,
    binders: &[String],
    substitution: &std::collections::HashMap<String, TypeKind>,
) -> TypeKind {
    let open = |name: &String| binders.contains(name) && !substitution.contains_key(name);
    let open_type = |ty: &TypeKind| {
        matches!(ty, TypeKind::Named { name, type_args } if type_args.is_empty() && open(name))
    };
    let contextual_slot = |ty: &Type| {
        let node = if open_type(&ty.node) {
            TypeKind::Error
        } else {
            substitute_type_kind(&ty.node, substitution)
        };
        Spanned::new(node, ty.span)
    };

    match param {
        TypeKind::Fn { params, return_ty } => TypeKind::Fn {
            params: params
                .iter()
                .map(|param| Spanned::new(substitute_type_kind(&param.node, substitution), param.span))
                .collect(),
            return_ty: Box::new(contextual_slot(return_ty)),
        },
        TypeKind::Named { name, type_args }
            if matches!((name.rsplit('.').next(), type_args.len()), (Some("Option"), 1) | (Some("Result"), 2)) =>
        {
            TypeKind::Named {
                name: name.clone(),
                type_args: type_args.iter().map(contextual_slot).collect(),
            }
        }
        other => {
            let uninferred: Vec<String> = binders.iter().filter(|name| open(name)).cloned().collect();
            if super::type_mentions_params(other, &uninferred) {
                TypeKind::Error
            } else {
                substitute_type_kind(other, substitution)
            }
        }
    }
}

/// Substitute generic type parameters with concrete types.
pub(crate) fn substitute_type_kind(
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
        TypeKind::MutRef { inner } => TypeKind::MutRef {
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
pub(super) fn is_string_view_type(ty: &TypeKind) -> bool {
    match ty {
        TypeKind::Str => true,
        TypeKind::Ref { inner } | TypeKind::MutRef { inner } => is_string_view_type(&inner.node),
        _ => false,
    }
}

fn parse_method_suffix(ty: &TypeKind) -> Option<&'static str> {
    match ty {
        TypeKind::Int8 => Some("i8"),
        TypeKind::Int16 => Some("i16"),
        TypeKind::Int32 => Some("i32"),
        TypeKind::Int64 => Some("i64"),
        TypeKind::Uint8 => Some("u8"),
        TypeKind::Uint16 => Some("u16"),
        TypeKind::Uint32 => Some("u32"),
        TypeKind::Uint64 => Some("u64"),
        TypeKind::Isize => Some("isize"),
        TypeKind::Usize => Some("usize"),
        TypeKind::Float32 => Some("f32"),
        TypeKind::Float64 => Some("f64"),
        _ => None,
    }
}

/// Infer generic parameters embedded in a struct field type. Unlike variadic
/// call inference, this follows the shape of arrays and slices on both sides.
fn infer_struct_type_subst(
    field_ty: &TypeKind,
    value_ty: &TypeKind,
    generic_params: &[String],
    subst: &mut std::collections::HashMap<String, TypeKind>,
) {
    match (field_ty, value_ty) {
        (TypeKind::Named { name, type_args }, value) if type_args.is_empty() => {
            if generic_params.contains(name) {
                subst.entry(name.clone()).or_insert_with(|| value.clone());
            }
        }
        (
            TypeKind::Named {
                name: field_name,
                type_args: field_args,
            },
            TypeKind::Named {
                name: value_name,
                type_args: value_args,
            },
        ) if field_name == value_name => {
            for (field_arg, value_arg) in field_args.iter().zip(value_args) {
                infer_struct_type_subst(&field_arg.node, &value_arg.node, generic_params, subst);
            }
        }
        (
            TypeKind::Array {
                elem_ty: field_elem,
                len: field_len,
            },
            TypeKind::Array {
                elem_ty: value_elem,
                len: value_len,
            },
        ) if field_len == value_len => {
            infer_struct_type_subst(&field_elem.node, &value_elem.node, generic_params, subst)
        }
        (
            TypeKind::Slice {
                elem_ty: field_elem,
            },
            TypeKind::Slice {
                elem_ty: value_elem,
            },
        )
        | (
            TypeKind::FlexibleArray {
                elem_ty: field_elem,
            },
            TypeKind::FlexibleArray {
                elem_ty: value_elem,
            },
        ) => infer_struct_type_subst(&field_elem.node, &value_elem.node, generic_params, subst),
        (TypeKind::Ref { inner: field }, TypeKind::Ref { inner: value })
        | (TypeKind::Ref { inner: field }, TypeKind::MutRef { inner: value })
        | (TypeKind::MutRef { inner: field }, TypeKind::MutRef { inner: value })
        | (TypeKind::RawPtr { inner: field }, TypeKind::RawPtr { inner: value }) => {
            infer_struct_type_subst(&field.node, &value.node, generic_params, subst);
        }
        _ => {}
    }
}

/// Returns the concrete arguments of a named receiver after peeling its
/// reference layers. Inherent methods are declared on the owner type, while
/// calls may use an `&Owner[T]` or `&Owner[T]!` view of that same owner.
fn receiver_named_type_args(ty: &TypeKind) -> Vec<crate::parser::ast::Spanned<TypeKind>> {
    match ty {
        TypeKind::Named { type_args, .. } => type_args.clone(),
        TypeKind::Ref { inner } | TypeKind::MutRef { inner } => {
            receiver_named_type_args(&inner.node)
        }
        _ => Vec::new(),
    }
}

fn receiver_is_named_type(ty: &TypeKind) -> bool {
    match ty {
        TypeKind::Named { .. } => true,
        TypeKind::Ref { inner } | TypeKind::MutRef { inner } => receiver_is_named_type(&inner.node),
        _ => false,
    }
}

fn addressable_ident(expr: &Expr) -> Option<&str> {
    match &expr.node {
        ExprKind::Ident(name) => Some(name),
        ExprKind::Group(inner) => addressable_ident(inner),
        _ => None,
    }
}

impl Analyzer {
    /// A reference parameter (`p`) or a field reached through one (`&p.a.b`).
    fn is_parameter_reference(&self, expr: &Expr) -> bool {
        let reference_parameter = |name: &str| {
            self.resolve_symbol(name).is_some_and(|symbol| {
                symbol.kind == SymbolKind::Parameter
                    && symbol.ty.as_ref().is_some_and(|ty| {
                        matches!(self.resolve_type_aliases(ty), TypeKind::Ref { .. } | TypeKind::MutRef { .. })
                    })
            })
        };
        match &expr.node {
            ExprKind::Group(inner) => self.is_parameter_reference(inner),
            ExprKind::Ident(name) => reference_parameter(name),
            ExprKind::Unary { op: UnaryOpKind::Ref, expr: inner }
                if matches!(inner.node, ExprKind::Field { .. }) =>
            {
                addressable_place_root(inner).is_some_and(reference_parameter)
            }
            // A reference-returning call derives its result from its reference
            // arguments and borrowed receiver (checked at its own `ret`).
            ExprKind::Call { args, named_args, .. } => args
                .iter()
                .chain(named_args.iter().map(|(_, arg)| arg))
                .all(|argument| self.is_parameter_derived_argument(argument)),
            ExprKind::MethodCall { object, args, named_args, .. } => {
                let static_call = addressable_place_root(object).is_some_and(|root| {
                    self.resolve_symbol(root)
                        .is_some_and(|symbol| !matches!(symbol.kind, SymbolKind::Variable { .. } | SymbolKind::Parameter))
                });
                (static_call
                    || addressable_place_root(object).is_some_and(reference_parameter)
                    || self.is_parameter_reference(object))
                    && args
                        .iter()
                        .chain(named_args.iter().map(|(_, arg)| arg))
                        .all(|argument| self.is_parameter_derived_argument(argument))
            }
            _ => false,
        }
    }

    /// A call argument that cannot be the source of a reference result
    /// (a non-reference value) or a reference derived from a parameter.
    fn is_parameter_derived_argument(&self, argument: &Expr) -> bool {
        let reference = self
            .annotated_exprs
            .iter()
            .rev()
            .find(|annotation| annotation.span == argument.span)
            .and_then(|annotation| annotation.ty.as_ref())
            .is_none_or(|ty| {
                matches!(self.resolve_type_aliases(ty), TypeKind::Ref { .. } | TypeKind::MutRef { .. })
            });
        !reference || self.is_parameter_reference(argument)
    }
}

/// The local or parameter at the root of an addressable place: `x` or a
/// field chain `x.a.b` (fields of a struct reached through a reference too).
pub(super) fn addressable_place_root(expr: &Expr) -> Option<&str> {
    match &expr.node {
        ExprKind::Field { object, .. } => addressable_place_root(object),
        _ => addressable_ident(expr),
    }
}

fn address_of_ident(expr: &Expr) -> Option<&str> {
    match &expr.node {
        ExprKind::Group(inner) => address_of_ident(inner),
        ExprKind::Unary {
            op: UnaryOpKind::Ref | UnaryOpKind::RefMut,
            expr: inner,
        } => addressable_ident(inner),
        _ => None,
    }
}

fn contains_non_string_reference(ty: &TypeKind) -> bool {
    match ty {
        TypeKind::Ref { inner } | TypeKind::MutRef { inner } => {
            !matches!(inner.node, TypeKind::Str)
        }
        TypeKind::Named { type_args, .. } => type_args
            .iter()
            .any(|argument| contains_non_string_reference(&argument.node)),
        TypeKind::Array { elem_ty, .. }
        | TypeKind::Slice { elem_ty }
        | TypeKind::FlexibleArray { elem_ty } => contains_non_string_reference(&elem_ty.node),
        TypeKind::Fn { return_ty, .. } | TypeKind::CFn { return_ty, .. } => {
            contains_non_string_reference(&return_ty.node)
        }
        _ => false,
    }
}

fn contains_nested_non_string_reference(ty: &TypeKind) -> bool {
    match ty {
        TypeKind::Ref { .. } | TypeKind::MutRef { .. } => false,
        TypeKind::Named { type_args, .. } => type_args
            .iter()
            .any(|argument| contains_non_string_reference(&argument.node)),
        TypeKind::Array { elem_ty, .. }
        | TypeKind::Slice { elem_ty }
        | TypeKind::FlexibleArray { elem_ty } => contains_non_string_reference(&elem_ty.node),
        TypeKind::Fn { return_ty, .. } | TypeKind::CFn { return_ty, .. } => {
            contains_non_string_reference(&return_ty.node)
        }
        _ => false,
    }
}

fn contains_owned_function_value(ty: &TypeKind) -> bool {
    match ty {
        TypeKind::Fn { .. } => true,
        TypeKind::Named { type_args, .. } => type_args
            .iter()
            .any(|argument| contains_owned_function_value(&argument.node)),
        TypeKind::Array { elem_ty, .. }
        | TypeKind::Slice { elem_ty }
        | TypeKind::FlexibleArray { elem_ty }
        | TypeKind::Ref { inner: elem_ty }
        | TypeKind::MutRef { inner: elem_ty }
        | TypeKind::RawPtr { inner: elem_ty } => contains_owned_function_value(&elem_ty.node),
        TypeKind::CFn { params, return_ty } => {
            params
                .iter()
                .any(|parameter| contains_owned_function_value(&parameter.node))
                || contains_owned_function_value(&return_ty.node)
        }
        _ => false,
    }
}

fn contains_nested_owned_function_value(ty: &TypeKind) -> bool {
    match ty {
        TypeKind::Fn { .. } => false,
        TypeKind::Named { type_args, .. } => type_args
            .iter()
            .any(|argument| contains_owned_function_value(&argument.node)),
        TypeKind::Array { elem_ty, .. }
        | TypeKind::Slice { elem_ty }
        | TypeKind::FlexibleArray { elem_ty } => contains_owned_function_value(&elem_ty.node),
        _ => false,
    }
}

fn closure_capture_is_plain_copy(ty: &TypeKind) -> bool {
    matches!(
        ty,
        TypeKind::Bool
            | TypeKind::Int8
            | TypeKind::Int16
            | TypeKind::Int32
            | TypeKind::Int64
            | TypeKind::Uint8
            | TypeKind::Uint16
            | TypeKind::Uint32
            | TypeKind::Uint64
            | TypeKind::Isize
            | TypeKind::Usize
            | TypeKind::Float16
            | TypeKind::Float32
            | TypeKind::Float64
            | TypeKind::RawPtr { .. }
            | TypeKind::CFn { .. }
    )
}

fn expression_root_ident(expr: &Expr) -> Option<&str> {
    match &expr.node {
        ExprKind::Ident(name) => Some(name),
        ExprKind::Group(inner) => expression_root_ident(inner),
        ExprKind::Field { object, .. } | ExprKind::Index { object, .. } => {
            expression_root_ident(object)
        }
        _ => None,
    }
}

fn expression_is_ident(expr: &Expr, target: &str) -> bool {
    match &expr.node {
        ExprKind::Ident(name) => name == target,
        ExprKind::Group(inner)
        | ExprKind::Try { expr: inner }
        | ExprKind::Cast { expr: inner, .. } => expression_is_ident(inner, target),
        _ => false,
    }
}

fn assignment_ident(expr: &Expr) -> Option<&str> {
    match &expr.node {
        ExprKind::Ident(name) => Some(name),
        ExprKind::Group(inner) => assignment_ident(inner),
        _ => None,
    }
}

fn expr_mutates_ident(expr: &Expr, target: &str) -> bool {
    expr_mutates_ident_with(expr, target, &|_| true)
}

/// Whether `expr` may mutate `target`; a method call on `target` mutates it
/// when `mutating_method(method)` says so (closure bodies ask the method's
/// receiver: only an exclusive `&T!` receiver mutates).
fn expr_mutates_ident_with(
    expr: &Expr,
    target: &str,
    mutating_method: &dyn Fn(&str) -> bool,
) -> bool {
    match &expr.node {
        ExprKind::Assign {
            target: place,
            value,
        }
        | ExprKind::CompoundAssign {
            target: place,
            value,
            ..
        } => {
            expression_root_ident(place) == Some(target)
                || expr_mutates_ident_with(place, target, mutating_method)
                || expr_mutates_ident_with(value, target, mutating_method)
        }
        ExprKind::IncDec { expr: inner, .. } => {
            expression_root_ident(inner) == Some(target) || expr_mutates_ident_with(inner, target, mutating_method)
        }
        ExprKind::MethodCall {
            object,
            method,
            args,
            named_args,
            ..
        } => {
            (expression_root_ident(object) == Some(target) && mutating_method(method))
                || expr_mutates_ident_with(object, target, mutating_method)
                || args
                    .iter()
                    .any(|argument| expr_mutates_ident_with(argument, target, mutating_method))
                || named_args
                    .iter()
                    .any(|(_, argument)| expr_mutates_ident_with(argument, target, mutating_method))
        }
        ExprKind::Group(inner)
        | ExprKind::Spread(inner)
        | ExprKind::Unary { expr: inner, .. }
        | ExprKind::Cast { expr: inner, .. }
        | ExprKind::Try { expr: inner } => expr_mutates_ident_with(inner, target, mutating_method),
        ExprKind::Binary { left, right, .. } => {
            expr_mutates_ident_with(left, target, mutating_method) || expr_mutates_ident_with(right, target, mutating_method)
        }
        ExprKind::Call {
            callee,
            args,
            named_args,
            ..
        } => {
            expr_mutates_ident_with(callee, target, mutating_method)
                || args
                    .iter()
                    .any(|argument| expr_mutates_ident_with(argument, target, mutating_method))
                || named_args
                    .iter()
                    .any(|(_, argument)| expr_mutates_ident_with(argument, target, mutating_method))
        }
        ExprKind::Field { object, .. } => expr_mutates_ident_with(object, target, mutating_method),
        ExprKind::StructInit { fields, .. } => fields
            .iter()
            .any(|(_, value)| expr_mutates_ident_with(value, target, mutating_method)),
        ExprKind::Match { scrutinee, arms } => {
            expr_mutates_ident_with(scrutinee, target, mutating_method)
                || arms.iter().any(|arm| {
                    !crate::parser::ast::pattern_all_bindings(&arm.pattern)
                        .iter()
                        .any(|binding| binding == target)
                        && (expr_mutates_ident_with(&arm.expr, target, mutating_method)
                            || arm
                                .guard
                                .as_ref()
                                .is_some_and(|guard| expr_mutates_ident_with(guard, target, mutating_method)))
                })
        }
        ExprKind::ArrayLit(elements) => elements
            .iter()
            .any(|element| expr_mutates_ident_with(element, target, mutating_method)),
        ExprKind::Index { object, indices } => {
            expr_mutates_ident_with(object, target, mutating_method)
                || indices
                    .iter()
                    .any(|index| expr_mutates_ident_with(index, target, mutating_method))
        }
        ExprKind::Closure { params, body, .. } => {
            !params.iter().any(|parameter| parameter == target) && expr_mutates_ident_with(body, target, mutating_method)
        }
        ExprKind::Ident(_) | ExprKind::Literal(_) => false,
    }
}

pub(super) fn collect_expr_identifiers(expr: &Expr, names: &mut Vec<String>) {
    match &expr.node {
        ExprKind::Ident(name) => names.push(name.clone()),
        // Placeholders of a string literal read values too (D-020).
        ExprKind::Literal(Literal::String(_)) => {
            if let Some(expanded) =
                crate::parser::format::expand_format_call_args(std::slice::from_ref(expr))
            {
                for argument in &expanded.args {
                    collect_expr_identifiers(argument, names);
                }
            }
        }
        ExprKind::Literal(_) => {}
        ExprKind::Group(inner)
        | ExprKind::Spread(inner)
        | ExprKind::Unary { expr: inner, .. }
        | ExprKind::Cast { expr: inner, .. }
        | ExprKind::IncDec { expr: inner, .. }
        | ExprKind::Try { expr: inner } => collect_expr_identifiers(inner, names),
        ExprKind::Binary { left, right, .. } => {
            collect_expr_identifiers(left, names);
            collect_expr_identifiers(right, names);
        }
        ExprKind::Assign { target, value } | ExprKind::CompoundAssign { target, value, .. } => {
            collect_expr_identifiers(target, names);
            collect_expr_identifiers(value, names);
        }
        ExprKind::Call {
            callee,
            args,
            named_args,
            ..
        } => {
            collect_expr_identifiers(callee, names);
            for argument in args {
                collect_expr_identifiers(argument, names);
            }
            for (_, argument) in named_args {
                collect_expr_identifiers(argument, names);
            }
        }
        ExprKind::Field { object, .. } => collect_expr_identifiers(object, names),
        ExprKind::StructInit { fields, .. } => {
            for (_, value) in fields {
                collect_expr_identifiers(value, names);
            }
        }
        ExprKind::MethodCall {
            object,
            args,
            named_args,
            ..
        } => {
            collect_expr_identifiers(object, names);
            for argument in args {
                collect_expr_identifiers(argument, names);
            }
            for (_, argument) in named_args {
                collect_expr_identifiers(argument, names);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            collect_expr_identifiers(scrutinee, names);
            for arm in arms {
                let mut arm_names = Vec::new();
                collect_expr_identifiers(&arm.expr, &mut arm_names);
                if let Some(guard) = &arm.guard {
                    collect_expr_identifiers(guard, &mut arm_names);
                }
                let bindings = crate::parser::ast::pattern_all_bindings(&arm.pattern);
                arm_names.retain(|name| !bindings.contains(name));
                names.extend(arm_names);
            }
        }
        ExprKind::ArrayLit(elements) => {
            for element in elements {
                collect_expr_identifiers(element, names);
            }
        }
        ExprKind::Index { object, indices } => {
            collect_expr_identifiers(object, names);
            for index in indices {
                collect_expr_identifiers(index, names);
            }
        }
        ExprKind::Closure { params, body, .. } => {
            let mut nested_names = Vec::new();
            collect_expr_identifiers(body, &mut nested_names);
            nested_names.retain(|name| !params.contains(name));
            names.extend(nested_names);
        }
    }
}

fn is_lexical_reference_expr(expr: &Expr) -> bool {
    match &expr.node {
        ExprKind::Ident(_) => true,
        ExprKind::Group(inner) => is_lexical_reference_expr(inner),
        ExprKind::Unary {
            op: UnaryOpKind::Ref | UnaryOpKind::RefMut,
            expr: inner,
        } => addressable_place_root(inner).is_some(),
        // A reference returned by a call derives from its reference
        // arguments; their roots must be named places so the ownership pass
        // can keep them borrowed.
        ExprKind::Call { args, named_args, .. } => args
            .iter()
            .chain(named_args.iter().map(|(_, arg)| arg))
            .all(is_simple_call_argument),
        ExprKind::MethodCall { object, args, named_args, .. } => {
            addressable_place_root(object).is_some()
                && args
                    .iter()
                    .chain(named_args.iter().map(|(_, arg)| arg))
                    .all(is_simple_call_argument)
        }
        _ => false,
    }
}

fn is_simple_call_argument(expr: &Expr) -> bool {
    matches!(expr.node, ExprKind::Literal(_))
        || addressable_place_root(expr).is_some()
        || is_lexical_reference_expr(expr)
}

pub(super) fn infer_type_subst(
    param_ty: &TypeKind,
    arg_ty: &TypeKind,
    generic_params: &[String],
    subst: &mut std::collections::HashMap<String, TypeKind>,
) {
    if matches!(arg_ty, TypeKind::Error) {
        return;
    }
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
            if let TypeKind::Ref { inner: arg_inner } | TypeKind::MutRef { inner: arg_inner } =
                arg_ty
            {
                infer_type_subst(&inner.node, &arg_inner.node, generic_params, subst);
            }
        }
        TypeKind::MutRef { inner } => {
            if let TypeKind::MutRef { inner: arg_inner } = arg_ty {
                infer_type_subst(&inner.node, &arg_inner.node, generic_params, subst);
            }
        }
        TypeKind::Fn { params, return_ty } => {
            if let TypeKind::Fn {
                params: arg_params,
                return_ty: arg_return,
            } = arg_ty
                && params.len() == arg_params.len()
            {
                for (param, arg) in params.iter().zip(arg_params) {
                    infer_type_subst(&param.node, &arg.node, generic_params, subst);
                }
                infer_type_subst(&return_ty.node, &arg_return.node, generic_params, subst);
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

/// Every bare (argument-free) type name inside `ty`.
fn collect_bare_names(ty: &TypeKind, names: &mut Vec<String>) {
    match ty {
        TypeKind::Named { name, type_args } if type_args.is_empty() => names.push(name.clone()),
        TypeKind::Named { type_args, .. } => {
            for argument in type_args {
                collect_bare_names(&argument.node, names);
            }
        }
        TypeKind::Ref { inner } | TypeKind::MutRef { inner } | TypeKind::RawPtr { inner } => {
            collect_bare_names(&inner.node, names)
        }
        _ => {}
    }
}
