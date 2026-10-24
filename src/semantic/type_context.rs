// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Establish defining-source identity before transparent alias expansion.
use super::*;

impl Analyzer {
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
        let mut source_intervals: Vec<_> = self.source_files.iter().enumerate().collect();
        source_intervals.sort_unstable_by_key(|(_, source)| source.start);
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
        let mut implicit = HashMap::new();
        let mut by_source = Vec::new();
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
                bindings.insert(import.source_name.as_str(), binding.name.as_str());
                if source.implicit_prelude
                    && public_imports
                        .partition_point(|span| span.start <= import.selector_span.start)
                        .checked_sub(1)
                        .and_then(|index| public_imports.get(index))
                        .is_some_and(|span| import.selector_span.end <= span.end)
                {
                    implicit.insert(import.source_name.as_str(), binding.name.as_str());
                }
            }
            by_source.push(bindings);
        }
        // Resolve with an empty alias table: retain alias applications and their
        // own binders, while converting only source names to declaration names.
        // Forward aliases and recursive aliases are checked by normal expansion.
        let mut updates = Vec::new();
        for item in &program.items {
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
            let source = source_intervals
                .partition_point(|(_, source)| source.start <= item.span.start)
                .checked_sub(1)
                .and_then(|index| source_intervals.get(index))
                .and_then(|(index, source)| source.contains(item.span).then_some(*index));
            let result = crate::type_resolution::resolve_type(
                body,
                &HashMap::new(),
                &HashMap::new(),
                &parameters.iter().cloned().collect(),
                |name| {
                    source
                        .and_then(|index| by_source[index].get(name))
                        .or_else(|| implicit.get(name))
                        .map(|name| (*name).to_owned())
                },
            );
            updates.push((name.clone(), item.span, result));
        }
        for (name, span, result) in updates {
            checkpoint()?;
            let body = match result {
                Ok(body) => body,
                Err(error) => {
                    self.push_error(span, "S14", error.to_string());
                    TypeKind::Error
                }
            };
            if let Some((_, stored)) = self.type_aliases.get_mut(&name) {
                *stored = body.clone();
            }
            if let Some(symbol) = self
                .scopes
                .first_mut()
                .and_then(|scope| scope.get_mut(&name))
                && symbol.span == span
                && symbol.kind == SymbolKind::TypeName
            {
                symbol.ty = Some(body);
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
