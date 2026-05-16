// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::HashMap;

use crate::parser::ast::*;

use super::*;

impl Analyzer {
    pub(super) fn declare_top_level_item(&mut self, item: &Item) {
        match &item.node {
            ItemKind::Fn {
                name,
                return_ty,
                params,
                attributes,
                pub_fn,
                unsafe_fn,
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
                // @syscall and @api functions are implicitly unsafe — raw syscalls and FFI
                // bypass OS/runtime safety guarantees, so callers must use unsafe {}.
                let is_syscall_or_api = attr_names
                    .iter()
                    .any(|a| matches!(a.as_str(), "syscall" | "api"));
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
                    },
                );
                // @panic_handler validation: must take exactly one PanicInfo or str param,
                // return ! or void.
                if attr_names.iter().any(|a| a == "panic_handler") {
                    let non_variadic: Vec<_> =
                        params.iter().filter(|p| !p.variadic).collect();
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
                            TypeKind::Str
                                | TypeKind::Ref { .. }
                                | TypeKind::Named { .. }
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
                    },
                );
                // Register field layout for codegen
                let field_defs: Vec<(String, TypeKind)> = fields
                    .iter()
                    .map(|(fname, ftype, _)| (fname.clone(), ftype.node.clone()))
                    .collect();
                self.struct_defs.insert(name.clone(), field_defs);

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
                name, attributes, ..
            } => self.declare(
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
                },
            ),
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
                    },
                );
                self.register_enum(name, variants, item.span);
            }
            ItemKind::Import(import_path) => self.declare_import_item(import_path, item.span),
            ItemKind::Impl { for_ty, trait_ty, methods, .. } => {
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
                        ..
                    } = &method.node
                    {
                        let mangled = format!("{}.{}", type_name, name);
                        let mut attr_names = extract_attribute_names(attributes);
                        let is_foreign = attr_names
                            .iter()
                            .any(|a| matches!(a.as_str(), "syscall" | "api" | "intrinsic"));
                        if is_foreign && !attr_names.contains(&"ignore".to_string()) {
                            attr_names.push("ignore".to_string());
                        }
                        let is_syscall_or_api = attr_names
                            .iter()
                            .any(|a| matches!(a.as_str(), "syscall" | "api"));
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
            }
        }

        if variants.is_empty() {
            self.push_warning(span, "W06", format!("enum '{}' has no variants", enum_name));
        }

        self.enums
            .insert(enum_name.to_string(), EnumInfo { variants: map, order });
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
                let all: Vec<String> = self.library_fn_names.iter().cloned().collect();
                for name in all {
                    self.explicitly_imported_fns.insert(name);
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
                    self.explicitly_imported_fns.insert(local_name);
                }
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
