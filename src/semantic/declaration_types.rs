// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Normalize stored declaration records before deferred layout and ownership use.
use super::*;

struct DeclarationWork {
    sites: usize,
    nodes: usize,
    text: usize,
    exhausted: bool,
}

impl DeclarationWork {
    fn new() -> Self {
        Self {
            sites: 1_000_000,
            nodes: 1_000_000,
            text: 16 * 1024 * 1024,
            exhausted: false,
        }
    }
    fn sites(&mut self, count: usize) -> bool {
        if self.exhausted || count > self.sites {
            return self.exhaust();
        }
        self.sites -= count;
        true
    }
    fn text(&mut self, bytes: usize) -> bool {
        if self.exhausted || bytes > self.text {
            return self.exhaust();
        }
        self.text -= bytes;
        true
    }
    fn exhaust(&mut self) -> bool {
        self.exhausted = true;
        false
    }
    fn ty(&mut self, ty: &TypeKind) -> bool {
        if self.exhausted {
            return self.exhaust();
        }
        let mut stack = vec![ty];
        while let Some(ty) = stack.pop() {
            if self.nodes == 0 {
                return self.exhaust();
            }
            self.nodes -= 1;
            let bytes = match ty {
                TypeKind::Named { name, .. } => name.len(),
                TypeKind::Dyn { trait_name } => trait_name.len(),
                _ => 0,
            };
            if bytes > self.text {
                return self.exhaust();
            }
            self.text -= bytes;
            match ty {
                TypeKind::Named { type_args, .. } => {
                    if type_args.len().saturating_add(stack.len()) > self.nodes {
                        return self.exhaust();
                    }
                    stack.extend(type_args.iter().map(|ty| &ty.node));
                }
                TypeKind::Fn { params, return_ty } | TypeKind::CFn { params, return_ty } => {
                    if params.len().saturating_add(1).saturating_add(stack.len()) > self.nodes {
                        return self.exhaust();
                    }
                    stack.extend(params.iter().map(|ty| &ty.node));
                    stack.push(&return_ty.node);
                }
                TypeKind::Ref { inner }
                | TypeKind::MutRef { inner }
                | TypeKind::RawPtr { inner } => stack.push(&inner.node),
                TypeKind::Array { elem_ty, .. }
                | TypeKind::Slice { elem_ty }
                | TypeKind::FlexibleArray { elem_ty } => stack.push(&elem_ty.node),
                _ => {}
            }
        }
        true
    }
}

impl Analyzer {
    pub(super) fn canonicalize_declaration_types(
        &mut self,
        program: &Program,
        checkpoint: &mut dyn FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<(), crate::cancel::Cancelled> {
        // Imported copies carry an import-site span. Resolve original records
        // first, then refresh copies from their target's canonical signature.
        let context = &self.source_type_contexts;
        let aliases = &self.type_aliases;
        let errors = &self.alias_resolution_errors;
        let work = std::cell::RefCell::new(DeclarationWork::new());
        let failure = |span| {
            errors
                .borrow_mut()
                .entry("declaration type normalization exceeds aggregate work budget".into())
                .or_insert(span);
        };
        let claim = |count, span| {
            if work.borrow_mut().sites(count) {
                true
            } else {
                failure(span);
                false
            }
        };
        let claim_binders = |parameters: &[String], span| {
            let mut budget = work.borrow_mut();
            if !budget.sites(parameters.len()) {
                drop(budget);
                failure(span);
                return false;
            }
            for name in parameters {
                if !budget.text(name.len()) {
                    drop(budget);
                    failure(span);
                    return false;
                }
            }
            true
        };
        let keep = |ty: TypeKind, span| {
            if work.borrow_mut().ty(&ty) {
                ty
            } else {
                failure(span);
                TypeKind::Error
            }
        };
        let resolve = |ty: &TypeKind, parameters: &[String], span| {
            if work.borrow().exhausted {
                return TypeKind::Error;
            }
            if !claim_binders(parameters, span) {
                return TypeKind::Error;
            }
            let binders = parameters.iter().cloned().collect();
            let result = context.resolve(ty, aliases, &binders, span);
            match result {
                Ok(ty) => keep(ty, span),
                Err(error) => {
                    errors.borrow_mut().entry(error.to_string()).or_insert(span);
                    TypeKind::Error
                }
            }
        };
        if !claim(
            program
                .items
                .len()
                .saturating_add(self.scopes.first().map_or(0, |scope| scope.len()))
                .saturating_add(self.binding_imports.len()),
            Span::new(0, 0, 0, 0),
        ) {
            return Ok(());
        }
        let mut definitions = HashSet::new();
        for item in &program.items {
            checkpoint()?;
            match &item.node {
                ItemKind::Fn { .. } | ItemKind::ForeignGlobal { .. } => {
                    definitions.insert((item.span.start, item.span.end));
                }
                ItemKind::Impl { methods, .. } => {
                    if !claim(methods.len(), item.span) {
                        return Ok(());
                    }
                    for method in methods {
                        checkpoint()?;
                        definitions.insert((method.span.start, method.span.end));
                    }
                }
                _ => {}
            }
        }
        let mut signatures = Vec::new();
        for (name, symbol) in self
            .scopes
            .first()
            .into_iter()
            .flat_map(|scope| scope.iter())
        {
            checkpoint()?;
            if symbol.is_import
                || !definitions.contains(&(symbol.span.start, symbol.span.end))
                || !(symbol.kind == SymbolKind::Function || self.foreign_globals.contains_key(name))
            {
                continue;
            }
            if !claim_binders(&symbol.generic_params, symbol.span)
                || !claim_binders(
                    self.impl_generic_params
                        .get(name)
                        .map_or(&[][..], Vec::as_slice),
                    symbol.span,
                )
            {
                return Ok(());
            }
            let mut binders = symbol.generic_params.clone();
            binders.extend(
                self.impl_generic_params
                    .get(name)
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
            if !claim(
                symbol.params.len().saturating_add(binders.len()),
                symbol.span,
            ) {
                return Ok(());
            }
            let mut params = Vec::new();
            for ty in &symbol.params {
                checkpoint()?;
                params.push(resolve(ty, &binders, symbol.span));
            }
            let result = symbol
                .ty
                .as_ref()
                .map(|ty| resolve(ty, &binders, symbol.span));
            signatures.push((name.clone(), params, result));
        }
        for (name, params, ty) in signatures {
            checkpoint()?;
            if let Some(global) = self.foreign_globals.get_mut(&name)
                && let Some(ty) = &ty
            {
                global.ty = ty.clone();
            }
            if let Some(symbol) = self
                .scopes
                .first_mut()
                .and_then(|scope| scope.get_mut(&name))
            {
                symbol.params = params;
                symbol.ty = ty;
            }
        }
        let mut imports = Vec::new();
        let import_bindings: HashMap<_, _> = self
            .binding_imports
            .iter()
            .map(|import| (import.local_name.as_str(), import))
            .collect();
        for (name, symbol) in self
            .scopes
            .first()
            .into_iter()
            .flat_map(|scope| scope.iter())
        {
            checkpoint()?;
            if !symbol.is_import {
                continue;
            }
            let Some(path) = symbol.import_path.as_deref() else {
                continue;
            };
            let proof = import_bindings.get(name.as_str()).filter(|import| {
                symbol.span.start <= import.selector_span.start
                    && import.selector_span.end <= symbol.span.end
            });
            let target = proof
                .map(|import| import.binding.name.clone())
                .unwrap_or_else(|| {
                    super::declare::mangle_import_path(path).unwrap_or_else(|| path.to_owned())
                });
            let Some(original) = self.scopes.first().and_then(|scope| scope.get(&target)) else {
                continue;
            };
            if !original.is_import
                && original.kind == symbol.kind
                && proof.is_none_or(|import| import.binding.span == original.span)
            {
                if !claim(original.params.len(), symbol.span) {
                    return Ok(());
                }
                let mut params = Vec::new();
                for ty in &original.params {
                    checkpoint()?;
                    if !work.borrow_mut().ty(ty) {
                        failure(symbol.span);
                        return Ok(());
                    }
                    params.push(ty.clone());
                }
                let ty = original.ty.as_ref().map(|ty| {
                    if work.borrow_mut().ty(ty) {
                        ty.clone()
                    } else {
                        failure(symbol.span);
                        TypeKind::Error
                    }
                });
                imports.push((name.clone(), params, ty));
            }
        }
        for (name, params, ty) in imports {
            checkpoint()?;
            if let Some(symbol) = self
                .scopes
                .first_mut()
                .and_then(|scope| scope.get_mut(&name))
            {
                symbol.params = params;
                symbol.ty = ty;
            }
        }
        for item in &program.items {
            checkpoint()?;
            match &item.node {
                ItemKind::Struct {
                    name,
                    generic_params,
                    attributes,
                    ..
                } if item_should_include(attributes) => {
                    if let Some(fields) = self.struct_defs.get(name) {
                        if !claim(fields.len(), item.span) {
                            return Ok(());
                        }
                        let mut canonical = Vec::new();
                        for (field, ty) in fields {
                            checkpoint()?;
                            canonical.push((field.clone(), resolve(ty, generic_params, item.span)));
                        }
                        self.struct_defs.insert(name.clone(), canonical);
                    }
                    if let Some(derive) = self.serialization_derives.get(name) {
                        if !claim(derive.fields.len(), item.span) {
                            return Ok(());
                        }
                        let mut canonical = Vec::new();
                        for field in &derive.fields {
                            checkpoint()?;
                            canonical.push(resolve(&field.ty, generic_params, field.span));
                        }
                        if let Some(derive) = self.serialization_derives.get_mut(name) {
                            for (field, ty) in derive.fields.iter_mut().zip(canonical) {
                                field.ty = ty;
                            }
                        }
                    }
                }
                ItemKind::Enum {
                    name,
                    generic_params,
                    attributes,
                    ..
                } if item_should_include(attributes) => {
                    if let Some(info) = self.enums.get(name) {
                        if !claim(info.variant_fields.len(), item.span) {
                            return Ok(());
                        }
                        let mut variants = HashMap::new();
                        for (variant, fields) in &info.variant_fields {
                            checkpoint()?;
                            if !claim(fields.len(), item.span) {
                                return Ok(());
                            }
                            let mut canonical = Vec::new();
                            for ty in fields {
                                checkpoint()?;
                                canonical.push(resolve(ty, generic_params, item.span));
                            }
                            variants.insert(variant.clone(), canonical);
                        }
                        if let Some(info) = self.enums.get_mut(name) {
                            info.variant_fields = variants;
                        }
                    }
                }
                ItemKind::Trait {
                    name,
                    generic_params,
                    attributes,
                    ..
                } if item_should_include(attributes) => {
                    if let Some(methods) = self.trait_method_signatures.get(name) {
                        if !claim(methods.len(), item.span) {
                            return Ok(());
                        }
                        let mut canonical = HashMap::new();
                        for (method, signature) in methods {
                            checkpoint()?;
                            if !claim(signature.params.len(), item.span)
                                || !claim_binders(generic_params, item.span)
                                || !claim_binders(&signature.generic_params, item.span)
                            {
                                return Ok(());
                            }
                            let mut binders = generic_params.clone();
                            binders.extend(signature.generic_params.iter().cloned());
                            binders.push("Self".into());
                            let mut params = Vec::new();
                            for ty in &signature.params {
                                checkpoint()?;
                                params.push(resolve(ty, &binders, item.span));
                            }
                            canonical.insert(
                                method.clone(),
                                TraitMethodSignature {
                                    has_explicit_receiver: signature.has_explicit_receiver,
                                    generic_params: signature.generic_params.clone(),
                                    params,
                                    return_ty: resolve(&signature.return_ty, &binders, item.span),
                                },
                            );
                        }
                        self.trait_method_signatures.insert(name.clone(), canonical);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declaration_records_and_imported_signatures_keep_defining_context() {
        let root = std::env::temp_dir().join(format!(
            "quazi_declaration_context_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("real.qz"),
            "pub type Word = i32; pub struct Record { value:i32, }",
        )
        .unwrap();
        std::fs::write(
            root.join("helper.qz"),
            r#"
import ./real as own;
import ./real.Word as T;
pub fn roundtrip(value:own.Word) own.Word { ret value; }
pub fn identity[T](value:T) T { ret value; }
pub fn collision[Word](value:own.Word) own.Word { ret value; }
pub fn word_identity[Word](value:Word) Word { ret value; }
pub type Nominal[Record] = own.Record;
pub type Concrete[Word] = own.Word;
pub type Keep[Word] = Word;
pub type Nested[Word] = Keep[own.Word];
pub struct Holder { value:own.Record, }
pub struct Generic[T] { value:T, }
pub enum Outcome { Value(own.Record), }
pub trait Convert { fn convert(self:&Self, value:own.Word) own.Word; }
@api("ffi_count") pub var counter:own.Word;
"#,
        )
        .unwrap();
        let main = root.join("main.qz");
        std::fs::write(&main, "import ./helper.roundtrip as invoke; import ./helper.counter as remote; import ./helper.*; fn main()i32{ret invoke(7);}").unwrap();
        let loaded = crate::loader::load_programs_configured(&[main], None, false, &[]).unwrap();
        assert!(loaded.parse_error.is_none(), "{:?}", loaded.parse_error);
        let unchanged = format!("{:?}", loaded.program);
        let mut analyzer = Analyzer::new();
        analyzer.set_source_files(loaded.source_files);
        analyzer.set_library_fns(loaded.library_fn_names);
        analyzer.set_library_char_ranges(loaded.library_char_ranges);
        analyzer.set_namespaced_paths(
            loaded
                .namespaced_paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        );
        for item in &loaded.program.items {
            analyzer.declare_top_level_item(item);
        }
        analyzer
            .register_source_type_bindings(&loaded.program, &mut || Ok(()))
            .unwrap();
        analyzer
            .canonicalize_source_aliases(&loaded.program, &mut || Ok(()))
            .unwrap();
        analyzer
            .canonicalize_declaration_types(&loaded.program, &mut || Ok(()))
            .unwrap();
        assert!(analyzer.errors.is_empty(), "{:?}", analyzer.errors);
        for (name, expected) in [
            ("Nominal", "Record"),
            ("Concrete", "i32"),
            ("Keep", "u64"),
            ("Nested", "i32"),
        ] {
            let application = TypeKind::Named {
                name: name.into(),
                type_args: vec![Spanned::new(TypeKind::Uint64, Span::new(0, 0, 0, 0))],
            };
            assert_eq!(
                analyzer.resolve_type_aliases(&application).to_string(),
                expected,
                "{name}"
            );
        }
        let keep = analyzer.resolve_symbol("Keep").unwrap();
        assert_eq!(keep.generic_params, vec!["Word"]);
        assert_eq!(keep.ty.unwrap().to_string(), "Word");
        for name in ["helper.roundtrip", "invoke", "roundtrip"] {
            let symbol = analyzer.resolve_symbol(name).unwrap();
            assert_eq!(symbol.params[0].to_string(), "i32", "{name}");
            assert_eq!(symbol.ty.unwrap().to_string(), "i32", "{name}");
        }
        for name in ["helper.collision", "collision"] {
            let symbol = analyzer.resolve_symbol(name).unwrap();
            assert_eq!(symbol.params[0].to_string(), "i32");
            assert_eq!(symbol.ty.unwrap().to_string(), "i32");
        }
        let identity = analyzer.resolve_symbol("helper.word_identity").unwrap();
        assert_eq!(identity.params[0].to_string(), "Word");
        assert_eq!(identity.ty.unwrap().to_string(), "Word");
        analyzer.current_generic_params.push(vec!["Word".into()]);
        let callee = analyzer.resolve_symbol("invoke").unwrap();
        assert!(matches!(
            analyzer.resolve_type_aliases(&callee.params[0]),
            TypeKind::Int32
        ));
        assert!(matches!(
            analyzer.resolve_type_aliases(callee.ty.as_ref().unwrap()),
            TypeKind::Int32
        ));
        analyzer.current_generic_params.pop();
        for name in ["helper.counter", "remote"] {
            assert_eq!(
                analyzer
                    .resolve_symbol(name)
                    .unwrap()
                    .ty
                    .unwrap()
                    .to_string(),
                "i32"
            );
        }
        assert_eq!(
            analyzer.foreign_globals["helper.counter"].ty.to_string(),
            "i32"
        );
        assert_eq!(analyzer.struct_defs["Holder"][0].1.to_string(), "Record");
        assert_eq!(analyzer.struct_defs["Generic"][0].1.to_string(), "T");
        assert_eq!(
            analyzer.enums["Outcome"].variant_fields["Value"][0].to_string(),
            "Record"
        );
        let method = &analyzer.trait_method_signatures["Convert"]["convert"];
        assert_eq!(method.params[0].to_string(), "&Self");
        assert_eq!(method.params[1].to_string(), "i32");
        assert_eq!(method.return_ty.to_string(), "i32");
        let report = analyzer.analyze_program(&loaded.program);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(format!("{:?}", loaded.program), unchanged);
        let parameter = report
            .symbol_table
            .entries
            .iter()
            .find(|entry| {
                entry.name == "value"
                    && entry.symbol.kind == SymbolKind::Parameter
                    && entry
                        .symbol
                        .ty
                        .as_ref()
                        .is_some_and(|ty| ty.to_string() == "i32")
            })
            .unwrap();
        assert_eq!(parameter.symbol.ty.as_ref().unwrap().to_string(), "i32");
        analyzer.set_namespaced_paths(HashSet::new());
        analyzer.set_library_fns(HashSet::new());
        let report = analyzer.analyze_program(&loaded.program);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let imported = report
            .symbol_table
            .entries
            .iter()
            .find(|entry| entry.name == "invoke")
            .unwrap();
        assert_eq!(imported.symbol.params[0].to_string(), "i32");
        let remote = report
            .symbol_table
            .entries
            .iter()
            .find(|entry| entry.name == "remote")
            .unwrap();
        assert_eq!(remote.symbol.ty.as_ref().unwrap().to_string(), "i32");

        // Method binders must stay protected through the final shape comparison,
        // even though a private import maps the same spelling T to Word.
        let helper = root.join("helper.qz");
        let mut text = std::fs::read_to_string(&helper).unwrap();
        text.push_str("pub struct Bad {} pub trait Wrong { fn accept[T](self:&Self,value:own.Word)own.Word; } impl Wrong for Bad { fn accept[T](self:&Bad,value:T)own.Word{ret 0;} }");
        std::fs::write(&helper, text).unwrap();
        let loaded =
            crate::loader::load_programs_configured(&[root.join("main.qz")], None, false, &[])
                .unwrap();
        assert!(loaded.parse_error.is_none(), "{:?}", loaded.parse_error);
        let report = crate::analysis::analyze_program_with_source_files(
            &loaded.merged_source,
            &loaded.program,
            loaded.library_fn_names,
            loaded.library_char_ranges,
            loaded.source_files,
            loaded
                .namespaced_paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        );
        assert!(
            report.errors.iter().any(|error| error.code == "S14"
                && error.message.contains("parameter 2 must")
                && error.message.contains("got `T`")),
            "{:?}",
            report.errors
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn declaration_work_bounds_nodes_text_and_sites_before_copies() {
        let ty = TypeKind::Named {
            name: "Record".into(),
            type_args: vec![Spanned::new(TypeKind::Int32, Span::new(0, 0, 0, 0))],
        };
        let mut work = DeclarationWork {
            sites: 1,
            nodes: 2,
            text: 6,
            exhausted: false,
        };
        assert!(work.sites(1));
        assert!(work.ty(&ty));
        assert!(!work.sites(1));
        assert!(work.exhausted);
        assert!(!work.ty(&TypeKind::Int32));
        let mut work = DeclarationWork {
            sites: 1,
            nodes: 1,
            text: 6,
            exhausted: false,
        };
        assert!(!work.ty(&ty));
        let mut work = DeclarationWork {
            sites: 1,
            nodes: 2,
            text: 5,
            exhausted: false,
        };
        assert!(!work.ty(&ty));
    }

    #[test]
    fn repeated_binder_materialization_exhausts_declaration_budget() {
        let binders = (0..2000)
            .map(|index| format!("T{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let params = (0..2000)
            .map(|index| format!("p{index}:i32"))
            .collect::<Vec<_>>()
            .join(",");
        let source = format!("fn probe[{binders}]({params})void{{}}");
        let program = crate::parser::Parser::new(crate::lexer::Lexer::new(&source).tokenize())
            .parse()
            .unwrap();
        let mut analyzer = Analyzer::new();
        for item in &program.items {
            analyzer.declare_top_level_item(item);
        }
        analyzer
            .canonicalize_declaration_types(&program, &mut || Ok(()))
            .unwrap();
        analyzer.flush_alias_resolution_errors();
        assert!(analyzer.errors.iter().any(|error| error.code == "S14" && error.message.contains("aggregate work budget")));
    }

    #[test]
    fn declaration_normalization_cancellation_is_operational() {
        let program =
            crate::parser::Parser::new(crate::lexer::Lexer::new("fn main()void{}").tokenize())
                .parse()
                .unwrap();
        let mut analyzer = Analyzer::new();
        for item in &program.items {
            analyzer.declare_top_level_item(item);
        }
        let mut polls = 0;
        assert!(
            analyzer
                .canonicalize_declaration_types(&program, &mut || {
                    polls += 1;
                    if polls == 3 {
                        Err(crate::cancel::Cancelled)
                    } else {
                        Ok(())
                    }
                })
                .is_err()
        );
        assert!(analyzer.errors.is_empty());
    }
}
