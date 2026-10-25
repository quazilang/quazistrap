// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Conservative source result dependencies. These parameter-slot sets are not
//! escape effects, projection provenance, call eligibility, or QZI certificates.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::cancel::Cancelled;
use crate::parser::ast::*;

use super::Analyzer;

type Dependencies = BTreeSet<usize>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RetentionIntrinsic {
    Allocate,
    Copy,
    Fill,
    StringPointer,
    PointerString,
}

fn retention_intrinsic(attributes: &[Attribute]) -> Option<RetentionIntrinsic> {
    let attribute = attributes
        .iter()
        .find(|attribute| attribute.name == "intrinsic")?;
    let AttrArg::Positional(AttrVal::Str(name)) = attribute.args.first()? else {
        return None;
    };
    match name.as_str() {
        "quazi.malloc" => Some(RetentionIntrinsic::Allocate),
        "quazi.memcpy" | "quazi.memmove" => Some(RetentionIntrinsic::Copy),
        "quazi.memset" => Some(RetentionIntrinsic::Fill),
        "quazi.str.as_ptr" => Some(RetentionIntrinsic::StringPointer),
        "quazi.str.from_ptr" => Some(RetentionIntrinsic::PointerString),
        _ => None,
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
struct Value {
    parameters: Dependencies,
    /// Local storage identities whose later writes are visible through this value.
    roots: BTreeSet<Storage>,
    /// Storage read by dereference/copy, separate from pointer identity.
    pointees: BTreeSet<Storage>,
    unknown_pointee: bool,
    /// A declared UTF-8 byte view needs no invented pointee cell. Existing
    /// tracked contents/uncertainty survive; writes may still alias storage.
    byte_pointer: bool,
}

impl Value {
    fn extend(&mut self, other: Self) {
        self.parameters.extend(other.parameters);
        self.roots.extend(other.roots);
        self.pointees.extend(other.pointees);
        self.unknown_pointee |= other.unknown_pointee;
        self.byte_pointer |= other.byte_pointer;
    }
}

/// Binding identities and stable allocation/call-site identities occupy
/// separate namespaces and survive cloned branch environments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Storage {
    Binding(usize),
    Allocation(usize, usize),
    Unmodeled(usize, usize),
}

#[derive(Clone, Default)]
struct Environment {
    scopes: Vec<BTreeMap<String, Storage>>,
    values: BTreeMap<Storage, Value>,
    next: usize,
    /// An unknown pointer may alias any known write. Conversely, a write
    /// through an unknown pointer may affect every later memory read.
    all_writes: Value,
    unknown_writes: Value,
}

impl Environment {
    fn enter(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    fn exit(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            for id in scope.into_values() {
                self.values.remove(&id);
            }
        }
    }

    fn declare(&mut self, name: &str, mut value: Value) {
        let id = Storage::Binding(self.next);
        self.next += 1;
        value.roots.insert(id);
        self.values.insert(id, value);
        self.scopes
            .last_mut()
            .expect("retention scope")
            .insert(name.into(), id);
    }

    fn id(&self, name: &str) -> Option<Storage> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    fn expand(&self, mut value: Value) -> Value {
        let mut pending = value.roots.iter().copied().collect::<Vec<_>>();
        let mut seen = BTreeSet::new();
        while let Some(root) = pending.pop() {
            if !seen.insert(root) {
                continue;
            }
            if let Some(stored) = self.values.get(&root) {
                value.parameters.extend(stored.parameters.iter().copied());
                value.pointees.extend(stored.pointees.iter().copied());
                value.unknown_pointee |= stored.unknown_pointee;
                value.byte_pointer |= stored.byte_pointer;
                pending.extend(stored.roots.iter().copied());
            }
        }
        // Reachable stored views remain dependencies of returned containers,
        // but their addresses do not become this pointer's pointees.
        let mut pending = value.pointees.iter().copied().collect::<Vec<_>>();
        if value.unknown_pointee {
            value
                .parameters
                .extend(self.all_writes.parameters.iter().copied());
            pending.extend(
                self.all_writes
                    .roots
                    .iter()
                    .chain(&self.all_writes.pointees)
                    .copied(),
            );
        }
        if !value.pointees.is_empty() || value.unknown_pointee {
            value
                .parameters
                .extend(self.unknown_writes.parameters.iter().copied());
            pending.extend(
                self.unknown_writes
                    .roots
                    .iter()
                    .chain(&self.unknown_writes.pointees)
                    .copied(),
            );
        }
        let mut seen = BTreeSet::new();
        while let Some(root) = pending.pop() {
            if !seen.insert(root) {
                continue;
            }
            if let Some(stored) = self.values.get(&root) {
                value.parameters.extend(stored.parameters.iter().copied());
                pending.extend(stored.roots.iter().chain(&stored.pointees).copied());
            }
        }
        value
    }

    fn allocate(&mut self, span: Span) -> Value {
        let id = Storage::Allocation(span.start, span.end);
        self.values.entry(id).or_default();
        Value {
            pointees: BTreeSet::from([id]),
            ..Value::default()
        }
    }

    fn uncertain_result(&mut self, span: Span, mut value: Value) -> Value {
        let id = Storage::Unmodeled(span.start, span.end);
        self.values.entry(id).or_default();
        value.pointees.insert(id);
        value.unknown_pointee = true;
        value
    }

    fn contents(&self, pointer: &Value) -> Value {
        let mut value = self.unknown_writes.clone();
        if pointer.unknown_pointee {
            value.extend(self.all_writes.clone());
            value.parameters.extend(pointer.parameters.iter().copied());
            value.unknown_pointee = true;
        }
        for id in &pointer.pointees {
            if let Some(stored) = self.values.get(id) {
                value.extend(self.expand(stored.clone()));
            }
        }
        value
    }

    fn write_contents(&mut self, pointer: &Value, value: &Value) {
        let value = self.expand(value.clone());
        self.all_writes.extend(value.clone());
        if pointer.unknown_pointee || pointer.byte_pointer {
            self.unknown_writes.extend(value.clone());
        }
        for id in &pointer.pointees {
            if let Some(stored) = self.values.get_mut(id) {
                stored.extend(value.clone());
            }
        }
    }

    fn get(&self, name: &str) -> Value {
        self.id(name)
            .and_then(|id| self.values.get(&id))
            .cloned()
            .map(|value| self.expand(value))
            .unwrap_or_default()
    }

    fn assign(&mut self, name: &str, mut value: Value) {
        if let Some(id) = self.id(name) {
            value.roots.insert(id);
            self.values.insert(id, value);
        }
    }

    fn mutate(&mut self, target: &Value, value: &Value) {
        self.write_contents(target, value);
        for root in &target.roots {
            if let Some(stored) = self.values.get_mut(root) {
                stored.extend(value.clone());
            }
        }
    }

    /// Preserve heap cells and weakly merge existing lexical bindings. Branch
    /// locals are removed by exit(), but escaped abstract allocations survive.
    fn merge(&mut self, branch: &Self) -> bool {
        let before = (
            self.values.clone(),
            self.all_writes.clone(),
            self.unknown_writes.clone(),
        );
        for (id, value) in &branch.values {
            if !matches!(id, Storage::Binding(_)) || self.values.contains_key(id) {
                self.values.entry(*id).or_default().extend(value.clone());
            }
        }
        self.all_writes.extend(branch.all_writes.clone());
        self.unknown_writes.extend(branch.unknown_writes.clone());
        before
            != (
                self.values.clone(),
                self.all_writes.clone(),
                self.unknown_writes.clone(),
            )
    }
}

struct Declaration<'a> {
    params: &'a [Param],
    return_ty: &'a Type,
    body: &'a Block,
    mutable_parameters: Vec<bool>,
}

impl Analyzer {
    pub(super) fn retention_annotation(
        &self,
        expression: &Expr,
    ) -> Option<&super::types::ExprAnnotation> {
        let index = self
            .retention_annotation_index
            .get(&(expression.span.start, expression.span.end))?;
        self.annotated_exprs.get(*index)
    }

    pub(super) fn retention_call_name(&self, expression: &Expr) -> Option<&str> {
        let name = self.retention_resolved_target(expression)?;
        self.result_retention
            .get_key_value(name)
            .or_else(|| {
                name.split_once('<')
                    .and_then(|(base, _)| self.result_retention.get_key_value(base))
            })
            .map(|(canonical, _)| canonical.as_str())
    }

    fn retention_resolved_target(&self, expression: &Expr) -> Option<&str> {
        let key = (expression.span.start, expression.span.end);
        let source_site = self
            .current_function
            .last()
            .and_then(|caller| self.ownership_call_sites.get(caller))
            .and_then(|sites| sites.get(&key));
        if let Some(site) = source_site {
            return site
                .declaration
                .as_deref()
                .or_else(|| site.target.as_ref().map(|(name, _)| name.as_str()));
        }
        // Helpers may also be queried after analysis, outside a callable stack.
        // A source span is usable only when all recorded resolutions agree.
        let mut sites = self
            .ownership_call_sites
            .values()
            .filter_map(|sites| sites.get(&key));
        if let Some(first) = sites.next() {
            let target = first
                .declaration
                .as_deref()
                .or_else(|| first.target.as_ref().map(|(name, _)| name.as_str()));
            return sites
                .all(|site| {
                    site.declaration
                        .as_deref()
                        .or_else(|| site.target.as_ref().map(|(name, _)| name.as_str()))
                        == target
                })
                .then_some(target)
                .flatten();
        }
        self.retention_annotation(expression)
            .and_then(|annotation| annotation.resolved_fn.as_deref())
    }

    pub(super) fn retention_instance_receiver(&self, expression: &Expr) -> bool {
        let ExprKind::MethodCall { object, .. } = &expression.node else {
            return false;
        };
        self.retention_annotation(object)
            .and_then(|annotation| annotation.ty.as_ref())
            .is_some_and(|ty| !matches!(ty, TypeKind::Error))
    }

    /// Match a proven source call's declared parameter slots to supplied values.
    /// A source variadic slot may receive several positional actual values.
    pub(super) fn retention_call_slots<'a>(
        &self,
        expression: &'a Expr,
    ) -> Option<Vec<Vec<&'a Expr>>> {
        let name = self.retention_call_name(expression)?;
        self.ownership_call_argument_slots(expression, name)
    }

    pub(super) fn derive_result_retention<F>(
        &mut self,
        program: &Program,
        checkpoint: &mut F,
    ) -> Result<(), Cancelled>
    where
        F: FnMut() -> Result<(), Cancelled>,
    {
        self.retention_annotation_index.clear();
        let result = self.derive_result_retention_inner(program, checkpoint);
        if result.is_err() {
            self.retention_annotation_index.clear();
        }
        result
    }

    fn derive_result_retention_inner<F>(
        &mut self,
        program: &Program,
        checkpoint: &mut F,
    ) -> Result<(), Cancelled>
    where
        F: FnMut() -> Result<(), Cancelled>,
    {
        checkpoint()?;
        self.result_retention.clear();
        self.fresh_result_intrinsics.clear();
        self.retention_intrinsics.clear();
        let mut annotations = HashMap::with_capacity(self.annotated_exprs.len());
        for (index, annotation) in self.annotated_exprs.iter().enumerate() {
            if index % 256 == 0 {
                checkpoint()?;
            }
            annotations.insert((annotation.span.start, annotation.span.end), index);
        }
        self.retention_annotation_index = annotations;
        let mut declarations = BTreeMap::new();
        for item in &program.items {
            checkpoint()?;
            if let ItemKind::Fn {
                name,
                body: None,
                attributes,
                ..
            } = &item.node
                && super::item_should_include(attributes)
                && fresh_allocation_attribute(attributes)
            {
                let canonical = self.retention_source_name(name, attributes, item.span);
                self.fresh_result_intrinsics.insert(canonical);
            }
            if let ItemKind::Fn {
                name, attributes, ..
            } = &item.node
                && super::item_should_include(attributes)
                && let Some(effect) = retention_intrinsic(attributes)
            {
                let canonical = self.retention_source_name(name, attributes, item.span);
                self.retention_intrinsics.insert(canonical, effect);
            }
            match &item.node {
                ItemKind::Fn {
                    name,
                    params,
                    return_ty,
                    body: Some(body),
                    attributes,
                    ..
                } if super::item_should_include(attributes) && !overrides_body(attributes) => {
                    let name = self.retention_source_name(name, attributes, item.span);
                    let mutable_parameters = self.retention_mutable_parameters(&name, params);
                    declarations.insert(
                        name,
                        Declaration {
                            params,
                            return_ty,
                            body,
                            mutable_parameters,
                        },
                    );
                }
                ItemKind::Impl {
                    for_ty, methods, ..
                } => {
                    let target = super::declare::type_kind_base_name(&for_ty.node);
                    for method in methods {
                        checkpoint()?;
                        if let ItemKind::Fn {
                            name,
                            params,
                            return_ty,
                            body: Some(body),
                            attributes,
                            ..
                        } = &method.node
                            && super::item_should_include(attributes)
                            && !overrides_body(attributes)
                        {
                            let canonical = format!("{target}.{name}");
                            let mutable_parameters =
                                self.retention_mutable_parameters(&canonical, params);
                            declarations.insert(
                                canonical,
                                Declaration {
                                    params,
                                    return_ty,
                                    body,
                                    mutable_parameters,
                                },
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        // Greatest fixed point: recursive/unresolved dependencies remain
        // conservative until source bodies prove individual slots unnecessary.
        let mut summaries = declarations
            .iter()
            .map(|(name, decl)| {
                (
                    name.clone(),
                    (0..decl.params.len()).collect::<Dependencies>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        loop {
            checkpoint()?;
            let mut changed = false;
            let mut next = summaries.clone();
            for (name, declaration) in &declarations {
                checkpoint()?;
                self.current_function.push(name.clone());
                self.current_generic_params
                    .push(self.generic_params_for_function(name));
                let result = (|| {
                    let mut evaluation = Evaluation {
                        analyzer: self,
                        declarations: &declarations,
                        summaries: &summaries,
                        checkpoint,
                        all_parameters: (0..declaration.params.len()).collect(),
                        returned: Dependencies::new(),
                        nodes: 0,
                        saturated: false,
                    };
                    let mut environment = Environment::default();
                    environment.enter();
                    for (slot, parameter) in declaration.params.iter().enumerate() {
                        environment.declare(
                            &parameter.name,
                            Value {
                                parameters: BTreeSet::from([slot]),
                                unknown_pointee: evaluation
                                    .analyzer
                                    .retention_type_may_contain_storage(
                                        &evaluation.analyzer.source_type(&parameter.ty),
                                    ),
                                ..Value::default()
                            },
                        );
                    }
                    evaluation.block(declaration.body, &mut environment)?;
                    if evaluation.saturated {
                        Ok(evaluation.all_parameters)
                    } else if evaluation.analyzer.retention_value_has_dependencies(
                        &evaluation.analyzer.source_type(declaration.return_ty),
                    ) {
                        Ok(evaluation.returned)
                    } else {
                        Ok(Dependencies::new())
                    }
                })();
                self.current_generic_params.pop();
                self.current_function.pop();
                let result: Dependencies = result?;
                let previous = &summaries[name];
                let refined = previous
                    .intersection(&result)
                    .copied()
                    .collect::<Dependencies>();
                changed |= refined != *previous;
                next.insert(name.clone(), refined);
            }
            summaries = next;
            if !changed {
                break;
            }
        }
        self.result_retention = summaries;
        Ok(())
    }

    pub(super) fn retention_source_name(
        &self,
        name: &str,
        attributes: &[Attribute],
        span: Span,
    ) -> String {
        if name.starts_with("__quazi_")
            || attributes
                .iter()
                .any(|attribute| attribute.name == "export")
        {
            name.to_string()
        } else {
            self.module_path_for_span(span)
                .map(|module| format!("{module}.{name}"))
                .unwrap_or_else(|| name.to_string())
        }
    }

    fn retention_mutable_parameters(&mut self, name: &str, params: &[Param]) -> Vec<bool> {
        self.current_generic_params
            .push(self.generic_params_for_function(name));
        let mutable = params
            .iter()
            .map(|param| {
                matches!(
                    self.resolve_type_aliases(&self.source_type(&param.ty)),
                    TypeKind::MutRef { .. } | TypeKind::RawPtr { .. }
                )
            })
            .collect();
        self.current_generic_params.pop();
        mutable
    }

    fn retention_value_has_dependencies(&self, ty: &TypeKind) -> bool {
        // Unsafe loads can turn pointer lineage back into a view. Keeping that
        // lineage here grants no permission for safe pointer/reference escape.
        self.retention_type_may_contain_storage(ty) || self.retention_type_may_contain_views(ty)
    }
}

fn overrides_body(attributes: &[Attribute]) -> bool {
    attributes
        .iter()
        .any(|attribute| matches!(attribute.name.as_str(), "intrinsic" | "syscall" | "api"))
}

fn fresh_allocation_attribute(attributes: &[Attribute]) -> bool {
    // Match the same first intrinsic attribute/first argument selected by
    // codegen. Only this intrinsic's backend implementation guarantees a new
    // allocation; the fact does not authorize borrowed calls to other intrinsics.
    attributes
        .iter()
        .find(|attribute| attribute.name == "intrinsic")
        .and_then(|attribute| attribute.args.first())
        .is_some_and(|argument| {
            matches!(argument,
            AttrArg::Positional(AttrVal::Str(name)) if name == "quazi.str_concat")
        })
}

struct Evaluation<'a, 'p, F> {
    analyzer: &'a Analyzer,
    declarations: &'a BTreeMap<String, Declaration<'p>>,
    summaries: &'a BTreeMap<String, Dependencies>,
    checkpoint: &'a mut F,
    all_parameters: Dependencies,
    returned: Dependencies,
    nodes: usize,
    saturated: bool,
}

impl<F: FnMut() -> Result<(), Cancelled>> Evaluation<'_, '_, F> {
    fn fallback(&mut self) -> Value {
        self.saturated = true;
        Value {
            parameters: self.all_parameters.clone(),
            unknown_pointee: true,
            ..Value::default()
        }
    }

    fn block(&mut self, block: &Block, env: &mut Environment) -> Result<bool, Cancelled> {
        env.enter();
        let mut terminal = false;
        for stmt in &block.stmts {
            (self.checkpoint)()?;
            if terminal {
                break;
            }
            terminal = self.statement(stmt, env)?;
        }
        env.exit();
        Ok(terminal)
    }

    fn statement(&mut self, stmt: &Stmt, env: &mut Environment) -> Result<bool, Cancelled> {
        match &stmt.node {
            StmtKind::Var { name, value, .. } => {
                let value = match value {
                    Some(value) => self.expression(value, env, 0)?,
                    None => Value::default(),
                };
                env.declare(name, value);
            }
            StmtKind::Const { name, value, .. } => {
                let value = self.expression(value, env, 0)?;
                env.declare(name, value);
            }
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    let value = self.expression(value, env, 0)?;
                    self.returned.extend(env.expand(value).parameters);
                }
                return Ok(true);
            }
            StmtKind::ExprStmt(expr) => {
                self.expression(expr, env, 0)?;
            }
            StmtKind::Break | StmtKind::Continue => return Ok(true),
            StmtKind::UnsafeBlock { body } => return self.block(body, env),
            StmtKind::CfgBlock { condition, body } => {
                if super::item_should_include(std::slice::from_ref(condition)) {
                    return self.block(body, env);
                }
            }
            StmtKind::If {
                condition,
                then_block,
                else_if,
                else_block,
            } => {
                self.expression(condition, env, 0)?;
                let initial = env.clone();
                let mut branch = initial.clone();
                let mut terminal = self.block(then_block, &mut branch)?;
                env.merge(&branch);
                let mut remaining = initial;
                for (condition, body) in else_if {
                    self.expression(condition, &mut remaining, 0)?;
                    let mut branch = remaining.clone();
                    terminal &= self.block(body, &mut branch)?;
                    env.merge(&branch);
                }
                if let Some(body) = else_block {
                    terminal &= self.block(body, &mut remaining)?;
                } else {
                    terminal = false;
                }
                env.merge(&remaining);
                return Ok(terminal);
            }
            StmtKind::For { kind, body } => {
                env.enter();
                let element = match kind {
                    ForLoop::Each { iter, .. } => match iter {
                        ForIter::Iter(expr) => self.projection_value(expr, env, 0)?,
                        ForIter::Range { start, end } => {
                            let mut value = self.expression(start, env, 0)?;
                            value.extend(self.expression(end, env, 0)?);
                            value
                        }
                    },
                    ForLoop::CStyle { init, .. } => {
                        if let Some(init) = init {
                            self.statement(init, env)?;
                        }
                        Value::default()
                    }
                    _ => Value::default(),
                };
                if let ForLoop::Each { vars, .. } = kind {
                    for name in vars {
                        env.declare(name, element.clone());
                    }
                }
                // Union across zero or more iterations. Every successful
                // iteration adds finite parameter/storage facts to outer slots.
                loop {
                    (self.checkpoint)()?;
                    if self.saturated {
                        break;
                    }
                    let mut iteration = env.clone();
                    match kind {
                        ForLoop::CStyle {
                            condition: Some(expr),
                            ..
                        }
                        | ForLoop::Cond {
                            condition: Some(expr),
                        } => {
                            self.expression(expr, &mut iteration, 0)?;
                        }
                        _ => {}
                    }
                    self.block(body, &mut iteration)?;
                    if let ForLoop::CStyle {
                        update: Some(expr), ..
                    } = kind
                    {
                        self.expression(expr, &mut iteration, 0)?;
                    }
                    if !env.merge(&iteration) {
                        break;
                    }
                }
                env.exit();
            }
        }
        Ok(false)
    }

    fn expression(
        &mut self,
        expr: &Expr,
        env: &mut Environment,
        depth: usize,
    ) -> Result<Value, Cancelled> {
        (self.checkpoint)()?;
        self.nodes += 1;
        if depth > 128 || self.nodes > 100_000 {
            return Ok(self.fallback());
        }
        let value = match &expr.node {
            ExprKind::Literal(_) => Value::default(),
            ExprKind::Ident(name) => env.get(name),
            ExprKind::Group(expr) | ExprKind::Cast { expr, .. } => {
                self.expression(expr, env, depth + 1)?
            }
            ExprKind::Unary { op, expr } => {
                if matches!(op, UnaryOpKind::Ref | UnaryOpKind::RefMut) {
                    self.address_value(expr, env, depth + 1)?
                } else {
                    let pointer = self.expression(expr, env, depth + 1)?;
                    if matches!(op, UnaryOpKind::Deref) {
                        let mut loaded = env.contents(&pointer);
                        loaded.parameters.extend(pointer.parameters);
                        loaded
                    } else {
                        pointer
                    }
                }
            }
            ExprKind::Try { expr } => {
                let value = self.expression(expr, env, depth + 1)?;
                self.returned.extend(value.parameters.iter().copied());
                value
            }
            ExprKind::Field { object, .. } => self.projection_value(object, env, depth + 1)?,
            ExprKind::Index { object, indices } => {
                let value = self.projection_value(object, env, depth + 1)?;
                for index in indices {
                    self.expression(index, env, depth + 1)?;
                }
                env.expand(value)
            }
            ExprKind::Binary { left, right, .. } => {
                let mut value = self.expression(left, env, depth + 1)?;
                value.extend(self.expression(right, env, depth + 1)?);
                value
            }
            ExprKind::ArrayLit(elements) => {
                let mut value = Value::default();
                for element in elements {
                    value.extend(self.expression(element, env, depth + 1)?);
                }
                value
            }
            ExprKind::StructInit { fields, .. } => {
                let mut value = Value::default();
                for (_, field) in fields {
                    value.extend(self.expression(field, env, depth + 1)?);
                }
                value
            }
            ExprKind::Assign { target, value } | ExprKind::CompoundAssign { target, value, .. } => {
                let target_address = self.address_value(target, env, depth + 1)?;
                let previous = if matches!(expr.node, ExprKind::CompoundAssign { .. }) {
                    env.contents(&target_address)
                } else {
                    Value::default()
                };
                let mut value = self.expression(value, env, depth + 1)?;
                value.extend(previous);
                if let ExprKind::Ident(name) = &target.node {
                    env.assign(name, value.clone());
                } else {
                    env.write_contents(&target_address, &value);
                }
                value
            }
            ExprKind::IncDec { expr, .. } => self.expression(expr, env, depth + 1)?,
            ExprKind::Closure { params, body } => {
                // A closure owns its captures even if its result ignores them.
                // Capture all visible values until lexical capture effects exist.
                let mut captured = Value::default();
                for scope in &env.scopes {
                    for name in scope.keys() {
                        if !params.contains(name) {
                            captured.extend(env.get(name));
                        }
                    }
                }
                // Creation never executes the closure body or its early returns.
                let _ = body;
                captured
            }
            ExprKind::Match { scrutinee, arms } => {
                let scrutinee = self.projection_value(scrutinee, env, depth + 1)?;
                let initial = env.clone();
                let mut value = Value::default();
                for arm in arms {
                    let mut branch = initial.clone();
                    branch.enter();
                    for name in pattern_all_bindings(&arm.pattern) {
                        branch.declare(&name, scrutinee.clone());
                    }
                    if let Some(guard) = &arm.guard {
                        self.expression(guard, &mut branch, depth + 1)?;
                    }
                    value.extend(self.expression(&arm.expr, &mut branch, depth + 1)?);
                    branch.exit();
                    env.merge(&branch);
                }
                value
            }
            ExprKind::Call { callee, .. } => {
                let callee_value = self.expression(callee, env, depth + 1)?;
                self.call(expr, None, callee_value, env, depth + 1)?
            }
            ExprKind::MethodCall { object, .. } => {
                let receiver = self.expression(object, env, depth + 1)?;
                self.call(expr, Some(receiver), Value::default(), env, depth + 1)?
            }
        };
        let ty = self
            .analyzer
            .retention_annotation(expr)
            .and_then(|annotation| annotation.ty.as_ref());
        let value = if value.pointees.is_empty()
            && !value.unknown_pointee
            && !value.byte_pointer
            && ty.is_some_and(|ty| self.analyzer.retention_type_may_contain_storage(ty))
        {
            env.uncertain_result(expr.span, value)
        } else {
            value
        };
        let value = env.expand(value);
        if ty.is_some_and(|ty| !self.analyzer.retention_value_has_dependencies(ty)) {
            // Evaluate effects first, then discard dependencies of a copied
            // scalar value. Address-of separately preserves its
            // operand's storage provenance rather than that copied value.
            Ok(Value::default())
        } else {
            Ok(value)
        }
    }

    fn address_value(
        &mut self,
        expr: &Expr,
        env: &mut Environment,
        depth: usize,
    ) -> Result<Value, Cancelled> {
        (self.checkpoint)()?;
        self.nodes += 1;
        if depth > 128 || self.nodes > 100_000 {
            return Ok(self.fallback());
        }
        match &expr.node {
            ExprKind::Ident(name) => {
                if let Some(id) = env.id(name) {
                    Ok(Value {
                        parameters: env.get(name).parameters,
                        pointees: BTreeSet::from([id]),
                        ..Value::default()
                    })
                } else {
                    Ok(self.fallback())
                }
            }
            ExprKind::Group(inner) => self.address_value(inner, env, depth + 1),
            ExprKind::Unary {
                op: UnaryOpKind::Deref,
                expr: inner,
            } => self.expression(inner, env, depth + 1),
            ExprKind::Field { object, .. } => self.projection_address(object, env, depth + 1),
            ExprKind::Index { object, indices } => {
                let address = self.projection_address(object, env, depth + 1)?;
                for index in indices {
                    self.expression(index, env, depth + 1)?;
                }
                Ok(address)
            }
            _ => Ok(self.fallback()),
        }
    }

    fn projection_value(
        &mut self,
        object: &Expr,
        env: &mut Environment,
        depth: usize,
    ) -> Result<Value, Cancelled> {
        let value = self.expression(object, env, depth + 1)?;
        if self.indirect_projection(object) {
            let mut loaded = env.contents(&value);
            loaded.parameters.extend(value.parameters);
            Ok(loaded)
        } else {
            Ok(value)
        }
    }

    fn indirect_projection(&self, object: &Expr) -> bool {
        self.analyzer
            .retention_annotation(object)
            .and_then(|annotation| annotation.ty.as_ref())
            .is_some_and(|ty| {
                matches!(
                    self.analyzer.resolve_type_aliases(ty),
                    TypeKind::Ref { .. } | TypeKind::MutRef { .. } | TypeKind::Slice { .. }
                )
            })
    }

    fn projection_address(
        &mut self,
        object: &Expr,
        env: &mut Environment,
        depth: usize,
    ) -> Result<Value, Cancelled> {
        if self.indirect_projection(object) {
            self.expression(object, env, depth + 1)
        } else {
            self.address_value(object, env, depth + 1)
        }
    }

    fn call(
        &mut self,
        expr: &Expr,
        receiver: Option<Value>,
        callee: Value,
        env: &mut Environment,
        depth: usize,
    ) -> Result<Value, Cancelled> {
        let (args, named_args) = match &expr.node {
            ExprKind::Call {
                args, named_args, ..
            }
            | ExprKind::MethodCall {
                args, named_args, ..
            } => (args, named_args),
            _ => return Ok(self.fallback()),
        };
        let annotation = self.analyzer.retention_annotation(expr);
        let resolved = self.analyzer.retention_resolved_target(expr);
        let declaration_name = resolved.and_then(|name| {
            self.declarations
                .get_key_value(name)
                .or_else(|| {
                    name.split_once('<')
                        .and_then(|(base, _)| self.declarations.get_key_value(base))
                })
                .map(|(name, _)| name.clone())
        });
        let declaration = declaration_name
            .as_ref()
            .and_then(|name| self.declarations.get(name));
        let mut all = callee;
        if let Some(receiver) = &receiver {
            all.extend(receiver.clone());
        }
        let mut positional = Vec::new();
        for argument in args {
            let value = self.expression(argument, env, depth + 1)?;
            all.extend(value.clone());
            positional.push(value);
        }
        let mut named = BTreeMap::new();
        for (name, argument) in named_args {
            let value = self.expression(argument, env, depth + 1)?;
            all.extend(value.clone());
            named.insert(name.as_str(), value);
        }
        if let Some(effect) = resolved
            .and_then(|name| self.analyzer.retention_intrinsics.get(name))
            .copied()
        {
            let slots =
                resolved.and_then(|name| self.analyzer.ownership_call_argument_slots(expr, name));
            if let Some(slots) = slots {
                let mut evaluated = HashMap::new();
                if let ExprKind::MethodCall { object, .. } = &expr.node
                    && let Some(value) = &receiver
                {
                    evaluated.insert(object.as_ref() as *const Expr, value);
                }
                for (argument, value) in args.iter().zip(&positional) {
                    evaluated.insert(argument as *const Expr, value);
                }
                for (name, argument) in named_args {
                    if let Some(value) = named.get(name.as_str()) {
                        evaluated.insert(argument as *const Expr, value);
                    }
                }
                let supplied = slots
                    .iter()
                    .map(|actuals| {
                        let mut value = Value::default();
                        for actual in actuals {
                            value.extend((*evaluated.get(&(*actual as *const Expr))?).clone());
                        }
                        Some(value)
                    })
                    .collect::<Option<Vec<_>>>();
                let Some(supplied) = supplied else {
                    return Ok(self.fallback());
                };
                match (effect, supplied.as_slice()) {
                    (RetentionIntrinsic::Allocate, [_]) => return Ok(env.allocate(expr.span)),
                    (RetentionIntrinsic::StringPointer, [source]) => {
                        let mut result = source.clone();
                        result.roots.clear();
                        result.byte_pointer = true;
                        return Ok(result);
                    }
                    (RetentionIntrinsic::PointerString, [source]) => {
                        return Ok(env.expand(source.clone()));
                    }
                    (RetentionIntrinsic::Copy, [destination, source, _]) => {
                        let contents = env.contents(source);
                        env.write_contents(destination, &contents);
                        return Ok(env.expand(destination.clone()));
                    }
                    (RetentionIntrinsic::Fill, [destination, _, _]) => {
                        return Ok(env.expand(destination.clone()));
                    }
                    _ => {}
                }
            }
        }
        let fresh_allocation =
            resolved.is_some_and(|name| self.analyzer.fresh_result_intrinsics.contains(name));
        if declaration.is_none() && !fresh_allocation {
            // Unavailable effects cannot establish that incoming storage was
            // left untouched. In particular, a void opaque pointer store may
            // populate an aggregate that the caller returns afterwards.
            env.mutate(&all, &all);
        }
        let mut result = all.clone();
        if let Some(declaration) = declaration {
            let mut supplied = vec![Value::default(); declaration.params.len()];
            let receiver_slot =
                usize::from(receiver.is_some() && self.analyzer.retention_instance_receiver(expr));
            if receiver_slot != 0 && supplied.is_empty() {
                return Ok(self.fallback());
            }
            if receiver_slot != 0 {
                supplied[0] = receiver.expect("receiver slot");
            }
            for (index, value) in positional.into_iter().enumerate() {
                let slot = index + receiver_slot;
                if slot < supplied.len() {
                    supplied[slot].extend(value);
                } else if declaration
                    .params
                    .last()
                    .is_some_and(|param| param.variadic)
                    && let Some(last) = supplied.last_mut()
                {
                    last.extend(value);
                }
            }
            for (name, value) in named {
                if let Some(slot) = declaration
                    .params
                    .iter()
                    .position(|param| param.name == name)
                {
                    supplied[slot].extend(value);
                }
            }
            // Exclusive/pointer source arguments can acquire any incoming provenance.
            // This is conservative write propagation, not an escape-effect proof.
            for (mutable, value) in declaration.mutable_parameters.iter().zip(&supplied) {
                if *mutable {
                    env.mutate(value, &all);
                }
            }
            result = Value::default();
            if let Some(summary) = declaration_name
                .as_ref()
                .and_then(|name| self.summaries.get(name))
            {
                for slot in summary {
                    if let Some(value) = supplied.get(*slot) {
                        result.extend(value.clone());
                    }
                }
            } else {
                result = all;
            }
        }
        // An instantiated call annotation is in the caller's binder context.
        // A raw callee return declaration is not: resolving it here could
        // mistake the callee's T for an unrelated caller/global alias.
        let result_type = annotation.and_then(|annotation| annotation.ty.as_ref());
        if fresh_allocation {
            return Ok(Value::default());
        }
        if result_type.is_some_and(|ty| !self.analyzer.retention_value_has_dependencies(ty)) {
            return Ok(Value::default());
        }
        if result_type.is_none_or(|ty| self.analyzer.retention_type_may_contain_storage(ty)) {
            result = env.uncertain_result(expr.span, result);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer::Lexer, parser::Parser};

    fn analyze(source: &str) -> Analyzer {
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        let mut analyzer = Analyzer::new();
        analyzer.analyze_program(&program);
        analyzer
    }

    fn slots(analyzer: &Analyzer, name: &str) -> Vec<usize> {
        analyzer
            .result_retention
            .get(name)
            .unwrap_or_else(|| panic!("missing {name}: {:?}", analyzer.result_retention))
            .iter()
            .copied()
            .collect()
    }

    #[test]
    fn retention_composes_only_used_arguments_and_named_order() {
        let analyzer = analyze(
            r#"
fn first(a: str, b: str) str { ret a; }
fn forward(a: str, b: str) str { ret first(a, b); }
fn named(a: str, b: str) str { ret first(b = a, a = b); }
fn literal(a: str) str { ret "literal"; }
fn ignores(a: str, b: str) str { first(a, b); ret "literal"; }
"#,
        );
        assert_eq!(slots(&analyzer, "first"), [0]);
        assert_eq!(slots(&analyzer, "forward"), [0]);
        assert_eq!(slots(&analyzer, "named"), [1]);
        assert!(slots(&analyzer, "literal").is_empty());
        assert!(slots(&analyzer, "ignores").is_empty());
    }

    #[test]
    fn retention_fresh_intrinsic_copies_do_not_retain_borrowed_inputs() {
        let analyzer = analyze(
            r#"
@intrinsic("quazi.str_concat")
fn fresh(left: str, right: str) str;
@intrinsic("quazi.strlen")
fn ordinary(value: str) str;
struct Copy { data: str }
fn copied(source: str) Copy { ret Copy { data: fresh(source, "") }; }
fn borrowed(source: str) Copy { ret Copy { data: source }; }
fn forwarding(source: str) Copy { ret copied(source); }
fn opaque(source: str) str { ret ordinary(source); }
fn shadowed(source: str) str { var fresh = |value| value; ret fresh(source); }
"#,
        );
        assert_eq!(
            analyzer.fresh_result_intrinsics,
            std::collections::HashSet::from(["fresh".into()])
        );
        assert!(slots(&analyzer, "copied").is_empty());
        assert!(slots(&analyzer, "forwarding").is_empty());
        assert_eq!(slots(&analyzer, "borrowed"), [0]);
        assert_eq!(slots(&analyzer, "opaque"), [0]);
        assert_eq!(slots(&analyzer, "shadowed"), [0]);
    }

    #[test]
    fn retention_scalar_projections_copy_but_address_of_keeps_storage() {
        let analyzer = analyze(
            r#"
struct View { text: str, length: i32 }
fn metadata(source: View) View { ret View { text: "static", length: source.length }; }
fn reference(source: i32) &i32 { ret &source; }
"#,
        );
        assert!(slots(&analyzer, "metadata").is_empty());
        assert_eq!(slots(&analyzer, "reference"), [0]);
    }

    #[test]
    fn retention_legacy_instance_and_associated_calls_use_semantic_targets() {
        let analyzer = analyze(
            r#"
struct Holder { value: str }
struct View { value: str }
impl Holder {
    fn view(receiver: Holder) View { ret View { value: receiver.value }; }
    fn make(value: str) Holder { ret Holder { value: value }; }
}
fn wrapper(value: str) View { var holder = Holder { value: value }; ret holder.view(); }
fn associated(value: str) Holder { ret Holder.make(value); }
"#,
        );
        assert_eq!(slots(&analyzer, "wrapper"), [0]);
        assert_eq!(slots(&analyzer, "associated"), [0]);
    }

    #[test]
    fn retention_generic_static_declarations_do_not_close_unresolved_graph_edges() {
        let source = r#"
struct Box[T] { value: str }
impl Box[T] {
    fn new() Box[T] { ret Box { value: "literal" }; }
    fn from(value: T) Box[T] { var result = Box.new(); ret result; }
}
fn main() void { var result = Box.from[str]("ignored"); }
"#;
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        let mut analyzer = Analyzer::new();
        let report = analyzer.analyze_program(&program);
        assert!(slots(&analyzer, "Box.from").is_empty());
        let site = analyzer.ownership_call_sites["Box.from"]
            .values()
            .find(|site| site.declaration.as_deref() == Some("Box.new"))
            .expect("resolved declaration fact");
        assert!(
            site.target.is_none(),
            "declaration must not invent concrete type arguments"
        );
        let caller = report
            .ownership_call_graph
            .iter()
            .find(|node| node.id.callable == "Box.from")
            .expect("concrete caller");
        assert!(!caller.source_calls_closed);
        assert!(caller.calls.iter().any(|call| !matches!(
            call.target,
            crate::semantic::types::OwnershipCallTarget::Direct(_)
        )));
    }

    #[test]
    fn overridden_bodies_never_prove_source_result_independence() {
        let analyzer = analyze(
            r#"
@intrinsic("quazi.strlen") fn fake(value: str) str { ret "literal"; }
@api("foreign") unsafe fn foreign(value: str) str { ret "literal"; }
@syscall(1) unsafe fn syscall(value: str) str { ret "literal"; }
fn intrinsic_wrapper(value: str) str { ret fake(value); }
fn foreign_wrapper(value: str) str { unsafe { ret foreign(value); } }
fn syscall_wrapper(value: str) str { unsafe { ret syscall(value); } }
"#,
        );
        assert!(!analyzer.result_retention.contains_key("fake"));
        assert!(!analyzer.result_retention.contains_key("foreign"));
        assert!(!analyzer.result_retention.contains_key("syscall"));
        assert_eq!(slots(&analyzer, "intrinsic_wrapper"), [0]);
        assert_eq!(slots(&analyzer, "foreign_wrapper"), [0]);
        assert_eq!(slots(&analyzer, "syscall_wrapper"), [0]);
    }

    #[test]
    fn retention_mutable_alias_capabilities_use_the_callee_binder_scope() {
        let analyzer = analyze(
            r#"
struct View { value: str }
type Borrow = &View!;
fn fill(out: Borrow, value: str) void { out.value = value; }
fn wrapper[Borrow](value: str, unused: Borrow) View {
    var output = View { value: "literal" };
    fill(&output!, value);
    ret output;
}
"#,
        );
        assert_eq!(slots(&analyzer, "wrapper"), [0]);
    }

    #[test]
    fn retention_pointer_lineage_survives_opaque_view_loads() {
        let analyzer = analyze(
            r#"
@intrinsic("quazi.array_load") unsafe fn load(pointer: *u8) str;
fn forward(pointer: *u8) *u8 { ret pointer; }
fn wrapper(pointer: *u8) str { unsafe { ret load(forward(pointer)); } }
"#,
        );
        assert_eq!(slots(&analyzer, "forward"), [0]);
        assert_eq!(slots(&analyzer, "wrapper"), [0]);
    }

    #[test]
    fn retention_source_and_opaque_pointer_writes_populate_returned_aggregates() {
        let analyzer = analyze(
            r#"
struct View { value: str }
unsafe fn write(out: *View, value: str) void { (*out).value = value; }
@api("store_view") unsafe fn opaque_write(out: *View, value: str) void;
fn source_write(value: str) View {
    var local = View { value: "literal" };
    unsafe { write((&local!) as *View, value); }
    ret local;
}
fn opaque_store(value: str) View {
    var local = View { value: "literal" };
    unsafe { opaque_write((&local!) as *View, value); }
    ret local;
}
"#,
        );
        assert_eq!(slots(&analyzer, "source_write"), [0]);
        assert_eq!(slots(&analyzer, "opaque_store"), [0]);
    }

    #[test]
    fn retention_invalid_instance_call_to_zero_parameter_function_never_panics() {
        let source = "struct Holder { value: str } impl Holder { fn new() Holder { ret Holder { value: \"literal\" }; } } fn invalid(holder: Holder) Holder { ret holder.new(); }";
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        let mut analyzer = Analyzer::new();
        let report = analyzer.analyze_program(&program);
        assert!(!report.errors.is_empty());
        assert_eq!(slots(&analyzer, "invalid"), [0]);
    }

    #[test]
    fn retention_source_variadics_group_actuals_and_preserve_named_order() {
        let source = r#"
struct View { text: str }
fn pack(...values: str) View { ret View { text: values[0] }; }
fn select(first: str, ...rest: str) View { ret View { text: first }; }
fn grouped(a: str, b: str) View { ret pack(a, b); }
fn named(a: str, b: str) View { ret select(rest = b, first = a); }
"#;
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        let mut analyzer = Analyzer::new();
        analyzer.analyze_program(&program);
        assert_eq!(slots(&analyzer, "grouped"), [0, 1]);
        assert_eq!(slots(&analyzer, "named"), [0]);
        for (name, expected) in [
            ("grouped", vec![vec!["a", "b"]]),
            ("named", vec![vec!["a"], vec!["b"]]),
        ] {
            let call = program
                .items
                .iter()
                .find_map(|item| match &item.node {
                    ItemKind::Fn {
                        name: candidate,
                        body: Some(body),
                        ..
                    } if candidate == name => match &body.stmts[0].node {
                        StmtKind::Return(Some(call)) => Some(call),
                        _ => None,
                    },
                    _ => None,
                })
                .unwrap();
            let grouped = analyzer
                .retention_call_slots(call)
                .expect("source argument grouping");
            let names = grouped
                .iter()
                .map(|values| {
                    values
                        .iter()
                        .map(|value| match &value.node {
                            ExprKind::Ident(name) => name.as_str(),
                            _ => panic!("expected argument identifier"),
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            assert_eq!(names, expected, "{name}");
        }
    }

    #[test]
    fn retention_covers_methods_nested_aggregates_enums_and_match() {
        let analyzer = analyze(
            r#"
struct View { text: str }
struct Owner { text: str }
enum Choice { Text(str), Empty }
impl Owner { fn view(self: &Owner, unused: str) View { ret View { text: self.text }; } }
fn via_method(owner: &Owner, unused: str) View { ret owner.view(unused); }
fn nested(a: str, unused: str) Choice { ret Choice.Text(View { text: a }.text); }
fn matched(value: Choice, unused: str) str { ret match value { Choice.Text(text) => text, Choice.Empty => "empty" }; }
fn static_view(unused: str) View { ret View { text: "static" }; }
"#,
        );
        assert_eq!(slots(&analyzer, "Owner.view"), [0]);
        assert_eq!(slots(&analyzer, "via_method"), [0]);
        assert_eq!(slots(&analyzer, "nested"), [0]);
        assert_eq!(slots(&analyzer, "matched"), [0]);
        assert!(slots(&analyzer, "static_view").is_empty());
    }

    #[test]
    fn retention_merges_loops_assignments_aliases_and_shadowed_bindings() {
        let analyzer = analyze(
            r#"
struct View { text: str }
fn looped(a: str, b: str, condition: bool) str {
    var output = a;
    for condition { output = b; }
    ret output;
}
fn branches(a: str, b: str, condition: bool) str {
    var output = a;
    if condition { output = b; } else { var output = "local"; }
    ret output;
}
fn mutated(a: str, b: str) View {
    var output = View { text: a };
    var alias = &output!;
    alias.text = b;
    ret output;
}
"#,
        );
        assert_eq!(slots(&analyzer, "looped"), [0, 1]);
        assert_eq!(slots(&analyzer, "branches"), [0, 1]);
        assert_eq!(slots(&analyzer, "mutated"), [0, 1]);
    }

    #[test]
    fn retention_recursive_cycles_start_conservative_and_static_results_refine() {
        let analyzer = analyze(
            r#"
fn recursive(a: str, b: str) str { ret recursive(a, b); }
fn left(a: str, b: str, condition: bool) str {
    if condition { ret right(a, b, condition); }
    ret a;
}
fn right(a: str, b: str, condition: bool) str { ret left(a, b, condition); }
fn static_left(a: str) str { ret static_right(a); }
fn static_right(a: str) str { ret "static"; }
"#,
        );
        assert_eq!(slots(&analyzer, "recursive"), [0, 1]);
        // Greatest fixed point deliberately cannot discard b from this cycle.
        assert_eq!(slots(&analyzer, "left"), [0, 1]);
        assert_eq!(slots(&analyzer, "right"), [0, 1]);
        assert!(slots(&analyzer, "static_left").is_empty());
    }

    #[test]
    fn retention_closure_creation_never_executes_its_try_expression() {
        let analyzer = analyze(
            r#"
enum Choice { Text(str), Empty }
fn ignored(a: str, value: Choice) str {
    var closure = || value?;
    ret "static";
}
"#,
        );
        assert!(slots(&analyzer, "ignored").is_empty());
    }

    #[test]
    fn retention_resets_and_cancellation_publishes_no_partial_summaries() {
        let mut analyzer = analyze("fn first(a: str) str { ret a; }");
        assert_eq!(slots(&analyzer, "first"), [0]);
        let empty = Parser::new(Lexer::new("").tokenize()).parse().unwrap();
        analyzer.analyze_program(&empty);
        assert!(analyzer.result_retention.is_empty());
        assert!(analyzer.fresh_result_intrinsics.is_empty());
        assert!(analyzer.retention_annotation_index.is_empty());
        let program = Parser::new(Lexer::new("fn first(a: str) str { ret a; }").tokenize())
            .parse()
            .unwrap();
        let mut polls = 0;
        analyzer.retention_annotation_index.insert((123, 456), 0);
        let result = analyzer.derive_result_retention(&program, &mut || {
            polls += 1;
            if polls >= 4 { Err(Cancelled) } else { Ok(()) }
        });
        assert_eq!(result, Err(Cancelled));
        assert!(analyzer.result_retention.is_empty());
        assert!(analyzer.retention_annotation_index.is_empty());
    }
}
