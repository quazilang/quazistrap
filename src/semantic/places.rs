// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Declaration-backed call operands for D-014. These are source facts, not
//! alias resolution, execution sequencing, ownership effects, or permissions.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::*;
use crate::cancel::Cancelled;

const MAX_BODY_NODES: usize = 100_000;
const MAX_PLACE_DEPTH: usize = 64;
pub(super) const MAX_GRAPH_ARGUMENT_NODES: usize = 1_000_000;

type BindingKey = (usize, usize, String);
type ArgumentFacts = BTreeMap<(usize, usize), OwnershipCallArguments>;

struct BindingInfo<'a> {
    ty: Option<&'a TypeKind>,
    global: bool,
}

impl Analyzer {
    /// Map actual expressions to declaration slots independently of whether a
    /// result-retention summary or a verified call boundary is available.
    pub(super) fn ownership_call_argument_slots<'a>(
        &self,
        expression: &'a Expr,
        callee: &str,
    ) -> Option<Vec<Vec<&'a Expr>>> {
        let names = self.fn_param_names.get(callee)?;
        let symbol = self.resolve_symbol(callee)?;
        let implicit = symbol.params.len().checked_sub(names.len())?;
        if implicit > 1 {
            return None;
        }
        let (receiver, args, named_args) = match &expression.node {
            ExprKind::Call {
                args, named_args, ..
            } => (None, args, named_args),
            ExprKind::MethodCall {
                object,
                args,
                named_args,
                ..
            } => (Some(object.as_ref()), args, named_args),
            _ => return None,
        };
        let offset = usize::from(self.retention_instance_receiver(expression));
        let mut slots = vec![Vec::new(); symbol.params.len()];
        if offset != 0 {
            slots.get_mut(0)?.push(receiver?);
        }
        for (index, argument) in args.iter().enumerate() {
            let index = index + offset;
            let index = if symbol.variadic {
                index.min(slots.len().checked_sub(1)?)
            } else {
                index
            };
            slots.get_mut(index)?.push(argument);
        }
        let named_slots = names
            .iter()
            .enumerate()
            .map(|(slot, name)| (name.as_str(), slot))
            .collect::<HashMap<_, _>>();
        for (name, argument) in named_args {
            let index = *named_slots.get(name.as_str())? + implicit;
            let slot = slots.get_mut(index)?;
            if !slot.is_empty() {
                return None;
            }
            slot.push(argument);
        }
        let fixed = slots.len().checked_sub(usize::from(symbol.variadic))?;
        if slots[..fixed].iter().any(Vec::is_empty) {
            return None;
        }
        Some(slots)
    }

    pub(super) fn record_ownership_argument_places<F>(
        &mut self,
        program: &Program,
        checkpoint: &mut F,
    ) -> Result<(), Cancelled>
    where
        F: FnMut() -> Result<(), Cancelled>,
    {
        checkpoint()?;
        let mut bindings = HashMap::new();
        if let Some(globals) = self.scopes.first() {
            for (name, symbol) in globals {
                checkpoint()?;
                bindings.insert(
                    binding_key(name, symbol.span),
                    BindingInfo {
                        ty: symbol.ty.as_ref(),
                        global: true,
                    },
                );
            }
        }
        for scope in &self.finished_scopes {
            for (name, symbol) in scope {
                checkpoint()?;
                bindings.insert(
                    binding_key(name, symbol.span),
                    BindingInfo {
                        ty: symbol.ty.as_ref(),
                        global: false,
                    },
                );
            }
        }
        let mut collected = Vec::new();
        let mut overflows = Vec::new();
        for item in &program.items {
            checkpoint()?;
            let mut functions = Vec::new();
            match &item.node {
                ItemKind::Fn {
                    name, attributes, ..
                } => {
                    functions.push((
                        self.retention_source_name(name, attributes, item.span),
                        item,
                    ));
                }
                ItemKind::Impl {
                    for_ty, methods, ..
                } => {
                    let target = super::declare::type_kind_base_name(&for_ty.node);
                    for method in methods {
                        if let ItemKind::Fn { name, .. } = &method.node {
                            functions.push((format!("{target}.{name}"), method));
                        }
                    }
                }
                _ => {}
            }
            for (caller, function) in functions {
                checkpoint()?;
                let ItemKind::Fn {
                    params,
                    body: Some(body),
                    attributes,
                    ..
                } = &function.node
                else {
                    continue;
                };
                if !super::item_should_include(attributes)
                    || attributes.iter().any(|attribute| {
                        matches!(attribute.name.as_str(), "intrinsic" | "syscall" | "api")
                    })
                {
                    continue;
                }
                let Some(sites) = self.ownership_call_sites.get(&caller) else {
                    continue;
                };
                let builder = Places {
                    analyzer: self,
                    bindings: &bindings,
                    parameters: params
                        .iter()
                        .enumerate()
                        .map(|(slot, param)| (binding_key(&param.name, param.name_span), slot))
                        .collect(),
                    binders: self.generic_params_for_function(&caller),
                };
                let (facts, overflow) = builder.body(body, sites, checkpoint)?;
                collected.push((caller, facts));
                if let Some(span) = overflow {
                    overflows.push(span);
                }
            }
        }
        checkpoint()?;
        // All snapshots were derived before mutating the source-site registry.
        for (caller, facts) in collected {
            checkpoint()?;
            if let Some(sites) = self.ownership_call_sites.get_mut(&caller) {
                for (span, arguments) in facts {
                    if let Some(site) = sites.get_mut(&span) {
                        site.arguments = Some(arguments);
                    }
                }
            }
        }
        for span in overflows {
            self.push_error(span, "S14", format!("ownership argument facts exceed their compiler limit ({MAX_BODY_NODES} body nodes / {MAX_PLACE_DEPTH} place levels)"));
        }
        Ok(())
    }

    pub(super) fn instantiate_ownership_arguments<F>(
        &self,
        source: &OwnershipCallArguments,
        substitution: &HashMap<String, TypeKind>,
        remaining: &mut usize,
        checkpoint: &mut F,
    ) -> Result<Option<OwnershipCallArguments>, Cancelled>
    where
        F: FnMut() -> Result<(), Cancelled>,
    {
        let structure = argument_cost(source, false);
        let Some(budget) = remaining.checked_sub(structure) else {
            return Ok(None);
        };
        *remaining = budget;
        let mut resolve = |ty: &TypeKind, span: Span| {
            let ty = match crate::type_resolution::resolve_type(
                ty,
                &self.type_aliases,
                substitution,
                &HashSet::new(),
                |_| None,
            ) {
                Ok(ty) => ty,
                Err(error) => {
                    self.alias_resolution_errors
                        .borrow_mut()
                        .entry(error.to_string())
                        .or_insert(span);
                    return Ok(None);
                }
            };
            let cost = type_cost(&ty);
            let Some(budget) = remaining.checked_sub(cost) else {
                return Err(());
            };
            *remaining = budget;
            Ok(Some(ty))
        };
        let mut actuals = Vec::new();
        for argument in &source.actuals {
            checkpoint()?;
            let ty = match argument
                .ty
                .as_ref()
                .map(|ty| resolve(ty, argument.span))
                .transpose()
            {
                Ok(ty) => ty.flatten(),
                Err(()) => return Ok(None),
            };
            let value = if let OwnershipArgumentValue::Place(place) = &argument.value {
                let root_ty = match place
                    .root
                    .ty
                    .as_ref()
                    .map(|ty| resolve(ty, argument.span))
                    .transpose()
                {
                    Ok(ty) => ty.flatten(),
                    Err(()) => return Ok(None),
                };
                let mut projections = Vec::new();
                for projection in &place.projections {
                    checkpoint()?;
                    projections.push(if let OwnershipProjection::Index(indices) = projection {
                        let mut instantiated = Vec::new();
                        for index in indices {
                            checkpoint()?;
                            let ty = match index
                                .ty
                                .as_ref()
                                .map(|ty| resolve(ty, index.span))
                                .transpose()
                            {
                                Ok(ty) => ty.flatten(),
                                Err(()) => return Ok(None),
                            };
                            instantiated.push(OwnershipIndex {
                                span: index.span,
                                ty,
                                constant: index.constant.clone(),
                            });
                        }
                        OwnershipProjection::Index(instantiated)
                    } else {
                        projection.clone()
                    });
                }
                OwnershipArgumentValue::Place(OwnershipPlace {
                    root: OwnershipPlaceRoot {
                        binding: place.root.binding.clone(),
                        storage: place.root.storage.clone(),
                        ty: root_ty,
                    },
                    projections,
                })
            } else {
                argument.value.clone()
            };
            actuals.push(OwnershipCallArgument {
                span: argument.span,
                role: argument.role.clone(),
                parameter_slot: argument.parameter_slot,
                ty,
                formation: argument.formation,
                auto_deref: argument.auto_deref,
                value,
            });
        }
        Ok(Some(OwnershipCallArguments {
            actuals,
            deferred_closures: source.deferred_closures.clone(),
        }))
    }
}

fn binding_key(name: &str, span: Span) -> BindingKey {
    (span.start, span.end, name.to_string())
}

struct Places<'a> {
    analyzer: &'a Analyzer,
    bindings: &'a HashMap<BindingKey, BindingInfo<'a>>,
    parameters: HashMap<BindingKey, usize>,
    binders: Vec<String>,
}

// Iterative source traversal preserves deferred closure attribution without
// treating closure creation as execution. It deliberately does not build a CFG.
enum Visit<'a> {
    Statement(&'a Stmt),
    Expression(&'a Expr),
    Closure(&'a Expr),
    EndClosure,
}

impl Places<'_> {
    fn ty(&self, ty: &TypeKind, span: Span) -> Option<TypeKind> {
        let resolved = self
            .analyzer
            .resolve_type_with_parameters(ty, &self.binders, span);
        (!matches!(resolved, TypeKind::Error)).then_some(resolved)
    }

    fn expression_type(&self, expression: &Expr) -> Option<TypeKind> {
        let annotation = self.analyzer.retention_annotation(expression)?;
        if annotation.resolved_global.is_some()
            && let Some(binding) = &annotation.resolved_binding
            && let Some(info) = self.bindings.get(&binding_key(&binding.name, binding.span))
            && info.global
            && let Some(ty) = info.ty
        {
            let ty = self
                .analyzer
                .resolve_type_with_parameters(ty, &[], expression.span);
            return (!matches!(ty, TypeKind::Error)).then_some(ty);
        }
        annotation.ty.as_ref().and_then(|ty| {
            // Group/deref/field/index result types come from their base
            // declaration too. Outer annotations need not repeat the
            // resolved-global marker retained on the base identifier.
            let mut base = expression;
            for _ in 0..MAX_PLACE_DEPTH {
                if self
                    .analyzer
                    .retention_annotation(base)
                    .is_some_and(|annotation| annotation.resolved_global.is_some())
                {
                    let ty = self
                        .analyzer
                        .resolve_type_with_parameters(ty, &[], expression.span);
                    return (!matches!(ty, TypeKind::Error)).then_some(ty);
                }
                base = match &base.node {
                    ExprKind::Group(inner)
                    | ExprKind::Unary {
                        op: UnaryOpKind::Deref,
                        expr: inner,
                    } => inner,
                    ExprKind::Field { object, .. } | ExprKind::Index { object, .. } => object,
                    _ => return self.ty(ty, expression.span),
                };
            }
            None
        })
    }

    fn body<F>(
        &self,
        body: &Block,
        sites: &BTreeMap<(usize, usize), super::ownership::SourceOwnershipCall>,
        checkpoint: &mut F,
    ) -> Result<(ArgumentFacts, Option<Span>), Cancelled>
    where
        F: FnMut() -> Result<(), Cancelled>,
    {
        let mut pending = body
            .stmts
            .iter()
            .rev()
            .map(Visit::Statement)
            .collect::<Vec<_>>();
        let mut closures = Vec::new();
        let mut facts = BTreeMap::new();
        let mut nodes = 0;
        while let Some(visit) = pending.pop() {
            checkpoint()?;
            nodes += 1;
            if nodes > MAX_BODY_NODES {
                return Ok((facts, Some(body.span)));
            }
            match visit {
                Visit::Closure(expr) => {
                    if closures.len() >= MAX_PLACE_DEPTH {
                        return Ok((facts, Some(expr.span)));
                    }
                    closures.push(expr.span);
                }
                Visit::EndClosure => {
                    closures.pop();
                }
                Visit::Statement(stmt) => match &stmt.node {
                    StmtKind::Var { value, .. } | StmtKind::Return(value) => {
                        if let Some(value) = value {
                            pending.push(Visit::Expression(value));
                        }
                    }
                    StmtKind::Const { value, .. } | StmtKind::ExprStmt(value) => {
                        pending.push(Visit::Expression(value))
                    }
                    StmtKind::UnsafeBlock { body } => {
                        pending.extend(body.stmts.iter().rev().map(Visit::Statement))
                    }
                    StmtKind::CfgBlock { condition, body }
                        if super::item_should_include(std::slice::from_ref(condition)) =>
                    {
                        pending.extend(body.stmts.iter().rev().map(Visit::Statement))
                    }
                    StmtKind::If {
                        condition,
                        then_block,
                        else_if,
                        else_block,
                    } => {
                        if let Some(body) = else_block {
                            pending.extend(body.stmts.iter().rev().map(Visit::Statement));
                        }
                        for (condition, body) in else_if.iter().rev() {
                            pending.extend(body.stmts.iter().rev().map(Visit::Statement));
                            pending.push(Visit::Expression(condition));
                        }
                        pending.extend(then_block.stmts.iter().rev().map(Visit::Statement));
                        pending.push(Visit::Expression(condition));
                    }
                    StmtKind::For { kind, body } => {
                        pending.extend(body.stmts.iter().rev().map(Visit::Statement));
                        match kind {
                            ForLoop::Each { iter, .. } => match iter {
                                ForIter::Iter(expr) => pending.push(Visit::Expression(expr)),
                                ForIter::Range { start, end } => {
                                    pending.push(Visit::Expression(end));
                                    pending.push(Visit::Expression(start));
                                }
                            },
                            ForLoop::CStyle {
                                init,
                                condition,
                                update,
                            } => {
                                if let Some(expr) = update {
                                    pending.push(Visit::Expression(expr));
                                }
                                if let Some(expr) = condition {
                                    pending.push(Visit::Expression(expr));
                                }
                                if let Some(stmt) = init {
                                    pending.push(Visit::Statement(stmt));
                                }
                            }
                            ForLoop::Cond { condition } => {
                                if let Some(expr) = condition {
                                    pending.push(Visit::Expression(expr));
                                }
                            }
                        }
                    }
                    _ => {}
                },
                Visit::Expression(expr) => {
                    let key = (expr.span.start, expr.span.end);
                    if let Some(site) = sites.get(&key) {
                        let (arguments, overflow) =
                            self.arguments(expr, site, &closures, checkpoint)?;
                        nodes = nodes.saturating_add(argument_cost(&arguments, true));
                        if overflow || nodes > MAX_BODY_NODES {
                            return Ok((facts, Some(expr.span)));
                        }
                        facts.insert(key, arguments);
                    }
                    match &expr.node {
                        ExprKind::Group(inner)
                        | ExprKind::Unary { expr: inner, .. }
                        | ExprKind::Cast { expr: inner, .. }
                        | ExprKind::Try { expr: inner }
                        | ExprKind::IncDec { expr: inner, .. } => {
                            pending.push(Visit::Expression(inner))
                        }
                        ExprKind::Field { object, .. } => pending.push(Visit::Expression(object)),
                        ExprKind::Index { object, indices } => {
                            pending.extend(indices.iter().rev().map(Visit::Expression));
                            pending.push(Visit::Expression(object));
                        }
                        ExprKind::Binary { left, right, .. } => {
                            pending.push(Visit::Expression(right));
                            pending.push(Visit::Expression(left));
                        }
                        ExprKind::Assign { target, value }
                        | ExprKind::CompoundAssign { target, value, .. } => {
                            pending.push(Visit::Expression(value));
                            pending.push(Visit::Expression(target));
                        }
                        ExprKind::Call {
                            callee,
                            args,
                            named_args,
                            ..
                        } => {
                            pending.extend(
                                named_args
                                    .iter()
                                    .rev()
                                    .map(|(_, expr)| Visit::Expression(expr)),
                            );
                            pending.extend(args.iter().rev().map(Visit::Expression));
                            pending.push(Visit::Expression(callee));
                        }
                        ExprKind::MethodCall {
                            object,
                            args,
                            named_args,
                            ..
                        } => {
                            pending.extend(
                                named_args
                                    .iter()
                                    .rev()
                                    .map(|(_, expr)| Visit::Expression(expr)),
                            );
                            pending.extend(args.iter().rev().map(Visit::Expression));
                            pending.push(Visit::Expression(object));
                        }
                        ExprKind::ArrayLit(values) => {
                            pending.extend(values.iter().rev().map(Visit::Expression))
                        }
                        ExprKind::StructInit { fields, .. } => pending
                            .extend(fields.iter().rev().map(|(_, expr)| Visit::Expression(expr))),
                        ExprKind::Match { scrutinee, arms } => {
                            for arm in arms.iter().rev() {
                                pending.push(Visit::Expression(&arm.expr));
                                if let Some(guard) = &arm.guard {
                                    pending.push(Visit::Expression(guard));
                                }
                            }
                            pending.push(Visit::Expression(scrutinee));
                        }
                        ExprKind::Closure { body, .. } => {
                            pending.push(Visit::EndClosure);
                            pending.push(Visit::Expression(body));
                            pending.push(Visit::Closure(expr));
                        }
                        ExprKind::Literal(_) | ExprKind::Ident(_) => {}
                    }
                }
            }
        }
        Ok((facts, None))
    }

    fn arguments<F>(
        &self,
        expression: &Expr,
        site: &super::ownership::SourceOwnershipCall,
        closures: &[Span],
        checkpoint: &mut F,
    ) -> Result<(OwnershipCallArguments, bool), Cancelled>
    where
        F: FnMut() -> Result<(), Cancelled>,
    {
        let mut expressions = Vec::new();
        let (args, named_args) = match &expression.node {
            ExprKind::Call {
                args, named_args, ..
            } => (args, named_args),
            ExprKind::MethodCall {
                object,
                args,
                named_args,
                ..
            } => {
                if self.analyzer.retention_instance_receiver(expression) {
                    expressions.push((OwnershipArgumentRole::Receiver, object.as_ref()));
                }
                (args, named_args)
            }
            _ => {
                return Ok((
                    OwnershipCallArguments {
                        actuals: Vec::new(),
                        deferred_closures: closures.to_vec(),
                    },
                    false,
                ));
            }
        };
        if args.len().saturating_add(named_args.len()) >= MAX_BODY_NODES {
            return Ok((
                OwnershipCallArguments {
                    actuals: Vec::new(),
                    deferred_closures: Vec::new(),
                },
                true,
            ));
        }
        expressions.extend(
            args.iter()
                .enumerate()
                .map(|(index, expr)| (OwnershipArgumentRole::Positional(index), expr)),
        );
        expressions.extend(
            named_args
                .iter()
                .map(|(name, expr)| (OwnershipArgumentRole::Named(name.clone()), expr)),
        );
        let slots = site.declaration.as_deref().and_then(|name| {
            self.analyzer
                .ownership_call_argument_slots(expression, name)
        });
        let mut slot_by_expression = HashMap::new();
        if let Some(slots) = &slots {
            for (slot, actuals) in slots.iter().enumerate() {
                for actual in actuals {
                    checkpoint()?;
                    slot_by_expression.insert(*actual as *const Expr, slot);
                }
            }
        }
        let mut actuals = Vec::new();
        let mut overflow = false;
        for (role, expr) in expressions {
            checkpoint()?;
            let parameter_slot = slot_by_expression.get(&(expr as *const Expr)).copied();
            let mut operand = expr;
            let mut groups = 0;
            while let ExprKind::Group(inner) = &operand.node {
                groups += 1;
                if groups >= MAX_PLACE_DEPTH {
                    overflow = true;
                    break;
                }
                operand = inner;
            }
            let (formation, value) = match &operand.node {
                ExprKind::Unary {
                    op: UnaryOpKind::Ref,
                    expr: inner,
                } => (
                    OwnershipArgumentFormation::SharedAddress,
                    self.place(inner, 0, &mut overflow),
                ),
                ExprKind::Unary {
                    op: UnaryOpKind::RefMut,
                    expr: inner,
                } => (
                    OwnershipArgumentFormation::ExclusiveAddress,
                    self.place(inner, 0, &mut overflow),
                ),
                _ => (
                    OwnershipArgumentFormation::Value,
                    self.place(expr, 0, &mut overflow),
                ),
            };
            actuals.push(OwnershipCallArgument {
                span: expr.span,
                role,
                parameter_slot,
                ty: self.expression_type(expr),
                formation,
                auto_deref: self
                    .analyzer
                    .retention_annotation(expr)
                    .is_some_and(|annotation| annotation.auto_deref),
                value,
            });
            if overflow {
                break;
            }
        }
        Ok((
            OwnershipCallArguments {
                actuals,
                deferred_closures: closures.to_vec(),
            },
            overflow,
        ))
    }

    fn binding_place(&self, binding: &ResolvedBinding, span: Span) -> OwnershipArgumentValue {
        if !matches!(
            binding.kind,
            SymbolKind::Variable { .. } | SymbolKind::Parameter
        ) {
            return OwnershipArgumentValue::Rvalue;
        }
        let key = binding_key(&binding.name, binding.span);
        let info = self.bindings.get(&key);
        let storage = if let Some(slot) = self.parameters.get(&key) {
            OwnershipPlaceStorage::Parameter(*slot)
        } else if matches!(binding.kind, SymbolKind::Parameter) {
            OwnershipPlaceStorage::ClosureParameter
        } else if info.is_some_and(|info| info.global) {
            OwnershipPlaceStorage::Global
        } else if info.is_some() {
            OwnershipPlaceStorage::Local
        } else {
            return OwnershipArgumentValue::Unknown;
        };
        // Global declarations have no caller binders. A caller's T must not
        // capture a global alias T and change the storage's concrete type.
        let ty = info
            .and_then(|info| info.ty.map(|ty| (info, ty)))
            .and_then(|(info, ty)| {
                if info.global {
                    let ty = self.analyzer.resolve_type_with_parameters(ty, &[], span);
                    (!matches!(ty, TypeKind::Error)).then_some(ty)
                } else {
                    self.ty(ty, span)
                }
            });
        // Import namespace bindings and recovery symbols have no storage type.
        // Their spelling must not invent a global aggregate field place.
        if ty.is_none() {
            return OwnershipArgumentValue::Unknown;
        }
        OwnershipArgumentValue::Place(OwnershipPlace {
            root: OwnershipPlaceRoot {
                binding: binding.clone(),
                storage,
                ty,
            },
            projections: Vec::new(),
        })
    }

    fn place(
        &self,
        expression: &Expr,
        depth: usize,
        overflow: &mut bool,
    ) -> OwnershipArgumentValue {
        if depth >= MAX_PLACE_DEPTH {
            *overflow = true;
            return OwnershipArgumentValue::Unknown;
        }
        let global = self
            .analyzer
            .retention_annotation(expression)
            .filter(|annotation| annotation.resolved_global.is_some())
            .and_then(|annotation| annotation.resolved_binding.as_ref());
        let mut result = if let Some(binding) = global {
            self.binding_place(binding, expression.span)
        } else {
            match &expression.node {
                ExprKind::Ident(_) => {
                    let Some(annotation) = self.analyzer.retention_annotation(expression) else {
                        return OwnershipArgumentValue::Unknown;
                    };
                    let Some(binding) = &annotation.resolved_binding else {
                        return OwnershipArgumentValue::Unknown;
                    };
                    self.binding_place(binding, expression.span)
                }
                ExprKind::Group(inner) => self.place(inner, depth + 1, overflow),
                ExprKind::Unary {
                    op: UnaryOpKind::Deref,
                    expr: inner,
                } => {
                    let mut place = self.place(inner, depth + 1, overflow);
                    if let OwnershipArgumentValue::Place(place) = &mut place {
                        place.projections.push(OwnershipProjection::Deref {
                            kind: deref_kind(self.expression_type(inner).as_ref()),
                            implicit: false,
                        });
                    }
                    projected_place(place)
                }
                ExprKind::Field { object, name } => {
                    let mut place = self.place(object, depth + 1, overflow);
                    if let OwnershipArgumentValue::Place(place) = &mut place {
                        append_reference_projection(place, self.expression_type(object).as_ref());
                        place
                            .projections
                            .push(OwnershipProjection::Field(name.clone()));
                    }
                    projected_place(place)
                }
                ExprKind::Index { object, indices } => {
                    let ty = self.expression_type(object);
                    let inner = match ty.as_ref() {
                        Some(TypeKind::Ref { inner } | TypeKind::MutRef { inner }) => {
                            Some(&inner.node)
                        }
                        ty => ty,
                    };
                    if !matches!(inner, Some(TypeKind::Array { .. } | TypeKind::Slice { .. })) {
                        return OwnershipArgumentValue::Unknown;
                    }
                    let mut place = self.place(object, depth + 1, overflow);
                    if let OwnershipArgumentValue::Place(place) = &mut place {
                        append_reference_projection(place, ty.as_ref());
                        place.projections.push(OwnershipProjection::Index(
                            indices
                                .iter()
                                .map(|expr| OwnershipIndex {
                                    span: expr.span,
                                    ty: self.expression_type(expr),
                                    constant: self
                                        .analyzer
                                        .retention_annotation(expr)
                                        .and_then(|annotation| annotation.const_value.clone()),
                                })
                                .collect(),
                        ));
                    }
                    projected_place(place)
                }
                ExprKind::Cast { .. }
                | ExprKind::Assign { .. }
                | ExprKind::CompoundAssign { .. }
                | ExprKind::Try { .. } => OwnershipArgumentValue::Unknown,
                _ => OwnershipArgumentValue::Rvalue,
            }
        };
        if self
            .analyzer
            .retention_annotation(expression)
            .is_some_and(|annotation| annotation.auto_deref)
            && let OwnershipArgumentValue::Place(place) = &mut result
        {
            let kind = if place.projections.is_empty() {
                deref_kind(place.root.ty.as_ref())
            } else {
                OwnershipDerefKind::Unknown
            };
            place.projections.push(OwnershipProjection::Deref {
                kind,
                implicit: true,
            });
        }
        result
    }
}

fn projected_place(value: OwnershipArgumentValue) -> OwnershipArgumentValue {
    match value {
        OwnershipArgumentValue::Place(_) => value,
        _ => OwnershipArgumentValue::Unknown,
    }
}

fn deref_kind(ty: Option<&TypeKind>) -> OwnershipDerefKind {
    match ty {
        Some(TypeKind::Ref { .. }) => OwnershipDerefKind::Shared,
        Some(TypeKind::MutRef { .. }) => OwnershipDerefKind::Exclusive,
        Some(TypeKind::RawPtr { .. }) => OwnershipDerefKind::Raw,
        _ => OwnershipDerefKind::Unknown,
    }
}

fn append_reference_projection(place: &mut OwnershipPlace, ty: Option<&TypeKind>) {
    let kind = deref_kind(ty);
    if kind != OwnershipDerefKind::Unknown {
        place.projections.push(OwnershipProjection::Deref {
            kind,
            implicit: true,
        });
    }
}

// Count retained structure and instantiated type nodes, not just visited AST
// expressions. Generic substitution must not multiply operand snapshots without
// consuming the graph's operational budget.
fn argument_cost(arguments: &OwnershipCallArguments, include_types: bool) -> usize {
    let mut cost = arguments.deferred_closures.len();
    for argument in &arguments.actuals {
        cost += 1;
        if include_types {
            cost += argument.ty.as_ref().map_or(0, type_cost);
        }
        if let OwnershipArgumentValue::Place(place) = &argument.value {
            cost += 1 + place.projections.len();
            if include_types {
                cost += place.root.ty.as_ref().map_or(0, type_cost);
            }
            for projection in &place.projections {
                if let OwnershipProjection::Index(indices) = projection {
                    cost += indices.len();
                    if include_types {
                        cost += indices
                            .iter()
                            .map(|index| index.ty.as_ref().map_or(0, type_cost))
                            .sum::<usize>();
                    }
                }
            }
        }
    }
    cost
}

fn type_cost(ty: &TypeKind) -> usize {
    let mut pending = vec![ty];
    let mut cost = 0;
    while let Some(ty) = pending.pop() {
        cost += 1;
        match ty {
            TypeKind::Named { type_args, .. } => {
                pending.extend(type_args.iter().map(|arg| &arg.node))
            }
            TypeKind::Ref { inner }
            | TypeKind::MutRef { inner }
            | TypeKind::RawPtr { inner }
            | TypeKind::Slice { elem_ty: inner }
            | TypeKind::FlexibleArray { elem_ty: inner }
            | TypeKind::Array { elem_ty: inner, .. } => pending.push(&inner.node),
            TypeKind::Fn { params, return_ty } | TypeKind::CFn { params, return_ty } => {
                pending.push(&return_ty.node);
                pending.extend(params.iter().map(|param| &param.node));
            }
            _ => {}
        }
    }
    cost
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer::Lexer, parser::Parser};

    fn analyze(source: &str) -> SemanticReport {
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        Analyzer::new().analyze_program(&program)
    }

    #[test]
    fn call_places_keep_field_and_native_index_paths() {
        let source = r#"
struct Pair { value: i32 }
fn observe(value: i32) void {}
fn caller(pair: &Pair, values: &[i32; 2], index: usize) void {
    observe(pair.value);
    observe(values[index]);
    observe(values[1]);
}
"#;
        let report = analyze(source);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let caller = report
            .ownership_call_graph
            .iter()
            .find(|node| node.id.callable == "caller")
            .unwrap();
        assert_eq!(caller.calls.len(), 3);
        let arguments = caller
            .calls
            .iter()
            .map(|site| &site.arguments.as_ref().unwrap().actuals[0])
            .collect::<Vec<_>>();
        let OwnershipArgumentValue::Place(field) = &arguments[0].value else {
            panic!("{:?}", arguments[0]);
        };
        assert_eq!(field.root.storage, OwnershipPlaceStorage::Parameter(0));
        assert!(
            matches!(field.projections.as_slice(), [OwnershipProjection::Deref { kind: OwnershipDerefKind::Shared, implicit: true }, OwnershipProjection::Field(name)] if name == "value")
        );
        let OwnershipArgumentValue::Place(index) = &arguments[1].value else {
            panic!("{:?}", arguments[1]);
        };
        assert_eq!(index.root.storage, OwnershipPlaceStorage::Parameter(1));
        assert!(
            matches!(index.projections.as_slice(), [OwnershipProjection::Deref { kind: OwnershipDerefKind::Shared, implicit: true }, OwnershipProjection::Index(indices)] if indices.len() == 1 && indices[0].constant.is_none())
        );
        let OwnershipArgumentValue::Place(index) = &arguments[2].value else {
            panic!("{:?}", arguments[2]);
        };
        assert!(
            matches!(index.projections.last(), Some(OwnershipProjection::Index(indices)) if matches!(indices[0].constant, Some(ConstValue::Int(1))))
        );
    }

    #[test]
    fn argument_instantiation_is_bounded_and_cancellable() {
        let report = analyze(
            "fn observe(value: i32) void {} fn main() void { var value = 1; observe(value); }",
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let main = report
            .ownership_call_graph
            .iter()
            .find(|node| node.id.callable == "main")
            .unwrap();
        let arguments = main.calls[0].arguments.as_ref().unwrap();
        let analyzer = Analyzer::new();
        let mut remaining = 0;
        assert!(
            analyzer
                .instantiate_ownership_arguments(
                    arguments,
                    &HashMap::new(),
                    &mut remaining,
                    &mut || Ok(())
                )
                .unwrap()
                .is_none()
        );
        let mut remaining = MAX_GRAPH_ARGUMENT_NODES;
        assert!(
            analyzer
                .instantiate_ownership_arguments(
                    arguments,
                    &HashMap::new(),
                    &mut remaining,
                    &mut || Err(Cancelled)
                )
                .is_err()
        );
        let mut remaining = argument_cost(arguments, true);
        assert!(
            analyzer
                .instantiate_ownership_arguments(
                    arguments,
                    &HashMap::new(),
                    &mut remaining,
                    &mut || Ok(())
                )
                .unwrap()
                .is_some()
        );
        assert_eq!(remaining, 0);
    }

    #[test]
    fn deep_argument_places_fail_explicitly_instead_of_truncating() {
        // Build the deep operand after ordinary analysis so this tests the
        // place pass's bound without exercising unrelated parser recursion.
        let source =
            "fn observe(value: i32) void {} fn main() void { var value = 1; observe(value); }";
        let mut program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        let mut analyzer = Analyzer::new();
        assert!(analyzer.analyze_program(&program).errors.is_empty());
        let ItemKind::Fn {
            body: Some(body), ..
        } = &mut program.items[1].node
        else {
            panic!()
        };
        let StmtKind::ExprStmt(call) = &mut body.stmts[1].node else {
            panic!()
        };
        let ExprKind::Call { args, .. } = &mut call.node else {
            panic!()
        };
        for _ in 0..MAX_PLACE_DEPTH {
            args[0] = Spanned::new(ExprKind::Group(Box::new(args[0].clone())), args[0].span);
        }
        analyzer
            .record_ownership_argument_places(&program, &mut || Ok(()))
            .unwrap();
        assert!(
            analyzer
                .errors
                .iter()
                .any(|error| error.code == "S14"
                    && error.message.contains("ownership argument facts")),
            "{:?}",
            analyzer.errors
        );
    }

    #[test]
    fn unavailable_edges_have_no_invented_arguments() {
        let report = analyze("fn missing(value: i32); fn main() void { missing(1); }");
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let missing = report
            .ownership_call_graph
            .iter()
            .find(|node| node.id.callable == "missing")
            .unwrap();
        assert!(missing.calls.iter().all(|site| matches!(
            site.target,
            OwnershipCallTarget::Unavailable
        ) && site.arguments.is_none()));
    }
}
