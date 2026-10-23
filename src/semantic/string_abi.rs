// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Runtime string identity comes from the selected prelude, never its spelling.
use super::*;

impl Analyzer {
    fn display_supported(&self, ty: &TypeKind) -> bool {
        let scalar = |ty: &TypeKind| {
            Self::is_integer(ty) || Self::is_float(ty) || matches!(ty, TypeKind::Bool)
        };
        scalar(ty)
            || Self::arithmetic_string_load(ty).is_some()
            || self.is_checked_owned_string(ty)
            || matches!(ty,TypeKind::Ref{inner}|TypeKind::MutRef{inner} if scalar(&inner.node) || self.is_checked_owned_string(&inner.node))
    }

    pub(super) fn check_display_argument(&mut self, ty: Option<&TypeKind>, span: Span) {
        let Some(ty) = ty else {
            self.push_error(span, "S14", "format argument has no proven type".into());
            return;
        };
        let ty = self.resolve_type_aliases(ty);
        if self.display_supported(&ty) || matches!(ty, TypeKind::Error) {
            return;
        }
        let mut cursor = &ty;
        if let TypeKind::Ref { inner } | TypeKind::MutRef { inner } = cursor {
            cursor = &inner.node;
        }
        let deferred = matches!(cursor,TypeKind::Named{name,type_args} if type_args.is_empty() && self.current_generic_params.iter().any(|scope|scope.contains(name)));
        if deferred {
            if self.display_requirements.len() >= 65_536 {
                self.push_error(
                    span,
                    "S14",
                    "format requirement site budget exceeded".into(),
                );
            } else if let Some(callable) = self.current_function.last() {
                let bytes = callable.len().saturating_add(ty.to_string().len());
                if self.display_requirement_bytes.saturating_add(bytes) > 16 * 1024 * 1024 {
                    self.push_error(
                        span,
                        "S14",
                        "format requirement text budget exceeded".into(),
                    );
                } else {
                    self.display_requirement_bytes += bytes;
                    self.display_requirements.push((callable.clone(), span, ty));
                }
            } else {
                self.push_error(
                    span,
                    "S14",
                    "generic format argument has no declaration context".into(),
                );
            }
        } else {
            self.push_error(span,"S06",format!("format requires a numeric value, bool, string view or checked runtime String; convert `{ty}` explicitly"));
        }
    }

    pub(super) fn validate_display_specializations(
        &mut self,
        concrete: &BTreeSet<OwnershipCallableId>,
        checkpoint: &mut dyn FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<(), crate::cancel::Cancelled> {
        let mut work = 1_000_000usize;
        let mut seen = BTreeSet::new();
        let mut diagnostics = Vec::new();
        let mut by_callable: HashMap<&str, Vec<_>> = HashMap::new();
        for (callable, span, ty) in &self.display_requirements {
            checkpoint()?;
            by_callable.entry(callable).or_default().push((span, ty));
        }
        for mono in &self.monomorphizations {
            checkpoint()?;
            let id = self.ownership_callable_id(&mono.fn_name, &mono.type_args);
            if !concrete.contains(&id) || !seen.insert(id) {
                continue;
            }
            let Some(sites) = by_callable.get(mono.fn_name.as_str()) else {
                continue;
            };
            let names = self.generic_params_for_function(&mono.fn_name);
            let substitution: HashMap<_, _> = names
                .into_iter()
                .zip(mono.type_args.iter().cloned())
                .collect();
            for &(span, ty) in sites {
                checkpoint()?;
                if work == 0 {
                    diagnostics.push((
                        *span,
                        "S14",
                        "format specialization work budget exceeded".to_string(),
                    ));
                    break;
                }
                work -= 1;
                let result = crate::type_resolution::resolve_type(
                    ty,
                    &self.type_aliases,
                    &substitution,
                    &HashSet::new(),
                    |name| {
                        self.type_name_bindings
                            .get(name)
                            .map(|binding| binding.name.clone())
                    },
                );
                match result {
                    Ok(ty) if self.display_supported(&ty) => {},
                    Ok(ty) => diagnostics.push((*span,"S06",format!("specialized format argument `{ty}` has no checked string representation; convert it explicitly"))),
                    Err(error) => diagnostics.push((*span,"S14",error.to_string())),
                }
            }
        }
        for (span, code, message) in diagnostics {
            self.push_error(span, code, message);
        }
        Ok(())
    }

    pub(super) fn register_source_type_bindings(
        &mut self,
        program: &Program,
        checkpoint: &mut dyn FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<(), crate::cancel::Cancelled> {
        // The loader authenticates the complete selector path and public export
        // graph. Module basenames and globally matching leaves are insufficient.
        let mut work = 1_000_000usize;
        let mut imports = Vec::new();
        let mut declarations = HashMap::new();
        for item in &program.items {
            checkpoint()?;
            let name = match &item.node {
                ItemKind::Struct {
                    name, public: true, ..
                }
                | ItemKind::Enum {
                    name, public: true, ..
                }
                | ItemKind::Trait {
                    name, public: true, ..
                }
                | ItemKind::TypeAlias {
                    name, public: true, ..
                } => name,
                _ => continue,
            };
            if let Some(file) = self
                .source_files
                .iter()
                .find(|file| file.contains(item.span))
            {
                declarations.insert((file.path.clone(), name.clone()), item.span);
            }
        }
        for source in &self.source_files {
            for import in &source.type_imports {
                checkpoint()?;
                if work == 0 {
                    self.push_error(
                        Span::new(0, 0, 0, 0),
                        "S14",
                        "type binding work budget exceeded".into(),
                    );
                    return Ok(());
                }
                work -= 1;
                if let Some(&span) =
                    declarations.get(&(import.target_path.clone(), import.target_name.clone()))
                {
                    if self
                        .scopes
                        .first()
                        .and_then(|scope| scope.get(&import.target_name))
                        .is_some_and(|symbol| {
                            symbol.kind == SymbolKind::TypeName && symbol.span == span
                        })
                    {
                        imports.push((
                            import.clone(),
                            ResolvedBinding {
                                name: import.target_name.clone(),
                                span,
                                kind: SymbolKind::TypeName,
                            },
                        ));
                    }
                }
            }
        }
        let mut authenticated = HashSet::new();
        for (import, binding) in imports {
            checkpoint()?;
            let local = &import.source_name;
            let conflict = self
                .scopes
                .first()
                .and_then(|scope| scope.get(local))
                .is_some_and(|symbol| {
                    symbol.kind != SymbolKind::TypeName || symbol.span != binding.span
                })
                || self
                    .type_name_bindings
                    .get(local)
                    .is_some_and(|previous| previous != &binding);
            if conflict {
                self.push_error(
                    import.selector_span,
                    "S04",
                    format!("type import alias `{local}` conflicts with an existing declaration"),
                );
                continue;
            }
            self.type_name_bindings
                .insert(local.clone(), binding.clone());
            if let Some((_, _, Some(selector), alias, _)) =
                self.pending_type_imports
                    .iter()
                    .find(|(name, _, selector, _, _)| {
                        name == local && *selector == Some(import.selector_span)
                    })
            {
                authenticated.insert((local.clone(), selector.start, selector.end));
                self.binding_imports.push(BindingImport {
                    binding,
                    selector_span: *selector,
                    alias_span: *alias,
                    local_name: local.clone(),
                });
            }
        }
        for (local, target, selector, _, span) in std::mem::take(&mut self.pending_type_imports) {
            checkpoint()?;
            if selector.is_some_and(|selector| {
                authenticated.contains(&(local.clone(), selector.start, selector.end))
            }) {
                continue;
            }
            // Direct parser-only clients have no loader proof. They may use an
            // already declared bare type, but cannot mint another nominal name.
            if self.source_files.is_empty()
                && self
                    .scopes
                    .first()
                    .and_then(|scope| scope.get(&local))
                    .is_some_and(|symbol| symbol.kind == SymbolKind::TypeName)
            {
                continue;
            }
            self.push_error(
                span,
                "S04",
                format!("type import `{target}` has no declaration-backed export"),
            );
        }
        Ok(())
    }

    pub(super) fn validate_runtime_string(
        &mut self,
        program: &Program,
        checkpoint: &mut dyn FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<(), crate::cancel::Cancelled> {
        let selected: Vec<_> = self
            .source_files
            .iter()
            .filter_map(|file| {
                file.runtime_string
                    .as_ref()
                    .map(|name| (file.clone(), name.clone()))
            })
            .collect();
        if selected.len() > 1 {
            self.push_error(
                Span::new(0, 0, 0, 0),
                "S14",
                "multiple selected runtime String declarations".into(),
            );
            return Ok(());
        }
        let Some((file, selected_name)) = selected.into_iter().next() else {
            return Ok(());
        };
        let mut declaration = None;
        for item in &program.items {
            checkpoint()?;
            if !file.contains(item.span) {
                continue;
            }
            let ItemKind::Struct {
                name,
                fields,
                generic_params,
                public,
                is_union,
                ..
            } = &item.node
            else {
                continue;
            };
            if name != &selected_name {
                continue;
            }
            let symbol = self.scopes.first().and_then(|scope| scope.get(name));
            let data = fields
                .iter()
                .enumerate()
                .find(|(_, field)| field.name == "data");
            let valid = *public
                && !*is_union
                && generic_params.is_empty()
                && fields
                    .len()
                    .checked_mul(8)
                    .is_some_and(|size| size <= u16::MAX as usize)
                && fields.iter().filter(|field| field.name == "data").count() == 1
                && !self.repr_c_structs.contains(name)
                && symbol.is_some_and(|symbol| {
                    symbol.kind == SymbolKind::TypeName && symbol.span == item.span
                })
                && data.is_some_and(|(_, field)| {
                    matches!(self.resolve_type_aliases(&field.ty.node), TypeKind::Str)
                });
            if valid
                && let Some((index, _)) = data
                && let Some(offset) = index
                    .checked_mul(8)
                    .and_then(|offset| u16::try_from(offset).ok())
            {
                declaration = Some(CheckedStringAbi {
                    declaration: ResolvedBinding {
                        name: name.clone(),
                        span: item.span,
                        kind: SymbolKind::TypeName,
                    },
                    data_offset: offset,
                });
            }
            break;
        }
        if declaration.is_none() {
            self.push_error(Span::new(file.start,file.start, file.line_start,1), "S14",
                format!("selected runtime String `{selected_name}` must be a public nongeneric ordinary struct with a str data field and representable layout"));
        }
        self.checked_string_abi = declaration;
        Ok(())
    }

    pub(super) fn is_checked_owned_string(&self, ty: &TypeKind) -> bool {
        if let TypeKind::Named { name, .. } = ty
            && self
                .current_generic_params
                .iter()
                .any(|scope| scope.contains(name))
        {
            return false;
        }
        self.checked_string_abi
            .as_ref()
            .is_some_and(|abi| abi.matches_type(ty))
    }

    pub(super) fn is_checked_string(&self, ty: &TypeKind) -> bool {
        super::typecheck::is_string_view_type(ty) || self.is_checked_owned_string(ty)
    }

    pub(super) fn checked_string_operand_type(&self, ty: TypeKind) -> TypeKind {
        match ty {
            TypeKind::Ref { ref inner } | TypeKind::MutRef { ref inner }
                if self.is_checked_owned_string(&inner.node) =>
            {
                inner.node.clone()
            }
            other => Self::autoderef_type(other),
        }
    }
}
