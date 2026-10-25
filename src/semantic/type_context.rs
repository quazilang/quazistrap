// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Establish defining-source identity before transparent alias expansion.
use super::*;

#[derive(Default)]
pub(super) struct SourceTypeContexts {
    sources: Vec<(usize, usize, HashMap<String, String>)>,
    implicit: HashMap<String, String>,
}

impl SourceTypeContexts {
    pub(super) fn canonical_name(&self, name: &str, span: Span) -> Option<String> {
        self.sources
            .partition_point(|(start, _, _)| *start <= span.start)
            .checked_sub(1)
            .and_then(|index| self.sources.get(index))
            .filter(|(_, end, _)| span.start < *end)
            .and_then(|(_, _, names)| names.get(name))
            .or_else(|| self.implicit.get(name))
            .cloned()
    }

    pub(super) fn resolve(
        &self,
        ty: &TypeKind,
        aliases: &HashMap<String, (Vec<String>, TypeKind)>,
        binders: &HashSet<String>,
        span: Span,
    ) -> Result<TypeKind, crate::type_resolution::AliasResolutionError> {
        crate::type_resolution::resolve_type(ty, aliases, &HashMap::new(), binders, |name| {
            self.canonical_name(name, span)
        })
    }
}

impl Analyzer {
    pub(super) fn source_type(&self, ty: &Type) -> TypeKind {
        self.source_type_at(&ty.node, ty.span)
    }

    pub(super) fn source_type_at(&self, ty: &TypeKind, span: Span) -> TypeKind {
        let binders = self
            .current_generic_params
            .iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        self.canonical_source_type(ty, &binders, span)
    }

    pub(super) fn canonical_source_type(
        &self,
        ty: &TypeKind,
        parameters: &[String],
        span: Span,
    ) -> TypeKind {
        let binders = parameters.iter().cloned().collect();
        let result = self
            .source_type_contexts
            .resolve(ty, &self.type_aliases, &binders, span);
        match result {
            Ok(ty) => ty,
            Err(error) => {
                self.alias_resolution_errors
                    .borrow_mut()
                    .entry(error.to_string())
                    .or_insert(span);
                TypeKind::Error
            }
        }
    }

    pub(super) fn canonicalize_source_aliases(
        &mut self,
        program: &Program,
        checkpoint: &mut dyn FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<(), crate::cancel::Cancelled> {
        // Only public selectors in the loader-selected prelude contribute to
        // the implicit environment. A module named `prelude` grants no role.
        let mut public_imports = Vec::new();
        for item in &program.items {
            checkpoint()?;
            match &item.node {
                ItemKind::Import(import)
                    if import.pub_import && item_should_include(&import.attributes) =>
                {
                    public_imports.push(item.span);
                }
                _ => {}
            }
        }
        public_imports.sort_unstable_by_key(|span| span.start);
        let source_paths: HashMap<_, _> = self
            .source_files
            .iter()
            .map(|source| (source.path.as_str(), source))
            .collect();
        if self
            .source_files
            .iter()
            .filter(|source| source.implicit_prelude)
            .count()
            > 1
        {
            self.push_error(
                Span::new(0, 0, 0, 0),
                "S14",
                "multiple implicit prelude source contexts".into(),
            );
            return Ok(());
        }
        let mut contexts = SourceTypeContexts::default();
        let mut work = 1_000_000usize;
        for source in &self.source_files {
            checkpoint()?;
            let mut bindings = HashMap::new();
            for import in &source.type_imports {
                checkpoint()?;
                if work == 0 {
                    self.push_error(
                        import.selector_span,
                        "S14",
                        "alias source context work budget exceeded".into(),
                    );
                    return Ok(());
                }
                work -= 1;
                if !source.contains(import.selector_span) || import.selector_span.end > source.end {
                    continue;
                }
                // Reuse authenticated declaration evidence, not a matching
                // leaf from untrusted SourceFile metadata.
                let Some(binding) =
                    self.type_name_bindings
                        .get(&import.source_name)
                        .filter(|binding| {
                            binding.name == import.target_name
                                && source_paths
                                    .get(import.target_path.as_str())
                                    .is_some_and(|target| target.contains(binding.span))
                        })
                else {
                    continue;
                };
                bindings.insert(import.source_name.clone(), binding.name.clone());
                if source.implicit_prelude
                    && public_imports
                        .partition_point(|span| span.start <= import.selector_span.start)
                        .checked_sub(1)
                        .and_then(|index| public_imports.get(index))
                        .is_some_and(|span| import.selector_span.end <= span.end)
                {
                    contexts
                        .implicit
                        .insert(import.source_name.clone(), binding.name.clone());
                }
            }
            contexts.sources.push((source.start, source.end, bindings));
        }
        contexts
            .sources
            .sort_unstable_by_key(|(start, _, _)| *start);
        self.source_type_contexts = contexts;
        // Resolve with an empty alias table: retain alias applications and their
        // own binders, while converting only source names to declaration names.
        // Forward aliases and recursive aliases are checked by normal expansion.
        let mut updates = Vec::new();
        let mut parameter_text = 16 * 1024 * 1024usize;
        for (item_index, item) in program.items.iter().enumerate() {
            checkpoint()?;
            let ItemKind::TypeAlias {
                name, attributes, ..
            } = &item.node
            else {
                continue;
            };
            if !item_should_include(attributes) {
                continue;
            }
            if work == 0 {
                self.push_error(
                    item.span,
                    "S14",
                    "alias source context work budget exceeded".into(),
                );
                return Ok(());
            }
            work -= 1;
            let Some((parameters, body)) = self.type_aliases.get(name) else {
                continue;
            };
            // Alpha-rename formals before removing import qualifiers. Otherwise
            // `type Pick[Record] = own.Record` would capture the nominal Record.
            // These keys cannot be spelled in source and contain no namespace dot.
            if parameters.len() > crate::type_resolution::MAX_TYPE_NODES
                || parameters.len() > work
                || parameters
                    .iter()
                    .try_fold(0usize, |sum, name| sum.checked_add(name.len()))
                    .is_none_or(|bytes| bytes > 1024 * 1024)
            {
                self.push_error(
                    item.span,
                    "S14",
                    "alias parameter count exceeds type-node limit".into(),
                );
                continue;
            }
            work -= parameters.len();
            let internal: Vec<_> = parameters
                .iter()
                .enumerate()
                .map(|(index, _)| format!("$alias_{}_{}_{}", item.span.start, item_index, index))
                .collect();
            // Account for both directions of the readable/private mapping and
            // its binder sets before materializing their owned strings.
            let copied_text =
                parameters
                    .iter()
                    .zip(&internal)
                    .try_fold(0usize, |sum, (source, key)| {
                        source
                            .len()
                            .checked_add(key.len())
                            .and_then(|bytes| bytes.checked_mul(4))
                            .and_then(|bytes| sum.checked_add(bytes))
                    });
            let Some(remaining) = copied_text.and_then(|bytes| parameter_text.checked_sub(bytes))
            else {
                self.push_error(
                    item.span,
                    "S14",
                    "alias parameter text exceeds aggregate work budget".into(),
                );
                return Ok(());
            };
            parameter_text = remaining;
            let substitutions = parameters
                .iter()
                .zip(&internal)
                .map(|(source, key)| {
                    (
                        source.clone(),
                        TypeKind::Named {
                            name: key.clone(),
                            type_args: Vec::new(),
                        },
                    )
                })
                .collect();
            let result = crate::type_resolution::resolve_type(
                body,
                &HashMap::new(),
                &substitutions,
                &internal.iter().cloned().collect(),
                |name| self.source_type_contexts.canonical_name(name, item.span),
            );
            // Symbol metadata remains readable; only the expansion template uses
            // private binder keys. The parsed declaration is never rewritten.
            let display = result.as_ref().ok().map(|body| {
                let substitutions = internal
                    .iter()
                    .zip(parameters)
                    .map(|(key, source)| {
                        (
                            key.clone(),
                            TypeKind::Named {
                                name: source.clone(),
                                type_args: Vec::new(),
                            },
                        )
                    })
                    .collect();
                crate::type_resolution::resolve_type(
                    body,
                    &HashMap::new(),
                    &substitutions,
                    &parameters.iter().cloned().collect(),
                    |_| None,
                )
            });
            updates.push((name.clone(), item.span, internal, result, display));
        }
        for (name, span, parameters, result, display) in updates {
            checkpoint()?;
            let body = match result {
                Ok(body) => body,
                Err(error) => {
                    self.push_error(span, "S14", error.to_string());
                    TypeKind::Error
                }
            };
            let display = match display {
                Some(Ok(body)) => body,
                Some(Err(error)) => {
                    self.push_error(span, "S14", error.to_string());
                    TypeKind::Error
                }
                None => TypeKind::Error,
            };
            if let Some((stored_parameters, stored)) = self.type_aliases.get_mut(&name) {
                *stored_parameters = parameters;
                *stored = body;
            }
            if let Some(symbol) = self
                .scopes
                .first_mut()
                .and_then(|scope| scope.get_mut(&name))
                && symbol.span == span
                && symbol.kind == SymbolKind::TypeName
            {
                symbol.ty = Some(display);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_file(source: &str) -> SourceFile {
        SourceFile {
            path: "source.qz".into(),
            module_name: None,
            implicit_prelude: false,
            runtime_string: None,
            type_imports: Vec::new(),
            start: 0,
            end: source.chars().count(),
            line_start: 1,
        }
    }

    #[test]
    fn hygienic_dynamic_alias_parameters_and_symbol_names() {
        let source = "trait View { fn view(self:&Self) i32; } type Erased[T] = dyn T;";
        let program = crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize())
            .parse()
            .unwrap();
        let mut analyzer = Analyzer::new();
        analyzer.set_source_files(vec![source_file(source)]);
        for item in &program.items {
            analyzer.declare_top_level_item(item);
        }
        analyzer
            .canonicalize_source_aliases(&program, &mut || Ok(()))
            .unwrap();
        let apply = |actual| TypeKind::Named {
            name: "Erased".into(),
            type_args: vec![Spanned::new(actual, Span::new(0, 0, 0, 0))],
        };
        let named = TypeKind::Named {
            name: "View".into(),
            type_args: Vec::new(),
        };
        assert_eq!(
            analyzer.resolve_type_aliases(&apply(named)).to_string(),
            "dyn View"
        );
        let symbol = analyzer.resolve_symbol("Erased").unwrap();
        assert_eq!(symbol.generic_params, vec!["T"]);
        assert_eq!(symbol.ty.unwrap().to_string(), "dyn T");
        assert!(matches!(
            analyzer.resolve_type_aliases(&apply(TypeKind::Int32)),
            TypeKind::Error
        ));
        assert!(
            analyzer
                .alias_resolution_errors
                .borrow()
                .keys()
                .any(|error| error.contains("named trait") && !error.contains("$alias"))
        );
    }

    #[test]
    fn bare_generic_alias_expression_requires_type_arguments() {
        let source = "type Identity[T] = T; fn main()void { var value = Identity; }";
        let program = crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize())
            .parse()
            .unwrap();
        let mut analyzer = Analyzer::new();
        analyzer.set_source_files(vec![source_file(source)]);
        let report = analyzer.analyze_program(&program);
        assert!(
            report.errors.iter().any(|error| error.code == "S14"
                && error.message.contains("Identity")
                && error.message.contains("expects 1")),
            "{:?}",
            report.errors
        );
        assert!(
            !report
                .errors
                .iter()
                .any(|error| error.message.contains("$alias"))
        );
    }

    #[test]
    fn alias_context_cancellation_is_operational() {
        let source = "type Keep = i32; fn main()void{}";
        let program = crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize())
            .parse()
            .unwrap();
        let mut analyzer = Analyzer::new();
        analyzer.set_source_files(vec![source_file(source)]);
        for item in &program.items {
            analyzer.declare_top_level_item(item);
        }
        let mut polls = 0;
        let result = analyzer.canonicalize_source_aliases(&program, &mut || {
            polls += 1;
            if polls == 3 {
                Err(crate::cancel::Cancelled)
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert!(analyzer.errors.is_empty());
    }

    #[test]
    fn multiple_implicit_preludes_are_rejected() {
        let source = "type Keep = i32;";
        let program = crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize())
            .parse()
            .unwrap();
        let mut file = source_file(source);
        file.implicit_prelude = true;
        let mut second = file.clone();
        second.path = "second.qz".into();
        let mut analyzer = Analyzer::new();
        analyzer.set_source_files(vec![file, second]);
        analyzer
            .canonicalize_source_aliases(&program, &mut || Ok(()))
            .unwrap();
        assert!(
            analyzer
                .errors
                .iter()
                .any(|error| error.code == "S14" && error.message.contains("multiple implicit"))
        );
    }

    #[test]
    fn selector_outside_its_source_cannot_grant_implicit_identity() {
        let source = "pub struct Record{value:i32,} pub import ./real.Record as Renamed; type Keep = Renamed;";
        let program = crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize())
            .parse()
            .unwrap();
        let mut file = source_file(source);
        file.implicit_prelude = true;
        file.type_imports.push(SourceTypeImport {
            selector_span: Span::new(1, 1, file.end + 1, file.end + 2),
            source_name: "Renamed".into(),
            target_path: file.path.clone(),
            target_name: "Record".into(),
        });
        let mut analyzer = Analyzer::new();
        analyzer.set_source_files(vec![file]);
        for item in &program.items {
            analyzer.declare_top_level_item(item);
        }
        analyzer.type_name_bindings.insert(
            "Renamed".into(),
            ResolvedBinding {
                name: "Record".into(),
                span: program.items[0].span,
                kind: SymbolKind::TypeName,
            },
        );
        analyzer
            .canonicalize_source_aliases(&program, &mut || Ok(()))
            .unwrap();
        assert_eq!(analyzer.type_aliases["Keep"].1.to_string(), "Renamed");
    }
}
