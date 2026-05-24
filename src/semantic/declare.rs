// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::HashMap;

use crate::parser::ast::*;

use super::*;

impl Analyzer {
    pub(super) fn declare_top_level_item(&mut self, item: &Item) {
        // Skip items disabled by @cfg on this platform.
        let attrs = match &item.node {
            ItemKind::TypeAlias { attributes, .. }
            | ItemKind::Fn { attributes, .. }
            | ItemKind::Struct { attributes, .. }
            | ItemKind::Trait { attributes, .. }
            | ItemKind::Enum { attributes, .. } => Some(attributes),
            _ => None,
        };
        if let Some(attrs) = attrs {
            if !super::item_should_include(attrs) {
                return;
            }
        }
        match &item.node {
            ItemKind::Fn {
                name,
                return_ty,
                params,
                attributes,
                pub_fn,
                unsafe_fn,
                generic_params,
                ..
            } => {
                let mut attr_names = extract_attribute_names(attributes);
                // Foreign functions (@syscall, @api, @intrinsic) are library stubs —
                // suppress all unused warnings for them automatically.
                let is_foreign = attr_names
                    .iter()
                    .any(|a| matches!(a.as_str(), "syscall" | "api" | "intrinsic"));
                if is_foreign && !attr_names.contains(&"ignore".to_string()) {
                    attr_names.push("ignore".to_string());
                }
                // Str-variadic fns: codegen coerces variadic args to str at call sites.
                // Detected by: ...args: str/ref, OR ...args: any with a preceding str param
                // (the any+str convention is how format-style fns like println are written).
                let has_str_variadic_param = params
                    .last()
                    .map(|p| {
                        if !p.variadic {
                            return false;
                        }
                        if matches!(
                            &p.ty.node,
                            crate::parser::ast::TypeKind::Str
                                | crate::parser::ast::TypeKind::Ref { .. }
                        ) {
                            return true;
                        }
                        matches!(&p.ty.node, crate::parser::ast::TypeKind::Any)
                            && params.iter().filter(|q| !q.variadic).any(|q| {
                                matches!(
                                    &q.ty.node,
                                    crate::parser::ast::TypeKind::Str
                                        | crate::parser::ast::TypeKind::Ref { .. }
                                )
                            })
                    })
                    .unwrap_or(false);
                if has_str_variadic_param {
                    attr_names.push("str_variadic".to_string());
                }
                // @syscall and @api functions are implicitly unsafe — raw syscalls and FFI
                // bypass OS/runtime safety guarantees, so callers must use unsafe {}.
                let is_syscall_or_api = attr_names
                    .iter()
                    .any(|a| matches!(a.as_str(), "syscall" | "api"));
                self.fn_param_names.insert(
                    name.clone(),
                    params
                        .iter()
                        .filter(|p| p.name != "self")
                        .map(|p| p.name.clone())
                        .collect(),
                );
                self.declare(
                    name.clone(),
                    Symbol {
                        kind: SymbolKind::Function,
                        span: item.span,
                        ty: Some(unwrap_type(return_ty)),
                        params: params.iter().map(|p| unwrap_type(&p.ty)).collect(),
                        used: false,
                        initialized: true,
                        is_import: false,
                        import_path: None,
                        const_value: None,
                        variadic: params.last().map(|p| p.variadic).unwrap_or(false),
                        attributes: attr_names.clone(),
                        public: *pub_fn,
                        unsafe_fn: *unsafe_fn || is_syscall_or_api,
                        generic_params: generic_params.clone(),
                    },
                );
                // @panic_handler validation: must take exactly one PanicInfo or str param,
                // return ! or void.
                if attr_names.iter().any(|a| a == "panic_handler") {
                    let non_variadic: Vec<_> = params.iter().filter(|p| !p.variadic).collect();
                    if non_variadic.len() != 1 {
                        self.push_error(
                            item.span,
                            "S13",
                            format!(
                                "@panic_handler '{}' must take exactly one parameter (PanicInfo or str), found {}",
                                name,
                                non_variadic.len()
                            ),
                        );
                    } else {
                        let param_ty = unwrap_type(&non_variadic[0].ty);
                        let ok = matches!(
                            &param_ty,
                            TypeKind::Str | TypeKind::Ref { .. } | TypeKind::Named { .. }
                        );
                        if !ok {
                            self.push_error(
                                item.span,
                                "S13",
                                format!(
                                    "@panic_handler '{}' parameter must be PanicInfo or str, found {}",
                                    name, param_ty
                                ),
                            );
                        }
                    }
                    let ret = unwrap_type(return_ty);
                    if !matches!(ret, TypeKind::Never | TypeKind::Void) {
                        self.push_error(
                            item.span,
                            "S13",
                            format!(
                                "@panic_handler '{}' must return ! or void, found {}",
                                name, ret
                            ),
                        );
                    }
                }
            }
            ItemKind::Struct {
                name,
                fields,
                generic_params,
                attributes,
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
                        variadic: false,
                        attributes: extract_attribute_names(attributes),
                        public: false,
                        unsafe_fn: false,
                        generic_params: vec![],
                    },
                );
                // Register field layout for codegen
                let field_defs: Vec<(String, TypeKind)> = fields
                    .iter()
                    .map(|(fname, ftype, _)| (fname.clone(), ftype.node.clone()))
                    .collect();
                self.struct_defs.insert(name.clone(), field_defs);
                if !generic_params.is_empty() {
                    self.struct_generic_params
                        .insert(name.clone(), generic_params.clone());
                }

                // @derive — register derived traits
                let derives: Vec<String> = attributes
                    .iter()
                    .filter(|a| a.name == "derive")
                    .flat_map(|a| {
                        a.args.iter().filter_map(|arg| {
                            if let crate::parser::ast::AttrArg::Positional(
                                crate::parser::ast::AttrVal::Ident(s),
                            ) = arg
                            {
                                Some(s.clone())
                            } else {
                                None
                            }
                        })
                    })
                    .collect();
                if !derives.is_empty() {
                    self.derived_traits.insert(name.clone(), derives);
                }
            }
            ItemKind::Trait {
                name,
                methods,
                attributes,
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
                        variadic: false,
                        attributes: extract_attribute_names(attributes),
                        public: false,
                        unsafe_fn: false,
                        generic_params: vec![],
                    },
                );
                // Record vtable slot order: method declaration order = slot index.
                let slots: Vec<String> = methods.iter().map(|m| m.name.clone()).collect();
                if !slots.is_empty() {
                    self.trait_method_slots.insert(name.clone(), slots);
                }
            }
            ItemKind::Enum {
                name,
                variants,
                attributes,
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
                        variadic: false,
                        attributes: extract_attribute_names(attributes),
                        public: false,
                        unsafe_fn: false,
                        generic_params: vec![],
                    },
                );
                self.register_enum(name, variants, item.span);
            }
            ItemKind::TypeAlias {
                name,
                generic_params,
                aliased_type,
                attributes,
            } => {
                self.declare(
                    name.clone(),
                    Symbol {
                        kind: SymbolKind::TypeName,
                        span: item.span,
                        ty: Some(aliased_type.node.clone()),
                        params: vec![],
                        used: false,
                        initialized: true,
                        is_import: false,
                        import_path: None,
                        const_value: None,
                        variadic: false,
                        attributes: extract_attribute_names(attributes),
                        public: false,
                        unsafe_fn: false,
                        generic_params: generic_params.clone(),
                    },
                );
                self.type_aliases.insert(
                    name.clone(),
                    (generic_params.clone(), aliased_type.node.clone()),
                );
            }
            ItemKind::Import(import_path) => self.declare_import_item(import_path, item.span),
            ItemKind::Impl {
                for_ty,
                trait_ty,
                methods,
                ..
            } => {
                let type_name = type_kind_base_name(&for_ty.node);
                if let Some(t) = trait_ty {
                    let trait_name = type_kind_base_name(&t.node);
                    self.trait_impls
                        .entry(type_name.clone())
                        .or_default()
                        .insert(trait_name);
                }
                for method in methods {
                    if let ItemKind::Fn {
                        name,
                        return_ty,
                        params,
                        attributes,
                        unsafe_fn,
                        pub_fn,
                        generic_params,
                        ..
                    } = &method.node
                    {
                        if !super::item_should_include(attributes) {
                            continue;
                        }
                        let mangled = format!("{}.{}", type_name, name);
                        let mut attr_names = extract_attribute_names(attributes);
                        let is_foreign = attr_names
                            .iter()
                            .any(|a| matches!(a.as_str(), "syscall" | "api" | "intrinsic"));
                        if is_foreign && !attr_names.contains(&"ignore".to_string()) {
                            attr_names.push("ignore".to_string());
                        }
                        let has_str_variadic_param2 = params
                            .last()
                            .map(|p| {
                                if !p.variadic {
                                    return false;
                                }
                                if matches!(
                                    &p.ty.node,
                                    crate::parser::ast::TypeKind::Str
                                        | crate::parser::ast::TypeKind::Ref { .. }
                                ) {
                                    return true;
                                }
                                matches!(&p.ty.node, crate::parser::ast::TypeKind::Any)
                                    && params.iter().filter(|q| !q.variadic).any(|q| {
                                        matches!(
                                            &q.ty.node,
                                            crate::parser::ast::TypeKind::Str
                                                | crate::parser::ast::TypeKind::Ref { .. }
                                        )
                                    })
                            })
                            .unwrap_or(false);
                        if has_str_variadic_param2 {
                            attr_names.push("str_variadic".to_string());
                        }
                        let is_syscall_or_api = attr_names
                            .iter()
                            .any(|a| matches!(a.as_str(), "syscall" | "api"));
                        self.fn_param_names.insert(
                            mangled.clone(),
                            params
                                .iter()
                                .filter(|p| p.name != "self")
                                .map(|p| p.name.clone())
                                .collect(),
                        );
                        self.declare(
                            mangled,
                            Symbol {
                                kind: SymbolKind::Function,
                                span: method.span,
                                ty: Some(unwrap_type(return_ty)),
                                params: params.iter().map(|p| unwrap_type(&p.ty)).collect(),
                                used: false,
                                initialized: true,
                                is_import: false,
                                import_path: None,
                                const_value: None,
                                variadic: params.last().map(|p| p.variadic).unwrap_or(false),
                                attributes: attr_names,
                                public: *pub_fn,
                                unsafe_fn: *unsafe_fn || is_syscall_or_api,
                                generic_params: generic_params.clone(),
                            },
                        );
                    }
                }
            }
        }
    }

    pub(super) fn register_enum(&mut self, enum_name: &str, variants: &[EnumVariant], span: Span) {
        let mut map = HashMap::new();
        let mut order = Vec::new();
        let mut variant_fields: HashMap<String, Vec<TypeKind>> = HashMap::new();

        for variant in variants {
            let arity = variant.payload_types.len();
            if map.insert(variant.name.clone(), arity).is_some() {
                self.push_error(
                    variant.span,
                    "S05",
                    format!(
                        "duplicate enum variant '{}' in enum '{}'",
                        variant.name, enum_name
                    ),
                );
            } else {
                order.push(variant.name.clone());
                variant_fields.insert(
                    variant.name.clone(),
                    variant
                        .payload_types
                        .iter()
                        .map(|t| t.node.clone())
                        .collect(),
                );
            }
        }

        if variants.is_empty() {
            self.push_warning(span, "W06", format!("enum '{}' has no variants", enum_name));
        }

        self.enums.insert(
            enum_name.to_string(),
            EnumInfo {
                variants: map,
                variant_fields,
                order,
            },
        );
    }

    pub(super) fn declare_import_item(&mut self, import_path: &ImportPath, span: Span) {
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
            ImportItems::All => {
                // Wildcard: allow all library functions to be called unqualified.
                // Value = "" to suppress conflict detection for wildcard entries.
                let all: Vec<String> = self.library_fn_names.iter().cloned().collect();
                for name in all {
                    self.explicitly_imported_fns
                        .entry(name)
                        .or_insert_with(String::new);
                }
            }
        }
    }

    pub(super) fn declare_import_binding(
        &mut self,
        local_name: String,
        full_path: String,
        span: Span,
    ) {
        self.add_dependency_edge(DependencyKind::Import, "__program__", &full_path);
        // If the name is already declared as a function (loaded from a library file),
        // this is an explicit by-name import — record it and skip the redundant binding.
        if let Some(existing) = self.resolve_symbol(&local_name) {
            if matches!(existing.kind, SymbolKind::TypeName) {
                // TypeName already in scope (e.g. from a library file loaded via mod.void
                // pub-import). Re-importing is a no-op — mark the existing symbol used so
                // it doesn't trigger an unused-import warning.
                return;
            }
            if matches!(existing.kind, SymbolKind::Function) {
                if self.library_fn_names.contains(&local_name) {
                    if !existing.public {
                        self.push_error(
                            span,
                            "S04",
                            format!("'{}' is private and cannot be imported", local_name),
                        );
                        return;
                    }
                    // Conflict: same short name imported from two different modules.
                    if let Some(existing_path) = self.explicitly_imported_fns.get(&local_name) {
                        if !existing_path.is_empty() && existing_path != &full_path {
                            self.push_error(
                                span,
                                "S15",
                                format!(
                                    "ambiguous import '{}': already imported from '{}'; \
                                     use 'import {} as ...' to alias",
                                    local_name, existing_path, full_path
                                ),
                            );
                            return;
                        }
                    }
                    self.explicitly_imported_fns.insert(local_name, full_path);
                }
                return;
            }
        }
        // Alias import: the alias name didn't exist in scope. Check if full_path's
        // leaf resolves to a library function and register the alias as Function.
        let leaf = full_path
            .rsplit('.')
            .next()
            .unwrap_or(full_path.as_str())
            .to_string();
        if let Some(original) = self.resolve_symbol(&leaf) {
            if matches!(original.kind, SymbolKind::Function) {
                self.explicitly_imported_fns
                    .insert(local_name.clone(), full_path.clone());
                self.declare(
                    local_name,
                    Symbol {
                        kind: SymbolKind::Function,
                        ty: original.ty,
                        span,
                        params: original.params,
                        used: false,
                        initialized: true,
                        is_import: true,
                        import_path: Some(full_path),
                        const_value: None,
                        variadic: original.variadic,
                        attributes: original.attributes,
                        public: false,
                        unsafe_fn: original.unsafe_fn,
                        generic_params: original.generic_params,
                    },
                );
                return;
            }
        }
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
                variadic: false,
                attributes: Vec::new(),
                public: false,
                unsafe_fn: false,
                generic_params: vec![],
            },
        );
    }

    pub(super) fn build_import_path(prefix: &[String], leaf: &str) -> String {
        if prefix.is_empty() {
            leaf.to_string()
        } else {
            format!("{}.{}", prefix.join("."), leaf)
        }
    }
}

/// Returns the base type name for mangling (strips type arguments, unwraps references/pointers).
pub(super) fn type_kind_base_name(ty: &TypeKind) -> String {
    match ty {
        TypeKind::Named { name, .. } => name.clone(),
        TypeKind::Int8 => "i8".to_string(),
        TypeKind::Int16 => "i16".to_string(),
        TypeKind::Int32 => "i32".to_string(),
        TypeKind::Int64 => "i64".to_string(),
        TypeKind::Uint8 => "u8".to_string(),
        TypeKind::Uint16 => "u16".to_string(),
        TypeKind::Uint32 => "u32".to_string(),
        TypeKind::Uint64 => "u64".to_string(),
        TypeKind::Isize => "isize".to_string(),
        TypeKind::Usize => "usize".to_string(),
        TypeKind::Float16 => "f16".to_string(),
        TypeKind::Float32 => "f32".to_string(),
        TypeKind::Float64 => "f64".to_string(),
        TypeKind::Bool => "bool".to_string(),
        TypeKind::Str => "str".to_string(),
        TypeKind::Ref { inner } => type_kind_base_name(&inner.node),
        TypeKind::RawPtr { inner } => type_kind_base_name(&inner.node),
        other => format!("{}", other),
    }
}
