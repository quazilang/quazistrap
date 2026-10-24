// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Checked transparent-alias expansion shared by analysis and lowering.
//! Substitution is performed during traversal, never by eagerly duplicating an
//! unchecked type tree. Each query owns its budgets and lexical environments.

use crate::parser::ast::{Spanned, Type, TypeKind};
use std::collections::{HashMap, HashSet};

pub const MAX_TYPE_DEPTH: usize = 64;
pub const MAX_TYPE_NODES: usize = 4096;
const MAX_ALIAS_DEPTH: usize = 64;
const MAX_VISIT_DEPTH: usize = 128;
const MAX_WORK: usize = 32768;
const MAX_TYPE_TEXT_BYTES: usize = 1024 * 1024;
const MAX_COPIED_TEXT_BYTES: usize = 16 * MAX_TYPE_TEXT_BYTES;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AliasResolutionError {
    Cycle(Vec<String>),
    Arity {
        alias: String,
        expected: usize,
        actual: usize,
    },
    ParameterArguments(String),
    SubstitutionCycle(String),
    Limit {
        context: Option<String>,
        kind: &'static str,
        limit: usize,
    },
}

impl AliasResolutionError {
    pub fn context_name(&self) -> Option<&str> {
        match self {
            Self::Cycle(path) => path.first().map(String::as_str),
            Self::Arity { alias, .. }
            | Self::ParameterArguments(alias)
            | Self::SubstitutionCycle(alias) => Some(alias),
            Self::Limit { context, .. } => context.as_deref(),
        }
    }
}

impl std::fmt::Display for AliasResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cycle(path) => write!(f, "cyclic type alias: {}", path.join(" -> ")),
            Self::Arity {
                alias,
                expected,
                actual,
            } => write!(
                f,
                "type alias `{alias}` expects {expected} type argument(s), got {actual}"
            ),
            Self::ParameterArguments(name) => {
                write!(f, "type parameter `{name}` cannot take type arguments")
            }
            Self::SubstitutionCycle(name) => write!(f, "cyclic type substitution for `{name}`"),
            Self::Limit {
                context,
                kind,
                limit,
            } => {
                write!(f, "type resolution exceeds the {limit} {kind} limit")?;
                if let Some(alias) = context {
                    write!(f, " while expanding `{alias}`")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for AliasResolutionError {}

/// Resolve aliases, lexical parameters, and concrete substitutions. Nominal
/// aggregates remain nominal; `canonical_name` optionally resolves qualified
/// source spellings to their declarations. Stored alias bodies must already be
/// canonicalized in their defining source context: the caller's name mapping
/// applies to the query and substitution values, never to an alias body.
/// No parsed AST is mutated.
pub fn resolve_type(
    ty: &TypeKind,
    aliases: &HashMap<String, (Vec<String>, TypeKind)>,
    substitutions: &HashMap<String, TypeKind>,
    bound_names: &HashSet<String>,
    canonical_name: impl Fn(&str) -> Option<String>,
) -> Result<TypeKind, AliasResolutionError> {
    let mut resolver = Resolver {
        aliases,
        substitutions,
        bound_names,
        canonical_name,
        active_aliases: Vec::new(),
        active_substitutions: HashSet::new(),
        work: 0,
        copied_text: 0,
    };
    resolver
        .visit(ty, &HashMap::new(), true, 0)
        .map(|value| value.ty)
}

struct Resolved {
    ty: TypeKind,
    nodes: usize,
    depth: usize,
    text: usize,
}

struct Resolver<'a, F> {
    aliases: &'a HashMap<String, (Vec<String>, TypeKind)>,
    substitutions: &'a HashMap<String, TypeKind>,
    bound_names: &'a HashSet<String>,
    canonical_name: F,
    active_aliases: Vec<String>,
    active_substitutions: HashSet<String>,
    work: usize,
    copied_text: usize,
}

impl<F: Fn(&str) -> Option<String>> Resolver<'_, F> {
    fn limit(&self, kind: &'static str, limit: usize) -> AliasResolutionError {
        AliasResolutionError::Limit {
            context: self.active_aliases.last().cloned(),
            kind,
            limit,
        }
    }

    fn charge(&mut self, work: usize, text: usize) -> Result<(), AliasResolutionError> {
        self.work = self.work.saturating_add(work);
        self.copied_text = self.copied_text.saturating_add(text);
        if self.work > MAX_WORK {
            return Err(self.limit("expansion work", MAX_WORK));
        }
        if self.copied_text > MAX_COPIED_TEXT_BYTES {
            return Err(self.limit("copied type-name bytes", MAX_COPIED_TEXT_BYTES));
        }
        Ok(())
    }

    fn costs(
        &self,
        children: &[Resolved],
        text: usize,
    ) -> Result<(usize, usize, usize), AliasResolutionError> {
        let nodes = children
            .iter()
            .fold(1usize, |sum, child| sum.saturating_add(child.nodes));
        let depth = 1 + children.iter().map(|child| child.depth).max().unwrap_or(0);
        let text = children
            .iter()
            .fold(text, |sum, child| sum.saturating_add(child.text));
        if nodes > MAX_TYPE_NODES {
            return Err(self.limit("expanded type nodes", MAX_TYPE_NODES));
        }
        if depth > MAX_TYPE_DEPTH {
            return Err(self.limit("type constructor depth", MAX_TYPE_DEPTH));
        }
        if text > MAX_TYPE_TEXT_BYTES {
            return Err(self.limit("expanded type-name bytes", MAX_TYPE_TEXT_BYTES));
        }
        Ok((nodes, depth, text))
    }

    fn copy(&mut self, value: &Resolved) -> Result<Resolved, AliasResolutionError> {
        // Charge the entire known bounded subtree before its recursive clone.
        self.charge(value.nodes, value.text)?;
        Ok(Resolved {
            ty: value.ty.clone(),
            nodes: value.nodes,
            depth: value.depth,
            text: value.text,
        })
    }

    fn visit(
        &mut self,
        ty: &TypeKind,
        parameters: &HashMap<String, Resolved>,
        source_context: bool,
        depth: usize,
    ) -> Result<Resolved, AliasResolutionError> {
        self.charge(1, 0)?;
        if depth > MAX_VISIT_DEPTH {
            return Err(self.limit("resolution stack", MAX_VISIT_DEPTH));
        }
        match ty {
            TypeKind::Named { name, type_args } => {
                // Check source names before diagnostics, binding keys, or
                // nominal output can copy them.
                if name.len() > MAX_TYPE_TEXT_BYTES {
                    return Err(self.limit("expanded type-name bytes", MAX_TYPE_TEXT_BYTES));
                }
                if !name.contains('.') {
                    if let Some(value) = parameters.get(name) {
                        if !type_args.is_empty() {
                            return Err(AliasResolutionError::ParameterArguments(name.clone()));
                        }
                        return self.copy(value);
                    }
                    if source_context && let Some(value) = self.substitutions.get(name) {
                        if !type_args.is_empty() {
                            return Err(AliasResolutionError::ParameterArguments(name.clone()));
                        }
                        if matches!(value, TypeKind::Named { name: value_name, type_args } if value_name == name && type_args.is_empty())
                        {
                            // An unresolved template parameter is an atom, not a
                            // recursive substitution or a same-named global alias.
                            return self.atom(name);
                        }
                        self.charge(0, name.len())?;
                        if !self.active_substitutions.insert(name.clone()) {
                            return Err(AliasResolutionError::SubstitutionCycle(name.clone()));
                        }
                        let result = self.visit(value, parameters, true, depth + 1);
                        self.active_substitutions.remove(name);
                        return result;
                    }
                    if source_context && self.bound_names.contains(name) {
                        if !type_args.is_empty() {
                            return Err(AliasResolutionError::ParameterArguments(name.clone()));
                        }
                        return self.atom(name);
                    }
                }
                // Qualified aliases require the same declaration evidence as
                // qualified nominals. A matching leaf alone is not identity.
                let canonical = source_context
                    .then(|| (self.canonical_name)(name))
                    .flatten();
                let entry = self.aliases.get_key_value(name).or_else(|| {
                    canonical
                        .as_ref()
                        .and_then(|name| self.aliases.get_key_value(name))
                });
                if let Some((alias, (names, body))) = entry {
                    if names.len() != type_args.len() {
                        return Err(AliasResolutionError::Arity {
                            alias: alias.clone(),
                            expected: names.len(),
                            actual: type_args.len(),
                        });
                    }
                    if let Some(index) = self
                        .active_aliases
                        .iter()
                        .position(|active| active == alias)
                    {
                        let mut path = self.active_aliases[index..].to_vec();
                        path.push(alias.clone());
                        return Err(AliasResolutionError::Cycle(path));
                    }
                    if self.active_aliases.len() >= MAX_ALIAS_DEPTH {
                        return Err(self.limit("active aliases", MAX_ALIAS_DEPTH));
                    }
                    if type_args.len() >= MAX_TYPE_NODES {
                        return Err(self.limit("expanded type nodes", MAX_TYPE_NODES));
                    }
                    // Normalize arguments outside the new alias frame, so
                    // Identity[Identity[i32]] does not falsely form a cycle.
                    let mut arguments = HashMap::new();
                    for (parameter, actual) in names.iter().zip(type_args) {
                        let value =
                            self.visit(&actual.node, parameters, source_context, depth + 1)?;
                        self.charge(0, parameter.len())?;
                        arguments.insert(parameter.clone(), value);
                    }
                    self.charge(0, alias.len())?;
                    self.active_aliases.push(alias.clone());
                    // Alias bodies see their own lexical parameters and global
                    // declarations, never the caller's binder/substitution names.
                    let result = self.visit(body, &arguments, false, depth + 1);
                    self.active_aliases.pop();
                    return result;
                }
                self.nominal(name, type_args, parameters, source_context, depth)
            }
            TypeKind::Ref { inner } => {
                self.unary(inner, parameters, source_context, depth, |inner| {
                    TypeKind::Ref { inner }
                })
            }
            TypeKind::MutRef { inner } => {
                self.unary(inner, parameters, source_context, depth, |inner| {
                    TypeKind::MutRef { inner }
                })
            }
            TypeKind::RawPtr { inner } => {
                self.unary(inner, parameters, source_context, depth, |inner| {
                    TypeKind::RawPtr { inner }
                })
            }
            TypeKind::Array { elem_ty, len } => {
                self.unary(elem_ty, parameters, source_context, depth, |elem_ty| {
                    TypeKind::Array { elem_ty, len: *len }
                })
            }
            TypeKind::Slice { elem_ty } => {
                self.unary(elem_ty, parameters, source_context, depth, |elem_ty| {
                    TypeKind::Slice { elem_ty }
                })
            }
            TypeKind::FlexibleArray { elem_ty } => {
                self.unary(elem_ty, parameters, source_context, depth, |elem_ty| {
                    TypeKind::FlexibleArray { elem_ty }
                })
            }
            TypeKind::Fn { params, return_ty } => {
                self.function(params, return_ty, parameters, source_context, depth, false)
            }
            TypeKind::CFn { params, return_ty } => {
                self.function(params, return_ty, parameters, source_context, depth, true)
            }
            TypeKind::Dyn { trait_name } => {
                if trait_name.len() > MAX_TYPE_TEXT_BYTES {
                    return Err(self.limit("expanded type-name bytes", MAX_TYPE_TEXT_BYTES));
                }
                let name = (source_context && !self.bound_names.contains(trait_name))
                    .then(|| (self.canonical_name)(trait_name))
                    .flatten()
                    .unwrap_or_else(|| trait_name.clone());
                let (nodes, depth, text) = self.costs(&[], name.len())?;
                self.charge(0, text)?;
                Ok(Resolved {
                    ty: TypeKind::Dyn { trait_name: name },
                    nodes,
                    depth,
                    text,
                })
            }
            _ => {
                let text = if let TypeKind::Dyn { trait_name } = ty {
                    trait_name.len()
                } else {
                    0
                };
                let (nodes, depth, text) = self.costs(&[], text)?;
                self.charge(0, text)?;
                Ok(Resolved {
                    ty: ty.clone(),
                    nodes,
                    depth,
                    text,
                })
            }
        }
    }

    fn nominal(
        &mut self,
        name: &str,
        args: &[Type],
        parameters: &HashMap<String, Resolved>,
        source: bool,
        depth: usize,
    ) -> Result<Resolved, AliasResolutionError> {
        if args.len() >= MAX_TYPE_NODES {
            return Err(self.limit("expanded type nodes", MAX_TYPE_NODES));
        }
        // The caller's canonicalization callback may produce a new declaration
        // name. Validate it before copying any child into the output.
        let name = source
            .then(|| (self.canonical_name)(name))
            .flatten()
            .unwrap_or_else(|| name.to_string());
        let mut costs = self.costs(&[], name.len())?;
        self.charge(0, name.len())?;
        let mut children = Vec::with_capacity(args.len());
        for arg in args {
            let child = self.visit(&arg.node, parameters, source, depth + 1)?;
            costs = self.append_child_costs(costs, &child)?;
            children.push(child);
        }
        let (nodes, depth, text) = costs;
        let type_args = children
            .into_iter()
            .zip(args)
            .map(|(child, arg)| Spanned::new(child.ty, arg.span))
            .collect();
        Ok(Resolved {
            ty: TypeKind::Named { name, type_args },
            nodes,
            depth,
            text,
        })
    }

    fn atom(&mut self, name: &str) -> Result<Resolved, AliasResolutionError> {
        let (nodes, depth, text) = self.costs(&[], name.len())?;
        self.charge(0, name.len())?;
        Ok(Resolved {
            ty: TypeKind::Named {
                name: name.to_string(),
                type_args: Vec::new(),
            },
            nodes,
            depth,
            text,
        })
    }

    fn append_child_costs(
        &self,
        costs: (usize, usize, usize),
        child: &Resolved,
    ) -> Result<(usize, usize, usize), AliasResolutionError> {
        let (nodes, depth, text) = costs;
        if nodes.saturating_add(child.nodes) > MAX_TYPE_NODES {
            return Err(self.limit("expanded type nodes", MAX_TYPE_NODES));
        }
        if depth.max(child.depth.saturating_add(1)) > MAX_TYPE_DEPTH {
            return Err(self.limit("type constructor depth", MAX_TYPE_DEPTH));
        }
        if text.saturating_add(child.text) > MAX_TYPE_TEXT_BYTES {
            return Err(self.limit("expanded type-name bytes", MAX_TYPE_TEXT_BYTES));
        }
        Ok((
            nodes + child.nodes,
            depth.max(child.depth + 1),
            text + child.text,
        ))
    }

    fn unary(
        &mut self,
        inner: &Type,
        parameters: &HashMap<String, Resolved>,
        source: bool,
        depth: usize,
        constructor: impl FnOnce(Box<Type>) -> TypeKind,
    ) -> Result<Resolved, AliasResolutionError> {
        let value = self.visit(&inner.node, parameters, source, depth + 1)?;
        let (nodes, depth, text) = self.costs(std::slice::from_ref(&value), 0)?;
        Ok(Resolved {
            ty: constructor(Box::new(Spanned::new(value.ty, inner.span))),
            nodes,
            depth,
            text,
        })
    }

    fn function(
        &mut self,
        params: &[Type],
        result: &Type,
        parameters: &HashMap<String, Resolved>,
        source: bool,
        depth: usize,
        c_abi: bool,
    ) -> Result<Resolved, AliasResolutionError> {
        if params.len() >= MAX_TYPE_NODES - 1 {
            return Err(self.limit("expanded type nodes", MAX_TYPE_NODES));
        }
        let mut costs = self.costs(&[], 0)?;
        let mut children = Vec::with_capacity(params.len() + 1);
        for param in params {
            let child = self.visit(&param.node, parameters, source, depth + 1)?;
            costs = self.append_child_costs(costs, &child)?;
            children.push(child);
        }
        let child = self.visit(&result.node, parameters, source, depth + 1)?;
        costs = self.append_child_costs(costs, &child)?;
        children.push(child);
        let (nodes, depth, text) = costs;
        let return_ty = Box::new(Spanned::new(
            children.pop().expect("result child").ty,
            result.span,
        ));
        let params = children
            .into_iter()
            .zip(params)
            .map(|(child, param)| Spanned::new(child.ty, param.span))
            .collect();
        let ty = if c_abi {
            TypeKind::CFn { params, return_ty }
        } else {
            TypeKind::Fn { params, return_ty }
        };
        Ok(Resolved {
            ty,
            nodes,
            depth,
            text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{ItemKind, Span};
    use crate::{lexer::Lexer, parser::Parser};

    fn parse_type(source: &str) -> TypeKind {
        let program = Parser::new(Lexer::new(&format!("type Test = {source};")).tokenize())
            .parse()
            .unwrap();
        let ItemKind::TypeAlias { aliased_type, .. } = &program.items[0].node else {
            unreachable!()
        };
        aliased_type.node.clone()
    }

    fn aliases(source: &str) -> HashMap<String, (Vec<String>, TypeKind)> {
        Parser::new(Lexer::new(source).tokenize())
            .parse()
            .unwrap()
            .items
            .into_iter()
            .filter_map(|item| match item.node {
                ItemKind::TypeAlias {
                    name,
                    generic_params,
                    aliased_type,
                    ..
                } => Some((name, (generic_params, aliased_type.node))),
                _ => None,
            })
            .collect()
    }

    fn resolve(
        source: &str,
        definitions: &HashMap<String, (Vec<String>, TypeKind)>,
    ) -> Result<TypeKind, AliasResolutionError> {
        resolve_type(
            &parse_type(source),
            definitions,
            &HashMap::new(),
            &HashSet::new(),
            |_| None,
        )
    }

    #[test]
    fn nested_identity_arguments_are_normalized_before_entering_alias() {
        let definitions = aliases("type Identity[T] = T;");
        assert_eq!(
            resolve("Identity[Identity[i32]]", &definitions)
                .unwrap()
                .to_string(),
            "i32"
        );
    }

    #[test]
    fn alias_bodies_use_lexical_parameters_and_global_declarations() {
        let definitions =
            aliases("type T = bool; type Global = *T; type Outer[T] = Pair[T, Global];");
        let substitutions = HashMap::from([
            ("T".to_string(), TypeKind::Int64),
            ("Global".to_string(), TypeKind::Int32),
        ]);
        let result = resolve_type(
            &parse_type("Outer[i32]"),
            &definitions,
            &substitutions,
            &HashSet::new(),
            |_| None,
        )
        .unwrap();
        assert_eq!(result.to_string(), "Pair[i32, *bool]");
    }

    #[test]
    fn bound_atoms_and_identity_substitutions_keep_their_names() {
        let definitions = aliases("type T = bool;");
        let bound = HashSet::from(["T".to_string()]);
        let canonical = |name: &str| Some(format!("global.{name}"));
        let result = resolve_type(
            &parse_type("T"),
            &definitions,
            &HashMap::new(),
            &bound,
            canonical,
        )
        .unwrap();
        assert_eq!(result.to_string(), "T");
        let substitutions = HashMap::from([("T".to_string(), parse_type("T"))]);
        let result = resolve_type(
            &parse_type("T"),
            &definitions,
            &substitutions,
            &HashSet::new(),
            canonical,
        )
        .unwrap();
        assert_eq!(result.to_string(), "T");
    }

    #[test]
    fn substitutions_resolve_through_caller_bindings() {
        let definitions = aliases("type U = bool;");
        let substitutions = HashMap::from([("T".to_string(), parse_type("U"))]);
        let result = resolve_type(
            &parse_type("T"),
            &definitions,
            &substitutions,
            &HashSet::from(["U".to_string()]),
            |_| None,
        )
        .unwrap();
        assert_eq!(result.to_string(), "U");
    }

    #[test]
    fn cycles_report_the_active_alias_path() {
        let definitions = aliases("type A = *B; type B = A;");
        assert_eq!(
            resolve("A", &definitions).unwrap_err(),
            AliasResolutionError::Cycle(vec!["A".to_string(), "B".to_string(), "A".to_string()])
        );
        let definitions = aliases("type Loop[T] = Loop[Pair[T, T]];");
        assert!(matches!(
            resolve("Loop[i32]", &definitions),
            Err(AliasResolutionError::Cycle(_))
        ));
    }

    #[test]
    fn all_alias_applications_require_exact_arity() {
        let definitions = aliases("type Identity[T] = T; type Scalar = i32;");
        for (query, alias, expected, actual) in [
            ("Identity", "Identity", 1, 0),
            ("Identity[i32, bool]", "Identity", 1, 2),
            ("Scalar[bool]", "Scalar", 0, 1),
        ] {
            assert_eq!(
                resolve(query, &definitions).unwrap_err(),
                AliasResolutionError::Arity {
                    alias: alias.to_string(),
                    expected,
                    actual,
                }
            );
        }
    }

    #[test]
    fn parameters_cannot_be_applied_as_generic_types() {
        let definitions = aliases("type Bad[T] = T[i32];");
        assert_eq!(
            resolve("Bad[bool]", &definitions).unwrap_err(),
            AliasResolutionError::ParameterArguments("T".to_string())
        );
        let result = resolve_type(
            &parse_type("T[i32]"),
            &HashMap::new(),
            &HashMap::new(),
            &HashSet::from(["T".to_string()]),
            |_| None,
        );
        assert_eq!(
            result.unwrap_err(),
            AliasResolutionError::ParameterArguments("T".to_string())
        );
    }

    #[test]
    fn substitution_cycles_are_rejected_without_expanding_forever() {
        for substitutions in [
            HashMap::from([
                ("T".to_string(), parse_type("U")),
                ("U".to_string(), parse_type("T")),
            ]),
            HashMap::from([("T".to_string(), parse_type("*T"))]),
        ] {
            let result = resolve_type(
                &parse_type("T"),
                &HashMap::new(),
                &substitutions,
                &HashSet::new(),
                |_| None,
            );
            assert_eq!(
                result.unwrap_err(),
                AliasResolutionError::SubstitutionCycle("T".to_string())
            );
        }
    }

    #[test]
    fn aliases_expand_inside_all_type_constructors() {
        let definitions = aliases("type I[T] = T;");
        let result = resolve(
            "fn(I[i32], *I[bool], [I[u8]; 3], [I[u16]], &I[i64], &I[u32]!) I[f64]",
            &definitions,
        )
        .unwrap();
        assert_eq!(
            result.to_string(),
            "fn(i32, *bool, [u8; 3], [u16], &i64, &u32!) -> f64"
        );
        let TypeKind::Fn { params, return_ty } = result else {
            unreachable!()
        };
        let c_function = TypeKind::CFn { params, return_ty };
        let result = resolve_type(
            &c_function,
            &definitions,
            &HashMap::new(),
            &HashSet::new(),
            |_| None,
        )
        .unwrap();
        assert!(matches!(result, TypeKind::CFn { .. }));
        assert_eq!(
            resolve("[I[u8]; ..]", &definitions).unwrap().to_string(),
            "[u8; ..]"
        );
    }

    #[test]
    fn qualified_aliases_and_nominals_share_canonical_lookup() {
        let definitions = aliases("type Scalar = i32; type Pointer = *Record;");
        assert_eq!(
            resolve_type(
                &parse_type("module.Scalar"),
                &definitions,
                &HashMap::new(),
                &HashSet::new(),
                |name| (name == "module.Scalar").then(|| "Scalar".to_string())
            )
            .unwrap()
            .to_string(),
            "i32"
        );
        let result = resolve_type(
            &parse_type("Pointer"),
            &definitions,
            &HashMap::new(),
            &HashSet::new(),
            |name| (name == "module.Record").then(|| "Record".to_string()),
        )
        .unwrap();
        assert_eq!(result.to_string(), "*Record");
        let direct = resolve_type(
            &parse_type("*module.Record"),
            &definitions,
            &HashMap::new(),
            &HashSet::new(),
            |name| (name == "module.Record").then(|| "Record".to_string()),
        )
        .unwrap();
        assert_eq!(direct.to_string(), "*Record");
    }

    #[test]
    fn canonical_alias_bodies_do_not_borrow_caller_imports() {
        let definitions = aliases("type Keep = *Record; type Wrap[T] = Pair[T, Keep];");
        let result = resolve_type(
            &parse_type("Wrap[caller.Raw]"),
            &definitions,
            &HashMap::new(),
            &HashSet::new(),
            |name| match name {
                "caller.Raw" => Some("Actual".into()),
                "Record" | "Pair" => panic!("canonical alias body consulted caller imports"),
                _ => None,
            },
        )
        .unwrap();
        assert_eq!(result.to_string(), "Pair[Actual, *Record]");
    }

    #[test]
    fn dynamic_trait_names_obey_source_and_canonical_alias_contexts() {
        let definitions = aliases("type Erased = dyn Record;");
        let source = resolve_type(
            &parse_type("dyn Record"),
            &definitions,
            &HashMap::new(),
            &HashSet::new(),
            |name| (name == "Record").then(|| "Actual".into()),
        )
        .unwrap();
        assert_eq!(source.to_string(), "dyn Actual");
        let canonical = resolve_type(
            &parse_type("Erased"),
            &definitions,
            &HashMap::new(),
            &HashSet::new(),
            |name| {
                assert_ne!(name, "Record", "alias body borrowed caller context");
                None
            },
        )
        .unwrap();
        assert_eq!(canonical.to_string(), "dyn Record");
        let bound = resolve_type(
            &parse_type("dyn Record"),
            &definitions,
            &HashMap::new(),
            &HashSet::from(["Record".to_string()]),
            |_| panic!("bound dynamic trait name borrowed imports"),
        )
        .unwrap();
        assert_eq!(bound.to_string(), "dyn Record");
    }

    #[test]
    fn exponential_alias_duplication_hits_a_budget() {
        let mut definitions = aliases("type Level0 = i32;");
        for level in 1..=17 {
            definitions.insert(
                format!("Level{level}"),
                (
                    Vec::new(),
                    parse_type(&format!("Pair[Level{}, Level{}]", level - 1, level - 1)),
                ),
            );
        }
        assert!(matches!(
            resolve("Level17", &definitions),
            Err(AliasResolutionError::Limit { .. })
        ));
    }

    #[test]
    fn raw_width_depth_and_alias_depth_are_bounded() {
        let span = Span::new(0, 0, 0, 0);
        let wide = TypeKind::Named {
            name: "Wide".to_string(),
            type_args: (0..MAX_TYPE_NODES)
                .map(|_| Spanned::new(TypeKind::Int32, span))
                .collect(),
        };
        assert!(matches!(
            resolve_type(
                &wide,
                &HashMap::new(),
                &HashMap::new(),
                &HashSet::new(),
                |_| None
            ),
            Err(AliasResolutionError::Limit { .. })
        ));
        let mut deep = TypeKind::Int32;
        for _ in 0..MAX_TYPE_DEPTH {
            deep = TypeKind::RawPtr {
                inner: Box::new(Spanned::new(deep, span)),
            };
        }
        assert!(matches!(
            resolve_type(
                &deep,
                &HashMap::new(),
                &HashMap::new(),
                &HashSet::new(),
                |_| None
            ),
            Err(AliasResolutionError::Limit { .. })
        ));
        let mut definitions = aliases("type End = i32;");
        let mut previous = "End".to_string();
        for index in 0..MAX_ALIAS_DEPTH {
            let name = format!("Alias{index}");
            definitions.insert(name.clone(), (Vec::new(), parse_type(&previous)));
            previous = name;
        }
        assert!(matches!(
            resolve(&previous, &definitions),
            Err(AliasResolutionError::Limit { .. })
        ));
    }

    #[test]
    fn oversized_names_are_rejected_before_nominal_canonicalization() {
        let ty = TypeKind::Named {
            name: "N".repeat(MAX_TYPE_TEXT_BYTES + 1),
            type_args: Vec::new(),
        };
        let result = resolve_type(
            &ty,
            &HashMap::new(),
            &HashMap::new(),
            &HashSet::new(),
            |_| panic!("oversized names must fail before canonicalization"),
        );
        assert!(matches!(result, Err(AliasResolutionError::Limit { .. })));
    }

    #[test]
    fn aggregate_children_are_checked_before_collecting_an_oversized_output() {
        let definitions = aliases("type Block = Wide[i32, i32, i32, i32, i32, i32, i32, i32];");
        let span = Span::new(0, 0, 0, 0);
        let block = parse_type("Block");
        let ty = TypeKind::Named {
            name: "Root".to_string(),
            type_args: (0..MAX_TYPE_NODES / 8)
                .map(|_| Spanned::new(block.clone(), span))
                .collect(),
        };
        let result = resolve_type(&ty, &definitions, &HashMap::new(), &HashSet::new(), |_| {
            None
        });
        assert!(matches!(
            result,
            Err(AliasResolutionError::Limit {
                kind: "expanded type nodes",
                ..
            })
        ));
    }
}
