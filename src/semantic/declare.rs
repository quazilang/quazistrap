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

    pub(super) fn register_enum(&mut self, enum_name: &str, variants: &[EnumVariant], span: Span) {
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
            ImportItems::All => {}
        }
    }

    pub(super) fn declare_import_binding(&mut self, local_name: String, full_path: String, span: Span) {
        self.add_dependency_edge(DependencyKind::Import, "__program__", &full_path);
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

    pub(super) fn build_import_path(prefix: &[String], leaf: &str) -> String {
        if prefix.is_empty() {
            leaf.to_string()
        } else {
            format!("{}.{}", prefix.join("."), leaf)
        }
    }
}
