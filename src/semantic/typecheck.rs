// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use crate::parser::ast::*;

use super::*;

impl Analyzer {
    /// Record the resolved internal-ABI value layout of a generic
    /// specialization (or, with empty `type_args`, a concrete function) so
    /// code generation can stop assuming one slot per value, then enforce the
    /// current gate: until the multi-slot internal ABI lands, non-single-slot
    /// parameters and results remain S14 errors. Records are keyed by the
    /// canonical resolved type arguments rather than the lossy mangled name.
    fn record_fn_value_layout(
        &mut self,
        function: &str,
        type_args: &[TypeKind],
        params: &[TypeKind],
        return_ty: Option<&TypeKind>,
        variadic: bool,
        span: Span,
    ) {
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
                        &self.resolve_type_aliases(&param.ty.node),
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
                if contains_non_string_reference(&self.resolve_type_aliases(&return_ty.node)) {
                    self.push_error(
                        return_ty.span,
                        "S10",
                        format!(
                            "function `{name}` cannot return a shared reference before Quazi has lifetime parameters; return an owned value instead"
                        ),
                    );
                }
                if contains_nested_owned_function_value(&self.resolve_type_aliases(&return_ty.node))
                {
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
                        let resolved = self.resolve_type_aliases(&param.ty.node);
                        let erased_format_param = erased_format_variadic
                            && matches!(resolved, TypeKind::Any);
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
                                    &self.resolve_type_aliases(&param.ty.node),
                                )
                            })
                            .collect(),
                        variadic_element: variadic_element.map(|element| {
                            crate::runtime_layout::runtime_layout_info(
                                &self.resolve_type_aliases(element),
                            )
                        }),
                        result: crate::runtime_layout::runtime_layout_info(
                            &self.resolve_type_aliases(&return_ty.node),
                        ),
                    };
                    self.fn_value_layouts.insert(fn_name.clone(), record);
                }
                let prev_module_path = self.current_module_path.clone();
                self.current_module_path = self.module_path_for_span(item.span);
                self.current_function.push(fn_name);
                self.current_generic_params.push(generic_params.clone());
                self.enter_scope();
                let fn_is_str_variadic = params.last().is_some_and(|p| {
                    p.variadic
                        && (matches!(&p.ty.node, TypeKind::Str | TypeKind::Ref { .. })
                            || erased_format_variadic)
                });
                for p in params {
                    if erased_format_variadic && p.variadic && matches!(p.ty.node, TypeKind::Any) {
                        // `@format ...args: any` is an erased call-site convention.
                        // The compiler formats each argument before the call, and the
                        // pseudo-parameter is intentionally unavailable to the body.
                        continue;
                    }
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
                bit_widths,
                is_union,
                attributes,
                ..
            } => {
                for (field_name, field_ty, _) in fields {
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
            ItemKind::Enum { name, variants, .. } => {
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
            }
            ItemKind::Import(_) => {}
            ItemKind::Trait { name, methods, .. } => {
                for method in methods {
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
                    if contains_non_string_reference(
                        &self.resolve_type_aliases(&method.return_ty.node),
                    ) {
                        self.push_error(
                            method.return_ty.span,
                            "S10",
                            format!(
                                "trait method `{name}.{}` cannot return a shared reference before Quazi has lifetime parameters",
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
                }
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
                if let Some(trait_ty) = trait_ty {
                    self.validate_trait_impl_conformance(trait_ty, for_ty, methods, item.span);
                }
                let type_name = crate::semantic::declare::type_kind_base_name(&for_ty.node);
                let impl_generic_params = match &for_ty.node {
                    TypeKind::Named { type_args, .. } => type_args
                        .iter()
                        .filter_map(|arg| match &arg.node {
                            TypeKind::Named { name, type_args } if type_args.is_empty() => {
                                Some(name.clone())
                            }
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                self.current_generic_params.push(impl_generic_params);
                for method in methods {
                    if let ItemKind::Fn { name, .. } = &method.node {
                        self.current_fn_name_override = Some(format!("{}.{}", type_name, name));
                    }
                    self.type_check_item(method);
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
            }
        }
    }

    fn validate_trait_impl_conformance(
        &mut self,
        trait_ty: &Type,
        for_ty: &Type,
        methods: &[Item],
        impl_span: Span,
    ) {
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
            if !signature.has_explicit_receiver {
                expected_params.push(for_ty.node.clone());
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
            let actual_params: Vec<TypeKind> = params[..runtime_param_count]
                .iter()
                .map(|param| param.ty.node.clone())
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
            if !self.types_have_same_runtime_shape(&expected_return, &return_ty.node) {
                self.push_error(
                    return_ty.span,
                    "S14",
                    format!(
                        "method `{trait_name}.{method_name}` must return `{expected_return}`, got `{}`",
                        return_ty.node
                    ),
                );
            }
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
                let declared_ty = ty.as_ref().map(|t| t.node.clone());
                let value_eval = value
                    .as_ref()
                    .map(|v| self.type_check_expr_expected(v, true, declared_ty.as_ref()))
                    .unwrap_or_default();

                if value_eval.ty.as_ref().is_some_and(|value_ty| {
                    matches!(
                        self.resolve_type_aliases(value_ty),
                        TypeKind::Ref { ref inner }
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
                if value_eval.ty.as_ref().is_some_and(|value_ty| {
                    matches!(
                        self.resolve_type_aliases(value_ty),
                        TypeKind::Ref { ref inner }
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
                        if address_of_ident(return_expr).is_some() {
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
                    && !matches!(condition_ty, TypeKind::Bool | TypeKind::Error)
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
                        && !matches!(ty, TypeKind::Bool | TypeKind::Error)
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
                            && !matches!(cond_ty, TypeKind::Bool | TypeKind::Error)
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
                                && !matches!(cond_ty, TypeKind::Bool | TypeKind::Error)
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
                                        TypeKind::Error
                                    }
                                    None => TypeKind::Error,
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

    fn type_check_expr_expected(
        &mut self,
        expr: &Expr,
        reachable: bool,
        expected: Option<&TypeKind>,
    ) -> ExprEval {
        if let Some(eval) = self.type_check_contextual_none_value(expr, reachable, expected) {
            self.reject_nested_owned_function_expression(expr, &eval);
            return eval;
        }
        if let Some(eval) = self.type_check_contextual_sum_constructor(expr, reachable, expected) {
            self.reject_nested_owned_function_expression(expr, &eval);
            return eval;
        }
        if let ExprKind::Closure { params, body } = &expr.node {
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

        if let Some(index) = payload_index {
            let resolved = self.resolve_type_aliases(&expected_args[index].node);
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

        if let (Some(index), Some(payload)) = (payload_index, args.first()) {
            let payload_eval =
                self.type_check_expr_expected(payload, reachable, Some(&expected_args[index].node));
            if let Some(actual) = payload_eval.ty
                && !self.check_expr_compat(payload, &expected_args[index].node, &actual)
            {
                self.push_error(
                    payload.span,
                    "S08",
                    format!(
                        "{constructor} payload: expected {}, got {}",
                        expected_args[index].node, actual
                    ),
                );
            }
            for extra in args.iter().skip(1) {
                self.type_check_expr(extra, reachable);
            }
        } else {
            for arg in args {
                self.type_check_expr(arg, reachable);
            }
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

    fn type_check_closure_expr(
        &mut self,
        expr: &Expr,
        params: &[String],
        body: &Expr,
        reachable: bool,
        expected: Option<&TypeKind>,
    ) -> ExprEval {
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
                if !closure_capture_is_plain_copy(&resolved) {
                    self.push_error(
                        expr.span,
                        "S10",
                        format!(
                            "closure cannot capture `{name}` of type `{resolved}` until owned and borrowed capture lifetimes are implemented"
                        ),
                    );
                }
                if expr_mutates_ident(body, &name) {
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
        if !matches!(
            expected_return,
            TypeKind::Error | TypeKind::Void | TypeKind::Never
        ) && !closure_capture_is_plain_copy(&self.resolve_type_aliases(&expected_return))
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
        let body_eval = self.type_check_expr_expected(body, reachable, Some(&expected_return));
        self.exit_scope_collect();

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
                        let addressable = addressable_ident(inner).and_then(|name| {
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
                                "address-of currently requires a local variable or parameter; fields, indexes, dereferences, calls, and temporaries are not addressable yet"
                                    .to_string(),
                            );
                        }
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
                            Some(TypeKind::Ref { inner: t }) => {
                                let pointee = self.resolve_type_aliases(&t.node);
                                if !Self::is_autoderef_value(&pointee) {
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
                    Some(src @ TypeKind::Ref { .. })
                        if matches!(target_ty, TypeKind::Ref { .. }) =>
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
                            TypeKind::Ref { .. } | TypeKind::Fn { .. } | TypeKind::CFn { .. }
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
                    Self::const_from_binary(op, lhs, rhs, left_eval.ty.as_ref())
                } else {
                    self.check_math_identities(expr.span, op, &left_eval, &right_eval)
                };

                ExprEval { ty, const_value }
            }
            ExprKind::Assign { target, value } => {
                let indexed_target_eval = matches!(target.node, ExprKind::Index { .. })
                    .then(|| self.type_check_expr(target, reachable));
                let assignment_expected = indexed_target_eval
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

                if let (Some(target_eval), Some(value_ty)) = (&indexed_target_eval, &value_eval.ty)
                    && let Some(target_ty) = &target_eval.ty
                    && !self.check_expr_compat(value, target_ty, value_ty)
                {
                    self.push_error(
                        target.span,
                        "S01",
                        format!(
                            "type mismatch in indexed assignment: expected {}, got {}",
                            target_ty, value_ty
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
                    // Mutable assignments may occur on only one control-flow
                    // path (or in a loop that executes zero times). Treating
                    // the assigned literal as globally constant makes later
                    // branches unsound. The bytecode dataflow pass still folds
                    // values where reaching definitions prove constancy.
                    self.set_symbol_const_value(name, None);
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
                                .map(|param| substitute_type_kind(param, &substitution))
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
                let positional_evals: Vec<ExprEval> = args
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        let expected = contextual_signature.as_ref().and_then(|(params, _)| {
                            params
                                .get(index)
                                .or_else(|| params.last().filter(|_| index >= params.len()))
                        });
                        self.type_check_expr_expected(arg, reachable, expected)
                    })
                    .collect();
                let named_evals: Vec<ExprEval> = named_args
                    .iter()
                    .map(|(name, arg)| {
                        let expected = contextual_signature.as_ref().and_then(|(params, names)| {
                            names
                                .iter()
                                .position(|parameter| parameter == name)
                                .and_then(|index| params.get(index))
                        });
                        self.type_check_expr_expected(arg, reachable, expected)
                    })
                    .collect();
                let arg_evals: Vec<ExprEval> =
                    positional_evals.into_iter().chain(named_evals).collect();

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
                        self.reject_nested_owned_function_expression(expr, &eval);
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
                    let resolved_method = self.resolve_module_method(object, method);
                    let contextual_signature = resolved_method.as_ref().and_then(|resolved| {
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
                                .map(|param| substitute_type_kind(param, &substitution))
                                .collect::<Vec<_>>();
                            let names = self
                                .fn_param_names
                                .get(resolved)
                                .cloned()
                                .unwrap_or_default();
                            (params, names)
                        })
                    });
                    let mut arg_evals: Vec<ExprEval> = args
                        .iter()
                        .enumerate()
                        .map(|(index, arg)| {
                            let expected = contextual_signature
                                .as_ref()
                                .and_then(|(params, _)| params.get(index));
                            self.type_check_expr_expected(arg, reachable, expected)
                        })
                        .collect();
                    for (name, arg) in named_args {
                        let expected = contextual_signature.as_ref().and_then(|(params, names)| {
                            names
                                .iter()
                                .position(|parameter| parameter == name)
                                .and_then(|index| params.get(index))
                        });
                        arg_evals.push(self.type_check_expr_expected(arg, reachable, expected));
                    }
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
                                    .enumerate()
                                    .map(|(index, arg)| {
                                        let expected = sym.params.get(index);
                                        self.type_check_expr_expected(arg, reachable, expected)
                                    })
                                    .collect();
                                let param_names = self
                                    .fn_param_names
                                    .get(&method_full)
                                    .cloned()
                                    .unwrap_or_default();
                                for (name, arg) in named_args {
                                    let expected = param_names
                                        .iter()
                                        .position(|parameter| parameter == name)
                                        .and_then(|index| sym.params.get(index));
                                    self.type_check_expr_expected(arg, reachable, expected);
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
                                            let specialized_params: Vec<TypeKind> = sym
                                                .params
                                                .iter()
                                                .map(|param| substitute_type_kind(param, &subst))
                                                .collect();
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
                            for arg in args {
                                let arg_eval = self.type_check_expr(arg, reachable);
                                if variant_arity.is_none() {
                                    continue;
                                }
                                let Some(payload_ty) = &arg_eval.ty else {
                                    continue;
                                };
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
                            // Keep the historical untyped result: codegen
                            // resolves valid qualified constructors
                            // structurally. Typing them in analysis is a
                            // separate feature.
                            let eval = ExprEval::default();
                            self.annotate_expr(expr, &eval, reachable, None);
                            self.reject_nested_owned_function_expression(expr, &eval);
                            return eval;
                        }
                    }

                    let object_eval = self.type_check_expr(object, reachable);
                    if object_eval.ty.as_ref().is_some_and(|ty| {
                        contains_non_string_reference(&self.resolve_type_aliases(ty))
                    }) || self.expr_dereferences_shared_reference(object)
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
                            let type_name = super::declare::type_kind_base_name(object_ty);
                            let mangled = format!("{}.{}", type_name, method);
                            let symbol = self.resolve_symbol(&mangled)?;
                            let substitution: std::collections::HashMap<String, TypeKind> =
                                match object_ty {
                                    TypeKind::Named { type_args, .. } if !type_args.is_empty() => {
                                        self.struct_generic_params
                                            .get(type_name.as_str())
                                            .map(|params| {
                                                params
                                                    .iter()
                                                    .zip(type_args.iter())
                                                    .map(|(param, ty)| {
                                                        (param.clone(), ty.node.clone())
                                                    })
                                                    .collect()
                                            })
                                            .unwrap_or_default()
                                    }
                                    _ => std::collections::HashMap::new(),
                                };
                            let params = symbol
                                .params
                                .iter()
                                .map(|param| substitute_type_kind(param, &substitution))
                                .collect::<Vec<_>>();
                            Some(params.get(1..).unwrap_or(&[]).to_vec())
                        });
                    let arg_evals: Vec<ExprEval> = args
                        .iter()
                        .enumerate()
                        .map(|(index, arg)| {
                            let expected = contextual_method_signature
                                .as_ref()
                                .and_then(|params| params.get(index));
                            self.type_check_expr_expected(arg, reachable, expected)
                        })
                        .collect();
                    let named_arg_evals: Vec<(String, Expr, ExprEval)> = named_args
                        .iter()
                        .map(|(name, arg)| {
                            let expected = object_eval.ty.as_ref().and_then(|object_ty| {
                                let type_name = super::declare::type_kind_base_name(object_ty);
                                let mangled = format!("{}.{}", type_name, method);
                                let position = self
                                    .fn_param_names
                                    .get(&mangled)?
                                    .iter()
                                    .position(|parameter| parameter == name)?;
                                contextual_method_signature.as_ref()?.get(position)
                            });
                            (
                                name.clone(),
                                arg.clone(),
                                self.type_check_expr_expected(arg, reachable, expected),
                            )
                        })
                        .collect();

                    // Library-defined impl methods take priority over compiler builtins.
                    // Primitive types use the same mangled namespace (`str.foo`,
                    // `i64.foo`, and so on) as named types, which keeps their API in
                    // Quazi instead of hardcoding every convenience method here.
                    // Returns Some(return_ty) when an impl method is found and side-effects recorded.
                    let impl_resolved: Option<Option<TypeKind>> = if let Some(object_ty) =
                        &object_eval.ty.clone()
                    {
                        // These operations are representation-level compiler builtins on
                        // primitive receivers. Prelude Display impls intentionally express
                        // their defaults in terms of them (for example i32.to_str calls
                        // i32.to_string), so resolving that inner call back to the impl
                        // would recurse forever. Named types still get normal impl lookup.
                        let prefer_primitive_builtin = !matches!(object_ty, TypeKind::Named { .. })
                            && matches!(
                                method.as_str(),
                                "len"
                                    | "to_str"
                                    | "to_string"
                                    | "as_string"
                                    | "as_str"
                                    | "as_ptr"
                                    | "parse"
                            );
                        if prefer_primitive_builtin {
                            None
                        } else {
                            let type_name = super::declare::type_kind_base_name(object_ty);
                            let type_args = match object_ty {
                                TypeKind::Named { type_args, .. } => type_args.clone(),
                                _ => Vec::new(),
                            };
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
                                    if let (Some(param_ty), Some(arg_ty)) = (param_ty, &arg_eval.ty)
                                    {
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
                                        let Some(pos) =
                                            param_names.iter().position(|p| p == arg_name)
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
                                // Apply type substitution to the result and validate the
                                // physical ABI before recording a specialization.
                                let ret_ty = if !type_args.is_empty() {
                                    sym.ty.as_ref().map(|ty| substitute_type_kind(ty, &subst))
                                } else {
                                    sym.ty.clone()
                                };
                                if !type_args.is_empty() {
                                    let type_kinds: Vec<TypeKind> =
                                        type_args.iter().map(|t| t.node.clone()).collect();
                                    self.record_fn_value_layout(
                                        &mangled,
                                        &type_kinds,
                                        &substituted_params,
                                        ret_ty.as_ref(),
                                        is_variadic,
                                        expr.span,
                                    );
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
                                    self.add_dependency_edge(
                                        DependencyKind::Call,
                                        &from,
                                        &mono_name,
                                    );
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
                                Some(ret_ty)
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
                            // Runtime primitive formatting currently returns a heap-backed,
                            // NUL-terminated string view, not the three-word String struct.
                            "to_string" => Some(TypeKind::Str),
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
                                self.reject_nested_owned_function_expression(expr, &eval);
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
                let generic_params = self
                    .struct_generic_params
                    .get(name)
                    .cloned()
                    .unwrap_or_default();
                let mut substitution = std::collections::HashMap::new();
                if let Some(field_defs) = self.struct_defs.get(name).cloned() {
                    for (fname, fval) in fields {
                        let val_eval = self.type_check_expr(fval, reachable);
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
                                if !self.types_compatible(got_ty, &concrete_expected) {
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
                                let builtin_payload =
                                    match (&scrutinee_eval.ty, ename.as_str(), variant.as_str()) {
                                        (
                                            Some(TypeKind::Named { name, type_args }),
                                            "Option",
                                            "Some",
                                        ) if name.rsplit('.').next() == Some("Option") => {
                                            type_args.first().map(|ty| ty.node.clone())
                                        }
                                        (
                                            Some(TypeKind::Named { name, type_args }),
                                            "Result",
                                            "Ok",
                                        ) if name.rsplit('.').next() == Some("Result") => {
                                            type_args.first().map(|ty| ty.node.clone())
                                        }
                                        (
                                            Some(TypeKind::Named { name, type_args }),
                                            "Result",
                                            "Err",
                                        ) if name.rsplit('.').next() == Some("Result") => {
                                            type_args.get(1).map(|ty| ty.node.clone())
                                        }
                                        _ => None,
                                    };
                                if let Some(payload) = builtin_payload {
                                    sub_patterns
                                        .iter()
                                        .find_map(|sub| match &sub.node {
                                            PatternKind::Bind(name) => {
                                                Some((name.clone(), payload.clone()))
                                            }
                                            _ => None,
                                        })
                                        .into_iter()
                                        .collect()
                                } else if let Some(info) = self.enums.get(&ename) {
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

                    // Type-check the optional guard expression (inside binding scope).
                    if let Some(guard) = &arm.guard {
                        let guard_eval = self.type_check_expr(guard, reachable);
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
                let idx_evals: Vec<ExprEval> = indices
                    .iter()
                    .map(|i| self.type_check_expr(i, reachable))
                    .collect();

                // Named type that explicitly implements the Index trait → dispatch to Type.index.
                // Checks trait_impls registry so accidental `fn index` methods don't trigger [].
                let maybe_index_mangled = if let Some(object_ty) = &obj_eval.ty {
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
                        let (generic_params, substitution) = match obj_eval.ty.as_ref() {
                            Some(object_ty @ TypeKind::Named { name, .. }) => {
                                let generic_params = self
                                    .struct_generic_params
                                    .get(name.as_str())
                                    .cloned()
                                    .unwrap_or_default();
                                let mut substitution = std::collections::HashMap::new();
                                if let Some(receiver_ty) = sym.params.first() {
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
                        if !substitution.is_empty() {
                            let type_args: Vec<TypeKind> = generic_params
                                .iter()
                                .filter_map(|param| substitution.get(param).cloned())
                                .collect();
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
                                if !self
                                    .monomorphizations
                                    .iter()
                                    .any(|mono| mono.mangled_name == mono_name)
                                {
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
                        &obj_eval.ty,
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
                        (&obj_eval.ty, idx_evals.first())
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
                        &obj_eval.ty,
                        Some(TypeKind::Array { .. } | TypeKind::Slice { .. } | TypeKind::Bytes)
                    ) {
                        let from = self
                            .current_function
                            .last()
                            .cloned()
                            .unwrap_or_else(|| "__program__".to_string());
                        self.add_dependency_edge(DependencyKind::Call, &from, "panic.panic");
                        self.add_dependency_edge(DependencyKind::Call, &from, "panic");
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
                        if (name == "Result" || name == "Option") && !type_args.is_empty() =>
                    {
                        Some(type_args[0].node.clone())
                    }
                    Some(TypeKind::Named { name, .. }) if name == "Result" || name == "Option" => {
                        Some(TypeKind::Error)
                    }
                    Some(ty) => {
                        self.push_error(
                            expr.span,
                            "S14",
                            format!("`?` operator requires Result or Option, got {}", ty),
                        );
                        Some(TypeKind::Error)
                    }
                    None => None,
                };
                ExprEval {
                    ty: payload_ty,
                    const_value: None,
                }
            }

            ExprKind::Closure { params, body } => {
                return self.type_check_closure_expr(expr, params, body, reachable, None);
            }
        };

        self.reject_nested_owned_function_expression(expr, &result);
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
            let eval = self.type_check_expr_expected(arg_expr, true, sym.params.get(pos));
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
        let prefix = match receiver {
            Some(TypeKind::Named { name, .. }) if name == "String" || name.ends_with(".String") => {
                "String"
            }
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
        // Auto-deref references for binary operations: &T op U becomes T op U.
        let left = left.clone().map(Self::autoderef_type);
        let right = right.clone().map(Self::autoderef_type);
        match op {
            BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul | BinOpKind::Div | BinOpKind::Mod => {
                match (&left, &right) {
                    (Some(l), Some(r)) if self.types_compatible(l, r) => {
                        // Integer division and remainder lower to an implicit panic guard.
                        // Record that hidden call so whole-program dead-code elimination keeps
                        // the prelude panic path reachable from the current function.
                        if matches!(op, BinOpKind::Div | BinOpKind::Mod) && Self::is_integer(l) {
                            let from = self
                                .current_function
                                .last()
                                .cloned()
                                .unwrap_or_else(|| "__program__".to_string());
                            self.add_dependency_edge(DependencyKind::Call, &from, "panic.panic");
                            // Non-namespaced/library compilation can expose the same
                            // prelude-compatible function under its bare name.
                            self.add_dependency_edge(DependencyKind::Call, &from, "panic");
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
                    && ((!self.types_compatible(l, r)) || (is_string_type(l) != is_string_type(r)))
                {
                    self.push_error(
                        span,
                        "S01",
                        format!("type mismatch in binary op: {} vs {}", l, r),
                    );
                }
                Some(TypeKind::Bool)
            }
            BinOpKind::Lt | BinOpKind::Gt | BinOpKind::LtEq | BinOpKind::GtEq => {
                if let (Some(l), Some(r)) = (&left, &right) {
                    let generic_ordering = match (l, r) {
                        (
                            TypeKind::Named {
                                name: left_name,
                                type_args: left_args,
                            },
                            TypeKind::Named {
                                name: right_name,
                                type_args: right_args,
                            },
                        ) if left_args.is_empty()
                            && right_args.is_empty()
                            && left_name == right_name =>
                        {
                            self.current_generic_params
                                .last()
                                .is_some_and(|params| params.contains(left_name))
                        }
                        _ => false,
                    };
                    let ordered = (Self::is_integer(l) && Self::is_integer(r))
                        || (Self::is_float(l) && Self::is_float(r))
                        || (is_string_type(l) && is_string_type(r))
                        || generic_ordering;
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
                Some(ConstValue::Int(a + b))
            }
            (BinOpKind::Sub, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(a - b))
            }
            (BinOpKind::Mul, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(a * b))
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
        if let TypeKind::Ref { inner } = &actual_resolved
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
        if let (
            TypeKind::Ref {
                inner: expected_inner,
            },
            TypeKind::Ref {
                inner: actual_inner,
            },
        ) = (&expected_resolved, &actual_resolved)
        {
            return self.types_have_same_runtime_shape(&expected_inner.node, &actual_inner.node);
        }
        if matches!(expected_resolved, TypeKind::Ref { .. })
            && !matches!(actual_resolved, TypeKind::Ref { .. } | TypeKind::Str)
        {
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
                    Some(TypeKind::Ref { .. }) => self.push_error(
                        target.span,
                        "S07",
                        "cannot assign through a shared reference".to_string(),
                    ),
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
                let object_eval = self.type_check_expr(object, true);
                if matches!(&object_eval.ty, Some(TypeKind::Bytes)) {
                    self.push_error(target.span, "S07", "byte strings are immutable".to_string());
                }
                if let Some(TypeKind::Named { name, type_args }) = &object_eval.ty
                    && name == "Array"
                {
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
                        if !self
                            .monomorphizations
                            .iter()
                            .any(|mono| mono.mangled_name == mangled)
                        {
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
                self.type_check_expr(target, true);
                self.push_error(target.span, "S07", "invalid assignment target".to_string());
            }
        }
    }

    fn lvalue_contains_shared_deref(&mut self, target: &Expr) -> bool {
        match &target.node {
            ExprKind::Group(inner) => self.lvalue_contains_shared_deref(inner),
            ExprKind::Field { object, .. } | ExprKind::Index { object, .. } => {
                self.lvalue_contains_shared_deref(object)
            }
            ExprKind::Unary {
                op: UnaryOpKind::Deref,
                expr: inner,
            } => matches!(
                self.type_check_expr(inner, true).ty,
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
            (TypeKind::Ref { inner: a }, TypeKind::Ref { inner: b }) => {
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
            (TypeKind::RawPtr { .. }, TypeKind::Ref { .. })
            | (TypeKind::Ref { .. }, TypeKind::RawPtr { .. }) => false,
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
        TypeKind::Ref { inner } | TypeKind::RawPtr { inner } => type_contains_error(&inner.node),
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
        TypeKind::Ref { inner } | TypeKind::RawPtr { inner } => type_contains_self(&inner.node),
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
fn is_string_view_type(ty: &TypeKind) -> bool {
    match ty {
        TypeKind::Str => true,
        TypeKind::Ref { inner } => is_string_view_type(&inner.node),
        _ => false,
    }
}

fn is_string_type(ty: &TypeKind) -> bool {
    is_string_view_type(ty)
        || matches!(ty, TypeKind::Named { name, .. } if name == "String" || name.ends_with(".String"))
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
        | (TypeKind::RawPtr { inner: field }, TypeKind::RawPtr { inner: value }) => {
            infer_struct_type_subst(&field.node, &value.node, generic_params, subst);
        }
        _ => {}
    }
}

fn addressable_ident(expr: &Expr) -> Option<&str> {
    match &expr.node {
        ExprKind::Ident(name) => Some(name),
        ExprKind::Group(inner) => addressable_ident(inner),
        _ => None,
    }
}

fn address_of_ident(expr: &Expr) -> Option<&str> {
    match &expr.node {
        ExprKind::Group(inner) => address_of_ident(inner),
        ExprKind::Unary {
            op: UnaryOpKind::Ref,
            expr: inner,
        } => addressable_ident(inner),
        _ => None,
    }
}

fn contains_non_string_reference(ty: &TypeKind) -> bool {
    match ty {
        TypeKind::Ref { inner } => !matches!(inner.node, TypeKind::Str),
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
        TypeKind::Ref { .. } => false,
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
                || expr_mutates_ident(place, target)
                || expr_mutates_ident(value, target)
        }
        ExprKind::IncDec { expr: inner, .. } => {
            expression_root_ident(inner) == Some(target) || expr_mutates_ident(inner, target)
        }
        ExprKind::MethodCall {
            object,
            args,
            named_args,
            ..
        } => {
            expression_root_ident(object) == Some(target)
                || expr_mutates_ident(object, target)
                || args
                    .iter()
                    .any(|argument| expr_mutates_ident(argument, target))
                || named_args
                    .iter()
                    .any(|(_, argument)| expr_mutates_ident(argument, target))
        }
        ExprKind::Group(inner)
        | ExprKind::Unary { expr: inner, .. }
        | ExprKind::Cast { expr: inner, .. }
        | ExprKind::Try { expr: inner } => expr_mutates_ident(inner, target),
        ExprKind::Binary { left, right, .. } => {
            expr_mutates_ident(left, target) || expr_mutates_ident(right, target)
        }
        ExprKind::Call {
            callee,
            args,
            named_args,
            ..
        } => {
            expr_mutates_ident(callee, target)
                || args
                    .iter()
                    .any(|argument| expr_mutates_ident(argument, target))
                || named_args
                    .iter()
                    .any(|(_, argument)| expr_mutates_ident(argument, target))
        }
        ExprKind::Field { object, .. } => expr_mutates_ident(object, target),
        ExprKind::StructInit { fields, .. } => fields
            .iter()
            .any(|(_, value)| expr_mutates_ident(value, target)),
        ExprKind::Match { scrutinee, arms } => {
            expr_mutates_ident(scrutinee, target)
                || arms.iter().any(|arm| {
                    !crate::parser::ast::pattern_all_bindings(&arm.pattern)
                        .iter()
                        .any(|binding| binding == target)
                        && (expr_mutates_ident(&arm.expr, target)
                            || arm
                                .guard
                                .as_ref()
                                .is_some_and(|guard| expr_mutates_ident(guard, target)))
                })
        }
        ExprKind::ArrayLit(elements) => elements
            .iter()
            .any(|element| expr_mutates_ident(element, target)),
        ExprKind::Index { object, indices } => {
            expr_mutates_ident(object, target)
                || indices
                    .iter()
                    .any(|index| expr_mutates_ident(index, target))
        }
        ExprKind::Closure { params, body } => {
            !params.iter().any(|parameter| parameter == target) && expr_mutates_ident(body, target)
        }
        ExprKind::Ident(_) | ExprKind::Literal(_) => false,
    }
}

fn collect_expr_identifiers(expr: &Expr, names: &mut Vec<String>) {
    match &expr.node {
        ExprKind::Ident(name) => names.push(name.clone()),
        ExprKind::Literal(_) => {}
        ExprKind::Group(inner)
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
        ExprKind::Closure { params, body } => {
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
            op: UnaryOpKind::Ref,
            expr: inner,
        } => addressable_ident(inner).is_some(),
        _ => false,
    }
}

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
