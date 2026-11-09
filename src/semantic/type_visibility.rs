// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Enforce per-source visibility of nominal type spellings.
//!
//! Declarations live in one merged symbol table, so global lookup alone would
//! let one file use a type that only another file imports. This pass checks
//! every source spelling against the environment of the file that contains it:
//! the file's own declarations, its authenticated imports, the public types of
//! the loader-selected prelude package, builtins, and lexical generic binders.
use super::type_context::SourceTypeContexts;
use super::*;

/// A loaded source file that declares a type name. Duplicate declarations are
/// reported separately (S05); each declaring file still sees its own name.
struct SourceDeclaration {
    source: usize,
    public: bool,
}

struct Visibility<'a> {
    sources: &'a [SourceFile],
    contexts: &'a SourceTypeContexts,
    declarations: HashMap<String, Vec<SourceDeclaration>>,
    prelude_sources: HashSet<usize>,
    known_type_names: HashSet<String>,
    imported_spellings: HashSet<String>,
    errors: Vec<(Span, &'static str, String)>,
    reported: HashSet<(usize, usize)>,
    work: usize,
}

impl Visibility<'_> {
    fn source_of(&self, span: Span) -> Option<usize> {
        self.sources.iter().position(|source| source.contains(span))
    }

    fn charge(&mut self, span: Span) -> bool {
        if self.work == 0 {
            if self.reported.insert((usize::MAX, usize::MAX)) {
                self.errors
                    .push((span, "S14", "type visibility work budget exceeded".into()));
            }
            return false;
        }
        self.work -= 1;
        true
    }

    /// Whether `name` denotes a type the file containing `span` may spell.
    /// `None` means the name is not a type at all.
    fn visible(&self, name: &str, span: Span) -> Option<bool> {
        if self.contexts.canonical_name(name, span).is_some() {
            return Some(true);
        }
        if let Some(declarations) = self.declarations.get(name) {
            let here = self.source_of(span);
            return Some(
                here.is_none()
                    || declarations.iter().any(|declaration| {
                        here == Some(declaration.source)
                            || (declaration.public
                                && self.prelude_sources.contains(&declaration.source))
                    }),
            );
        }
        // An import spelling bound by another file is never visible here.
        if self.imported_spellings.contains(name) {
            return Some(false);
        }
        // Builtins and QZI library types have no source declaration.
        (!name.contains('.') && self.known_type_names.contains(name)).then_some(true)
    }

    fn report(&mut self, span: Span, message: String) {
        if self.reported.insert((span.start, span.end)) {
            self.errors.push((span, "S04", message));
        }
    }

    fn check_type(&mut self, ty: &Type, binders: &HashSet<String>) {
        if !self.charge(ty.span) {
            return;
        }
        match &ty.node {
            TypeKind::Named { name, type_args } => {
                if !binders.contains(name.as_str()) {
                    match self.visible(name, ty.span) {
                        Some(true) => {}
                        Some(false) => self.report(
                            ty.span,
                            format!("type '{name}' is not imported into this file"),
                        ),
                        None if name == "Self" => self.report(
                            ty.span,
                            "'Self' is only available inside traits and impl blocks".into(),
                        ),
                        None => self.report(ty.span, format!("unknown type '{name}'")),
                    }
                }
                for arg in type_args {
                    self.check_type(arg, binders);
                }
            }
            TypeKind::Array { elem_ty, .. }
            | TypeKind::FlexibleArray { elem_ty }
            | TypeKind::Slice { elem_ty } => self.check_type(elem_ty, binders),
            TypeKind::Ref { inner } | TypeKind::MutRef { inner } | TypeKind::RawPtr { inner } => {
                self.check_type(inner, binders)
            }
            TypeKind::Fn { params, return_ty } | TypeKind::CFn { params, return_ty } => {
                for param in params {
                    self.check_type(param, binders);
                }
                self.check_type(return_ty, binders);
            }
            TypeKind::Dyn { trait_name } => {
                if !binders.contains(trait_name.as_str()) {
                    match self.visible(trait_name, ty.span) {
                        Some(true) => {}
                        Some(false) => self.report(
                            ty.span,
                            format!("trait '{trait_name}' is not imported into this file"),
                        ),
                        None => self.report(ty.span, format!("unknown trait '{trait_name}'")),
                    }
                }
            }
            _ => {}
        }
    }

    /// Check a value-position type head such as `Point { .. }`, `Point.new()`
    /// or a `Color.Red` pattern. Unknown names are left to the type checker,
    /// which reports them as unknown identifiers or variants.
    fn check_type_head(&mut self, name: &str, span: Span, scope: &Scope) {
        let head = name.split('.').next().unwrap_or(name);
        if scope.shadows(head) || self.contexts.canonical_name(name, span).is_some() {
            return;
        }
        if self.visible(head, span) == Some(false) {
            self.report(
                span,
                format!("type '{head}' is not imported into this file"),
            );
        }
    }

    fn check_block(&mut self, block: &Block, scope: &mut Scope) {
        let outer = scope.locals.clone();
        for stmt in &block.stmts {
            self.check_stmt(stmt, scope);
        }
        scope.locals = outer;
    }

    fn check_stmt(&mut self, stmt: &Stmt, scope: &mut Scope) {
        if !self.charge(stmt.span) {
            return;
        }
        match &stmt.node {
            StmtKind::Var {
                name, ty, value, ..
            } => {
                if let Some(ty) = ty {
                    self.check_type(ty, &scope.binders);
                }
                if let Some(value) = value {
                    self.check_expr(value, scope);
                }
                scope.locals.insert(name.clone());
            }
            StmtKind::Const {
                name, ty, value, ..
            } => {
                if let Some(ty) = ty {
                    self.check_type(ty, &scope.binders);
                }
                self.check_expr(value, scope);
                scope.locals.insert(name.clone());
            }
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    self.check_expr(value, scope);
                }
            }
            StmtKind::If {
                condition,
                then_block,
                else_if,
                else_block,
            } => {
                self.check_expr(condition, scope);
                self.check_block(then_block, scope);
                for (condition, block) in else_if {
                    self.check_expr(condition, scope);
                    self.check_block(block, scope);
                }
                if let Some(block) = else_block {
                    self.check_block(block, scope);
                }
            }
            StmtKind::For { kind, body } => {
                let outer = scope.locals.clone();
                match kind {
                    ForLoop::Each { vars, iter, .. } => {
                        match iter {
                            ForIter::Range { start, end } => {
                                self.check_expr(start, scope);
                                self.check_expr(end, scope);
                            }
                            ForIter::Iter(expr) => self.check_expr(expr, scope),
                        }
                        scope.locals.extend(vars.iter().cloned());
                    }
                    ForLoop::CStyle {
                        init,
                        condition,
                        update,
                    } => {
                        if let Some(init) = init {
                            self.check_stmt(init, scope);
                        }
                        for expr in condition.iter().chain(update) {
                            self.check_expr(expr, scope);
                        }
                    }
                    ForLoop::Cond { condition } => {
                        if let Some(condition) = condition {
                            self.check_expr(condition, scope);
                        }
                    }
                }
                self.check_block(body, scope);
                scope.locals = outer;
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::ExprStmt(expr) => self.check_expr(expr, scope),
            StmtKind::CfgBlock { condition, body } => {
                if item_should_include(std::slice::from_ref(condition)) {
                    self.check_block(body, scope);
                }
            }
            StmtKind::UnsafeBlock { body } => self.check_block(body, scope),
        }
    }

    fn check_exprs<'e>(&mut self, exprs: impl IntoIterator<Item = &'e Expr>, scope: &mut Scope) {
        for expr in exprs {
            self.check_expr(expr, scope);
        }
    }

    fn check_expr(&mut self, expr: &Expr, scope: &mut Scope) {
        if !self.charge(expr.span) {
            return;
        }
        match &expr.node {
            ExprKind::Literal(_) => {}
            ExprKind::Ident(name) => self.check_type_head(name, expr.span, scope),
            ExprKind::Group(inner)
            | ExprKind::Unary { expr: inner, .. }
            | ExprKind::IncDec { expr: inner, .. }
            | ExprKind::Try { expr: inner } => self.check_expr(inner, scope),
            ExprKind::Cast { expr: inner, ty } => {
                self.check_expr(inner, scope);
                self.check_type(ty, &scope.binders);
            }
            ExprKind::Binary { left, right, .. } => {
                self.check_expr(left, scope);
                self.check_expr(right, scope);
            }
            ExprKind::Assign { target, value } | ExprKind::CompoundAssign { target, value, .. } => {
                self.check_expr(target, scope);
                self.check_expr(value, scope);
            }
            ExprKind::Call {
                callee,
                type_args,
                args,
                named_args,
            } => {
                self.check_expr(callee, scope);
                for ty in type_args {
                    self.check_type(ty, &scope.binders);
                }
                self.check_exprs(args, scope);
                self.check_exprs(named_args.iter().map(|(_, arg)| arg), scope);
            }
            ExprKind::Field { object, name } => {
                // `module.Type` used as a value, e.g. `module.Type.new()`.
                // `Enum.Variant` and `Type.member` are not module paths, even
                // when the member is spelled like another type.
                if let ExprKind::Ident(module) = &object.node
                    && !scope.shadows(module)
                    && !self.declarations.contains_key(module)
                    && !self.known_type_names.contains(module)
                    && self.declarations.contains_key(name)
                {
                    let spelling = format!("{module}.{name}");
                    if self.contexts.canonical_name(&spelling, expr.span).is_none() {
                        self.report(
                            expr.span,
                            format!("type '{spelling}' is not imported into this file"),
                        );
                    }
                }
                self.check_expr(object, scope);
            }
            ExprKind::StructInit { name, fields } => {
                self.check_type_head(name, expr.span, scope);
                self.check_exprs(fields.iter().map(|(_, value)| value), scope);
            }
            ExprKind::MethodCall {
                object,
                type_args,
                args,
                named_args,
                ..
            } => {
                self.check_expr(object, scope);
                for ty in type_args {
                    self.check_type(ty, &scope.binders);
                }
                self.check_exprs(args, scope);
                self.check_exprs(named_args.iter().map(|(_, arg)| arg), scope);
            }
            ExprKind::Match { scrutinee, arms } => {
                self.check_expr(scrutinee, scope);
                for arm in arms {
                    let outer = scope.locals.clone();
                    self.check_pattern(&arm.pattern, scope);
                    if let Some(guard) = &arm.guard {
                        self.check_expr(guard, scope);
                    }
                    self.check_expr(&arm.expr, scope);
                    scope.locals = outer;
                }
            }
            ExprKind::ArrayLit(elements) => self.check_exprs(elements, scope),
            ExprKind::Index { object, indices } => {
                self.check_expr(object, scope);
                self.check_exprs(indices, scope);
            }
            ExprKind::Closure { params, body } => {
                let outer = scope.locals.clone();
                scope.locals.extend(params.iter().cloned());
                self.check_expr(body, scope);
                scope.locals = outer;
            }
        }
    }

    fn check_pattern(&mut self, pattern: &Pattern, scope: &mut Scope) {
        match &pattern.node {
            PatternKind::Bind(name) => {
                scope.locals.insert(name.clone());
            }
            PatternKind::Wildcard | PatternKind::Literal(_) => {}
            PatternKind::Variant {
                enum_name,
                sub_patterns,
                ..
            } => {
                if let Some(enum_name) = enum_name {
                    self.check_type_head(enum_name, pattern.span, scope);
                }
                for sub in sub_patterns {
                    self.check_pattern(sub, scope);
                }
            }
        }
    }

    fn check_function(
        &mut self,
        generic_params: &[String],
        params: &[Param],
        return_ty: &Type,
        body: Option<&Block>,
        enclosing: &HashSet<String>,
    ) {
        let mut scope = Scope {
            binders: enclosing
                .iter()
                .cloned()
                .chain(generic_params.iter().cloned())
                .collect(),
            locals: params.iter().map(|param| param.name.clone()).collect(),
        };
        for param in params {
            self.check_type(&param.ty, &scope.binders);
        }
        self.check_type(return_ty, &scope.binders);
        if let Some(body) = body {
            self.check_block(body, &mut scope);
        }
    }
}

struct Scope {
    binders: HashSet<String>,
    locals: HashSet<String>,
}

impl Scope {
    fn shadows(&self, name: &str) -> bool {
        self.binders.contains(name) || self.locals.contains(name)
    }
}

impl Analyzer {
    /// Reject type spellings that the containing source file cannot see.
    /// Parser-only clients without loader source maps keep global lookup.
    pub(super) fn validate_source_type_visibility(
        &mut self,
        program: &Program,
        checkpoint: &mut dyn FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<(), crate::cancel::Cancelled> {
        if self.source_files.is_empty() {
            return Ok(());
        }
        let prelude_root = self
            .source_files
            .iter()
            .find(|source| source.implicit_prelude)
            .and_then(|source| std::path::Path::new(&source.path).parent())
            .map(std::path::Path::to_path_buf);
        let prelude_sources = self
            .source_files
            .iter()
            .enumerate()
            .filter(|(_, source)| {
                prelude_root
                    .as_ref()
                    .is_some_and(|root| std::path::Path::new(&source.path).starts_with(root))
            })
            .map(|(index, _)| index)
            .collect();
        let known_type_names = self
            .scopes
            .first()
            .into_iter()
            .flatten()
            .filter(|(_, symbol)| symbol.kind == SymbolKind::TypeName)
            .map(|(name, _)| name.clone())
            .collect();
        let imported_spellings = self.type_name_bindings.keys().cloned().collect();
        let mut visibility = Visibility {
            sources: &self.source_files,
            contexts: &self.source_type_contexts,
            declarations: HashMap::new(),
            prelude_sources,
            known_type_names,
            imported_spellings,
            errors: Vec::new(),
            reported: HashSet::new(),
            work: 4_000_000,
        };
        for item in &program.items {
            checkpoint()?;
            let (name, public, attributes) = match &item.node {
                ItemKind::Struct {
                    name,
                    public,
                    attributes,
                    ..
                }
                | ItemKind::Enum {
                    name,
                    public,
                    attributes,
                    ..
                }
                | ItemKind::Trait {
                    name,
                    public,
                    attributes,
                    ..
                }
                | ItemKind::TypeAlias {
                    name,
                    public,
                    attributes,
                    ..
                } => (name, *public, attributes),
                _ => continue,
            };
            if !item_should_include(attributes) {
                continue;
            }
            if let Some(source) = visibility.source_of(item.span) {
                visibility
                    .declarations
                    .entry(name.clone())
                    .or_default()
                    .push(SourceDeclaration { source, public });
            }
        }
        let no_binders = HashSet::new();
        for item in &program.items {
            checkpoint()?;
            match &item.node {
                ItemKind::Fn {
                    generic_params,
                    params,
                    return_ty,
                    body,
                    attributes,
                    ..
                } if item_should_include(attributes) => visibility.check_function(
                    generic_params,
                    params,
                    return_ty,
                    body.as_ref(),
                    &no_binders,
                ),
                ItemKind::Struct {
                    generic_params,
                    fields,
                    attributes,
                    ..
                } if item_should_include(attributes) => {
                    let binders = generic_params.iter().cloned().collect();
                    for field in fields {
                        visibility.check_type(&field.ty, &binders);
                    }
                }
                ItemKind::Enum {
                    generic_params,
                    variants,
                    attributes,
                    ..
                } if item_should_include(attributes) => {
                    let binders = generic_params.iter().cloned().collect();
                    for variant in variants {
                        for ty in &variant.payload_types {
                            visibility.check_type(ty, &binders);
                        }
                    }
                }
                ItemKind::Trait {
                    generic_params,
                    methods,
                    attributes,
                    ..
                } if item_should_include(attributes) => {
                    for method in methods {
                        let binders = generic_params
                            .iter()
                            .chain(&method.generic_params)
                            .cloned()
                            .chain(["Self".to_string()])
                            .collect();
                        for ty in method.params.iter().chain([&method.return_ty]) {
                            visibility.check_type(ty, &binders);
                        }
                    }
                }
                ItemKind::TypeAlias {
                    generic_params,
                    aliased_type,
                    attributes,
                    ..
                } if item_should_include(attributes) => {
                    visibility.check_type(aliased_type, &generic_params.iter().cloned().collect());
                }
                ItemKind::ForeignGlobal { ty, attributes, .. }
                    if item_should_include(attributes) =>
                {
                    visibility.check_type(ty, &no_binders);
                }
                ItemKind::Impl {
                    trait_ty,
                    for_ty,
                    methods,
                } => {
                    let binders: HashSet<_> = self
                        .resolved_impl_generic_parameters(&for_ty.node)
                        .into_iter()
                        .chain(["Self".to_string()])
                        .collect();
                    visibility.check_type(for_ty, &binders);
                    if let Some(trait_ty) = trait_ty {
                        visibility.check_type(trait_ty, &binders);
                    }
                    for method in methods {
                        checkpoint()?;
                        if let ItemKind::Fn {
                            generic_params,
                            params,
                            return_ty,
                            body,
                            attributes,
                            ..
                        } = &method.node
                            && item_should_include(attributes)
                        {
                            visibility.check_function(
                                generic_params,
                                params,
                                return_ty,
                                body.as_ref(),
                                &binders,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        let errors = visibility.errors;
        for (span, code, message) in errors {
            self.push_error(span, code, message);
        }
        Ok(())
    }
}
