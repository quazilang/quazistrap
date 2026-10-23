// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Concrete source-call evidence for the future ownership/effect solver.
//! This is intentionally separate from the lexical temporary-loan shortcut:
//! neither a closed source graph nor a signature is an ownership certificate.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::*;

/// An operational compiler bound, not a truncation or a template fallback.
/// Recursive substitution otherwise grows types until the compiler exhausts
/// its stack/memory. Keep it shared by emission and ownership graph closure.
pub(super) const MAX_SPECIALIZATION_TYPE_DEPTH: usize = crate::type_resolution::MAX_TYPE_DEPTH;
pub(super) const MAX_SPECIALIZATION_TYPE_NODES: usize = crate::type_resolution::MAX_TYPE_NODES;

pub(super) fn specialization_types_are_bounded(types: &[TypeKind]) -> bool {
    let mut pending = types.iter().map(|ty| (ty, 1)).collect::<Vec<_>>();
    let mut visited = 0;
    while let Some((ty, depth)) = pending.pop() {
        visited += 1;
        if depth > MAX_SPECIALIZATION_TYPE_DEPTH || visited > MAX_SPECIALIZATION_TYPE_NODES {
            return false;
        }
        match ty {
            TypeKind::Named { type_args, .. } => {
                pending.extend(type_args.iter().map(|arg| (&arg.node, depth + 1)));
            }
            TypeKind::Ref { inner }
            | TypeKind::MutRef { inner }
            | TypeKind::RawPtr { inner }
            | TypeKind::Slice { elem_ty: inner }
            | TypeKind::FlexibleArray { elem_ty: inner }
            | TypeKind::Array { elem_ty: inner, .. } => pending.push((&inner.node, depth + 1)),
            TypeKind::Fn { params, return_ty } | TypeKind::CFn { params, return_ty } => {
                pending.extend(params.iter().map(|arg| (&arg.node, depth + 1)));
                pending.push((&return_ty.node, depth + 1));
            }
            _ => {}
        }
    }
    true
}

#[derive(Debug, Clone)]
pub(super) struct SourceOwnershipCall {
    pub(super) span: Span,
    /// Resolved declaration even when generic arguments are not yet complete.
    /// Never proves that this edge is concrete or ownership-closed.
    pub(super) declaration: Option<String>,
    pub(super) target: Option<(String, Vec<TypeKind>)>,
    pub(super) arguments: Option<OwnershipCallArguments>,
}

impl Analyzer {
    pub(super) fn has_monomorphization(
        &self,
        callable: &str,
        arguments: &[TypeKind],
        emitted: &str,
    ) -> bool {
        let identity = self.ownership_callable_id(callable, arguments);
        self.monomorphizations.iter().any(|mono| {
            mono.mangled_name == emitted
                && self.ownership_callable_id(&mono.fn_name, &mono.type_args) == identity
        })
    }

    pub(super) fn validate_specialization_identity(
        &mut self,
        callable: &str,
        arguments: &[TypeKind],
        span: Span,
    ) -> bool {
        if arguments.is_empty() {
            return true;
        }
        if !specialization_types_are_bounded(arguments) {
            self.push_error(span, "S14", format!(
                "specialization `{callable}` exceeds the compiler's type nesting limit ({MAX_SPECIALIZATION_TYPE_DEPTH} levels / {MAX_SPECIALIZATION_TYPE_NODES} type nodes); recursive specialization must have a finite bounded expansion"
            ));
            return false;
        }
        let resolved = arguments
            .iter()
            .map(|ty| self.resolve_type_aliases(ty))
            .collect::<Vec<_>>();
        if !specialization_types_are_bounded(&resolved) {
            self.push_error(span, "S14", format!(
                "specialization `{callable}` exceeds the compiler's type nesting limit ({MAX_SPECIALIZATION_TYPE_DEPTH} levels / {MAX_SPECIALIZATION_TYPE_NODES} type nodes) after alias resolution"
            ));
            return false;
        }
        // Some bytecode helper chunks still use symbolic template arguments.
        // They share the emitted namespace, so collisions must be diagnosed
        // there too until that template-emission path is removed coherently.
        let emitted_name = typecheck::mangle_monomorphized(callable, arguments);
        let binders = self
            .current_generic_params
            .iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        let symbolic = arguments
            .iter()
            .any(|ty| type_mentions_params(ty, &binders));
        if symbolic {
            self.symbolic_specialization_names
                .insert(emitted_name.clone());
        }
        let identity = self.ownership_callable_id(callable, &resolved);
        if let Some(previous) = self.specialization_identities.get(&emitted_name) {
            if previous != &identity {
                self.push_error(span, "S14", format!(
                    "distinct specializations of `{callable}` collide at bytecode symbol `{emitted_name}`; the current bytecode name mangling cannot represent both"
                ));
                return false;
            }
        } else {
            if self.specialization_identities.len() >= u16::MAX as usize {
                self.push_error(
                    span,
                    "S14",
                    "specialization count exceeds the bytecode function-index limit".to_string(),
                );
                return false;
            }
            self.specialization_identities
                .insert(emitted_name, identity.clone());
        }
        if !symbolic
            && resolved
                .iter()
                .all(|ty| self.ownership_type_is_concrete(ty))
        {
            self.concrete_specialization_ids.insert(identity);
        }
        true
    }

    /// Start every source call conservatively. Resolution may replace this
    /// site's unknown target without changing the existing lexical call facts.
    pub(super) fn record_ownership_call_site(
        &mut self,
        span: Span,
        target: Option<(&str, &[TypeKind])>,
    ) {
        let Some(caller) = self.current_function.last() else {
            return;
        };
        if !self.source_function_symbols.contains(caller) {
            return;
        }
        let calls = self.ownership_call_sites.entry(caller.clone()).or_default();
        let declaration = target.map(|(name, _)| name.to_string());
        let target = target.map(|(name, args)| (name.to_string(), args.to_vec()));
        if let Some(site) = calls.get_mut(&(span.start, span.end)) {
            // Named-argument checking may visit a site twice. Preserve its
            // resolved identity when a later visit starts conservatively.
            if target.is_some() {
                site.declaration = declaration;
                site.target = target;
            }
        } else {
            calls.insert(
                (span.start, span.end),
                SourceOwnershipCall {
                    span,
                    declaration,
                    target,
                    arguments: None,
                },
            );
        }
    }

    pub(super) fn record_source_call_declaration(&mut self, span: Span, declaration: &str) {
        self.record_ownership_call_site(span, None);
        if let Some(site) = self
            .current_function
            .last()
            .and_then(|caller| self.ownership_call_sites.get_mut(caller))
            .and_then(|sites| sites.get_mut(&(span.start, span.end)))
        {
            site.declaration = Some(declaration.to_string());
        }
    }

    pub(super) fn ownership_callable_id(
        &self,
        name: &str,
        args: &[TypeKind],
    ) -> OwnershipCallableId {
        OwnershipCallableId {
            callable: name.to_string(),
            type_arguments: args
                .iter()
                .map(|ty| canonical_ownership_type(&self.resolve_type_aliases(ty)))
                .collect(),
        }
    }

    fn concrete_ownership_signature(
        &self,
        template: &CallableOwnershipSummary,
        args: &[TypeKind],
    ) -> Option<CallableOwnershipSummary> {
        let generic_params = self.generic_params_for_function(&template.callable);
        if !specialization_types_are_bounded(args)
            || generic_params.len() != args.len()
            // These arguments came from concrete source sites, not symbolic
            // mono roots. A global type may have the same spelling as the
            // callee's binder without being that binder.
            || args.iter().any(|ty| !self.ownership_type_is_concrete(ty))
        {
            return None;
        }
        let symbol = self.resolve_symbol(&template.callable)?;
        let substitution = generic_params
            .into_iter()
            .zip(args.iter().cloned())
            .collect::<HashMap<_, _>>();
        let params = symbol
            .params
            .iter()
            .map(|ty| self.resolve_type_aliases(&substitute_type_kind(ty, &substitution)))
            .collect::<Vec<_>>();
        let result = symbol
            .ty
            .as_ref()
            .map(|ty| self.resolve_type_aliases(&substitute_type_kind(ty, &substitution)));
        if params.iter().enumerate().any(|(index, ty)| {
            !(self.ownership_type_is_concrete(ty)
                || (matches!(ty, TypeKind::Any) && symbol.variadic && index + 1 == params.len()))
        }) || result
            .as_ref()
            .is_some_and(|ty| !self.ownership_type_is_concrete(ty))
        {
            return None;
        }
        let receiver_count = usize::from(template.receiver.is_some());
        let fixed_count = params.len().checked_sub(usize::from(symbol.variadic))?;
        let parameters = params
            .get(receiver_count..fixed_count)?
            .iter()
            .map(|ty| self.ownership_capability_for_type(ty))
            .collect::<Option<Vec<_>>>()?;
        let receiver = if receiver_count == 1 {
            Some(self.ownership_capability_for_type(params.first()?)?)
        } else {
            None
        };
        let variadic = if symbol.variadic {
            let ty = params.last()?;
            Some(if matches!(ty, TypeKind::Any) {
                VariadicOwnership::Erased
            } else {
                VariadicOwnership::Elements(self.ownership_capability_for_type(ty)?)
            })
        } else {
            None
        };
        Some(CallableOwnershipSummary {
            callable: template.callable.clone(),
            receiver,
            parameters,
            variadic,
            result: self.ownership_result_for_type(result.as_ref())?,
            has_body: template.has_body,
            generic_template: false,
            // These are the lexical shortcut's flags, not graph permissions.
            // Do not promote a concrete signature into that separate proof.
            direct_call_eligible: false,
            source_effects_verified: false,
            transitive_effects_verified: false,
        })
    }

    pub(super) fn ownership_type_is_concrete(&self, ty: &TypeKind) -> bool {
        match ty {
            TypeKind::Any | TypeKind::Error | TypeKind::FlexibleArray { .. } => false,
            TypeKind::Named { name, type_args } => {
                self.resolve_symbol(name)
                    .is_some_and(|symbol| symbol.kind == SymbolKind::TypeName)
                    && self.declared_type_parameters(name).map_or(0, Vec::len) == type_args.len()
                    && type_args
                        .iter()
                        .all(|arg| self.ownership_type_is_concrete(&arg.node))
            }
            TypeKind::Ref { inner }
            | TypeKind::MutRef { inner }
            | TypeKind::RawPtr { inner }
            | TypeKind::Slice { elem_ty: inner } => self.ownership_type_is_concrete(&inner.node),
            TypeKind::Array { elem_ty, .. } => self.ownership_type_is_concrete(&elem_ty.node),
            TypeKind::Fn { params, return_ty } | TypeKind::CFn { params, return_ty } => {
                params
                    .iter()
                    .all(|arg| self.ownership_type_is_concrete(&arg.node))
                    && self.ownership_type_is_concrete(&return_ty.node)
            }
            _ => true,
        }
    }

    pub(super) fn build_ownership_call_graph(
        &mut self,
        templates: &[CallableOwnershipSummary],
        checkpoint: &mut impl FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<Vec<OwnershipCallable>, crate::cancel::Cancelled> {
        self.build_ownership_call_graph_with_limit(templates, checkpoint, u16::MAX as usize)
    }

    fn build_ownership_call_graph_with_limit(
        &mut self,
        templates: &[CallableOwnershipSummary],
        checkpoint: &mut impl FnMut() -> Result<(), crate::cancel::Cancelled>,
        limit: usize,
    ) -> Result<Vec<OwnershipCallable>, crate::cancel::Cancelled> {
        let templates = templates
            .iter()
            .map(|summary| (summary.callable.as_str(), summary))
            .collect::<BTreeMap<_, _>>();
        let mut pending = VecDeque::new();
        let mut scheduled = BTreeSet::new();
        let mut overflow_span = None;
        // Concrete declarations are roots. Discover specializations from real
        // source sites, since template mono names lose generic binding context.
        // The closure below does not depend on a mangled-name lookup: two types
        // that collide there must still have distinct ownership identities.
        for template in templates.values() {
            checkpoint()?;
            if self
                .generic_params_for_function(&template.callable)
                .is_empty()
            {
                if scheduled.len() < limit {
                    scheduled.insert(self.ownership_callable_id(&template.callable, &[]));
                    pending.push_back((template.callable.clone(), Vec::new()));
                } else {
                    overflow_span = self
                        .resolve_symbol(&template.callable)
                        .map(|symbol| symbol.span);
                }
            }
        }
        let mut nodes = BTreeMap::new();
        let mut argument_budget = super::places::MAX_GRAPH_ARGUMENT_NODES;
        let mut argument_overflow = None;
        while let Some((name, args)) = pending.pop_front() {
            checkpoint()?;
            let args = args
                .iter()
                .map(|ty| self.resolve_type_aliases(ty))
                .collect::<Vec<_>>();
            let id = self.ownership_callable_id(&name, &args);
            if nodes.contains_key(&id) {
                continue;
            }
            let Some(template) = templates.get(name.as_str()) else {
                continue;
            };
            let Some(signature) = self.concrete_ownership_signature(template, &args) else {
                continue;
            };
            let substitution = self
                .generic_params_for_function(&name)
                .into_iter()
                .zip(args)
                .collect::<HashMap<_, _>>();
            let mut calls = Vec::new();
            if !signature.has_body {
                calls.push(OwnershipCallSite {
                    span: self.resolve_symbol(&name).expect("declared callable").span,
                    target: OwnershipCallTarget::Unavailable,
                    arguments: None,
                });
            } else if let Some(sites) = self.ownership_call_sites.get(&name) {
                for site in sites.values() {
                    checkpoint()?;
                    let target = if let Some((callee, arguments)) = &site.target {
                        let arguments = arguments
                            .iter()
                            .map(|ty| {
                                self.resolve_type_aliases(&substitute_type_kind(ty, &substitution))
                            })
                            .collect::<Vec<_>>();
                        let target_id = self.ownership_callable_id(callee, &arguments);
                        if templates.get(callee.as_str()).is_some_and(|template| {
                            self.concrete_ownership_signature(template, &arguments)
                                .is_some()
                        }) {
                            if scheduled.contains(&target_id) {
                                OwnershipCallTarget::Direct(target_id)
                            } else if scheduled.len() < limit {
                                scheduled.insert(target_id.clone());
                                pending.push_back((callee.clone(), arguments));
                                OwnershipCallTarget::Direct(target_id)
                            } else {
                                overflow_span = Some(site.span);
                                OwnershipCallTarget::Unresolved(target_id)
                            }
                        } else {
                            OwnershipCallTarget::Unresolved(target_id)
                        }
                    } else {
                        OwnershipCallTarget::Opaque
                    };
                    let arguments = match &site.arguments {
                        Some(arguments) => {
                            let result = self.instantiate_ownership_arguments(
                                arguments,
                                &substitution,
                                &mut argument_budget,
                                checkpoint,
                            )?;
                            if result.is_none() {
                                argument_overflow.get_or_insert(site.span);
                            }
                            result
                        }
                        None => None,
                    };
                    calls.push(OwnershipCallSite {
                        span: site.span,
                        target,
                        arguments,
                    });
                }
            }
            calls.sort_by(|left, right| {
                (left.span.start, left.span.end, &left.target).cmp(&(
                    right.span.start,
                    right.span.end,
                    &right.target,
                ))
            });
            nodes.insert(
                id.clone(),
                OwnershipCallable {
                    id,
                    signature,
                    calls,
                    control_flow: None,
                    source_calls_closed: false,
                },
            );
        }
        if let Some(span) = overflow_span {
            self.push_error(
                span,
                "S14",
                format!("ownership call graph exceeds its {limit}-callable compiler limit"),
            );
        }
        if let Some(span) = argument_overflow {
            self.push_error(
                span,
                "S14",
                format!(
                    "ownership argument graph exceeds its {}-node compiler limit",
                    super::places::MAX_GRAPH_ARGUMENT_NODES
                ),
            );
        }
        close_source_calls(&mut nodes, self, checkpoint)?;
        Ok(nodes.into_values().collect())
    }
}

/// Tags distinguish nested constructors; length-prefixed user names prevent
/// punctuation in a name from being interpreted as part of the encoding.
fn canonical_ownership_type(ty: &TypeKind) -> String {
    let encode = canonical_ownership_type;
    let list = |types: &[Type]| {
        types
            .iter()
            .map(|ty| encode(&ty.node))
            .collect::<Vec<_>>()
            .join(",")
    };
    match ty {
        TypeKind::Named { name, type_args } => {
            format!("named({}:{}[{}])", name.len(), name, list(type_args))
        }
        TypeKind::Array { elem_ty, len } => format!("array({len};{})", encode(&elem_ty.node)),
        TypeKind::FlexibleArray { elem_ty } => format!("flexible({})", encode(&elem_ty.node)),
        TypeKind::Slice { elem_ty } => format!("slice({})", encode(&elem_ty.node)),
        TypeKind::Ref { inner } => format!("shared({})", encode(&inner.node)),
        TypeKind::MutRef { inner } => format!("exclusive({})", encode(&inner.node)),
        TypeKind::RawPtr { inner } => format!("pointer({})", encode(&inner.node)),
        TypeKind::Fn { params, return_ty } => {
            format!("fn([{}];{})", list(params), encode(&return_ty.node))
        }
        TypeKind::CFn { params, return_ty } => {
            format!("cfn([{}];{})", list(params), encode(&return_ty.node))
        }
        TypeKind::Dyn { trait_name } => format!("dyn({}:{trait_name})", trait_name.len()),
        _ => ty.to_string(),
    }
}

/// Propagate rejected boundaries backward in linear graph work. Safe recursive
/// components survive only if no member can reach an opaque/unavailable call.
fn close_source_calls(
    nodes: &mut BTreeMap<OwnershipCallableId, OwnershipCallable>,
    analyzer: &Analyzer,
    checkpoint: &mut impl FnMut() -> Result<(), crate::cancel::Cancelled>,
) -> Result<(), crate::cancel::Cancelled> {
    let mut rejected = BTreeSet::new();
    let mut callers: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    for (id, node) in nodes.iter() {
        checkpoint()?;
        let symbol = analyzer
            .resolve_symbol(&id.callable)
            .expect("declared callable");
        if !node.signature.has_body
            || symbol.unsafe_fn
            || symbol.variadic
            || symbol
                .attributes
                .iter()
                .any(|attribute| attribute == "intrinsic")
        {
            rejected.insert(id.clone());
        }
        for call in &node.calls {
            checkpoint()?;
            match &call.target {
                OwnershipCallTarget::Direct(callee) if nodes.contains_key(callee) => {
                    callers
                        .entry(callee.clone())
                        .or_default()
                        .insert(id.clone());
                }
                _ => {
                    rejected.insert(id.clone());
                }
            }
        }
    }
    let mut pending = rejected.iter().cloned().collect::<VecDeque<_>>();
    while let Some(callee) = pending.pop_front() {
        checkpoint()?;
        if let Some(parents) = callers.get(&callee) {
            for parent in parents {
                checkpoint()?;
                if rejected.insert(parent.clone()) {
                    pending.push_back(parent.clone());
                }
            }
        }
    }
    for (id, node) in nodes {
        checkpoint()?;
        node.source_calls_closed = !rejected.contains(id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer::Lexer, parser::Parser};

    fn analyze(source: &str) -> SemanticReport {
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        Analyzer::new().analyze_program(&program)
    }

    fn id(name: &str, args: &[&str]) -> OwnershipCallableId {
        OwnershipCallableId {
            callable: name.to_string(),
            type_arguments: args
                .iter()
                .map(|arg| match *arg {
                    "i32" => arg.to_string(),
                    "[i32; 3]" => "array(3;i32)".to_string(),
                    _ => format!("named({}:{}[])", arg.len(), arg),
                })
                .collect(),
        }
    }

    fn node<'a>(report: &'a SemanticReport, name: &str, args: &[&str]) -> &'a OwnershipCallable {
        let identity = id(name, args);
        report
            .ownership_call_graph
            .iter()
            .find(|node| node.id == identity)
            .unwrap_or_else(|| panic!("missing {identity:?}: {:?}", report.ownership_call_graph))
    }

    #[test]
    fn ownership_graph_retains_leaf_flow_and_shares_template_flow() {
        let report = analyze(
            "fn leaf[T](value: T) T { ret value; } \
             fn ordinary() void {} fn unavailable() void; \
             fn main() void { leaf[i32](1); leaf[bool](true); }",
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(node(&report, "ordinary", &[]).control_flow.is_some());
        assert!(node(&report, "unavailable", &[]).control_flow.is_none());
        let leaves = report
            .ownership_call_graph
            .iter()
            .filter(|node| node.id.callable == "leaf")
            .collect::<Vec<_>>();
        assert_eq!(leaves.len(), 2);
        let first = leaves[0].control_flow.as_ref().unwrap();
        let second = leaves[1].control_flow.as_ref().unwrap();
        assert!(std::sync::Arc::ptr_eq(first, second));
        for leaf in leaves {
            assert!(leaf.source_calls_closed);
            assert!(!leaf.signature.direct_call_eligible);
            assert!(!leaf.signature.transitive_effects_verified);
        }
    }

    #[test]
    fn ownership_graph_closes_concrete_calls_and_recomputes_capabilities() {
        let report = analyze(
            r#"
struct Token { value: i32 }
fn ordinary() void {}
fn leaf[T](value: T) T { ordinary(); ret value; }
fn middle[U](value: U) U { ret leaf[U](value); }
fn outer[V](value: V) V { ret middle[V](value); }
fn main() void {
    outer[i32](7);
    outer[Token](Token { value: 8 });
}
"#,
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        for (ty, capability) in [
            ("i32", OwnershipCapability::Copy),
            ("Token", OwnershipCapability::Move),
        ] {
            for (caller, callee) in [("outer", "middle"), ("middle", "leaf")] {
                let record = node(&report, caller, &[ty]);
                assert_eq!(record.signature.parameters, vec![capability]);
                assert_eq!(record.signature.result, OwnershipResult::Value(capability));
                assert_eq!(record.calls.len(), 1);
                assert_eq!(
                    record.calls[0].target,
                    OwnershipCallTarget::Direct(id(callee, &[ty]))
                );
                assert!(record.source_calls_closed);
                assert!(!record.signature.direct_call_eligible);
                assert!(!record.signature.transitive_effects_verified);
            }
            assert_eq!(
                node(&report, "leaf", &[ty]).calls[0].target,
                OwnershipCallTarget::Direct(id("ordinary", &[]))
            );
        }
        assert!(!report.ownership_call_graph.iter().any(|node| {
            node.id
                .type_arguments
                .iter()
                .any(|ty| ["T", "U", "V"].contains(&ty.as_str()))
        }));
    }

    #[test]
    fn ownership_graph_canonicalizes_aliases_and_preserves_call_sites() {
        let source = r#"
type Count = i32;
fn leaf[T](value: T) T { ret value; }
fn main() void { leaf[Count](1); leaf[i32](2); }
"#;
        let report = analyze(source);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(
            report
                .ownership_call_graph
                .iter()
                .filter(|node| node.id.callable == "leaf")
                .count(),
            1
        );
        let calls = &node(&report, "main", &[]).calls;
        assert_eq!(calls.len(), 2);
        assert_ne!(calls[0].span.start, calls[1].span.start);
        for call in calls {
            assert_eq!(
                call.target,
                OwnershipCallTarget::Direct(id("leaf", &["i32"]))
            );
            assert!(source[call.span.start..call.span.end].starts_with("leaf["));
        }
        let identities = report
            .ownership_call_graph
            .iter()
            .map(|node| &node.id)
            .collect::<Vec<_>>();
        let again = analyze(source);
        assert_eq!(
            identities,
            again
                .ownership_call_graph
                .iter()
                .map(|node| &node.id)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn ownership_graph_does_not_merge_colliding_bytecode_names() {
        let report = analyze(
            r#"
struct _i32__3_ { value: i32 }
fn leaf[T](value: T) T { ret value; }
fn middle[U](value: U) U { ret leaf[U](value); }
fn main() void {
    middle[[i32; 3]]([1, 2, 3]);
    middle[_i32__3_](_i32__3_ { value: 4 });
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.code == "S14"
                    && error.message.contains("collide at bytecode symbol")),
            "{:?}",
            report.errors
        );
        let array = TypeKind::Array {
            elem_ty: Box::new(Spanned::new(TypeKind::Int32, Span::new(0, 0, 0, 0))),
            len: 3,
        };
        let named = TypeKind::Named {
            name: "_i32__3_".to_string(),
            type_args: Vec::new(),
        };
        assert_eq!(
            typecheck::mangle_monomorphized("middle", &[array]),
            typecheck::mangle_monomorphized("middle", &[named])
        );
        for name in ["middle", "leaf"] {
            assert_eq!(
                node(&report, name, &["[i32; 3]"]).signature.parameters,
                vec![OwnershipCapability::Move]
            );
            assert_eq!(
                node(&report, name, &["_i32__3_"]).signature.parameters,
                vec![OwnershipCapability::Move]
            );
        }
    }

    #[test]
    fn ownership_graph_propagates_opaque_boundaries_through_recursion() {
        let report = analyze(
            r#"
fn safe_left[T](value: T) T { ret safe_right[T](value); }
fn safe_right[U](value: U) U { ret safe_left[U](value); }
fn unavailable() void;
fn bad_left[T](value: T) T { ret bad_right[T](value); }
fn bad_right[U](value: U) U { unavailable(); ret bad_left[U](value); }
fn main() void { safe_left[i32](1); bad_left[i32](2); }
"#,
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        for name in ["safe_left", "safe_right"] {
            assert!(node(&report, name, &["i32"]).source_calls_closed);
        }
        for name in ["bad_left", "bad_right"] {
            assert!(!node(&report, name, &["i32"]).source_calls_closed);
        }
        assert!(!node(&report, "main", &[]).source_calls_closed);
        assert_eq!(
            node(&report, "unavailable", &[]).calls[0].target,
            OwnershipCallTarget::Unavailable
        );
    }

    #[test]
    fn ownership_graph_preserves_shadowed_indirect_and_missing_calls() {
        let report = analyze(
            r#"
fn target() i32 { ret 1; }
fn indirect(target: fn() i32) i32 { ret target(); }
fn broken() void { missing(); }
fn main() void {}
"#,
        );
        assert!(report.errors.iter().any(|error| error.code == "S04"));
        for name in ["indirect", "broken"] {
            let record = node(&report, name, &[]);
            assert_eq!(record.calls[0].target, OwnershipCallTarget::Opaque);
            assert!(!record.source_calls_closed);
        }
        assert!(node(&report, "target", &[]).source_calls_closed);
    }

    #[test]
    fn ownership_graph_records_inherent_specializations_without_widening_loans() {
        let report = analyze(
            r#"
struct Token { value: i32 }
struct Holder[T] { value: T }
impl Holder[T] {
    fn observe(self: &Holder[T]) void {}
}
fn caller() void {
    var holder: Holder[Token] = Holder { value: Token { value: 1 } };
    holder.observe();
}
fn main() void {}
"#,
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let method = node(&report, "Holder.observe", &["Token"]);
        method.control_flow.as_ref().unwrap().validate().unwrap();
        assert_eq!(
            method.signature.receiver,
            Some(OwnershipCapability::SharedBorrow)
        );
        assert!(method.signature.parameters.is_empty());
        let caller = node(&report, "caller", &[]);
        assert_eq!(
            caller.calls[0].target,
            OwnershipCallTarget::Direct(id("Holder.observe", &["Token"]))
        );
        assert!(caller.source_calls_closed);
        let lexical = report
            .callable_ownership_summaries
            .iter()
            .find(|summary| summary.callable == "caller")
            .unwrap();
        assert!(
            !lexical.direct_call_eligible,
            "method syntax remains outside the lexical shortcut"
        );
    }

    #[test]
    fn ownership_graph_structural_identity_distinguishes_ambiguous_display() {
        let scalar = Spanned::new(TypeKind::Int32, Span::new(1, 1, 0, 1));
        let shared_exclusive = TypeKind::Ref {
            inner: Box::new(Spanned::new(
                TypeKind::MutRef {
                    inner: Box::new(scalar.clone()),
                },
                scalar.span,
            )),
        };
        let exclusive_shared = TypeKind::MutRef {
            inner: Box::new(Spanned::new(
                TypeKind::Ref {
                    inner: Box::new(scalar.clone()),
                },
                scalar.span,
            )),
        };
        assert_eq!(shared_exclusive.to_string(), exclusive_shared.to_string());
        assert_ne!(
            canonical_ownership_type(&shared_exclusive),
            canonical_ownership_type(&exclusive_shared)
        );
        let other_span = TypeKind::Ref {
            inner: Box::new(Spanned::new(
                TypeKind::MutRef {
                    inner: Box::new(Spanned::new(TypeKind::Int32, Span::new(9, 8, 7, 6))),
                },
                Span::new(5, 4, 3, 2),
            )),
        };
        assert_eq!(
            canonical_ownership_type(&shared_exclusive),
            canonical_ownership_type(&other_span)
        );
    }

    #[test]
    fn ownership_graph_rejects_unsafe_intrinsic_and_unresolved_descendants() {
        let report = analyze(
            r#"
unsafe fn danger() void {}
@intrinsic("quazi.size_of")
fn layout[T]() usize;
fn unsafe_wrapper() void { unsafe { danger(); } }
fn intrinsic_wrapper() void { layout[i32](); }
fn generic[T]() void {}
fn wrong_arguments() void { generic[i32, u32](); }
fn main() void {}
"#,
        );
        assert!(!report.errors.is_empty());
        for name in [
            "danger",
            "unsafe_wrapper",
            "intrinsic_wrapper",
            "wrong_arguments",
        ] {
            assert!(!node(&report, name, &[]).source_calls_closed, "{name}");
        }
        assert!(matches!(
            node(&report, "wrong_arguments", &[]).calls[0].target,
            OwnershipCallTarget::Unresolved(_)
        ));
    }

    #[test]
    fn ownership_graph_deduplicates_named_argument_visits_at_one_site() {
        let report = analyze(
            r#"
type Count = i32;
fn leaf[T](value: T) T { ret value; }
fn sink(value: i32) void {}
fn main() void { sink(value = leaf[Count](1)); }
"#,
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let calls = &node(&report, "main", &[]).calls;
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[0].target,
            OwnershipCallTarget::Direct(id("sink", &[]))
        );
        assert_eq!(
            calls[1].target,
            OwnershipCallTarget::Direct(id("leaf", &["i32"]))
        );
    }

    #[test]
    fn ownership_graph_build_obeys_cooperative_cancellation() {
        let program = Parser::new(
            Lexer::new("fn a() void { b(); } fn b() void {} fn main() void { a(); }").tokenize(),
        )
        .parse()
        .unwrap();
        let mut analyzer = Analyzer::new();
        let report = analyzer.analyze_program(&program);
        let mut polls = 0;
        let result =
            analyzer.build_ownership_call_graph(&report.callable_ownership_summaries, &mut || {
                polls += 1;
                if polls == 6 {
                    Err(crate::cancel::Cancelled)
                } else {
                    Ok(())
                }
            });
        assert!(matches!(result, Err(crate::cancel::Cancelled)));
        assert_eq!(polls, 6);
    }

    #[test]
    fn ownership_graph_retains_qualified_import_alias_identity() {
        let module = "pub fn leaf[T](value: T) T { ret value; }";
        let main = "import helpers; import helpers.leaf as renamed; fn main() void { renamed[i32](1); helpers.leaf[i32](2); }";
        let merged = format!("{module}\n{main}");
        let files = vec![
            SourceFile {
                path: "helpers.qz".to_string(),
                module_name: Some("helpers".to_string()),
                runtime_string: None,
                type_imports: Vec::new(),
                start: 0,
                end: module.len(),
                line_start: 1,
            },
            SourceFile {
                path: "main.qz".to_string(),
                module_name: None,
                runtime_string: None,
                type_imports: Vec::new(),
                start: module.len() + 1,
                end: merged.len(),
                line_start: 2,
            },
        ];
        let program =
            Parser::new_with_source_files(Lexer::new(&merged).tokenize(), &merged, files.clone())
                .parse()
                .unwrap();
        let mut analyzer = Analyzer::new();
        analyzer.set_source_files(files);
        analyzer.set_namespaced_paths(HashSet::from(["helpers.qz".to_string()]));
        let report = analyzer.analyze_program(&program);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let calls = &node(&report, "main", &[]).calls;
        assert_eq!(calls.len(), 2);
        for call in calls {
            assert_eq!(
                call.target,
                OwnershipCallTarget::Direct(id("helpers.leaf", &["i32"]))
            );
        }
        assert!(
            node(&report, "helpers.leaf", &["i32"])
                .control_flow
                .is_some()
        );
        assert_eq!(
            node(&report, "helpers.leaf", &["i32"]).signature.parameters,
            vec![OwnershipCapability::Copy]
        );
    }

    #[test]
    fn ownership_graph_resolves_nested_generic_aliases() {
        let report = analyze(
            r#"
struct Token { value: i32 }
struct Holder[T] { value: T }
type Alias[T] = Holder[T];
fn identity[T](value: T) T { ret value; }
fn main() void {
    var held: Holder[Token] = Holder { value: Token { value: 1 } };
    identity[Alias[Token]](held);
}
"#,
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let target = node(&report, "main", &[]).calls[0].target.clone();
        let OwnershipCallTarget::Direct(identity) = target else {
            panic!("{target:?}");
        };
        assert_eq!(
            identity.type_arguments,
            vec!["named(6:Holder[named(5:Token[])])"]
        );
        let record = report
            .ownership_call_graph
            .iter()
            .find(|node| node.id == identity)
            .unwrap();
        assert_eq!(record.signature.parameters, vec![OwnershipCapability::Move]);
        assert_eq!(
            record.signature.result,
            OwnershipResult::Value(OwnershipCapability::Move)
        );
    }

    #[test]
    fn ownership_graph_limits_duplicating_type_substitution() {
        let report = analyze(
            r#"
struct Pair[A, B] { first: A, second: B }
fn grow[T]() void { grow[Pair[T, T]](); }
fn main() void { grow[i32](); }
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.code == "S14" && error.message.contains("4096 type nodes")),
            "{:?}",
            report.errors
        );
        assert!(!node(&report, "main", &[]).source_calls_closed);
        assert!(report.ownership_call_graph.len() < MAX_SPECIALIZATION_TYPE_DEPTH);
    }

    #[test]
    fn ownership_graph_limits_branching_specialization_count_explicitly() {
        let source = "fn branch[T]() void { branch[[T; 1]](); branch[[T; 2]](); } fn main() void { branch[i32](); }";
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        let mut analyzer = Analyzer::new();
        for item in &program.items {
            analyzer.declare_top_level_item(item);
        }
        for item in &program.items {
            analyzer.type_check_item(item, &mut || Ok(())).unwrap();
        }
        let summaries =
            analyzer.derive_callable_ownership_summaries(&analyzer.build_symbol_table());
        let graph = analyzer
            .build_ownership_call_graph_with_limit(&summaries, &mut || Ok(()), 8)
            .unwrap();
        assert!(analyzer.errors.iter().any(
            |error| error.code == "S14" && error.message.contains("8-callable compiler limit")
        ));
        assert_eq!(graph.len(), 8);
        assert!(graph.iter().all(|node| !node.source_calls_closed));
        assert!(graph.iter().any(|node| {
            node.calls
                .iter()
                .any(|call| matches!(call.target, OwnershipCallTarget::Unresolved(_)))
        }));
    }

    #[test]
    fn ownership_graph_does_not_seed_shadowed_template_arguments() {
        let report = analyze(
            r#"
struct U { value: i32 }
fn leaf[T]() void {}
fn caller[U]() void { leaf[U](); }
fn main() void { caller[i32](); }
"#,
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(
            report
                .ownership_call_graph
                .iter()
                .any(|node| node.id == id("leaf", &["i32"]))
        );
        assert!(
            !report
                .ownership_call_graph
                .iter()
                .any(|node| node.id == id("leaf", &["U"]))
        );
    }

    #[test]
    fn ownership_graph_keeps_concrete_types_named_like_callable_binders() {
        let source = r#"
struct T { value: i32 }
fn identity[T](value: T) T { ret value; }
fn leaf[U](value: U) U { ret value; }
fn caller[T](value: T) T { ret leaf[T](value); }
fn main() void {
    identity[T](T { value: 1 });
    caller[T](T { value: 2 });
}
"#;
        let report = analyze(source);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        for name in ["identity", "caller", "leaf"] {
            let concrete = node(&report, name, &["T"]);
            assert_eq!(
                concrete.signature.parameters,
                vec![OwnershipCapability::Move]
            );
            assert_eq!(
                concrete.signature.result,
                OwnershipResult::Value(OwnershipCapability::Move)
            );
            assert!(concrete.source_calls_closed, "{name}: {concrete:?}");
        }
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        crate::bytecode::codegen::Codegen::new(&report)
            .compile_program(&program, &[])
            .expect("globally declared concrete T must lower despite a same-named callable binder");
    }

    #[test]
    fn concrete_same_named_alias_arguments_are_resolved_before_lowering() {
        let source = "type T = [i32; 3]; fn identity[T](value: T) T { ret value; } fn main() i32 { var result = identity[T]([11, 22, 33]); ret result[2] - 33; }";
        let report = analyze(source);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let mono = report
            .monomorphizations
            .iter()
            .find(|mono| mono.mangled_name == "identity<T>")
            .unwrap();
        assert!(matches!(
            mono.type_args.as_slice(),
            [TypeKind::Array { len: 3, .. }]
        ));
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        crate::bytecode::codegen::Codegen::new(&report)
            .compile_program(&program, &[])
            .unwrap();
    }

    #[test]
    fn impl_formal_binders_shadow_global_aliases() {
        let source = "type T = [i32; 3]; struct Holder[T] { value: T, } impl Holder[T] { fn forward(self: &Holder[T], value: T) T { ret value; } } fn main() i32 { var held: Holder[i32] = Holder { value: 1 }; ret held.forward(7) - 7; }";
        let report = analyze(source);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(
            report.impl_generic_params.get("Holder.forward"),
            Some(&vec!["T".to_string()])
        );
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        crate::bytecode::codegen::Codegen::new(&report)
            .compile_program(&program, &[])
            .unwrap();
    }

    #[test]
    fn symbolic_helpers_keep_their_registration_binder_context() {
        let report = analyze(
            "type T = [i32; 3]; fn leaf[U]() void {} fn caller[T]() void { leaf[T](); } fn main() void { caller[i32](); }",
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(
            report
                .monomorphizations
                .iter()
                .any(|mono| mono.mangled_name == "leaf<i32>")
        );
        assert!(
            !report
                .ownership_call_graph
                .iter()
                .any(|node| node.id == id("leaf", &["[i32; 3]"]))
        );
    }

    #[test]
    fn enum_formal_parameters_support_concrete_graph_signatures() {
        let report = analyze(
            "enum Choice[T] { One(T), Empty, } fn take(value: Choice[i32]) void {} fn main() void { var choice: Choice[i32] = Choice.One(7); take(choice); }",
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let concrete = node(&report, "take", &[]);
        assert_eq!(
            concrete.signature.parameters,
            vec![OwnershipCapability::Move]
        );
    }

    #[test]
    fn generic_copy_templates_ignore_same_named_global_owners() {
        let report = analyze(
            "struct T { value: i32 } fn repeat[T](value: T) T { const again = value; ret value; } fn main() void { repeat[i32](7); }",
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }

    #[test]
    fn analyzer_reuse_resets_specialization_binding_provenance() {
        let mut analyzer = Analyzer::new();
        let first = Parser::new(Lexer::new("type T = i32; fn leaf[U]() void {} fn caller[T]() void { leaf[T](); } fn main() void { caller[u32](); }").tokenize()).parse().unwrap();
        let report = analyzer.analyze_program(&first);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(analyzer.symbolic_specialization_names.contains("leaf<T>"));
        let second = Parser::new(Lexer::new("struct T { value: i32 } fn leaf[U](value: U) void {} fn main() void { leaf[T](T { value: 1 }); }").tokenize()).parse().unwrap();
        let report = analyzer.analyze_program(&second);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(!analyzer.symbolic_specialization_names.contains("leaf<T>"));
        assert!(
            report
                .ownership_call_graph
                .iter()
                .any(|record| record.id == id("leaf", &["T"]))
        );
        assert!(
            !report
                .ownership_call_graph
                .iter()
                .any(|record| record.id == id("leaf", &["u32"]))
        );
    }

    #[test]
    fn ownership_graph_does_not_infer_method_generics_from_a_module_name() {
        let module = "pub fn leaf() void {}";
        let main = "struct helpers[T] { value: T } import helpers.leaf as renamed; fn main() void { renamed(); }";
        let merged = format!("{module}\n{main}");
        let files = vec![
            SourceFile {
                path: "helpers.qz".to_string(),
                module_name: Some("helpers".to_string()),
                runtime_string: None,
                type_imports: Vec::new(),
                start: 0,
                end: module.len(),
                line_start: 1,
            },
            SourceFile {
                path: "main.qz".to_string(),
                module_name: None,
                runtime_string: None,
                type_imports: Vec::new(),
                start: module.len() + 1,
                end: merged.len(),
                line_start: 2,
            },
        ];
        let program =
            Parser::new_with_source_files(Lexer::new(&merged).tokenize(), &merged, files.clone())
                .parse()
                .unwrap();
        let mut analyzer = Analyzer::new();
        analyzer.set_source_files(files);
        analyzer.set_namespaced_paths(HashSet::from(["helpers.qz".to_string()]));
        let report = analyzer.analyze_program(&program);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(
            node(&report, "main", &[]).calls[0].target,
            OwnershipCallTarget::Direct(id("helpers.leaf", &[]))
        );
        assert!(node(&report, "helpers.leaf", &[]).source_calls_closed);
    }

    #[test]
    fn ownership_graph_and_emission_reject_unbounded_generic_expansion() {
        let report = analyze(
            r#"
fn expand[T](value: T) void { expand[[T; 1]]([value]); }
fn main() void { expand[i32](1); }
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.code == "S14" && error.message.contains("type nesting limit")),
            "{:?}",
            report.errors
        );
        assert!(!node(&report, "main", &[]).source_calls_closed);
        assert!(report.ownership_call_graph.len() <= MAX_SPECIALIZATION_TYPE_DEPTH + 1);
        assert!(report.ownership_call_graph.iter().any(|node| {
            node.calls
                .iter()
                .any(|call| matches!(call.target, OwnershipCallTarget::Unresolved(_)))
        }));
    }

    #[test]
    fn ownership_graph_uses_renamed_impl_binders() {
        let source = r#"
struct Holder[T] { marker: i32, pointer: *T }
impl Holder[U] {
    fn forward(self: &Holder[U], value: U) U { ret value; }
}
fn main() void {
    var pointer: *[i32; 3] = 0;
    var holder: Holder[[i32; 3]] = Holder { marker: 1, pointer: pointer };
    holder.forward([11, 22, 33]);
}
"#;
        let report = analyze(source);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let method = node(&report, "Holder.forward", &["[i32; 3]"]);
        assert_eq!(method.signature.parameters, vec![OwnershipCapability::Move]);
        assert_eq!(
            method.signature.receiver,
            Some(OwnershipCapability::SharedBorrow)
        );
        assert!(
            !report
                .ownership_call_graph
                .iter()
                .any(|node| node.id == id("Holder.forward", &[]))
        );
        assert_eq!(report.impl_generic_params["Holder.forward"], vec!["U"]);
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        let chunks = crate::bytecode::codegen::Codegen::new(&report)
            .compile_program(&program, &[])
            .expect("renamed multi-slot impl must lower");
        let emitted = chunks
            .iter()
            .find(|chunk| chunk.name == "Holder.forward<_i32__3_>")
            .unwrap();
        assert_eq!(
            emitted.param_count, 5,
            "sret, receiver, and three element slots"
        );
    }

    #[test]
    fn ownership_graph_preserves_reordered_impl_binders() {
        let report = analyze(
            r#"
struct Pair[A, B] { first: *A, second: *B }
impl Pair[B, A] {
    fn forward(self: &Pair[B, A], value: B) B { ret value; }
}
fn main() void {
    var first: *i32 = 0;
    var second: *bool = 0;
    var pair: Pair[i32, bool] = Pair { first: first, second: second };
    pair.forward(7);
}
"#,
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.impl_generic_params["Pair.forward"], vec!["B", "A"]);
        let method = report
            .ownership_call_graph
            .iter()
            .find(|node| node.id.callable == "Pair.forward")
            .unwrap();
        assert_eq!(method.id.type_arguments, vec!["i32", "bool"]);
        assert_eq!(method.signature.parameters, vec![OwnershipCapability::Copy]);
        assert_eq!(
            method.signature.result,
            OwnershipResult::Value(OwnershipCapability::Copy)
        );
    }

    #[test]
    fn ownership_graph_never_substitutes_a_concrete_impl_type() {
        let report = analyze(
            r#"
struct Token { value: i32 }
struct Holder[T] { pointer: *T }
impl Holder[Token] {
    fn forward(self: &Holder[Token], value: Token) Token { ret value; }
    fn observe(self: &Holder[Token]) void {}
}
fn main() void {
    var pointer: *i32 = 0;
    var holder: Holder[i32] = Holder { pointer: pointer };
    holder.forward(7);
    holder.observe();
}
"#,
        );
        assert!(report.impl_generic_params["Holder.forward"].is_empty());
        assert!(
            report
                .errors
                .iter()
                .filter(
                    |error| error.code == "S08" && error.message.contains("implementation target")
                )
                .count()
                >= 2,
            "{:?}",
            report.errors
        );
        let fixed = node(&report, "Holder.forward", &[]);
        assert_eq!(fixed.signature.parameters, vec![OwnershipCapability::Move]);
        assert_eq!(
            fixed.signature.result,
            OwnershipResult::Value(OwnershipCapability::Move)
        );
        assert!(!node(&report, "main", &[]).source_calls_closed);
    }
}
