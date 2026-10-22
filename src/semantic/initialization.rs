// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Definite initialization of source bindings on normal control-flow paths.
//! This tracks initialized storage, not moves, loans or interprocedural effects.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

const MATCH_INITIALIZATION_BUDGET: usize = 1_000_000;

use super::{Analyzer, ConstValue, ExprEval, Symbol, SymbolKind};
use crate::parser::ast::{Block, Expr, LiteralValue, Pattern, PatternKind, Span, TypeKind};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Binding {
    depth: usize,
    name: String,
    declaration: (usize, usize),
}

#[derive(Clone, Default)]
pub(super) struct InitializationState(BTreeMap<Binding, bool>);

impl InitializationState {
    /// Keep the receiving declaration domain; shadows never supply its facts.
    pub(super) fn intersect(&mut self, other: &Self) {
        for (binding, initialized) in &mut self.0 {
            *initialized &= other.0.get(binding).copied().unwrap_or(false);
        }
    }

    pub(super) fn project(&self, domain: &Self) -> Self {
        let mut result = domain.clone();
        for (binding, initialized) in &mut result.0 {
            *initialized = self.0.get(binding).copied().unwrap_or(false);
        }
        result
    }
}

/// None means no normal incoming path, rather than an uninitialized binding.
pub(super) fn join_initialization(
    output: &mut Option<InitializationState>,
    incoming: Option<InitializationState>,
) {
    if let Some(incoming) = incoming {
        if let Some(output) = output {
            output.intersect(&incoming);
        } else {
            *output = Some(incoming);
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct InitializationCondition {
    pub(super) truthy: Option<InitializationState>,
    pub(super) falsy: Option<InitializationState>,
}

impl InitializationCondition {
    pub(super) fn normal(&self) -> Option<InitializationState> {
        let mut result = self.truthy.clone();
        join_initialization(&mut result, self.falsy.clone());
        result
    }
}

pub(super) struct InitializationLoop {
    pub(super) domain: InitializationState,
    pub(super) breaks: Option<InitializationState>,
    pub(super) continues: Option<InitializationState>,
}

enum MatchCase {
    // Zero and nonzero equivalence classes, not canonical Boolean bits.
    Truth(bool),
    Variant(String),
    Other(Option<ConstValue>),
}

#[derive(Clone, Copy)]
pub(super) enum PatternCoverage {
    Always,
    Maybe,
    Never,
}

pub(super) struct InitializationMatch {
    pub(super) entry: InitializationState,
    remaining: Vec<(MatchCase, Option<Rc<InitializationState>>)>,
    result: InitializationCondition,
    budget: usize,
    exhausted: bool,
}

impl InitializationMatch {
    fn charge(&mut self, work: usize) -> bool {
        if self.exhausted || work > self.budget {
            self.exhausted = true;
            false
        } else {
            self.budget -= work;
            true
        }
    }

    pub(super) fn arm(
        &mut self,
        pattern: &Pattern,
    ) -> (Vec<PatternCoverage>, Option<InitializationState>) {
        if !self.charge(self.remaining.len()) {
            return (Vec::new(), None);
        }
        let mut input = None;
        let mut seen = HashSet::new();
        let mut work = 0usize;
        let coverage = self
            .remaining
            .iter()
            .map(|(case, state)| {
                let coverage = match (&pattern.node, case) {
                    (PatternKind::Wildcard | PatternKind::Bind(_), _) => PatternCoverage::Always,
                    (
                        PatternKind::Literal(LiteralValue::Bool(pattern)),
                        MatchCase::Truth(value),
                    ) => {
                        if pattern == value {
                            PatternCoverage::Always
                        } else {
                            PatternCoverage::Never
                        }
                    }
                    (PatternKind::Literal(LiteralValue::Int(pattern)), MatchCase::Truth(value)) => {
                        // Bool values can originate from compatible integer values.
                        // A nonzero integer pattern covers only part of the truthy class.
                        if *pattern == 0 && !value {
                            PatternCoverage::Always
                        } else if *pattern != 0 && *value {
                            PatternCoverage::Maybe
                        } else {
                            PatternCoverage::Never
                        }
                    }
                    (
                        PatternKind::Variant {
                            variant,
                            sub_patterns,
                            ..
                        },
                        MatchCase::Variant(value),
                    ) => {
                        if variant != value {
                            PatternCoverage::Never
                        } else if sub_patterns.iter().all(|pattern| {
                            matches!(pattern.node, PatternKind::Wildcard | PatternKind::Bind(_))
                        }) {
                            PatternCoverage::Always
                        } else {
                            PatternCoverage::Maybe
                        }
                    }
                    (PatternKind::Literal(pattern), MatchCase::Other(Some(value))) => {
                        let matches = match (pattern, value) {
                            (LiteralValue::Int(a), ConstValue::Int(b)) => Some(*a as i64 == *b),
                            (LiteralValue::Bool(a), ConstValue::Int(b)) => Some(*a == (*b != 0)),
                            (LiteralValue::Float(a), ConstValue::Float(b)) => Some(a == b),
                            (LiteralValue::Str(a), ConstValue::String(b)) => {
                                // Runtime string views and literal-pattern comparisons
                                // end at the first NUL, including inside a literal.
                                Some(a.split('\0').next() == b.split('\0').next())
                            }
                            (LiteralValue::Bool(a), ConstValue::Bool(b)) => Some(a == b),
                            _ => None,
                        };
                        match matches {
                            Some(true) => PatternCoverage::Always,
                            Some(false) => PatternCoverage::Never,
                            None => PatternCoverage::Maybe,
                        }
                    }
                    _ => PatternCoverage::Maybe,
                };
                if !matches!(coverage, PatternCoverage::Never) {
                    if let Some(state) = state
                        && seen.insert(Rc::as_ptr(state))
                    {
                        work = work.saturating_add(state.0.len());
                        if work <= self.budget {
                            join_initialization(&mut input, Some((**state).clone()));
                        }
                    }
                }
                coverage
            })
            .collect();
        if !self.charge(work) {
            return (Vec::new(), None);
        }
        (coverage, input)
    }

    pub(super) fn finish_arm(
        &mut self,
        coverage: &[PatternCoverage],
        guard_false: Option<InitializationState>,
        output: InitializationCondition,
    ) {
        if !self.charge(
            self.remaining
                .len()
                .saturating_add(self.entry.0.len().saturating_mul(3)),
        ) {
            return;
        }
        for (destination, incoming) in [
            (&mut self.result.truthy, output.truthy),
            (&mut self.result.falsy, output.falsy),
        ] {
            join_initialization(
                destination,
                incoming.map(|state| state.project(&self.entry)),
            );
        }
        let guard_false = guard_false.map(|state| Rc::new(state.project(&self.entry)));
        let mut merged = HashMap::new();
        for ((_, state), coverage) in self.remaining.iter_mut().zip(coverage) {
            if state.is_none() {
                continue;
            }
            match coverage {
                PatternCoverage::Always => *state = guard_false.clone(),
                PatternCoverage::Maybe => {
                    if let Some(incoming) = &guard_false {
                        let previous = state.as_ref().expect("remaining match path");
                        let identity = Rc::as_ptr(previous);
                        let value = if let Some(value) = merged.get(&identity) {
                            Rc::clone(value)
                        } else {
                            if previous.0.len() > self.budget {
                                self.exhausted = true;
                                return;
                            }
                            self.budget -= previous.0.len();
                            let mut value = (**previous).clone();
                            value.intersect(incoming);
                            let value = Rc::new(value);
                            merged.insert(identity, Rc::clone(&value));
                            value
                        };
                        *state = Some(value);
                    }
                }
                PatternCoverage::Never => {}
            }
        }
    }

    pub(super) fn finish(mut self) -> (InitializationCondition, bool, bool) {
        if !self.charge(self.remaining.len()) {
            return (
                InitializationCondition {
                    truthy: Some(self.entry.clone()),
                    falsy: Some(self.entry),
                },
                false,
                true,
            );
        }
        let mut unmatched = None;
        let mut seen = HashSet::new();
        for (_, state) in self.remaining.into_iter() {
            if let Some(state) = state
                && seen.insert(Rc::as_ptr(&state))
            {
                if state.0.len() > self.budget {
                    return (
                        InitializationCondition {
                            truthy: Some(self.entry.clone()),
                            falsy: Some(self.entry),
                        },
                        false,
                        true,
                    );
                }
                self.budget -= state.0.len();
                join_initialization(&mut unmatched, Some((*state).clone()));
            }
        }
        let exhaustive = unmatched.is_none();
        // An unmatched path proves no arm's effects or default result value.
        join_initialization(&mut self.result.truthy, unmatched.clone());
        join_initialization(&mut self.result.falsy, unmatched);
        (self.result, exhaustive, false)
    }
}

impl InitializationLoop {
    pub(super) fn new(domain: InitializationState) -> Self {
        Self {
            domain,
            breaks: None,
            continues: None,
        }
    }
}

impl Analyzer {
    pub(super) fn initialization_match(
        &self,
        ty: Option<&TypeKind>,
        eval: &ExprEval,
    ) -> InitializationMatch {
        let entry = self.initialization_snapshot();
        let cases = match ty {
            Some(TypeKind::Bool) => match eval.const_value {
                Some(ConstValue::Bool(value)) => vec![MatchCase::Truth(value)],
                Some(ConstValue::Int(value)) => vec![MatchCase::Truth(value != 0)],
                _ => vec![MatchCase::Truth(false), MatchCase::Truth(true)],
            },
            Some(ty) if Self::is_integer(ty) && eval.const_value.is_none() => {
                vec![MatchCase::Truth(false), MatchCase::Truth(true)]
            }
            Some(TypeKind::Named { name, .. }) if self.enums.contains_key(name) => self.enums[name]
                .order
                .iter()
                .cloned()
                .map(MatchCase::Variant)
                .collect(),
            _ => vec![MatchCase::Other(eval.const_value.clone())],
        };
        let shared_entry = Rc::new(entry.clone());
        let mut flow = InitializationMatch {
            remaining: cases
                .into_iter()
                .map(|case| (case, Some(Rc::clone(&shared_entry))))
                .collect(),
            entry,
            result: InitializationCondition::default(),
            budget: MATCH_INITIALIZATION_BUDGET,
            exhausted: false,
        };
        flow.charge(flow.remaining.len().saturating_add(flow.entry.0.len()));
        flow
    }

    pub(super) fn type_check_condition_initialization(
        &mut self,
        expr: &Expr,
        context: &str,
    ) -> InitializationCondition {
        let eval = self.type_check_expr(expr, true);
        if let Some(ty) = &eval.ty
            && !matches!(ty, TypeKind::Bool | TypeKind::Error)
            && !Self::is_integer(ty)
        {
            self.push_error(
                expr.span,
                "S01",
                format!("{context} condition must be bool or integer, got {ty}"),
            );
        }
        self.take_initialization_condition(expr, &eval)
    }

    pub(super) fn type_check_initialization_block(
        &mut self,
        block: &Block,
        expected_return: Option<&TypeKind>,
        incoming: Option<&InitializationState>,
        fallback: &InitializationState,
        checkpoint: &mut dyn FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<Option<InitializationState>, crate::cancel::Cancelled> {
        self.restore_initialization(incoming.unwrap_or(fallback));
        let previous_reachable = self.initialization_reachable;
        self.initialization_reachable &= incoming.is_some();
        let terminal = self.type_check_block(block, expected_return, checkpoint)?;
        self.initialization_reachable = previous_reachable;
        Ok((incoming.is_some() && !terminal).then(|| self.initialization_snapshot()))
    }

    pub(super) fn type_check_if_initialization(
        &mut self,
        condition: &Expr,
        then_block: &Block,
        else_if: &[(Expr, Block)],
        else_block: Option<&Block>,
        expected_return: Option<&TypeKind>,
        checkpoint: &mut dyn FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<bool, crate::cancel::Cancelled> {
        let entry = self.initialization_snapshot();
        let condition = self.type_check_condition_initialization(condition, "if");
        let mut normal = self.type_check_initialization_block(
            then_block,
            expected_return,
            condition.truthy.as_ref(),
            &entry,
            checkpoint,
        )?;
        let mut remaining = condition.falsy;
        for (condition, block) in else_if {
            self.restore_initialization(remaining.as_ref().unwrap_or(&entry));
            let previous_reachable = self.initialization_reachable;
            self.initialization_reachable &= remaining.is_some();
            let branch = self.type_check_condition_initialization(condition, "if");
            let outcome = self.type_check_initialization_block(
                block,
                expected_return,
                branch.truthy.as_ref().filter(|_| remaining.is_some()),
                &entry,
                checkpoint,
            )?;
            self.initialization_reachable = previous_reachable;
            join_initialization(&mut normal, outcome);
            remaining = remaining.and(branch.falsy);
        }
        let outcome = if let Some(block) = else_block {
            self.type_check_initialization_block(
                block,
                expected_return,
                remaining.as_ref(),
                &entry,
                checkpoint,
            )?
        } else {
            remaining
        };
        join_initialization(&mut normal, outcome);
        self.restore_initialization(normal.as_ref().unwrap_or(&entry));
        Ok(normal.is_none())
    }

    pub(super) fn type_check_loop_initialization(
        &mut self,
        body: &Block,
        condition: Option<&Expr>,
        update: Option<&Expr>,
        may_be_empty: bool,
        expected_return: Option<&TypeKind>,
        checkpoint: &mut dyn FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<bool, crate::cancel::Cancelled> {
        let entry = self.initialization_snapshot();
        let condition = if let Some(expr) = condition {
            self.type_check_condition_initialization(expr, "for")
        } else {
            InitializationCondition {
                truthy: Some(entry.clone()),
                falsy: may_be_empty.then(|| entry.clone()),
            }
        };
        self.initialization_loops
            .push(InitializationLoop::new(entry.clone()));
        self.loop_depth += 1;
        let mut backedge = self.type_check_initialization_block(
            body,
            expected_return,
            condition.truthy.as_ref(),
            &entry,
            checkpoint,
        )?;
        let frame = self
            .initialization_loops
            .pop()
            .expect("initialization loop");
        join_initialization(&mut backedge, frame.continues);
        if let Some(update) = update {
            // Even unreachable updates retain type diagnostics, but cannot
            // initialize the first body entry or any exit that bypasses them.
            self.restore_initialization(backedge.as_ref().unwrap_or(&entry));
            let previous_reachable = self.initialization_reachable;
            self.initialization_reachable &= backedge.is_some();
            self.type_check_expr(update, true);
            self.initialization_reachable = previous_reachable;
        }
        self.loop_depth -= 1;
        let mut normal = condition.falsy;
        join_initialization(&mut normal, frame.breaks);
        self.restore_initialization(normal.as_ref().unwrap_or(&entry));
        Ok(normal.is_none())
    }

    pub(super) fn initialization_snapshot(&self) -> InitializationState {
        let mut bindings = BTreeMap::new();
        for (depth, scope) in self.scopes.iter().enumerate() {
            for (name, symbol) in scope {
                if matches!(
                    symbol.kind,
                    SymbolKind::Variable { .. } | SymbolKind::Parameter
                ) {
                    bindings.insert(
                        Binding {
                            depth,
                            name: name.clone(),
                            declaration: (symbol.span.start, symbol.span.end),
                        },
                        symbol.initialized,
                    );
                }
            }
        }
        InitializationState(bindings)
    }

    pub(super) fn restore_initialization(&mut self, state: &InitializationState) {
        for (binding, initialized) in &state.0 {
            if let Some(symbol) = self
                .scopes
                .get_mut(binding.depth)
                .and_then(|scope| scope.get_mut(&binding.name))
                && (symbol.span.start, symbol.span.end) == binding.declaration
            {
                symbol.initialized = *initialized;
            }
        }
    }

    pub(super) fn require_initialized(&mut self, name: &str, symbol: &Symbol, span: Span) {
        if matches!(
            symbol.kind,
            SymbolKind::Variable { .. } | SymbolKind::Parameter
        ) && !symbol.initialized
            && self.initialization_reachable
        {
            self.push_error(
                span,
                "S02",
                format!("use of '{name}' before initialization"),
            );
        }
    }

    pub(super) fn take_initialization_condition(
        &mut self,
        expr: &Expr,
        eval: &ExprEval,
    ) -> InitializationCondition {
        if let Some((span, condition)) = self.initialization_condition.take()
            && span == expr.span
        {
            return condition;
        }
        let state = self.initialization_snapshot();
        let truth = match eval.const_value {
            Some(ConstValue::Bool(value)) => Some(value),
            Some(ConstValue::Int(value)) => Some(value != 0),
            _ => None,
        };
        InitializationCondition {
            truthy: (truth != Some(false)).then(|| state.clone()),
            falsy: (truth != Some(true)).then_some(state),
        }
    }

    pub(super) fn initialize_condition_binding(
        &self,
        name: &str,
        condition: &mut InitializationCondition,
    ) {
        let binding = self
            .scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(depth, scope)| {
                scope.get(name).map(|symbol| Binding {
                    depth,
                    name: name.to_string(),
                    declaration: (symbol.span.start, symbol.span.end),
                })
            });
        if let Some(binding) = binding {
            for state in [&mut condition.truthy, &mut condition.falsy]
                .into_iter()
                .flatten()
            {
                state.0.insert(binding.clone(), true);
            }
        }
    }

    pub(super) fn record_initialization_loop_exit(&mut self, is_break: bool) {
        if !self.initialization_reachable {
            return;
        }
        let state = self.initialization_snapshot();
        if let Some(frame) = self.initialization_loops.last_mut() {
            let state = state.project(&frame.domain);
            let target = if is_break {
                &mut frame.breaks
            } else {
                &mut frame.continues
            };
            join_initialization(target, Some(state));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_fact_budget_exhaustion_publishes_no_initialization_proof() {
        let entry = InitializationState(BTreeMap::from([(
            Binding {
                depth: 1,
                name: "value".into(),
                declaration: (1, 2),
            },
            false,
        )]));
        let mut flow = InitializationMatch {
            remaining: vec![(MatchCase::Truth(false), Some(Rc::new(entry.clone())))],
            entry,
            result: InitializationCondition::default(),
            budget: 0,
            exhausted: false,
        };
        let pattern =
            crate::parser::ast::Spanned::new(PatternKind::Wildcard, Span::new(1, 1, 0, 1));
        assert!(flow.arm(&pattern).1.is_none());
        let (condition, exhaustive, exhausted) = flow.finish();
        assert!(exhausted);
        assert!(!exhaustive);
        assert!(
            condition
                .normal()
                .unwrap()
                .0
                .values()
                .all(|initialized| !initialized)
        );
    }

    #[test]
    fn match_variants_share_the_initial_binding_state() {
        let mut analyzer = Analyzer::new();
        analyzer.enums.insert(
            "Wide".into(),
            super::super::EnumInfo {
                variants: Default::default(),
                variant_fields: Default::default(),
                order: (0..10_000).map(|index| format!("Case{index}")).collect(),
            },
        );
        let ty = TypeKind::Named {
            name: "Wide".into(),
            type_args: Vec::new(),
        };
        let flow = analyzer.initialization_match(Some(&ty), &ExprEval::default());
        let first = flow.remaining[0].1.as_ref().unwrap();
        assert!(
            flow.remaining
                .iter()
                .all(|(_, state)| Rc::ptr_eq(first, state.as_ref().unwrap()))
        );
    }
}
