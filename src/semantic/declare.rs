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
                        attributes: attr_names,
                        public: *pub_fn,
                    },
                );
            }
            ItemKind::Struct {
                name, attributes, ..
            }
            | ItemKind::Trait {
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
                    },
                );
                self.register_enum(name, variants, item.span);
            }
            ItemKind::Import(import_path) => self.declare_import_item(import_path, item.span),
            ItemKind::Impl { .. } => {}
        }
    }

    pub(super) fn register_enum(&mut self, enum_name: &str, variants: &[EnumVariant], span: Span) {
        let mut map = HashMap::new();

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
            }
        }

        if variants.is_empty() {
            self.push_warning(span, "W06", format!("enum '{}' has no variants", enum_name));
        }

        self.enums
            .insert(enum_name.to_string(), EnumInfo { variants: map });
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
