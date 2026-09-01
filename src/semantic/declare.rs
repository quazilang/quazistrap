// Quazi Programming Language
// Copyright (c) 2026 quazilang
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
                return_ty,
                params,
                attributes,
                pub_fn,
                unsafe_fn,
                generic_params,
                c_variadic,
                ..
            } => {
                let mut attr_names = extract_attribute_names(attributes);
                if *c_variadic {
                    attr_names.push("c_variadic".to_string());
                }
                // Foreign functions (@syscall, @api, @intrinsic) are library stubs —
                // suppress all unused warnings for them automatically.
                let is_foreign = attr_names
                    .iter()
                    .any(|a| matches!(a.as_str(), "syscall" | "api" | "intrinsic"));
                if is_foreign && !attr_names.contains(&"ignore".to_string()) {
                    attr_names.push("ignore".to_string());
                }
                // Exported functions are roots invoked by native consumers, so they are
                // never dead merely because no Quazi call site references them.
                if attr_names
                    .iter()
                    .any(|a| matches!(a.as_str(), "export" | "test"))
                    && !attr_names.contains(&"ignore".to_string())
                {
                    attr_names.push("ignore".to_string());
                }
                // `any` is only the erased call-site convention of an explicit
                // `@format` function; it is never a runtime parameter type.
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
                            && attributes
                                .iter()
                                .any(|attribute| attribute.name == "format")
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
                let has_export = attr_names.iter().any(|a| a == "export");
                // Internal runtime symbols (e.g. __quazi_panic_handler) keep their bare
                // names so the runtime stub can find them.
                let export_name = attributes
                    .iter()
                    .find(|a| a.name == "export")
                    .and_then(|a| {
                        a.args.first().and_then(|arg| match arg {
                            AttrArg::Positional(AttrVal::Str(symbol)) => Some(symbol.clone()),
                            _ => None,
                        })
                    });
                let register_name = if name.starts_with("__quazi_") || has_export {
                    name.clone()
                } else if let Some(module) = self.module_path_for_span(item.span) {
                    format!("{}.{}", module, name)
                } else {
                    name.clone()
                };
                if attr_names.iter().any(|attribute| attribute == "test")
                    && !self.is_library_span(item.span)
                {
                    self.test_functions.push(register_name.clone());
                }
                if attr_names.iter().any(|a| a == "export") {
                    self.exported_symbols.insert(
                        register_name.clone(),
                        export_name.unwrap_or_else(|| name.clone()),
                    );
                }
                self.fn_param_names.insert(
                    register_name.clone(),
                    params
                        .iter()
                        .filter(|p| p.name != "self")
                        .map(|p| p.name.clone())
                        .collect(),
                );
                self.declare(
                    register_name,
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
                is_union,
                generic_params,
                attributes,
                public,
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
                        public: *public,
                        unsafe_fn: false,
                        generic_params: vec![],
                    },
                );
                // Register field layout for codegen
                let field_defs: Vec<(String, TypeKind)> = fields
                    .iter()
                    .map(|field| (field.name.clone(), field.ty.node.clone()))
                    .collect();
                self.struct_defs.insert(name.clone(), field_defs);
                self.struct_field_bit_widths.insert(
                    name.clone(),
                    fields
                        .iter()
                        .map(|field| (field.name.clone(), field.bit_width))
                        .collect(),
                );
                if attributes.iter().any(|attr| {
                    attr.name == "repr"
                        && matches!(
                            attr.args.first(),
                            Some(AttrArg::Positional(AttrVal::Ident(value))) if value == "C"
                        )
                }) {
                    self.repr_c_structs.insert(name.clone());
                    if *is_union {
                        self.repr_c_unions.insert(name.clone());
                    }
                    for arg in attributes
                        .iter()
                        .find(|attr| attr.name == "repr")
                        .into_iter()
                        .flat_map(|attr| attr.args.iter().skip(1))
                    {
                        match arg {
                            AttrArg::Positional(AttrVal::Ident(value)) if value == "packed" => {
                                self.repr_c_packed.insert(name.clone());
                            }
                            AttrArg::KeyValue(key, AttrVal::Int(value))
                                if key == "align" && *value > 0 =>
                            {
                                self.repr_c_alignments.insert(name.clone(), *value as usize);
                            }
                            _ => {}
                        }
                    }
                    if fields.last().is_some_and(|field| {
                        matches!(field.ty.node, TypeKind::FlexibleArray { .. })
                    }) {
                        self.flexible_array_structs.insert(name.clone());
                    }
                }
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
                let mut serialization_traits: Vec<String> = Vec::new();
                for attribute in attributes.iter().filter(|attribute| attribute.name == "derive") {
                    for argument in &attribute.args {
                        let AttrArg::Positional(AttrVal::Ident(trait_name)) = argument else {
                            continue;
                        };
                        if trait_name != "Serialize" && trait_name != "Deserialize" {
                            continue;
                        }
                        if serialization_traits.contains(trait_name) {
                            self.push_error(
                                attribute.span,
                                "S06",
                                format!("duplicate serialization derive '{trait_name}'"),
                            );
                        } else {
                            serialization_traits.push(trait_name.clone());
                        }
                    }
                }
                let has_serialization_derive = !serialization_traits.is_empty();
                if has_serialization_derive && *is_union {
                    self.push_error(
                        item.span,
                        "S14",
                        "Serialize and Deserialize currently support structs, not unions".to_string(),
                    );
                }
                if has_serialization_derive && !generic_params.is_empty() {
                    self.push_error(
                        item.span,
                        "S14",
                        "Serialize and Deserialize currently do not support generic structs"
                            .to_string(),
                    );
                }
                let mut json_names: HashMap<String, String> = HashMap::new();
                let serialization_fields = fields
                    .iter()
                    .map(|field| {
                        let json_attributes: Vec<&Attribute> = field
                            .attributes
                            .iter()
                            .filter(|attribute| attribute.name == "json")
                            .collect();
                        let json_name = json_attributes.iter().find_map(|attribute| match attribute.args.as_slice() {
                            [AttrArg::KeyValue(key, AttrVal::Str(value))]
                                if key == "name" && !value.is_empty() => Some(value.clone()),
                            _ => None,
                        });

                        if !json_attributes.is_empty() && !has_serialization_derive {
                            self.push_error(
                                json_attributes[0].span,
                                "S06",
                                "@json is only valid on a field of a struct deriving Serialize or Deserialize"
                                    .to_string(),
                            );
                        }
                        if json_attributes.len() > 1 {
                            self.push_error(
                                json_attributes[1].span,
                                "S06",
                                format!("field '{}' has more than one @json attribute", field.name),
                            );
                        }
                        for attribute in &json_attributes {
                            if !matches!(
                                attribute.args.as_slice(),
                                [AttrArg::KeyValue(key, AttrVal::Str(value))]
                                    if key == "name" && !value.is_empty()
                            ) {
                                self.push_error(
                                    attribute.span,
                                    "S06",
                                    "@json must be exactly @json(name=\"non-empty JSON key\")"
                                        .to_string(),
                                );
                            }
                        }
                        let wire_name = json_name.clone().unwrap_or_else(|| field.name.clone());
                        if let Some(previous_field) = json_names.insert(wire_name.clone(), field.name.clone())
                        {
                            self.push_error(
                                json_attributes.first().map_or(field.ty.span, |attribute| attribute.span),
                                "S06",
                                format!(
                                    "JSON key '{}' is used by both '{}' and '{}'",
                                    wire_name, previous_field, field.name
                                ),
                            );
                        }

                        SerializationFieldMetadata {
                            name: field.name.clone(),
                            ty: field.ty.node.clone(),
                            json_name,
                            attributes: field
                                .attributes
                                .iter()
                                .map(derive_field_attribute)
                                .collect(),
                        }
                    })
                    .collect();
                if has_serialization_derive {
                    self.serialization_derives.insert(
                        name.clone(),
                        SerializationDeriveMetadata {
                            type_name: name.clone(),
                            requested_traits: serialization_traits,
                            generic_params: generic_params.clone(),
                            is_union: *is_union,
                            fields: serialization_fields,
                        },
                    );
                }
            }
            ItemKind::Trait {
                name,
                generic_params,
                methods,
                attributes,
                public,
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
                        public: *public,
                        unsafe_fn: false,
                        generic_params: generic_params.clone(),
                    },
                );
                // Record vtable slot order: method declaration order = slot index.
                let slots: Vec<String> = methods.iter().map(|m| m.name.clone()).collect();
                if !slots.is_empty() {
                    self.trait_method_slots.insert(name.clone(), slots);
                }
                let signatures = methods
                    .iter()
                    .map(|method| {
                        (
                            method.name.clone(),
                            TraitMethodSignature {
                                has_explicit_receiver: method
                                    .param_names
                                    .first()
                                    .is_some_and(|name| name == "self"),
                                generic_params: method.generic_params.clone(),
                                params: method
                                    .params
                                    .iter()
                                    .map(|param| param.node.clone())
                                    .collect(),
                                return_ty: method.return_ty.node.clone(),
                            },
                        )
                    })
                    .collect();
                self.trait_method_signatures
                    .insert(name.clone(), signatures);
            }
            ItemKind::Enum {
                name,
                variants,
                attributes,
                public,
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
                        public: *public,
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
                public,
            } => {
                let stored_type = if attributes.iter().any(|attribute| {
                    attribute.name == "repr"
                        && matches!(
                            attribute.args.first(),
                            Some(AttrArg::Positional(AttrVal::Ident(value))) if value == "C"
                        )
                }) {
                    match &aliased_type.node {
                        TypeKind::Fn { params, return_ty } => TypeKind::CFn {
                            params: params.clone(),
                            return_ty: return_ty.clone(),
                        },
                        other => other.clone(),
                    }
                } else {
                    aliased_type.node.clone()
                };
                self.declare(
                    name.clone(),
                    Symbol {
                        kind: SymbolKind::TypeName,
                        span: item.span,
                        ty: Some(stored_type.clone()),
                        params: vec![],
                        used: false,
                        initialized: true,
                        is_import: false,
                        import_path: None,
                        const_value: None,
                        variadic: false,
                        attributes: extract_attribute_names(attributes),
                        public: *public,
                        unsafe_fn: false,
                        generic_params: generic_params.clone(),
                    },
                );
                self.type_aliases
                    .insert(name.clone(), (generic_params.clone(), stored_type));
            }
            ItemKind::ForeignGlobal {
                name,
                ty,
                attributes,
                public,
            } => {
                let mut attr_names = extract_attribute_names(attributes);
                attr_names.push("foreign_global".to_string());
                if !attr_names.iter().any(|attribute| attribute == "ignore") {
                    attr_names.push("ignore".to_string());
                }
                let register_name = if let Some(module) = self.module_path_for_span(item.span) {
                    format!("{}.{}", module, name)
                } else {
                    name.clone()
                };
                let symbol = attributes
                    .iter()
                    .find(|attribute| attribute.name == "api")
                    .and_then(|attribute| attribute.args.first())
                    .and_then(|argument| match argument {
                        AttrArg::Positional(AttrVal::Str(symbol)) => Some(symbol.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| name.clone());
                self.foreign_globals.insert(
                    register_name.clone(),
                    ForeignGlobalInfo {
                        symbol,
                        ty: ty.node.clone(),
                    },
                );
                self.declare(
                    register_name,
                    Symbol {
                        kind: SymbolKind::Variable { mutable: true },
                        span: item.span,
                        ty: Some(ty.node.clone()),
                        params: vec![],
                        used: false,
                        initialized: true,
                        is_import: false,
                        import_path: None,
                        const_value: None,
                        variadic: false,
                        attributes: attr_names,
                        public: *public,
                        unsafe_fn: false,
                        generic_params: vec![],
                    },
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
                                    && attributes
                                        .iter()
                                        .any(|attribute| attribute.name == "format")
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
                let full = build_import_path(&import_path.path, name);
                let mangled = mangle_import_path(&full);
                self.declare_import_binding(name.clone(), full, mangled, span);
            }
            ImportItems::Aliased(name, alias) => {
                let full = build_import_path(&import_path.path, name);
                let mangled = mangle_import_path(&full);
                self.declare_import_binding(alias.clone(), full, mangled, span);
            }
            ImportItems::Multiple(names) => {
                for name in names {
                    let full = build_import_path(&import_path.path, name);
                    let mangled = mangle_import_path(&full);
                    self.declare_import_binding(name.clone(), full, mangled, span);
                }
            }
            ImportItems::All => {
                // Wildcard: bring library functions into scope as bare aliases when
                // there is no name collision. Each alias points to the module-qualified
                // target so codegen uses the real mangled name.
                let all: Vec<String> = self.library_fn_names.iter().cloned().collect();
                for mangled in all {
                    let leaf = mangled.rsplit('.').next().unwrap_or(&mangled).to_string();
                    if self.resolve_symbol(&leaf).is_some() {
                        continue;
                    }
                    if let Some(original) = self.resolve_symbol(&mangled) {
                        if !matches!(original.kind, SymbolKind::Function) {
                            continue;
                        }
                        self.explicitly_imported_fns
                            .entry(leaf.clone())
                            .or_default();
                        self.declare(
                            leaf,
                            Symbol {
                                kind: SymbolKind::Function,
                                ty: original.ty,
                                span,
                                params: original.params.clone(),
                                used: false,
                                initialized: true,
                                is_import: true,
                                import_path: Some(mangled),
                                const_value: None,
                                variadic: original.variadic,
                                attributes: original.attributes.clone(),
                                public: false,
                                unsafe_fn: original.unsafe_fn,
                                generic_params: original.generic_params.clone(),
                            },
                        );
                    }
                }
            }
        }
    }

    pub(super) fn declare_import_binding(
        &mut self,
        local_name: String,
        full_path: String,
        mangled: Option<String>,
        span: Span,
    ) {
        self.add_dependency_edge(DependencyKind::Import, "__program__", &full_path);

        // Short-circuit: local_name already names a library function in scope
        // (LSP pre-load or wildcard alias). Record the explicit import and return.
        if let Some(existing) = self.resolve_symbol(&local_name)
            && matches!(existing.kind, SymbolKind::Function)
            && (existing.is_import || self.library_fn_names.contains(&local_name))
        {
            if !existing.public && !existing.is_import {
                self.push_error(
                    span,
                    "S04",
                    format!("'{}' is private and cannot be imported", local_name),
                );
                return;
            }
            if let Some(existing_path) = self.explicitly_imported_fns.get(&local_name)
                && !existing_path.is_empty()
                && existing_path != &full_path
            {
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
            self.explicitly_imported_fns
                .insert(local_name.clone(), full_path.clone());
            return;
        }

        // Module-namespace import (e.g. `import bar`, `import std.core`): no mangled
        // item target; just register the local name as a module namespace symbol.
        let Some(mangled_target) = mangled else {
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
            return;
        };

        // Look up the actual mangled function symbol in the global table.
        // If namespacing is not in effect (e.g. LSP mode), fall back to the bare leaf name.
        let leaf = mangled_target
            .rsplit('.')
            .next()
            .unwrap_or(&mangled_target)
            .to_string();
        let original = self
            .resolve_symbol(&mangled_target)
            .or_else(|| self.resolve_symbol(&leaf));
        let Some(original) = original else {
            // Target doesn't exist yet — possibly a type/constant import or the file
            // hasn't been loaded. Fall back to a namespace variable.
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
            return;
        };

        if !matches!(original.kind, SymbolKind::Function) {
            if original
                .attributes
                .iter()
                .any(|attribute| attribute == "foreign_global")
            {
                if !original.public {
                    self.push_error(
                        span,
                        "S04",
                        format!("'{}' is private and cannot be imported", leaf),
                    );
                    return;
                }
                self.declare(
                    local_name,
                    Symbol {
                        kind: original.kind,
                        ty: original.ty,
                        span,
                        params: vec![],
                        used: false,
                        initialized: true,
                        is_import: true,
                        import_path: Some(full_path),
                        const_value: None,
                        variadic: false,
                        attributes: original.attributes,
                        public: false,
                        unsafe_fn: false,
                        generic_params: vec![],
                    },
                );
                return;
            }
            if matches!(original.kind, SymbolKind::TypeName) {
                if !original.public {
                    self.push_error(
                        span,
                        "S04",
                        format!("'{}' is private and cannot be imported", leaf),
                    );
                    return;
                }
                if local_name == leaf {
                    return;
                }
            }
            // Not a function import — treat as namespace/variable reference.
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
            return;
        }

        if !original.public {
            self.push_error(
                span,
                "S04",
                format!(
                    "'{}' is private and cannot be imported",
                    mangled_target.rsplit('.').next().unwrap_or(&mangled_target)
                ),
            );
            return;
        }

        // Conflict: a function with the same local name already exists in this module.
        if let Some(existing) = self.resolve_symbol(&local_name) {
            if matches!(existing.kind, SymbolKind::Function) && !existing.is_import {
                self.push_error(
                    span,
                    "S15",
                    format!(
                        "import '{}' conflicts with existing function definition; \
                         use 'import ... as ...' to alias",
                        local_name
                    ),
                );
                return;
            }
            if matches!(existing.kind, SymbolKind::Function) && existing.is_import {
                // Already imported from somewhere else — ambiguous unless same target.
                if let Some(existing_path) = self.explicitly_imported_fns.get(&local_name)
                    && !existing_path.is_empty()
                    && existing_path != &full_path
                {
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
        }

        self.explicitly_imported_fns
            .insert(local_name.clone(), full_path.clone());
        self.declare(
            local_name,
            Symbol {
                kind: SymbolKind::Function,
                ty: original.ty,
                span,
                params: original.params.clone(),
                used: false,
                initialized: true,
                is_import: true,
                import_path: Some(full_path),
                const_value: None,
                variadic: original.variadic,
                attributes: original.attributes.clone(),
                public: false,
                unsafe_fn: original.unsafe_fn,
                generic_params: original.generic_params.clone(),
            },
        );
    }
}

fn derive_field_attribute(attribute: &Attribute) -> DeriveFieldAttribute {
    DeriveFieldAttribute {
        name: attribute.name.clone(),
        args: attribute
            .args
            .iter()
            .map(|argument| match argument {
                AttrArg::Positional(value) => {
                    DeriveAttributeArgument::Positional(derive_attribute_value(value))
                }
                AttrArg::KeyValue(key, value) => DeriveAttributeArgument::KeyValue {
                    key: key.clone(),
                    value: derive_attribute_value(value),
                },
            })
            .collect(),
    }
}

fn derive_attribute_value(value: &AttrVal) -> DeriveAttributeValue {
    match value {
        AttrVal::Str(value) => DeriveAttributeValue::String(value.clone()),
        AttrVal::Int(value) => DeriveAttributeValue::Integer(*value),
        AttrVal::Ident(value) => DeriveAttributeValue::Identifier(value.clone()),
    }
}

/// Convert an import path to the mangled function name it references.
/// The file module is the second-to-last segment: `std.core.write` → `core.write`.
/// Single-segment paths are returned unchanged and treated as module namespaces.
pub(super) fn mangle_import_path(full_path: &str) -> Option<String> {
    let segments: Vec<&str> = full_path.split('.').collect();
    if segments.len() < 2 {
        return None;
    }
    let module = segments[segments.len() - 2];
    let item = segments[segments.len() - 1];
    Some(format!("{}.{}", module, item))
}

pub(super) fn build_import_path(prefix: &[String], leaf: &str) -> String {
    if prefix.is_empty() {
        leaf.to_string()
    } else {
        format!("{}.{}", prefix.join("."), leaf)
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
