// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Bounded source-statement control flow. Span events are occurrence references,
//! not an ordering of calls/effects within expressions. Captures, destruction,
//! implicit runtime effects and opaque evaluation remain unverified; these graphs
//! grant no ownership permission and are not artifact certificates.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use super::{Analyzer, item_should_include};
use crate::cancel::Cancelled;
use crate::parser::ast::*;

pub type OwnershipFlowBlockId = usize;
pub type OwnershipFlowScopeId = usize;

#[derive(Debug, Clone)]
pub struct OwnershipControlFlow {
    pub entry: OwnershipFlowBlockId,
    pub blocks: Vec<OwnershipFlowBlock>,
    pub scopes: Vec<OwnershipFlowScope>,
    /// Lexically deferred evaluations, with no synchronous incoming edge.
    pub deferred: Vec<OwnershipDeferredFlow>,
}

#[derive(Debug, Clone)]
pub struct OwnershipDeferredFlow {
    pub span: Span,
    pub graph: Arc<OwnershipControlFlow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnershipFlowScopeKind {
    Function,
    Block,
    Loop,
    Closure,
}

#[derive(Debug, Clone)]
pub struct OwnershipFlowScope {
    pub span: Span,
    pub parent: Option<OwnershipFlowScopeId>,
    pub kind: OwnershipFlowScopeKind,
}

#[derive(Debug, Clone)]
pub struct OwnershipFlowBlock {
    /// Active lexical scopes, outermost first.
    pub scopes: Vec<OwnershipFlowScopeId>,
    pub events: Vec<OwnershipFlowEvent>,
    pub terminator: OwnershipFlowTerminator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnershipEvaluationKind {
    Statement,
    Condition,
    Return,
    LoopUpdate,
    IterationInput,
    ClosureBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnershipIterationStage {
    Prepare,
    Next,
}

#[derive(Debug, Clone)]
pub enum OwnershipFlowEvent {
    EnterScope(OwnershipFlowScopeId),
    Statement(Span),
    /// The spans form one opaque region; vector order does not promise runtime
    /// evaluation order. In particular, calls/branches inside a region are not CFG nodes.
    Evaluate {
        spans: Vec<Span>,
        kind: OwnershipEvaluationKind,
    },
    /// Unknown protocol/storage effects; never a resolved call or effect proof.
    IterationProtocol {
        span: Span,
        stage: OwnershipIterationStage,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnershipReturnKind {
    Explicit,
    TryFailure,
    ClosureResult,
}

#[derive(Debug, Clone)]
pub enum OwnershipBranchCondition {
    Expression(Span),
    IterationProtocol(Span),
}

#[derive(Debug, Clone)]
pub enum OwnershipFlowTerminator {
    /// Scope exits are ordered innermost first, for future destruction work.
    Goto {
        target: OwnershipFlowBlockId,
        exited_scopes: Vec<OwnershipFlowScopeId>,
    },
    Branch {
        condition: OwnershipBranchCondition,
        then_target: OwnershipFlowBlockId,
        else_target: OwnershipFlowBlockId,
    },
    /// Normal completion is possible, not proved. `early_return` overapproximates
    /// a `?` anywhere in this region, excluding nested closure bodies.
    Opaque {
        normal: OwnershipFlowBlockId,
        early_return: Option<OwnershipFlowBlockId>,
    },
    Return {
        span: Span,
        kind: OwnershipReturnKind,
        exited_scopes: Vec<OwnershipFlowScopeId>,
    },
    Stop {
        exited_scopes: Vec<OwnershipFlowScopeId>,
    },
}

impl OwnershipFlowTerminator {
    pub fn successors(&self) -> Vec<OwnershipFlowBlockId> {
        match self {
            Self::Goto { target, .. } => vec![*target],
            Self::Branch {
                then_target,
                else_target,
                ..
            } => vec![*then_target, *else_target],
            Self::Opaque {
                normal,
                early_return,
            } => std::iter::once(*normal)
                .chain(early_return.iter().copied())
                .collect(),
            Self::Return { .. } | Self::Stop { .. } => Vec::new(),
        }
    }
}

enum ValidationError {
    Cancelled,
    Invalid(String),
}

impl OwnershipControlFlow {
    /// Validate structural edges and lexical-scope transfers only. This does
    /// not validate expression effects, ownership, destruction or feasibility.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_with_checkpoint(&mut || Ok(()))
            .map_err(|error| match error {
                ValidationError::Invalid(message) => message,
                ValidationError::Cancelled => unreachable!("noncancellable validation"),
            })
    }

    fn validate_with_checkpoint<F>(&self, checkpoint: &mut F) -> Result<(), ValidationError>
    where
        F: FnMut() -> Result<(), Cancelled>,
    {
        let mut pending = vec![self];
        let mut seen = std::collections::HashSet::new();
        let invalid = |message: &str| ValidationError::Invalid(message.to_string());
        while let Some(graph) = pending.pop() {
            checkpoint().map_err(|_| ValidationError::Cancelled)?;
            if !seen.insert(graph as *const Self) {
                continue;
            }
            if graph.entry >= graph.blocks.len() {
                return Err(invalid("entry block is out of range"));
            }
            let entry = &graph.blocks[graph.entry];
            if entry.scopes.len() != 1 || !entry.events.iter().any(|event| matches!(event, OwnershipFlowEvent::EnterScope(scope) if Some(scope) == entry.scopes.first())) {
                return Err(invalid("entry does not introduce its callable scope"));
            }
            let Some(root_scope) = graph.scopes.get(entry.scopes[0]) else {
                return Err(invalid("entry scope is out of range"));
            };
            if root_scope.parent.is_some()
                || !matches!(
                    root_scope.kind,
                    OwnershipFlowScopeKind::Function | OwnershipFlowScopeKind::Closure
                )
            {
                return Err(invalid("entry scope is not a callable scope"));
            }
            for (index, scope) in graph.scopes.iter().enumerate() {
                checkpoint().map_err(|_| ValidationError::Cancelled)?;
                if scope.parent.is_some_and(|parent| parent >= index) {
                    return Err(invalid("scope parent is not an earlier scope"));
                }
            }
            for block in &graph.blocks {
                checkpoint().map_err(|_| ValidationError::Cancelled)?;
                let mut parent = None;
                for scope in &block.scopes {
                    let Some(info) = graph.scopes.get(*scope) else {
                        return Err(invalid("block scope is out of range"));
                    };
                    if info.parent != parent {
                        return Err(invalid("block scopes do not form a lexical chain"));
                    }
                    parent = Some(*scope);
                }
                for event in &block.events {
                    if let OwnershipFlowEvent::EnterScope(scope) = event
                        && block.scopes.last() != Some(scope)
                    {
                        return Err(invalid("scope entry does not match block scopes"));
                    }
                }
                let full_exit = block.scopes.iter().rev().copied().collect::<Vec<_>>();
                match &block.terminator {
                    OwnershipFlowTerminator::Return { exited_scopes, .. }
                    | OwnershipFlowTerminator::Stop { exited_scopes }
                        if *exited_scopes != full_exit =>
                    {
                        return Err(invalid(
                            "terminal transfer does not exit every active scope",
                        ));
                    }
                    _ => {}
                }
                for target in block.terminator.successors() {
                    checkpoint().map_err(|_| ValidationError::Cancelled)?;
                    let Some(destination) = graph.blocks.get(target) else {
                        return Err(invalid("edge target is out of range"));
                    };
                    let common = block
                        .scopes
                        .iter()
                        .zip(&destination.scopes)
                        .take_while(|(left, right)| left == right)
                        .count();
                    if common < block.scopes.len().min(destination.scopes.len()) {
                        return Err(invalid("edge jumps between unrelated lexical scopes"));
                    }
                    let exits = block.scopes[common..]
                        .iter()
                        .rev()
                        .copied()
                        .collect::<Vec<_>>();
                    match &block.terminator {
                        OwnershipFlowTerminator::Goto { exited_scopes, .. }
                            if *exited_scopes == exits => {}
                        OwnershipFlowTerminator::Goto { .. } => {
                            return Err(invalid("edge has incorrect exited scopes"));
                        }
                        _ if exits.is_empty() => {}
                        _ => return Err(invalid("non-goto edge silently exits a scope")),
                    }
                    if destination.scopes.len() > common
                        && (destination.scopes.len() != common + 1
                            || !destination.events.iter().any(|event| matches!(event, OwnershipFlowEvent::EnterScope(scope) if Some(scope) == destination.scopes.last())))
                    {
                        return Err(invalid("edge enters a scope without scope-entry evidence"));
                    }
                }
            }
            for deferred in &graph.deferred {
                checkpoint().map_err(|_| ValidationError::Cancelled)?;
                pending.push(deferred.graph.as_ref());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(super) struct FlowLimits {
    pub nodes: usize,
    pub blocks: usize,
    pub edges: usize,
    pub depth: usize,
}
impl Default for FlowLimits {
    fn default() -> Self {
        Self {
            nodes: 1_000_000,
            blocks: 250_000,
            edges: 500_000,
            depth: 64,
        }
    }
}
struct Budget {
    limits: FlowLimits,
    nodes: usize,
    blocks: usize,
    edges: usize,
}
enum FlowError {
    Cancelled,
    Limit(Span),
    Invalid(Span, String),
}
type FlowResult<T> = Result<T, FlowError>;

impl Budget {
    fn node(&mut self, span: Span) -> FlowResult<()> {
        self.charge(1, span)
    }
    fn charge(&mut self, count: usize, span: Span) -> FlowResult<()> {
        self.nodes = self.nodes.saturating_add(count);
        if self.nodes > self.limits.nodes {
            Err(FlowError::Limit(span))
        } else {
            Ok(())
        }
    }
    fn depth(&self, depth: usize, span: Span) -> FlowResult<()> {
        if depth > self.limits.depth {
            Err(FlowError::Limit(span))
        } else {
            Ok(())
        }
    }
}

impl Analyzer {
    pub(super) fn record_ownership_control_flow<F>(
        &mut self,
        program: &Program,
        checkpoint: &mut F,
    ) -> Result<BTreeMap<String, Arc<OwnershipControlFlow>>, Cancelled>
    where
        F: FnMut() -> Result<(), Cancelled>,
    {
        self.record_ownership_control_flow_with_limits(program, checkpoint, FlowLimits::default())
    }

    pub(super) fn record_ownership_control_flow_with_limits<F>(
        &mut self,
        program: &Program,
        checkpoint: &mut F,
        limits: FlowLimits,
    ) -> Result<BTreeMap<String, Arc<OwnershipControlFlow>>, Cancelled>
    where
        F: FnMut() -> Result<(), Cancelled>,
    {
        let mut budget = Budget {
            limits,
            nodes: 0,
            blocks: 0,
            edges: 0,
        };
        let mut result = BTreeMap::new();
        for item in &program.items {
            checkpoint()?;
            let functions = match &item.node {
                ItemKind::Fn { .. } => vec![(None, item)],
                ItemKind::Impl {
                    for_ty, methods, ..
                } => {
                    if budget.charge(methods.len(), item.span).is_err() {
                        self.push_error(
                            item.span,
                            "S14",
                            "ownership control flow exceeds its aggregate compiler node limit"
                                .into(),
                        );
                        return Ok(BTreeMap::new());
                    }
                    let mut functions = Vec::new();
                    for method in methods {
                        checkpoint()?;
                        functions.push((Some(&for_ty.node), method));
                    }
                    functions
                }
                _ => Vec::new(),
            };
            for (target, function) in functions {
                checkpoint()?;
                let ItemKind::Fn {
                    name,
                    attributes,
                    body: Some(body),
                    ..
                } = &function.node
                else {
                    continue;
                };
                if !item_should_include(attributes)
                    || attributes.iter().any(|attribute| {
                        matches!(attribute.name.as_str(), "intrinsic" | "syscall" | "api")
                    })
                {
                    continue;
                }
                let canonical = target
                    .map(|target| format!("{}.{name}", super::declare::type_kind_base_name(target)))
                    .unwrap_or_else(|| self.retention_source_name(name, attributes, function.span));
                if budget.charge(canonical.len(), function.span).is_err() {
                    self.push_error(
                        function.span,
                        "S14",
                        "ownership control flow exceeds its aggregate compiler node limit".into(),
                    );
                    return Ok(BTreeMap::new());
                }
                match build_family(body, &mut budget, checkpoint) {
                    Ok(graph) => {
                        result.insert(canonical, graph);
                    }
                    Err(FlowError::Cancelled) => return Err(Cancelled),
                    Err(FlowError::Invalid(span, message)) => {
                        self.push_error(
                            span,
                            "S14",
                            format!("invalid ownership source control flow: {message}"),
                        );
                        return Ok(BTreeMap::new());
                    }
                    Err(FlowError::Limit(span)) => {
                        self.push_error(span, "S14", format!("ownership control flow exceeds its aggregate compiler limit ({} nodes / {} blocks / {} edges / {} levels)", limits.nodes, limits.blocks, limits.edges, limits.depth));
                        return Ok(BTreeMap::new());
                    }
                }
            }
        }
        checkpoint()?;
        Ok(result)
    }
}

#[derive(Clone)]
struct LoopContext {
    break_target: usize,
    continue_target: usize,
    break_depth: usize,
    continue_depth: usize,
}
struct Sequence<'a> {
    statements: &'a [Stmt],
    index: usize,
    current: usize,
    scopes: Vec<usize>,
    loops: Vec<LoopContext>,
    next: usize,
    exited: Vec<usize>,
}
struct Builder<'a, 'b, F> {
    graph: OwnershipControlFlow,
    closures: Vec<&'a Expr>,
    finished: Vec<bool>,
    budget: &'b mut Budget,
    checkpoint: &'b mut F,
}

enum Root<'a> {
    Function(&'a Block),
    Closure(&'a Expr),
}

struct RootTask<'a> {
    root: Root<'a>,
    depth: usize,
    parent: Option<(usize, Span)>,
}

fn build_family<F>(
    body: &Block,
    budget: &mut Budget,
    checkpoint: &mut F,
) -> FlowResult<Arc<OwnershipControlFlow>>
where
    F: FnMut() -> Result<(), Cancelled>,
{
    let mut pending = VecDeque::from([RootTask {
        root: Root::Function(body),
        depth: 0,
        parent: None,
    }]);
    let mut drafts = Vec::<(OwnershipControlFlow, Vec<(Span, usize)>)>::new();
    while let Some(RootTask {
        root,
        depth,
        parent,
    }) = pending.pop_front()
    {
        checkpoint().map_err(|_| FlowError::Cancelled)?;
        budget.depth(depth, body.span)?;
        let index = drafts.len();
        if let Some((parent, span)) = parent {
            drafts[parent].1.push((span, index));
        }
        let mut builder = Builder {
            graph: OwnershipControlFlow {
                entry: 0,
                blocks: Vec::new(),
                scopes: Vec::new(),
                deferred: Vec::new(),
            },
            closures: Vec::new(),
            finished: Vec::new(),
            budget,
            checkpoint,
        };
        builder.root(root)?;
        if builder.finished.iter().any(|finished| !finished) {
            return Err(FlowError::Invalid(
                body.span,
                "unterminated source-flow block".into(),
            ));
        }
        builder
            .graph
            .validate_with_checkpoint(builder.checkpoint)
            .map_err(|error| match error {
                ValidationError::Cancelled => FlowError::Cancelled,
                ValidationError::Invalid(message) => FlowError::Invalid(body.span, message),
            })?;
        for closure in &builder.closures {
            builder.budget.charge(2, closure.span)?;
            pending.push_back(RootTask {
                root: Root::Closure(closure),
                depth: depth + 1,
                parent: Some((index, closure.span)),
            });
        }
        drafts.push((builder.graph, Vec::new()));
    }
    let mut completed = vec![None; drafts.len()];
    for (index, (mut graph, children)) in drafts.into_iter().enumerate().rev() {
        checkpoint().map_err(|_| FlowError::Cancelled)?;
        for (span, child) in children {
            checkpoint().map_err(|_| FlowError::Cancelled)?;
            graph.deferred.push(OwnershipDeferredFlow {
                span,
                graph: completed[child].take().expect("built deferred graph"),
            });
        }
        completed[index] = Some(Arc::new(graph));
    }
    Ok(completed[0].take().expect("function graph"))
}

impl<'a, F> Builder<'a, '_, F>
where
    F: FnMut() -> Result<(), Cancelled>,
{
    fn tick(&mut self, span: Span) -> FlowResult<()> {
        (self.checkpoint)().map_err(|_| FlowError::Cancelled)?;
        self.budget.node(span)
    }
    fn block(&mut self, scopes: &[usize], span: Span) -> FlowResult<usize> {
        self.tick(span)?;
        self.budget.depth(scopes.len(), span)?;
        self.budget.blocks += 1;
        if self.budget.blocks > self.budget.limits.blocks {
            return Err(FlowError::Limit(span));
        }
        let id = self.graph.blocks.len();
        self.budget.charge(scopes.len(), span)?;
        self.finished.push(false);
        self.graph.blocks.push(OwnershipFlowBlock {
            scopes: scopes.to_vec(),
            events: Vec::new(),
            terminator: OwnershipFlowTerminator::Stop {
                exited_scopes: Vec::new(),
            },
        });
        Ok(id)
    }
    fn terminate(
        &mut self,
        block: usize,
        terminator: OwnershipFlowTerminator,
        span: Span,
    ) -> FlowResult<()> {
        self.tick(span)?;
        if self.finished[block] {
            return Err(FlowError::Invalid(
                span,
                "source-flow block terminated twice".into(),
            ));
        }
        self.finished[block] = true;
        let exits = match &terminator {
            OwnershipFlowTerminator::Goto { exited_scopes, .. }
            | OwnershipFlowTerminator::Return { exited_scopes, .. }
            | OwnershipFlowTerminator::Stop { exited_scopes } => exited_scopes.len(),
            _ => 0,
        };
        self.budget.charge(exits, span)?;
        self.budget.edges += terminator.successors().len();
        if self.budget.edges > self.budget.limits.edges {
            return Err(FlowError::Limit(span));
        }
        self.graph.blocks[block].terminator = terminator;
        Ok(())
    }
    fn scope(
        &mut self,
        parent: &[usize],
        span: Span,
        kind: OwnershipFlowScopeKind,
    ) -> FlowResult<(Vec<usize>, usize)> {
        self.tick(span)?;
        self.budget.depth(parent.len() + 1, span)?;
        let id = self.graph.scopes.len();
        self.graph.scopes.push(OwnershipFlowScope {
            span,
            parent: parent.last().copied(),
            kind,
        });
        self.budget.charge(parent.len() + 2, span)?;
        let mut scopes = parent.to_vec();
        scopes.push(id);
        let entry = self.block(&scopes, span)?;
        self.graph.blocks[entry]
            .events
            .push(OwnershipFlowEvent::EnterScope(id));
        Ok((scopes, entry))
    }
    fn root(&mut self, root: Root<'a>) -> FlowResult<()> {
        let (span, kind) = match root {
            Root::Function(body) => (body.span, OwnershipFlowScopeKind::Function),
            Root::Closure(expr) => (expr.span, OwnershipFlowScopeKind::Closure),
        };
        let (scopes, entry) = self.scope(&[], span, kind)?;
        self.graph.entry = entry;
        match root {
            Root::Function(body) => {
                let stop = self.block(&[], span)?;
                self.terminate(
                    stop,
                    OwnershipFlowTerminator::Stop {
                        exited_scopes: Vec::new(),
                    },
                    span,
                )?;
                self.sequence(Sequence {
                    statements: &body.stmts,
                    index: 0,
                    current: entry,
                    scopes: scopes.clone(),
                    loops: Vec::new(),
                    next: stop,
                    exited: scopes.into_iter().rev().collect(),
                })
            }
            Root::Closure(closure) => {
                let ExprKind::Closure { body, .. } = &closure.node else {
                    unreachable!();
                };
                let result = self.block(&scopes, body.span)?;
                self.terminate(
                    result,
                    OwnershipFlowTerminator::Return {
                        span: body.span,
                        kind: OwnershipReturnKind::ClosureResult,
                        exited_scopes: scopes.iter().rev().copied().collect(),
                    },
                    body.span,
                )?;
                self.evaluate(
                    entry,
                    &[body],
                    OwnershipEvaluationKind::ClosureBody,
                    result,
                    &scopes,
                )
            }
        }
    }
    fn expression_child(
        &mut self,
        pending: &mut Vec<&'a Expr>,
        expression: &'a Expr,
    ) -> FlowResult<()> {
        self.tick(expression.span)?;
        pending.push(expression);
        Ok(())
    }
    fn evaluate(
        &mut self,
        current: usize,
        expressions: &[&'a Expr],
        kind: OwnershipEvaluationKind,
        normal: usize,
        scopes: &[usize],
    ) -> FlowResult<()> {
        let span = expressions
            .first()
            .map(|expression| expression.span)
            .unwrap_or(Span::new(0, 0, 0, 0));
        self.budget
            .charge(expressions.len().saturating_mul(2), span)?;
        let mut pending = expressions.to_vec();
        let mut has_try = false;
        while let Some(expr) = pending.pop() {
            self.tick(expr.span)?;
            if matches!(expr.node, ExprKind::Closure { .. }) {
                self.budget.charge(1, expr.span)?;
                self.closures.push(expr);
                continue;
            }
            macro_rules! child {
                ($expression:expr) => {
                    self.expression_child(&mut pending, $expression)?
                };
            }
            match &expr.node {
                ExprKind::Try { expr } => {
                    has_try = true;
                    child!(expr);
                }
                ExprKind::Group(expr)
                | ExprKind::Unary { expr, .. }
                | ExprKind::Cast { expr, .. }
                | ExprKind::IncDec { expr, .. } => child!(expr),
                ExprKind::Binary { left, right, .. }
                | ExprKind::Assign {
                    target: left,
                    value: right,
                }
                | ExprKind::CompoundAssign {
                    target: left,
                    value: right,
                    ..
                } => {
                    child!(left);
                    child!(right);
                }
                ExprKind::Field { object, .. } => child!(object),
                ExprKind::Index { object, indices } => {
                    child!(object);
                    for index in indices {
                        child!(index);
                    }
                }
                ExprKind::Call {
                    callee,
                    args,
                    named_args,
                    ..
                } => {
                    child!(callee);
                    for argument in args {
                        child!(argument);
                    }
                    for (_, value) in named_args {
                        child!(value);
                    }
                }
                ExprKind::MethodCall {
                    object,
                    args,
                    named_args,
                    ..
                } => {
                    child!(object);
                    for argument in args {
                        child!(argument);
                    }
                    for (_, value) in named_args {
                        child!(value);
                    }
                }
                ExprKind::ArrayLit(values) => {
                    for value in values {
                        child!(value);
                    }
                }
                ExprKind::StructInit { fields, .. } => {
                    for (_, value) in fields {
                        child!(value);
                    }
                }
                ExprKind::Match { scrutinee, arms } => {
                    child!(scrutinee);
                    for arm in arms {
                        self.tick(arm.span)?;
                        if let Some(guard) = &arm.guard {
                            child!(guard);
                        }
                        child!(&arm.expr);
                    }
                }
                ExprKind::Literal(_) | ExprKind::Ident(_) => {}
                ExprKind::Closure { .. } => unreachable!(),
            }
        }
        self.graph.blocks[current]
            .events
            .push(OwnershipFlowEvent::Evaluate {
                spans: expressions
                    .iter()
                    .map(|expression| expression.span)
                    .collect(),
                kind,
            });
        let early_return = if has_try {
            let failure = self.block(scopes, span)?;
            self.terminate(
                failure,
                OwnershipFlowTerminator::Return {
                    span,
                    kind: OwnershipReturnKind::TryFailure,
                    exited_scopes: scopes.iter().rev().copied().collect(),
                },
                span,
            )?;
            Some(failure)
        } else {
            None
        };
        self.terminate(
            current,
            OwnershipFlowTerminator::Opaque {
                normal,
                early_return,
            },
            span,
        )
    }

    fn sequence(&mut self, initial: Sequence<'a>) -> FlowResult<()> {
        let mut pending = vec![initial];
        while let Some(mut task) = pending.pop() {
            let Some(statement) = task.statements.get(task.index) else {
                self.terminate(
                    task.current,
                    OwnershipFlowTerminator::Goto {
                        target: task.next,
                        exited_scopes: task.exited,
                    },
                    self.graph.scopes[*task.scopes.last().expect("active scope")].span,
                )?;
                continue;
            };
            self.tick(statement.span)?;
            self.budget.charge(
                task.scopes.len().saturating_mul(4)
                    + task.loops.len().saturating_mul(8)
                    + task.exited.len().saturating_mul(2),
                statement.span,
            )?;
            if let StmtKind::CfgBlock { condition, .. } = &statement.node
                && !item_should_include(std::slice::from_ref(condition))
            {
                task.index += 1;
                pending.push(task);
                continue;
            }
            let current = task.current;
            self.graph.blocks[current]
                .events
                .push(OwnershipFlowEvent::Statement(statement.span));
            let next = self.block(&task.scopes, statement.span)?;
            let mut continuation = Sequence {
                statements: task.statements,
                index: task.index + 1,
                current: next,
                scopes: task.scopes.clone(),
                loops: task.loops.clone(),
                next: task.next,
                exited: task.exited.clone(),
            };
            match &statement.node {
                StmtKind::Var { value, .. } | StmtKind::Return(value) => {
                    let destination = if matches!(statement.node, StmtKind::Return(_)) {
                        let exit = self.block(&task.scopes, statement.span)?;
                        self.terminate(
                            exit,
                            OwnershipFlowTerminator::Return {
                                span: statement.span,
                                kind: OwnershipReturnKind::Explicit,
                                exited_scopes: task.scopes.iter().rev().copied().collect(),
                            },
                            statement.span,
                        )?;
                        exit
                    } else {
                        next
                    };
                    if let Some(value) = value {
                        self.evaluate(
                            current,
                            &[value],
                            if matches!(statement.node, StmtKind::Return(_)) {
                                OwnershipEvaluationKind::Return
                            } else {
                                OwnershipEvaluationKind::Statement
                            },
                            destination,
                            &task.scopes,
                        )?;
                    } else {
                        self.terminate(
                            current,
                            OwnershipFlowTerminator::Goto {
                                target: destination,
                                exited_scopes: Vec::new(),
                            },
                            statement.span,
                        )?;
                    }
                }
                StmtKind::Const { value, .. } | StmtKind::ExprStmt(value) => self.evaluate(
                    current,
                    &[value],
                    OwnershipEvaluationKind::Statement,
                    next,
                    &task.scopes,
                )?,
                StmtKind::Break | StmtKind::Continue => {
                    if let Some(context) = task.loops.last() {
                        let is_break = matches!(statement.node, StmtKind::Break);
                        let (target, depth) = if is_break {
                            (context.break_target, context.break_depth)
                        } else {
                            (context.continue_target, context.continue_depth)
                        };
                        self.terminate(
                            current,
                            OwnershipFlowTerminator::Goto {
                                target,
                                exited_scopes: task.scopes[depth..].iter().rev().copied().collect(),
                            },
                            statement.span,
                        )?;
                    } else {
                        self.terminate(
                            current,
                            OwnershipFlowTerminator::Stop {
                                exited_scopes: task.scopes.iter().rev().copied().collect(),
                            },
                            statement.span,
                        )?;
                    }
                }
                StmtKind::UnsafeBlock { body } | StmtKind::CfgBlock { body, .. } => {
                    let (scopes, entry) =
                        self.scope(&task.scopes, body.span, OwnershipFlowScopeKind::Block)?;
                    self.terminate(
                        current,
                        OwnershipFlowTerminator::Goto {
                            target: entry,
                            exited_scopes: Vec::new(),
                        },
                        statement.span,
                    )?;
                    pending.push(continuation);
                    pending.push(Sequence {
                        statements: &body.stmts,
                        index: 0,
                        current: entry,
                        exited: vec![*scopes.last().unwrap()],
                        scopes,
                        loops: task.loops,
                        next,
                    });
                    continue;
                }
                StmtKind::If {
                    condition,
                    then_block,
                    else_if,
                    else_block,
                } => {
                    pending.push(continuation);
                    self.budget
                        .charge(else_if.len().saturating_add(1), statement.span)?;
                    let cases = std::iter::once((condition, then_block))
                        .chain(else_if.iter().map(|(condition, block)| (condition, block)));
                    let mut false_target = if let Some(body) = else_block {
                        let (scopes, entry) =
                            self.scope(&task.scopes, body.span, OwnershipFlowScopeKind::Block)?;
                        pending.push(Sequence {
                            statements: &body.stmts,
                            index: 0,
                            current: entry,
                            exited: vec![*scopes.last().unwrap()],
                            scopes,
                            loops: task.loops.clone(),
                            next,
                        });
                        entry
                    } else {
                        next
                    };
                    for (condition, body) in cases.rev() {
                        self.tick(condition.span)?;
                        self.budget.charge(
                            task.scopes.len() + task.loops.len().saturating_mul(4),
                            condition.span,
                        )?;
                        let (scopes, entry) =
                            self.scope(&task.scopes, body.span, OwnershipFlowScopeKind::Block)?;
                        pending.push(Sequence {
                            statements: &body.stmts,
                            index: 0,
                            current: entry,
                            exited: vec![*scopes.last().unwrap()],
                            scopes,
                            loops: task.loops.clone(),
                            next,
                        });
                        let branch = self.block(&task.scopes, condition.span)?;
                        self.terminate(
                            branch,
                            OwnershipFlowTerminator::Branch {
                                condition: OwnershipBranchCondition::Expression(condition.span),
                                then_target: entry,
                                else_target: false_target,
                            },
                            condition.span,
                        )?;
                        let evaluation = self.block(&task.scopes, condition.span)?;
                        self.evaluate(
                            evaluation,
                            &[condition],
                            OwnershipEvaluationKind::Condition,
                            branch,
                            &task.scopes,
                        )?;
                        false_target = evaluation;
                    }
                    self.terminate(
                        current,
                        OwnershipFlowTerminator::Goto {
                            target: false_target,
                            exited_scopes: Vec::new(),
                        },
                        statement.span,
                    )?;
                    continue;
                }
                StmtKind::For { kind, body } => {
                    let (scopes, entry) =
                        self.scope(&task.scopes, statement.span, OwnershipFlowScopeKind::Loop)?;
                    self.terminate(
                        current,
                        OwnershipFlowTerminator::Goto {
                            target: entry,
                            exited_scopes: Vec::new(),
                        },
                        statement.span,
                    )?;
                    let header = self.block(&scopes, statement.span)?;
                    let exit = self.block(&scopes, statement.span)?;
                    self.terminate(
                        exit,
                        OwnershipFlowTerminator::Goto {
                            target: next,
                            exited_scopes: vec![*scopes.last().unwrap()],
                        },
                        statement.span,
                    )?;
                    let update = match kind {
                        ForLoop::CStyle {
                            update: Some(expression),
                            ..
                        } => {
                            let update = self.block(&scopes, expression.span)?;
                            self.evaluate(
                                update,
                                &[expression],
                                OwnershipEvaluationKind::LoopUpdate,
                                header,
                                &scopes,
                            )?;
                            update
                        }
                        _ => header,
                    };
                    let (body_scopes, body_entry) =
                        self.scope(&scopes, body.span, OwnershipFlowScopeKind::Block)?;
                    let mut loops = task.loops.clone();
                    loops.push(LoopContext {
                        break_target: next,
                        continue_target: update,
                        break_depth: task.scopes.len(),
                        continue_depth: scopes.len(),
                    });
                    let body_task = Sequence {
                        statements: &body.stmts,
                        index: 0,
                        current: body_entry,
                        exited: vec![*body_scopes.last().unwrap()],
                        scopes: body_scopes,
                        loops,
                        next: update,
                    };
                    let condition = match kind {
                        ForLoop::CStyle { condition, .. } | ForLoop::Cond { condition } => {
                            condition.as_ref()
                        }
                        _ => None,
                    };
                    if let Some(condition) = condition {
                        let branch = self.block(&scopes, condition.span)?;
                        self.terminate(
                            branch,
                            OwnershipFlowTerminator::Branch {
                                condition: OwnershipBranchCondition::Expression(condition.span),
                                then_target: body_entry,
                                else_target: exit,
                            },
                            condition.span,
                        )?;
                        self.evaluate(
                            header,
                            &[condition],
                            OwnershipEvaluationKind::Condition,
                            branch,
                            &scopes,
                        )?;
                    } else if matches!(kind, ForLoop::Each { .. }) {
                        self.graph.blocks[header].events.push(
                            OwnershipFlowEvent::IterationProtocol {
                                span: statement.span,
                                stage: OwnershipIterationStage::Next,
                            },
                        );
                        self.terminate(
                            header,
                            OwnershipFlowTerminator::Branch {
                                condition: OwnershipBranchCondition::IterationProtocol(
                                    statement.span,
                                ),
                                then_target: body_entry,
                                else_target: exit,
                            },
                            statement.span,
                        )?;
                    } else {
                        self.terminate(
                            header,
                            OwnershipFlowTerminator::Goto {
                                target: body_entry,
                                exited_scopes: Vec::new(),
                            },
                            statement.span,
                        )?;
                    }
                    pending.push(continuation);
                    pending.push(body_task);
                    match kind {
                        ForLoop::CStyle {
                            init: Some(init), ..
                        } => pending.push(Sequence {
                            statements: std::slice::from_ref(init.as_ref()),
                            index: 0,
                            current: entry,
                            scopes,
                            loops: task.loops,
                            next: header,
                            exited: Vec::new(),
                        }),
                        ForLoop::Each { iter, .. } => {
                            let prepare = self.block(&scopes, statement.span)?;
                            self.graph.blocks[prepare].events.push(
                                OwnershipFlowEvent::IterationProtocol {
                                    span: statement.span,
                                    stage: OwnershipIterationStage::Prepare,
                                },
                            );
                            self.terminate(
                                prepare,
                                OwnershipFlowTerminator::Goto {
                                    target: header,
                                    exited_scopes: Vec::new(),
                                },
                                statement.span,
                            )?;
                            let expressions = match iter {
                                ForIter::Iter(expression) => vec![expression.as_ref()],
                                ForIter::Range { start, end } => vec![start.as_ref(), end.as_ref()],
                            };
                            self.evaluate(
                                entry,
                                &expressions,
                                OwnershipEvaluationKind::IterationInput,
                                prepare,
                                &scopes,
                            )?;
                        }
                        _ => self.terminate(
                            entry,
                            OwnershipFlowTerminator::Goto {
                                target: header,
                                exited_scopes: Vec::new(),
                            },
                            statement.span,
                        )?,
                    }
                    continue;
                }
            }
            // There is deliberately no edge into this continuation after an
            // abrupt transfer. Its source statements remain disconnected.
            continuation.current = next;
            pending.push(continuation);
        }
        Ok(())
    }
}
