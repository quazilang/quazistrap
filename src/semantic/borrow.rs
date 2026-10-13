// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::parser::ast::*;

use super::*;

// ── Copy / Move classification ────────────────────────────────────────────────

// ── Per-variable ownership state ──────────────────────────────────────────────

#[derive(Debug, Clone)]
struct OwnedVar {
    ty: Option<TypeKind>,
    /// Some(span) → moved at that span; None → valid (not moved).
    moved_at: Option<Span>,
    /// Loop depth at declaration site (used to detect move-in-loop).
    loop_depth_at_decl: usize,
    control_depth_at_decl: usize,
    /// First live shared loan of this root.
    shared_borrow: Option<Loan>,
    /// First live exclusive loan of this root.
    exclusive_borrow: Option<Loan>,
    /// Conservative roots of a string view or a reference used to derive one.
    view_origins: BTreeSet<ViewOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ViewOrigin {
    Owner { name: String, scope_depth: usize },
    Temporary,
}

/// A lexical loan. Borrowed references cannot escape their declaring scope, so
/// this is a sound lower bound on their region until call-effect analysis can
/// derive smaller regions from uses.
#[derive(Debug, Clone, Copy)]
struct Loan {
    at: Span,
    scope_depth: usize,
}

/// The capability a resolved direct call requires for one explicit argument.
///
/// This is deliberately derived from the declared parameter type, rather than
/// an API name or an assumption that every argument is consumed. Calls whose
/// target is opaque keep the conservative `Consume` behavior until D-014 has
/// composed effects for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallArgumentEffect {
    Consume,
    SharedBorrow,
    ExclusiveBorrow,
}

// ── Scoped move environment ───────────────────────────────────────────────────

#[derive(Clone)]
struct MoveEnv {
    scopes: Vec<HashMap<String, OwnedVar>>,
    loop_depth: usize,
    control_depth: usize,
    /// Variables currently being re-assigned (`x = f(x)`). Move-in-loop is
    /// suppressed for these because the assignment immediately re-owns the value.
    reassign_targets: std::collections::HashSet<String>,
    /// A consuming destructor owns this whole receiver while it tears down its
    /// fields. Its projections are the single pre-structural-drop exception to
    /// the ordinary partial-move rule.
    destructor_receiver: Option<String>,
    /// A bare receiver owned by the current inherent method.  A narrow return
    /// rule may transfer one of its fields only when code generation can leave
    /// the remaining structural cleanup intact.
    consuming_receiver: Option<String>,
    /// A `return self.field` checkpoint may transfer one direct field from a
    /// consumed receiver when compiler-generated structural cleanup can prove
    /// the remaining fields are still destructible.
    returning_receiver_field: Option<String>,
    capture_boundary: Option<usize>,
}

impl MoveEnv {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            loop_depth: 0,
            control_depth: 0,
            reassign_targets: std::collections::HashSet::new(),
            destructor_receiver: None,
            consuming_receiver: None,
            returning_receiver_field: None,
            capture_boundary: None,
        }
    }

    fn for_consuming_destructor(receiver: String) -> Self {
        Self {
            destructor_receiver: Some(receiver),
            consuming_receiver: Some("self".to_string()),
            ..Self::new()
        }
    }

    fn for_consuming_receiver(receiver: String) -> Self {
        Self {
            consuming_receiver: Some(receiver),
            ..Self::new()
        }
    }

    fn allows_destructor_projection_move(&self, expr: &Expr) -> bool {
        self.destructor_receiver
            .as_deref()
            .is_some_and(|receiver| projection_root_ident(expr) == Some(receiver))
    }

    fn allows_returned_receiver_field_move(&self, expr: &Expr) -> bool {
        self.returning_receiver_field
            .as_deref()
            .is_some_and(|receiver| {
                matches!(&expr.node, ExprKind::Field { object, .. }
                if matches!(&object.node, ExprKind::Ident(name) if name == receiver))
            })
    }

    fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn exit_scope(&mut self) {
        let exiting_depth = self.scopes.len().saturating_sub(1);
        self.scopes.pop();
        for scope in &mut self.scopes {
            for variable in scope.values_mut() {
                if variable
                    .shared_borrow
                    .is_some_and(|loan| loan.scope_depth >= exiting_depth)
                {
                    variable.shared_borrow = None;
                }
                if variable
                    .exclusive_borrow
                    .is_some_and(|loan| loan.scope_depth >= exiting_depth)
                {
                    variable.exclusive_borrow = None;
                }
            }
        }
    }

    fn declare(&mut self, name: String, ty: Option<TypeKind>) {
        let depth = self.loop_depth;
        let control_depth = self.control_depth;
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                name,
                OwnedVar {
                    ty,
                    moved_at: None,
                    loop_depth_at_decl: depth,
                    control_depth_at_decl: control_depth,
                    shared_borrow: None,
                    exclusive_borrow: None,
                    view_origins: BTreeSet::new(),
                },
            );
        }
    }

    fn lookup(&self, name: &str) -> Option<&OwnedVar> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v);
            }
        }
        None
    }

    fn binding_depth(&self, name: &str) -> Option<usize> {
        self.scopes
            .iter()
            .rposition(|scope| scope.contains_key(name))
    }

    fn set_view_origins(&mut self, name: &str, origins: BTreeSet<ViewOrigin>) {
        if let Some(depth) = self.binding_depth(name) {
            self.scopes[depth]
                .get_mut(name)
                .expect("binding exists")
                .view_origins = origins;
        }
    }

    fn mark_moved(&mut self, name: &str, at: Span) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(v) = scope.get_mut(name) {
                v.moved_at = Some(at);
                return;
            }
        }
    }

    fn mark_shared_borrowed(&mut self, name: &str, at: Span) {
        let scope_depth = self.scopes.len().saturating_sub(1);
        for scope in self.scopes.iter_mut().rev() {
            if let Some(variable) = scope.get_mut(name) {
                variable
                    .shared_borrow
                    .get_or_insert(Loan { at, scope_depth });
                return;
            }
        }
    }

    fn mark_exclusive_borrowed(&mut self, name: &str, at: Span) {
        let scope_depth = self.scopes.len().saturating_sub(1);
        for scope in self.scopes.iter_mut().rev() {
            if let Some(variable) = scope.get_mut(name) {
                variable
                    .exclusive_borrow
                    .get_or_insert(Loan { at, scope_depth });
                return;
            }
        }
    }

    /// Clear the moved state — called when a variable is re-assigned a new value.
    fn reinit(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(v) = scope.get_mut(name) {
                v.moved_at = None;
                return;
            }
        }
    }

    /// Conservative branch merge: if a variable was moved in `branch`, mark it
    /// moved in `self` too (union of moved sets).
    fn apply_branch_moves(&mut self, branch: &MoveEnv) {
        for scope in &branch.scopes {
            for (name, var) in scope {
                if let Some(at) = var.moved_at {
                    self.mark_moved(name, at);
                }
                if let Some(loan) = var.shared_borrow {
                    self.mark_shared_borrowed(name, loan.at);
                }
                if let Some(loan) = var.exclusive_borrow {
                    self.mark_exclusive_borrowed(name, loan.at);
                }
                if let Some(depth) = self.binding_depth(name) {
                    self.scopes[depth]
                        .get_mut(name)
                        .expect("binding exists")
                        .view_origins
                        .extend(var.view_origins.iter().cloned());
                }
            }
        }
    }
}

fn assignment_target_ident(expr: &Expr) -> Option<&str> {
    match &expr.node {
        ExprKind::Ident(name) => Some(name),
        ExprKind::Group(inner) => assignment_target_ident(inner),
        _ => None,
    }
}

fn projection_root_ident(expr: &Expr) -> Option<&str> {
    match &expr.node {
        ExprKind::Ident(name) => Some(name),
        ExprKind::Group(inner)
        | ExprKind::Unary { expr: inner, .. }
        | ExprKind::Cast { expr: inner, .. }
        | ExprKind::Try { expr: inner } => projection_root_ident(inner),
        ExprKind::Field { object, .. } | ExprKind::Index { object, .. } => {
            projection_root_ident(object)
        }
        _ => None,
    }
}

// ── Borrow-check pass ─────────────────────────────────────────────────────────

impl Analyzer {
    pub(super) fn run_borrow_check_pass(&mut self, program: &Program) {
        for item in &program.items {
            // Skip @cfg-disabled items.
            let attrs = match &item.node {
                ItemKind::Fn { attributes, .. } => Some(attributes),
                _ => None,
            };
            if let Some(attrs) = attrs
                && !super::item_should_include(attrs)
            {
                continue;
            }
            match &item.node {
                ItemKind::Fn {
                    name,
                    params,
                    body: Some(body),
                    ..
                } => {
                    self.current_function.push(
                        self.module_path_for_span(item.span)
                            .map(|module| format!("{module}.{name}"))
                            .unwrap_or_else(|| name.clone()),
                    );
                    let callable = self
                        .current_function
                        .last()
                        .expect("pushed callable")
                        .clone();
                    self.current_generic_params
                        .push(self.generic_params_for_function(&callable));
                    let mut env = MoveEnv::new();
                    for p in params {
                        self.bc_record_contiguous_destructor_root(&p.ty.node);
                        env.declare(p.name.clone(), Some(p.ty.node.clone()));
                    }
                    self.bc_block(body, &mut env);
                    self.current_generic_params.pop();
                    let _ = self.current_function.pop();
                }
                ItemKind::Impl {
                    for_ty, methods, ..
                } => {
                    let type_name = crate::semantic::declare::type_kind_base_name(&for_ty.node);
                    for m in methods {
                        if let ItemKind::Fn {
                            name,
                            params,
                            body: Some(body),
                            ..
                        } = &m.node
                        {
                            self.current_function.push(format!("{type_name}.{name}"));
                            let callable = self
                                .current_function
                                .last()
                                .expect("pushed callable")
                                .clone();
                            self.current_generic_params
                                .push(self.generic_params_for_function(&callable));
                            let has_consuming_receiver = params.first().is_some_and(|param| {
                                param.name == "self"
                                    && !matches!(
                                        self.resolve_type_aliases(&param.ty.node),
                                        TypeKind::Ref { .. } | TypeKind::MutRef { .. }
                                    )
                            });
                            let is_consuming_destructor =
                                has_consuming_receiver && name.rsplit('.').next() == Some("free");
                            let mut env = if is_consuming_destructor {
                                MoveEnv::for_consuming_destructor("self".to_string())
                            } else if has_consuming_receiver {
                                MoveEnv::for_consuming_receiver("self".to_string())
                            } else {
                                MoveEnv::new()
                            };
                            for p in params {
                                self.bc_record_contiguous_destructor_root(&p.ty.node);
                                env.declare(p.name.clone(), Some(p.ty.node.clone()));
                            }
                            self.bc_block(body, &mut env);
                            self.current_generic_params.pop();
                            let _ = self.current_function.pop();
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn bc_block(&mut self, block: &Block, env: &mut MoveEnv) {
        env.enter_scope();
        for stmt in &block.stmts {
            self.bc_stmt(stmt, env);
        }
        env.exit_scope();
    }

    fn bc_is_string_view(&self, ty: &TypeKind) -> bool {
        match self.resolve_type_aliases(ty) {
            TypeKind::Str => true,
            TypeKind::Ref { inner } | TypeKind::MutRef { inner } => {
                matches!(inner.node, TypeKind::Str)
            }
            _ => false,
        }
    }

    fn bc_view_origins(&self, expr: &Expr, env: &MoveEnv) -> BTreeSet<ViewOrigin> {
        if !self
            .bc_annotated_type(expr)
            .as_ref()
            .is_some_and(|ty| self.bc_is_string_view(ty))
        {
            return BTreeSet::new();
        }
        match &expr.node {
            ExprKind::Ident(name) => env
                .lookup(name)
                .map(|var| var.view_origins.clone())
                .unwrap_or_default(),
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Unary { expr: inner, .. }
            | ExprKind::Try { expr: inner } => self.bc_view_origins(inner, env),
            ExprKind::Field { object, .. } | ExprKind::Index { object, .. } => {
                self.bc_owner_origins(object, env)
            }
            ExprKind::MethodCall {
                object,
                args,
                named_args,
                ..
            } => {
                let mut roots = self.bc_view_origins(object, env);
                if !self.bc_has_consuming_receiver(expr) {
                    roots.extend(self.bc_owner_origins(object, env));
                }
                for argument in args.iter().chain(named_args.iter().map(|(_, arg)| arg)) {
                    roots.extend(self.bc_borrowed_argument_origins(argument, env));
                }
                roots
            }
            ExprKind::Call {
                args, named_args, ..
            } => args
                .iter()
                .chain(named_args.iter().map(|(_, arg)| arg))
                .flat_map(|argument| self.bc_borrowed_argument_origins(argument, env))
                .collect(),
            ExprKind::Assign { value, .. } => self.bc_view_origins(value, env),
            ExprKind::Match { arms, .. } => arms
                .iter()
                .flat_map(|arm| self.bc_view_origins(&arm.expr, env))
                .collect(),
            _ => BTreeSet::new(),
        }
    }

    fn bc_borrowed_argument_origins(&self, expr: &Expr, env: &MoveEnv) -> BTreeSet<ViewOrigin> {
        if self
            .bc_annotated_type(expr)
            .as_ref()
            .is_some_and(|ty| self.bc_is_string_view(ty))
        {
            return self.bc_view_origins(expr, env);
        }
        if self.bc_annotated_type(expr).as_ref().is_some_and(|ty| {
            matches!(
                self.resolve_type_aliases(ty),
                TypeKind::Ref { .. } | TypeKind::MutRef { .. }
            )
        }) {
            return self.bc_owner_origins(expr, env);
        }
        BTreeSet::new()
    }

    fn bc_owner_origins(&self, expr: &Expr, env: &MoveEnv) -> BTreeSet<ViewOrigin> {
        match &expr.node {
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Unary { expr: inner, .. }
            | ExprKind::Try { expr: inner } => {
                return self.bc_owner_origins(inner, env);
            }
            ExprKind::Ident(name) => {
                if let Some(var) = env.lookup(name) {
                    if !var.view_origins.is_empty() {
                        return var.view_origins.clone();
                    }
                    if var.ty.as_ref().is_some_and(|ty| {
                        let resolved = self.resolve_type_aliases(ty);
                        matches!(resolved, TypeKind::Named { .. })
                            && self.bc_is_move_type(&resolved)
                    }) {
                        return BTreeSet::from([ViewOrigin::Owner {
                            name: name.clone(),
                            scope_depth: env.binding_depth(name).expect("binding exists"),
                        }]);
                    }
                }
                return BTreeSet::new();
            }
            ExprKind::Field { object, .. } | ExprKind::Index { object, .. } => {
                return self.bc_owner_origins(object, env);
            }
            _ => {}
        }
        match self
            .bc_annotated_type(expr)
            .as_ref()
            .map(|ty| self.resolve_type_aliases(ty))
        {
            Some(ty @ TypeKind::Named { .. }) if self.bc_is_move_type(&ty) => {
                BTreeSet::from([ViewOrigin::Temporary])
            }
            Some(TypeKind::Ref { .. } | TypeKind::MutRef { .. }) => match &expr.node {
                ExprKind::MethodCall { object, .. } => self.bc_owner_origins(object, env),
                ExprKind::Call {
                    args, named_args, ..
                } => args
                    .iter()
                    .chain(named_args.iter().map(|(_, arg)| arg))
                    .flat_map(|arg| self.bc_borrowed_argument_origins(arg, env))
                    .collect(),
                _ => BTreeSet::new(),
            },
            _ => BTreeSet::new(),
        }
    }

    fn bc_bind_view(&mut self, name: &str, value: &Expr, env: &mut MoveEnv) {
        let roots = self.bc_binding_view_origins(value, env);
        self.bc_bind_view_origins(name, value, roots, env);
    }

    fn bc_binding_view_origins(&self, value: &Expr, env: &MoveEnv) -> BTreeSet<ViewOrigin> {
        let is_view = self
            .bc_annotated_type(value)
            .as_ref()
            .is_some_and(|ty| self.bc_is_string_view(ty));
        if is_view {
            self.bc_view_origins(value, env)
        } else {
            self.bc_borrowed_argument_origins(value, env)
        }
    }

    fn bc_bind_view_origins(
        &mut self,
        name: &str,
        value: &Expr,
        roots: BTreeSet<ViewOrigin>,
        env: &mut MoveEnv,
    ) {
        let is_view = self
            .bc_annotated_type(value)
            .as_ref()
            .is_some_and(|ty| self.bc_is_string_view(ty));
        let binding_depth = env.binding_depth(name).expect("view binding declared");
        for root in roots.iter().filter(|_| is_view) {
            match root {
                ViewOrigin::Temporary => self.push_error(value.span, "S10",
                    "cannot store a string view borrowed from a temporary owner; keep the owner in a local binding".to_string()),
                ViewOrigin::Owner { name: owner, scope_depth } => {
                    if *scope_depth > binding_depth {
                        self.push_error(value.span, "S10", format!(
                            "string view binding `{name}` would outlive its owner `{owner}`"));
                    }
                    if let Some(var) = env.scopes.get_mut(*scope_depth).and_then(|scope| scope.get_mut(owner)) {
                        if let Some(loan) = &mut var.shared_borrow {
                            loan.scope_depth = loan.scope_depth.min(binding_depth);
                        } else {
                            var.shared_borrow = Some(Loan { at: value.span, scope_depth: binding_depth });
                        }
                    }
                }
            }
        }
        env.set_view_origins(name, roots);
    }

    fn bc_loan_view_argument(&self, value: &Expr, env: &mut MoveEnv) {
        let depth = env.scopes.len().saturating_sub(1);
        for root in self.bc_view_origins(value, env) {
            if let ViewOrigin::Owner { name, scope_depth } = root
                && let Some(var) = env
                    .scopes
                    .get_mut(scope_depth)
                    .and_then(|scope| scope.get_mut(&name))
            {
                if let Some(loan) = &mut var.shared_borrow {
                    loan.scope_depth = loan.scope_depth.min(depth);
                } else {
                    var.shared_borrow = Some(Loan {
                        at: value.span,
                        scope_depth: depth,
                    });
                }
            }
        }
    }

    fn bc_reject_view_escape(&mut self, value: &Expr, env: &MoveEnv, destination: &str) {
        if !self.bc_view_origins(value, env).is_empty() {
            self.push_error(value.span, "S10", format!(
                "cannot {destination} a string view rooted in an owned local or temporary before escape analysis proves its lifetime"));
        }
    }

    fn bc_stmt(&mut self, stmt: &Stmt, env: &mut MoveEnv) {
        match &stmt.node {
            StmtKind::Var {
                name, ty, value, ..
            } => {
                let var_ty = ty.as_ref().map(|t| t.node.clone()).or_else(|| {
                    value
                        .as_ref()
                        .and_then(|value| self.bc_annotated_type(value))
                });
                if let Some(v) = value {
                    self.bc_expr(v, env, true);
                }
                let origins = value
                    .as_ref()
                    .map(|value| self.bc_binding_view_origins(value, env));
                if let Some(var_ty) = &var_ty {
                    self.bc_record_contiguous_destructor_root(var_ty);
                }
                env.declare(name.clone(), var_ty);
                if let Some(value) = value {
                    self.bc_bind_view_origins(
                        name,
                        value,
                        origins.expect("initialized binding"),
                        env,
                    );
                }
            }
            StmtKind::Const {
                name, ty, value, ..
            } => {
                let var_ty = ty
                    .as_ref()
                    .map(|t| t.node.clone())
                    .or_else(|| self.bc_annotated_type(value));
                self.bc_expr(value, env, true);
                let origins = self.bc_binding_view_origins(value, env);
                if let Some(var_ty) = &var_ty {
                    self.bc_record_contiguous_destructor_root(var_ty);
                }
                env.declare(name.clone(), var_ty);
                self.bc_bind_view_origins(name, value, origins, env);
            }
            StmtKind::Return(Some(expr)) => {
                self.bc_reject_view_escape(expr, env, "return");
                // Return exits the function; moves here don't affect post-return code.
                // Use a cloned env with loop_depth=0 to suppress the "move in loop" error
                // (the loop never runs again after a return) and don't apply back.
                let mut ret_env = env.clone();
                ret_env.loop_depth = 0;
                if self.bc_can_return_receiver_field(expr, &ret_env) {
                    ret_env.returning_receiver_field = ret_env.consuming_receiver.clone();
                }
                self.bc_expr(expr, &mut ret_env, true);
            }
            StmtKind::Return(None) => {}
            StmtKind::ExprStmt(expr) => {
                self.bc_expr(expr, env, false);
            }
            StmtKind::If {
                condition,
                then_block,
                else_if,
                else_block,
            } => {
                self.bc_expr(condition, env, false);
                let mut then_env = env.clone();
                then_env.control_depth += 1;
                self.bc_block(then_block, &mut then_env);
                let mut all_envs = vec![then_env];
                for (else_if_cond, else_if_block) in else_if {
                    let mut ei_env = env.clone();
                    ei_env.control_depth += 1;
                    self.bc_expr(else_if_cond, &mut ei_env, false);
                    self.bc_block(else_if_block, &mut ei_env);
                    all_envs.push(ei_env);
                }
                if let Some(eb) = else_block {
                    let mut else_env = env.clone();
                    else_env.control_depth += 1;
                    self.bc_block(eb, &mut else_env);
                    all_envs.push(else_env);
                }
                // Conservative: moved in any branch → moved after.
                for branch_env in &all_envs {
                    env.apply_branch_moves(branch_env);
                }
            }
            StmtKind::For { kind, body } => {
                // Evaluate iterable/range bounds BEFORE entering the loop scope so
                // that moves of the iterable happen at the outer loop depth (matching
                // Rust's `for x in collection` semantics).
                if let ForLoop::Each { iter, .. } = kind {
                    match iter {
                        ForIter::Range { start, end } => {
                            self.bc_expr(start, env, false);
                            self.bc_expr(end, env, false);
                        }
                        ForIter::Iter(expr) => {
                            // `for x : iterable` moves the iterable (like Rust's
                            // `for x in collection`). Borrow with `for x : &collection`.
                            self.bc_expr(expr, env, true);
                        }
                    }
                }
                env.loop_depth += 1;
                let mut loop_env = env.clone();
                loop_env.control_depth += 1;
                match kind {
                    ForLoop::Each { vars, .. } => {
                        loop_env.enter_scope();
                        for var in vars {
                            loop_env.declare(var.clone(), None);
                        }
                        for s in &body.stmts {
                            self.bc_stmt(s, &mut loop_env);
                        }
                        loop_env.exit_scope();
                    }
                    ForLoop::CStyle {
                        init,
                        condition,
                        update,
                    } => {
                        if let Some(init_stmt) = init {
                            self.bc_stmt(init_stmt, &mut loop_env);
                        }
                        if let Some(cond) = condition {
                            self.bc_expr(cond, &mut loop_env, false);
                        }
                        self.bc_block(body, &mut loop_env);
                        if let Some(upd) = update {
                            self.bc_expr(upd, &mut loop_env, false);
                        }
                    }
                    ForLoop::Cond { condition } => {
                        if let Some(cond) = condition {
                            self.bc_expr(cond, &mut loop_env, false);
                        }
                        self.bc_block(body, &mut loop_env);
                    }
                }
                env.loop_depth -= 1;
                env.apply_branch_moves(&loop_env);
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::UnsafeBlock { body } => {
                self.bc_block(body, env);
            }
            StmtKind::CfgBlock { body, .. } => {
                self.bc_block(body, env);
            }
        }
    }

    /// Check an expression for ownership violations.
    ///
    /// `consumed = true` means this expression is in move position (its value is
    /// taken by the surrounding construct). For non-Copy idents, this triggers a
    /// move and checks for move-in-loop and use-after-move.
    /// Returns true only for Named types that are concrete user-defined structs/enums.
    /// Generic type params (K, V, T, etc.) and unknown names are treated as Copy.
    pub(super) fn bc_is_move_type(&self, ty: &TypeKind) -> bool {
        let resolved = self.resolve_type_aliases(ty);
        match &resolved {
            // Primitives and shared references are Copy. Exclusive references are
            // affine capabilities, so assigning one transfers it instead.
            TypeKind::Bool
            | TypeKind::Int8
            | TypeKind::Int16
            | TypeKind::Int32
            | TypeKind::Int64
            | TypeKind::Uint8
            | TypeKind::Uint16
            | TypeKind::Uint32
            | TypeKind::Uint64
            | TypeKind::Isize
            | TypeKind::Usize
            | TypeKind::Float16
            | TypeKind::Float32
            | TypeKind::Float64
            | TypeKind::Str
            | TypeKind::Bytes
            | TypeKind::RawPtr { .. }
            | TypeKind::CFn { .. }
            | TypeKind::Ref { .. } => false,
            TypeKind::Named { name, type_args } if type_args.is_empty() => {
                self.is_declared_type_name(name)
                    && (self.struct_defs.contains_key(name) || self.enums.contains_key(name))
            }
            // All other types (structs, enums, arrays, slices, dyn Trait, etc.) are move types.
            _ => true,
        }
    }

    /// Resolve a span to a human-readable `file:line:col` label using source_files.
    fn span_label(&self, span: Span) -> String {
        self.source_files
            .iter()
            .find(|f| f.contains(span))
            .map(|f| f.label(span))
            .unwrap_or_else(|| format!("{}:{}", span.line, span.col))
    }

    fn bc_expr(&mut self, expr: &Expr, env: &mut MoveEnv, consumed: bool) {
        match &expr.node {
            ExprKind::Ident(name) => {
                let Some(var) = env.lookup(name) else { return };
                if !var.view_origins.is_empty() {
                    if env.capture_boundary.is_some_and(|boundary| {
                        env.binding_depth(name)
                            .is_some_and(|depth| depth < boundary)
                    }) {
                        self.push_error(expr.span, "S10", format!(
                            "cannot capture owner-rooted string view `{name}` before escape analysis proves its lifetime"));
                    }
                    for origin in &var.view_origins {
                        if let ViewOrigin::Owner {
                            name: owner,
                            scope_depth,
                        } = origin
                            && env
                                .scopes
                                .get(*scope_depth)
                                .and_then(|scope| scope.get(owner))
                                .is_none_or(|root| root.moved_at.is_some())
                        {
                            self.push_error(
                                expr.span,
                                "S10",
                                format!(
                                    "string view `{name}` refers to an unavailable owner `{owner}`"
                                ),
                            );
                        }
                    }
                }
                if let Some(borrowed_at) = var.exclusive_borrow.map(|loan| loan.at) {
                    self.push_error(
                        expr.span,
                        "S10",
                        format!(
                            "cannot use `{name}` while it is exclusively borrowed (borrowed at {})",
                            self.span_label(borrowed_at)
                        ),
                    );
                    return;
                }
                let is_move = var.ty.as_ref().is_some_and(|t| self.bc_is_move_type(t));
                if !is_move {
                    return;
                } // Copy type or unresolved generic: no tracking needed.

                // Use-after-move check (both consuming and non-consuming reads).
                if let Some(moved_at) = var.moved_at {
                    self.push_error(
                        expr.span,
                        "S10",
                        format!(
                            "use of moved value '{}' (moved at {})",
                            name,
                            self.span_label(moved_at)
                        ),
                    );
                    return;
                }

                if consumed {
                    if matches!(
                        var.ty.as_ref().map(|ty| self.resolve_type_aliases(ty)),
                        Some(TypeKind::Fn { .. })
                    ) && env.control_depth > var.control_depth_at_decl
                    {
                        self.push_error(
                            expr.span,
                            "S10",
                            format!(
                                "cannot move function owner `{name}` on only one control-flow path before path-sensitive cleanup is implemented"
                            ),
                        );
                        return;
                    }
                    if let Some(borrowed_at) = var.shared_borrow.map(|loan| loan.at) {
                        self.push_error(
                            expr.span,
                            "S10",
                            format!(
                                "cannot move `{name}` while it is shared-borrowed (borrowed at {})",
                                self.span_label(borrowed_at)
                            ),
                        );
                        return;
                    }
                    if let Some(borrowed_at) = var.exclusive_borrow.map(|loan| loan.at) {
                        self.push_error(
                            expr.span,
                            "S10",
                            format!(
                                "cannot move `{name}` while it is exclusively borrowed (borrowed at {})",
                                self.span_label(borrowed_at)
                            ),
                        );
                        return;
                    }
                    // Moving inside a loop when the var was declared at a lower loop depth.
                    // Suppressed when the variable is the target of the enclosing assignment
                    // (x = f(x) pattern) — the reassignment immediately re-owns the value.
                    if env.loop_depth > var.loop_depth_at_decl
                        && !env.reassign_targets.contains(name.as_str())
                    {
                        self.push_error(
                            expr.span,
                            "S10",
                            format!(
                                "cannot move '{}' inside a loop: value would be moved on the first iteration and invalid on subsequent ones",
                                name
                            ),
                        );
                        env.mark_moved(name, expr.span);
                        return;
                    }
                    env.mark_moved(name, expr.span);
                }
            }

            ExprKind::Assign { target, value } => {
                if consumed
                    && let Some(name) = assignment_target_ident(target)
                    && env.lookup(name).is_some_and(|variable| {
                        variable.ty.as_ref().is_some_and(|ty| {
                            matches!(self.resolve_type_aliases(ty), TypeKind::Fn { .. })
                        })
                    })
                {
                    self.push_error(
                        expr.span,
                        "S10",
                        "a function-valued assignment cannot itself transfer ownership; assign first, then move the binding"
                            .to_string(),
                    );
                }
                self.bc_reject_borrowed_write(target, env);
                // Mark target as being re-assigned so move-in-loop is suppressed
                // for `x = f(x)` patterns (value is immediately re-owned).
                if let Some(name) = assignment_target_ident(target) {
                    env.reassign_targets.insert(name.to_string());
                }
                self.bc_expr(value, env, true);
                if let Some(name) = assignment_target_ident(target) {
                    env.reassign_targets.remove(name);
                    env.reinit(name);
                    if env.lookup(name).is_some() {
                        self.bc_bind_view(name, value, env);
                    } else {
                        self.bc_reject_view_escape(value, env, "store");
                    }
                } else {
                    self.bc_reject_view_escape(value, env, "store");
                    // Indexed replacement evaluates the storage and index,
                    // but does not materialize the old element as a value.
                    if let ExprKind::Index { object, indices } = &target.node {
                        self.bc_expr(object, env, false);
                        for index in indices {
                            self.bc_expr(index, env, false);
                        }
                    } else {
                        self.bc_expr(target, env, false);
                    }
                }
            }

            ExprKind::CompoundAssign { target, value, .. } => {
                // Read-modify-write: no ownership transfer.
                self.bc_reject_borrowed_write(target, env);
                self.bc_expr(target, env, false);
                self.bc_expr(value, env, false);
            }

            ExprKind::IncDec { expr: inner, .. } => {
                // In-place mutation: not a move.
                self.bc_reject_borrowed_write(inner, env);
                self.bc_expr(inner, env, false);
            }

            ExprKind::Call {
                callee,
                args,
                named_args,
                ..
            } => {
                self.bc_expr(callee, env, false);
                if let ExprKind::Ident(variant) = &callee.node
                    && self.bc_is_enum_constructor(expr, variant)
                {
                    for argument in args.iter().chain(named_args.iter().map(|(_, arg)| arg)) {
                        self.bc_reject_view_escape(argument, env, "store in an enum");
                    }
                }
                // A resolved safe source function cannot currently return,
                // store, or capture a non-string reference. Give its
                // direct-call arguments a synthetic lexical scope so an
                // address taken only for that call ends when the call returns.
                // Function values, foreign/unsafe calls, variadics, and QZI
                // interfaces remain opaque and retain the surrounding loan.
                let direct_call = self.bc_resolved_direct_call(expr);
                // Keep an owner-backed string argument alive through sibling
                // argument evaluation. This region is only a local evaluation
                // guard; returned views are propagated separately and it is
                // not an opaque-callee escape certificate.
                let view_arguments = args
                    .iter()
                    .chain(named_args.iter().map(|(_, arg)| arg))
                    .any(|argument| !self.bc_view_origins(argument, env).is_empty())
                    || self.bc_is_display_variadic_call(expr);
                if direct_call || view_arguments {
                    env.enter_scope();
                }
                for (index, arg) in args.iter().enumerate() {
                    self.bc_call_argument(
                        arg,
                        self.bc_call_argument_effect(expr, index, None),
                        env,
                    );
                }
                for (name, arg) in named_args {
                    self.bc_call_argument(
                        arg,
                        self.bc_call_argument_effect(expr, args.len(), Some(name)),
                        env,
                    );
                }
                if direct_call || view_arguments {
                    env.exit_scope();
                }
            }

            ExprKind::MethodCall {
                object,
                method,
                args,
                named_args,
                ..
            } => {
                if self.bc_is_enum_constructor(expr, method) {
                    for argument in args.iter().chain(named_args.iter().map(|(_, arg)| arg)) {
                        self.bc_reject_view_escape(argument, env, "store in an enum");
                    }
                }
                self.bc_check_contiguous_element_value_read(expr, object);
                let shared_receiver = self.bc_has_explicit_shared_receiver(expr);
                let exclusive_receiver = self.bc_has_explicit_exclusive_receiver(expr);
                let view_receiver = !self.bc_view_origins(object, env).is_empty();
                let view_arguments = args
                    .iter()
                    .chain(named_args.iter().map(|(_, arg)| arg))
                    .any(|argument| !self.bc_view_origins(argument, env).is_empty())
                    || self.bc_is_display_variadic_call(expr);
                let consuming_receiver =
                    self.bc_has_consuming_receiver(expr) && !self.bc_is_dynamic_receiver(object);
                if exclusive_receiver {
                    self.bc_reject_borrowed_write(object, env);
                }
                self.bc_expr(object, env, consuming_receiver);
                if shared_receiver || exclusive_receiver || view_receiver || view_arguments {
                    env.enter_scope();
                    if shared_receiver {
                        self.bc_mark_shared_receiver_loan(object, env, expr.span);
                    } else if exclusive_receiver {
                        self.bc_mark_exclusive_receiver_loan(object, env, expr.span);
                    }
                    self.bc_loan_view_argument(object, env);
                }
                for (index, arg) in args.iter().enumerate() {
                    if self.bc_is_display_variadic_call(expr) {
                        self.bc_call_argument(
                            arg,
                            self.bc_call_argument_effect(expr, index, None),
                            env,
                        );
                    } else {
                        self.bc_expr(arg, env, true);
                        self.bc_loan_view_argument(arg, env);
                    }
                }
                for (_, arg) in named_args {
                    self.bc_expr(arg, env, true);
                    self.bc_loan_view_argument(arg, env);
                }
                if shared_receiver || exclusive_receiver || view_receiver || view_arguments {
                    env.exit_scope();
                }
            }

            ExprKind::Binary { left, op, right } => {
                // Arithmetic / comparison: reads both operands, no move.
                self.bc_expr(left, env, false);
                if matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr) {
                    let mut right_env = env.clone();
                    right_env.control_depth += 1;
                    self.bc_expr(right, &mut right_env, false);
                    env.apply_branch_moves(&right_env);
                } else {
                    self.bc_expr(right, env, false);
                }
            }

            ExprKind::Unary {
                expr: inner,
                op: op @ (UnaryOpKind::Ref | UnaryOpKind::RefMut),
            } => {
                // Evaluate the place before recording the new loan so taking
                // an exclusive reference does not conflict with its own read.
                self.bc_expr(inner, env, false);
                let mut place = inner;
                while let ExprKind::Group(grouped) = &place.node {
                    place = grouped;
                }
                if let ExprKind::Ident(name) = &place.node {
                    let existing = env.lookup(name).cloned();
                    match op {
                        UnaryOpKind::Ref
                            if existing
                                .as_ref()
                                .and_then(|value| value.exclusive_borrow)
                                .is_some() =>
                        {
                            let borrowed_at = existing
                                .and_then(|value| value.exclusive_borrow)
                                .map(|loan| loan.at)
                                .expect("exclusive loan was checked");
                            self.push_error(
                                expr.span,
                                "S10",
                                format!("cannot shared-borrow `{name}` while it is exclusively borrowed (borrowed at {})", self.span_label(borrowed_at)),
                            );
                        }
                        UnaryOpKind::RefMut
                            if existing
                                .as_ref()
                                .and_then(|value| value.shared_borrow)
                                .is_some()
                                || existing
                                    .as_ref()
                                    .and_then(|value| value.exclusive_borrow)
                                    .is_some() =>
                        {
                            let borrowed_at = existing
                                .as_ref()
                                .and_then(|value| value.exclusive_borrow)
                                .map(|loan| loan.at)
                                .or_else(|| {
                                    existing
                                        .as_ref()
                                        .and_then(|value| value.shared_borrow)
                                        .map(|loan| loan.at)
                                })
                                .expect("conflicting loan was checked");
                            self.push_error(
                                expr.span,
                                "S10",
                                format!("cannot exclusively borrow `{name}` while it is already borrowed (borrowed at {})", self.span_label(borrowed_at)),
                            );
                        }
                        UnaryOpKind::Ref => env.mark_shared_borrowed(name, expr.span),
                        UnaryOpKind::RefMut => env.mark_exclusive_borrowed(name, expr.span),
                        _ => unreachable!("borrow expression has a reference operator"),
                    }
                }
            }

            ExprKind::Unary {
                op: UnaryOpKind::Deref,
                expr: inner,
            } => {
                if consumed
                    && self.bc_receiver_is_move_type(expr)
                    && self.bc_is_safe_reference_expr(inner)
                    && !env.allows_destructor_projection_move(expr)
                {
                    self.bc_reject_partial_move(expr);
                }
                self.bc_expr(inner, env, false);
            }

            ExprKind::Unary { expr: inner, .. } => {
                self.bc_expr(inner, env, false);
            }

            ExprKind::Cast { expr: inner, .. } => {
                self.bc_expr(inner, env, consumed);
            }

            ExprKind::Group(inner) => {
                // Transparent wrapper: propagate consumed flag.
                self.bc_expr(inner, env, consumed);
            }

            ExprKind::Field { object, .. } => {
                if consumed
                    && self.bc_receiver_is_move_type(expr)
                    && !env.allows_destructor_projection_move(expr)
                    && !env.allows_returned_receiver_field_move(expr)
                {
                    self.bc_reject_partial_move(expr);
                }
                self.bc_expr(object, env, false);
            }

            ExprKind::Index { object, indices } => {
                self.bc_check_contiguous_element_value_read(expr, object);
                if consumed
                    && self.bc_receiver_is_move_type(expr)
                    && self.bc_is_builtin_index_projection(object)
                    && !env.allows_destructor_projection_move(expr)
                {
                    self.bc_reject_partial_move(expr);
                }
                self.bc_expr(object, env, false);
                for idx in indices {
                    self.bc_expr(idx, env, false);
                }
            }

            ExprKind::ArrayLit(elems) => {
                // Array construction consumes each element.
                for elem in elems {
                    self.bc_reject_view_escape(elem, env, "store in an array");
                    self.bc_expr(elem, env, true);
                }
            }

            ExprKind::Match { scrutinee, arms } => {
                // Scrutinee is consumed (matched/destructured).
                self.bc_expr(scrutinee, env, true);
                // Each arm body is checked in an independent clone; then merge.
                let base_env = env.clone();
                for arm in arms {
                    let mut arm_env = base_env.clone();
                    arm_env.control_depth += 1;
                    for b in crate::parser::ast::pattern_all_bindings(&arm.pattern) {
                        arm_env.declare(b, Some(TypeKind::Error));
                    }
                    // Guard expression is checked in the arm's scope (bindings available).
                    if let Some(guard) = &arm.guard {
                        self.bc_expr(guard, &mut arm_env, false);
                    }
                    self.bc_expr(&arm.expr, &mut arm_env, consumed);
                    env.apply_branch_moves(&arm_env);
                }
            }

            ExprKind::Literal(_) => {}

            ExprKind::StructInit { fields, .. } => {
                // Struct construction consumes each field value.
                for (_, fval) in fields {
                    self.bc_reject_view_escape(fval, env, "store in an aggregate");
                    self.bc_expr(fval, env, true);
                }
            }

            ExprKind::Try { expr: inner } => {
                self.bc_expr(inner, env, consumed);
            }

            ExprKind::Closure { params, body } => {
                let parameter_types = match self.bc_annotated_type(expr) {
                    Some(TypeKind::Fn { params, .. }) => params,
                    _ => Vec::new(),
                };
                let mut closure_env = env.clone();
                closure_env.capture_boundary = Some(closure_env.scopes.len());
                closure_env.enter_scope();
                for (index, name) in params.iter().enumerate() {
                    closure_env.declare(
                        name.clone(),
                        parameter_types.get(index).map(|ty| ty.node.clone()),
                    );
                }
                self.bc_reject_view_escape(body, &closure_env, "return from a closure");
                self.bc_expr(body, &mut closure_env, true);
                closure_env.exit_scope();
            }
        }
    }

    fn bc_annotated_type(&self, expr: &Expr) -> Option<TypeKind> {
        self.annotated_exprs
            .iter()
            .rev()
            .find(|annotation| {
                annotation.span.start == expr.span.start && annotation.span.end == expr.span.end
            })
            .and_then(|annotation| annotation.ty.clone())
    }

    fn bc_is_enum_constructor(&self, expr: &Expr, variant: &str) -> bool {
        if self.bc_resolved_call_name(expr).is_some_and(|name| {
            self.resolve_symbol(name)
                .is_some_and(|symbol| !matches!(symbol.ty, Some(TypeKind::Error)))
        }) {
            return false;
        }
        let Some(TypeKind::Named { name, .. }) = self
            .bc_annotated_type(expr)
            .map(|ty| self.resolve_type_aliases(&ty))
        else {
            return false;
        };
        self.enums
            .get(&name)
            .is_some_and(|info| info.variants.contains_key(variant))
    }

    fn bc_resolved_direct_call(&self, expr: &Expr) -> bool {
        self.bc_resolved_call_name(expr).is_some_and(|name| {
            self.ownership_signature_index
                .get(name)
                .is_some_and(|summary| summary.direct_call_eligible)
        })
    }

    fn bc_call_argument(&mut self, argument: &Expr, effect: CallArgumentEffect, env: &mut MoveEnv) {
        match effect {
            CallArgumentEffect::Consume => self.bc_expr(argument, env, true),
            // Explicit `&value` / `&value!` expressions already create the
            // call-scoped loan while they are evaluated. Existing reference
            // capabilities are reborrowed for this call, never moved or made
            // into a second independent loan here.
            CallArgumentEffect::SharedBorrow | CallArgumentEffect::ExclusiveBorrow => {
                self.bc_expr(argument, env, false);
                if effect == CallArgumentEffect::SharedBorrow
                    && self.bc_annotated_type(argument).is_some_and(|ty| {
                        matches!(self.resolve_type_aliases(&ty), TypeKind::Named { name, .. } if name == "String")
                    })
                {
                    self.bc_mark_shared_receiver_loan(argument, env, argument.span);
                }
            }
        }
        self.bc_loan_view_argument(argument, env);
    }

    fn bc_is_display_variadic_call(&self, call: &Expr) -> bool {
        self.bc_resolved_call_name(call)
            .and_then(|name| self.resolve_symbol(name))
            .is_some_and(|symbol| {
                symbol.variadic
                    && symbol
                        .attributes
                        .iter()
                        .any(|attribute| attribute == "str_variadic")
            })
    }

    fn bc_call_argument_effect(
        &self,
        call: &Expr,
        positional_index: usize,
        named_argument: Option<&str>,
    ) -> CallArgumentEffect {
        let Some(resolved) = self.bc_resolved_call_name(call) else {
            return CallArgumentEffect::Consume;
        };
        if let Some(symbol) = self.resolve_symbol(resolved)
            && symbol.variadic
            && symbol
                .attributes
                .iter()
                .any(|attribute| attribute == "str_variadic")
            && named_argument.is_none()
            && positional_index
                + symbol.params.len().saturating_sub(
                    self.fn_param_names
                        .get(resolved)
                        .map_or(symbol.params.len(), Vec::len),
                )
                >= symbol.params.len().saturating_sub(1)
        {
            return CallArgumentEffect::SharedBorrow;
        }
        let Some(summary) = self.ownership_signature_index.get(resolved) else {
            return CallArgumentEffect::Consume;
        };
        if !summary.direct_call_eligible {
            return CallArgumentEffect::Consume;
        }
        let parameter_names = self.fn_param_names.get(resolved);
        let parameter_index = match named_argument {
            Some(name) => parameter_names
                .and_then(|names| names.iter().position(|candidate| candidate == name)),
            None => Some(positional_index),
        };
        let Some(parameter_index) = parameter_index else {
            return CallArgumentEffect::Consume;
        };
        match summary.parameters.get(parameter_index) {
            Some(OwnershipCapability::SharedBorrow) => CallArgumentEffect::SharedBorrow,
            Some(OwnershipCapability::ExclusiveBorrow) => CallArgumentEffect::ExclusiveBorrow,
            _ => CallArgumentEffect::Consume,
        }
    }

    fn bc_resolved_call_name(&self, expr: &Expr) -> Option<&str> {
        self.annotated_exprs
            .iter()
            .rev()
            .find(|annotation| {
                annotation.span.start == expr.span.start && annotation.span.end == expr.span.end
            })
            .and_then(|annotation| annotation.resolved_fn.as_deref())
    }

    fn bc_has_explicit_shared_receiver(&self, expr: &Expr) -> bool {
        let resolved = self
            .annotated_exprs
            .iter()
            .rev()
            .find(|annotation| {
                annotation.span.start == expr.span.start && annotation.span.end == expr.span.end
            })
            .and_then(|annotation| annotation.resolved_fn.as_deref());
        resolved.is_some_and(|name| self.explicit_shared_receiver_methods.contains(name))
    }

    fn bc_has_explicit_exclusive_receiver(&self, expr: &Expr) -> bool {
        let resolved = self
            .annotated_exprs
            .iter()
            .rev()
            .find(|annotation| {
                annotation.span.start == expr.span.start && annotation.span.end == expr.span.end
            })
            .and_then(|annotation| annotation.resolved_fn.as_deref());
        resolved.is_some_and(|name| self.explicit_exclusive_receiver_methods.contains(name))
    }

    fn bc_has_consuming_receiver(&self, expr: &Expr) -> bool {
        let resolved = self
            .annotated_exprs
            .iter()
            .rev()
            .find(|annotation| {
                annotation.span.start == expr.span.start && annotation.span.end == expr.span.end
            })
            .and_then(|annotation| annotation.resolved_fn.as_deref());
        resolved.is_some_and(|name| self.consuming_receiver_methods.contains(name))
    }

    /// A contiguous-container accessor that returns its owned element by
    /// value currently lowers to a raw load, which would duplicate the
    /// element's owner. The rule is derived from the storage declaration and
    /// resolved types, rather than a container or accessor spelling. Plain
    /// elements remain readable until D-015 supplies provenance-tracked
    /// borrowed element references.
    fn bc_contiguous_element_value_type(&self, expr: &Expr, object: &Expr) -> Option<TypeKind> {
        // Index lowering has no ownership-transfer proof, even for an
        // exclusive implementation. A separate exclusive remove method may
        // transfer an element through its validated mutation contract.
        if !matches!(expr.node, ExprKind::Index { .. })
            && !self.bc_has_explicit_shared_receiver(expr)
        {
            return None;
        }
        let Some(receiver) = self.bc_annotated_type(object) else {
            return None;
        };
        let receiver = self.resolve_type_aliases(&receiver);
        let mut owner = &receiver;
        while let TypeKind::Ref { inner } | TypeKind::MutRef { inner } = owner {
            owner = &inner.node;
        }
        let TypeKind::Named { name, type_args } = owner else {
            return None;
        };
        let Some(contract) = self.contiguous_element_contracts.get(name) else {
            return None;
        };
        let Some(element) = type_args.get(contract.element_param_index) else {
            return None;
        };
        let element = self.resolve_type_aliases(&element.node);
        let result = if matches!(expr.node, ExprKind::Index { .. }) {
            // This is a rejection proof, not permission to lower borrowed
            // Index receivers. Their executable ABI still needs D-014 work.
            // Derive the result from the declaration, because indexing through
            // an alias/reference may have an unresolved expression annotation.
            if !self
                .trait_impls
                .get(name)
                .is_some_and(|traits| traits.contains("Index"))
            {
                return None;
            }
            let target = self
                .bc_resolved_call_name(expr)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{name}.index"));
            let Some(symbol) = self.resolve_symbol(&target) else {
                return None;
            };
            let Some(mut receiver) = symbol.params.first() else {
                return None;
            };
            while let TypeKind::Ref { inner } | TypeKind::MutRef { inner } = receiver {
                receiver = &inner.node;
            }
            let parameters = self
                .impl_generic_params
                .get(&target)
                .cloned()
                .unwrap_or_default();
            let mut substitution = HashMap::new();
            super::typecheck::infer_type_subst(receiver, owner, &parameters, &mut substitution);
            symbol
                .ty
                .as_ref()
                .map(|result| super::typecheck::substitute_type_kind(result, &substitution))
        } else {
            self.bc_annotated_type(expr)
        };
        result
            .as_ref()
            .filter(|result| self.resolve_type_aliases(result).to_string() == element.to_string())
            .map(|_| element)
    }

    fn bc_check_contiguous_element_value_read(&mut self, expr: &Expr, object: &Expr) {
        let Some(element) = self.bc_contiguous_element_value_type(expr, object) else {
            return;
        };
        if self.bc_is_move_type(&element) {
            self.push_error(expr.span, "S10", "cannot read an owned contiguous-container element by value before borrowed-element access is implemented".to_string());
        } else if self
            .current_generic_params
            .last()
            .is_some_and(|parameters| !parameters.is_empty())
            && let Some(callable) = self.current_function.last()
        {
            // A template parameter is not known to be Copy. Recheck the
            // source read after its proven concrete specializations close.
            self.pending_contiguous_value_reads
                .push((callable.clone(), expr.span, element));
        }
    }

    pub(super) fn validate_contiguous_value_read_specializations(
        &mut self,
        concrete: &BTreeSet<OwnershipCallableId>,
        checkpoint: &mut impl FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<(), crate::cancel::Cancelled> {
        let mut reads: HashMap<String, Vec<(Span, TypeKind)>> = HashMap::new();
        for (callable, span, element) in &self.pending_contiguous_value_reads {
            checkpoint()?;
            reads
                .entry(callable.clone())
                .or_default()
                .push((*span, element.clone()));
        }
        let mut rejected = HashSet::new();
        let mut diagnostics = Vec::new();
        for mono in &self.monomorphizations {
            checkpoint()?;
            if !concrete.contains(&self.ownership_callable_id(&mono.fn_name, &mono.type_args)) {
                continue;
            }
            let Some(sites) = reads.get(&mono.fn_name) else {
                continue;
            };
            let parameters = self.generic_params_for_function(&mono.fn_name);
            let substitution: HashMap<_, _> = parameters
                .into_iter()
                .zip(mono.type_args.iter().cloned())
                .collect();
            for (span, element) in sites {
                checkpoint()?;
                let resolved = crate::type_resolution::resolve_type(
                    element,
                    &self.type_aliases,
                    &substitution,
                    &HashSet::new(),
                    |_| None,
                );
                match resolved {
                    Ok(element)
                        if self.bc_is_move_type(&element)
                            && rejected.insert((span.start, span.end)) =>
                    {
                        diagnostics.push(SemanticError {
                            span: *span, code: "S10",
                            message: format!("specialization `{}` cannot read an owned contiguous-container element by value before borrowed-element access is implemented", mono.mangled_name),
                        });
                    }
                    Err(error) => diagnostics.push(SemanticError {
                        span: *span,
                        code: "S14",
                        message: error.to_string(),
                    }),
                    _ => {}
                }
            }
        }
        self.errors.extend(diagnostics);
        Ok(())
    }

    /// Implicit scope cleanup is an ownership use just like an explicit
    /// `value.free()`. Record every reachable contiguous container release
    /// hook now so tree shaking and monomorphization retain the chunks that
    /// generated structural cleanup must call.
    fn bc_record_contiguous_destructor_root(&mut self, ty: &TypeKind) {
        let Some(owner) = self.current_function.last().cloned() else {
            return;
        };
        self.bc_record_contiguous_destructor_roots(ty, &owner, &mut HashSet::new());
    }

    fn bc_record_contiguous_destructor_roots(
        &mut self,
        ty: &TypeKind,
        owner: &str,
        visiting: &mut HashSet<String>,
    ) {
        let TypeKind::Named { name, type_args } = self.resolve_type_aliases(ty) else {
            return;
        };
        let visit_key = format!(
            "{name}[{}]",
            type_args
                .iter()
                .map(|arg| arg.node.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        if !visiting.insert(visit_key) {
            return;
        }
        if !type_args.is_empty()
            && let Some(contract) = self.contiguous_element_contracts.get(&name)
        {
            let release = format!("{name}.free");
            let type_args = type_args
                .iter()
                .map(|argument| argument.node.clone())
                .collect::<Vec<_>>();
            let element_type = type_args.get(contract.element_param_index).cloned();
            let mangled_name =
                crate::semantic::typecheck::mangle_monomorphized(&release, &type_args);
            let generic_params = self.generic_params_for_function(&release);
            self.record_specialization_layout(&release, &generic_params, &type_args);
            if !self.has_monomorphization(&release, &type_args, &mangled_name) {
                self.monomorphizations.push(MonomorphizationInfo {
                    fn_name: release,
                    type_args,
                    mangled_name: mangled_name.clone(),
                });
            }
            self.add_dependency_edge(DependencyKind::Call, owner, &mangled_name);
            // Generated destruction recursively lowers the concrete element
            // action. Keep the element's release specialization reachable too;
            // otherwise Buffer[Buffer[Token]] can reach codegen without the
            // Buffer.free<Token> chunk it must invoke.
            if let Some(element) = element_type.as_ref() {
                self.bc_record_contiguous_destructor_roots(element, owner, visiting);
            }
            return;
        }
        if self
            .consuming_receiver_methods
            .contains(&format!("{name}.free"))
            || self.repr_c_structs.contains(&name)
        {
            return;
        }
        let Some(fields) = self.struct_defs.get(&name).cloned() else {
            return;
        };
        let params = self
            .struct_generic_params
            .get(&name)
            .cloned()
            .unwrap_or_default();
        let substitutions = params
            .into_iter()
            .zip(type_args.into_iter().map(|argument| argument.node))
            .collect::<HashMap<_, _>>();
        for (_, field_ty) in fields {
            let field_ty =
                crate::semantic::typecheck::substitute_type_kind(&field_ty, &substitutions);
            self.bc_record_contiguous_destructor_roots(&field_ty, owner, visiting);
        }
    }

    fn bc_mark_shared_receiver_loan(&mut self, object: &Expr, env: &mut MoveEnv, at: Span) {
        if let Some(name) = assignment_root_ident(object) {
            env.mark_shared_borrowed(name, at);
        }
    }

    fn bc_receiver_is_move_type(&self, expr: &Expr) -> bool {
        self.bc_annotated_type(expr)
            .as_ref()
            .is_some_and(|ty| self.bc_is_move_type(&self.resolve_type_aliases(ty)))
    }

    fn bc_is_dynamic_receiver(&self, expr: &Expr) -> bool {
        self.bc_annotated_type(expr)
            .as_ref()
            .is_some_and(|ty| matches!(self.resolve_type_aliases(ty), TypeKind::Dyn { .. }))
    }

    fn bc_is_safe_reference_expr(&self, expr: &Expr) -> bool {
        self.bc_annotated_type(expr).as_ref().is_some_and(|ty| {
            matches!(
                self.resolve_type_aliases(ty),
                TypeKind::Ref { .. } | TypeKind::MutRef { .. }
            )
        })
    }

    fn bc_is_builtin_index_projection(&self, object: &Expr) -> bool {
        self.bc_annotated_type(object).as_ref().is_some_and(|ty| {
            matches!(
                self.resolve_type_aliases(ty),
                TypeKind::Array { .. }
                    | TypeKind::Slice { .. }
                    | TypeKind::FlexibleArray { .. }
                    | TypeKind::Bytes
            )
        })
    }

    fn bc_reject_partial_move(&mut self, expr: &Expr) {
        self.push_error(expr.span, "S10", "cannot move out of a field, indexed element, or safe dereference before place-level moves and structural destruction are implemented".to_string());
    }

    /// The first place-level move exception is deliberately confined to a
    /// terminal `return self.field` from a by-value inherent receiver.  The
    /// code generator independently requires a structural drop plan with an
    /// action for this exact field, and filters that action for this return
    /// only.  Keeping this predicate syntactic avoids accidentally admitting
    /// aliases, nested projections, casts, or ordinary call arguments.
    fn bc_can_return_receiver_field(&self, expr: &Expr, env: &MoveEnv) -> bool {
        let Some(receiver) = env.consuming_receiver.as_deref() else {
            return false;
        };
        let ExprKind::Field {
            object,
            name: field,
        } = &expr.node
        else {
            return false;
        };
        let ExprKind::Ident(object_name) = &object.node else {
            return false;
        };
        if object_name != receiver {
            return false;
        }
        let Some(TypeKind::Named { name, type_args }) = env
            .lookup(receiver)
            .and_then(|variable| variable.ty.as_ref())
            .map(|ty| self.resolve_type_aliases(ty))
        else {
            return false;
        };
        if !type_args.is_empty()
            || self.repr_c_structs.contains(&name)
            || self
                .consuming_receiver_methods
                .contains(&format!("{name}.free"))
        {
            return false;
        }
        let Some((_, field_ty)) = self
            .struct_defs
            .get(&name)
            .and_then(|fields| fields.iter().find(|(candidate, _)| candidate == field))
        else {
            return false;
        };
        let TypeKind::Named {
            name: field_name,
            type_args: field_type_args,
        } = self.resolve_type_aliases(field_ty)
        else {
            return false;
        };
        field_type_args.is_empty()
            && self.bc_structural_drop_action_for_named(&name, &mut HashSet::new())
            && (self
                .consuming_receiver_methods
                .contains(&format!("{field_name}.free"))
                || self.bc_structural_drop_action_for_named(&field_name, &mut HashSet::new()))
    }

    /// Mirrors the deliberately small structural-drop domain in codegen.  This
    /// makes the borrow-check exception conditional on an actual field action,
    /// rather than merely on a field having a named type.
    fn bc_structural_drop_action_for_named(
        &self,
        name: &str,
        visiting: &mut HashSet<String>,
    ) -> bool {
        if self.repr_c_structs.contains(name) || !visiting.insert(name.to_string()) {
            return false;
        }
        let Some(fields) = self.struct_defs.get(name) else {
            visiting.remove(name);
            return false;
        };
        let mut has_action = false;
        for (_, field_ty) in fields {
            let TypeKind::Named {
                name: field_name,
                type_args,
            } = self.resolve_type_aliases(field_ty)
            else {
                if matches!(
                    self.resolve_type_aliases(field_ty),
                    TypeKind::Fn { .. }
                        | TypeKind::Dyn { .. }
                        | TypeKind::Array { .. }
                        | TypeKind::Slice { .. }
                        | TypeKind::FlexibleArray { .. }
                ) {
                    visiting.remove(name);
                    return false;
                }
                continue;
            };
            if !type_args.is_empty() {
                visiting.remove(name);
                return false;
            }
            let drop_hook = format!("{field_name}.free");
            if self.consuming_receiver_methods.contains(&drop_hook) {
                has_action = true;
            } else if self.bc_structural_drop_action_for_named(&field_name, visiting) {
                has_action = true;
            } else {
                visiting.remove(name);
                return false;
            }
        }
        visiting.remove(name);
        has_action
    }

    fn bc_mark_exclusive_receiver_loan(&mut self, object: &Expr, env: &mut MoveEnv, at: Span) {
        if let Some(name) = assignment_root_ident(object) {
            env.mark_exclusive_borrowed(name, at);
        }
    }

    fn bc_reject_borrowed_write(&mut self, target: &Expr, env: &MoveEnv) {
        let Some(name) = projection_root_ident(target) else {
            return;
        };
        if let Some(variable) = env.lookup(name) {
            for origin in &variable.view_origins {
                if let ViewOrigin::Owner {
                    name: owner,
                    scope_depth,
                } = origin
                    && env
                        .scopes
                        .get(*scope_depth)
                        .and_then(|scope| scope.get(owner))
                        .is_some_and(|root| root.shared_borrow.is_some())
                {
                    self.push_error(target.span, "S10", format!(
                        "cannot mutate through `{name}` while a string view borrows owner `{owner}`"));
                }
            }
        }
        if let Some(borrowed_at) = env
            .lookup(name)
            .and_then(|variable| variable.shared_borrow)
            .map(|loan| loan.at)
        {
            self.push_error(
                target.span,
                "S10",
                format!(
                    "cannot mutate `{name}` while it is shared-borrowed (borrowed at {})",
                    self.span_label(borrowed_at)
                ),
            );
        }
        if let Some(borrowed_at) = env
            .lookup(name)
            .and_then(|variable| variable.exclusive_borrow)
            .map(|loan| loan.at)
        {
            self.push_error(
                target.span,
                "S10",
                format!(
                    "cannot mutate `{name}` while it is exclusively borrowed (borrowed at {})",
                    self.span_label(borrowed_at)
                ),
            );
        }
    }
}

fn assignment_root_ident(expr: &Expr) -> Option<&str> {
    match &expr.node {
        ExprKind::Ident(name) => Some(name),
        ExprKind::Group(inner) => assignment_root_ident(inner),
        ExprKind::Field { object, .. } | ExprKind::Index { object, .. } => {
            assignment_root_ident(object)
        }
        _ => None,
    }
}

#[cfg(test)]
mod string_view_tests {
    use super::*;
    use crate::{lexer::Lexer, parser::Parser};

    const OWNERS: &str = r#"
struct Owner { text: str }
impl Owner {
    fn view(self: &Owner) str { ret self.text; }
    fn edit(self: &Owner!) void { self.text = "changed"; }
    fn free(self: Owner) void {}
}
fn make() Owner { ret Owner { text: "held" }; }
fn sink(text: str) void {}
fn forward(text: str) str { ret text; }
fn borrowed(owner: &Owner) str { ret owner.view(); }
"#;

    fn analyze(body: &str) -> SemanticReport {
        let source = format!("{OWNERS}\n{body}");
        let program = Parser::new(Lexer::new(&source).tokenize()).parse().unwrap();
        Analyzer::new().analyze_program(&program)
    }

    fn rejects_view(body: &str) {
        let report = analyze(body);
        assert!(
            report.errors.iter().any(|error| error.code == "S10"
                && (error.message.contains("string view")
                    || error.message.contains("shared-borrow"))),
            "missing ownership diagnostic for {body}: {:?}",
            report.errors
        );
    }

    #[test]
    fn temporary_views_cannot_be_bound_returned_or_forwarded_into_storage() {
        for body in [
            "fn main() void { var view = make().view(); }",
            "fn leak() str { ret make().view(); } fn main() void {}",
            "fn main() void { const view: str = forward(make().view()); }",
            "fn main() void { var view: str = \"\"; view = make().view(); }",
        ] {
            rejects_view(body);
        }
    }

    #[test]
    fn full_expression_temporary_views_remain_usable() {
        let report = analyze(
            "fn main() void { sink(make().view()); sink(forward(make().view())); var length = make().view().len(); }",
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }

    #[test]
    fn local_views_freeze_owner_moves_and_mutation() {
        for action in [
            "owner.free();",
            "owner.edit();",
            "owner.text = \"changed\";",
        ] {
            rejects_view(&format!(
                "fn main() void {{ var owner = make(); var view = owner.view(); {action} sink(view); }}"
            ));
        }
        rejects_view(
            "type Alias = Owner; fn main() void { var owner: Alias = make(); var view = owner.view(); owner.free(); sink(view); }",
        );
    }

    #[test]
    fn view_aliases_and_reference_aliases_retain_the_owner_root() {
        rejects_view(
            "fn main() void { var owner = make(); var view = owner.view(); var alias: str = (view as str); owner.free(); sink(alias); }",
        );
        rejects_view(
            "fn leak() str { var owner = make(); var reference = &owner; ret reference.view(); } fn main() void {}",
        );
        rejects_view(
            "fn main() void { var owner = make(); var reference = &owner!; var view = reference.view(); reference.edit(); sink(view); }",
        );
    }

    #[test]
    fn reference_mutation_without_a_derived_view_is_legal() {
        let report = analyze(
            "fn main() void { var owner = make(); var reference = &owner!; reference.edit(); }",
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }

    #[test]
    fn by_value_parameters_and_owned_local_fields_cannot_return_views() {
        rejects_view("fn leak(owner: Owner) str { ret owner.view(); } fn main() void {}");
        rejects_view("fn leak() str { var owner = make(); ret owner.text; } fn main() void {}");
    }

    #[test]
    fn borrowed_inputs_remain_external_and_caller_roots_are_propagated() {
        let report = analyze("fn main() void { sink(borrowed(&make())); }");
        // Existing address-of checking independently rejects references to
        // temporaries; direct method borrowing is the supported expression form.
        assert!(!report.errors.is_empty());
        rejects_view(
            "fn main() void { var owner = make(); var view = borrowed(&owner); owner.free(); sink(view); }",
        );
        rejects_view(
            "fn main() void { var owner = make(); var view = owner.view(); var alias = forward(&view); owner.free(); sink(alias); }",
        );
        let report = analyze(
            "fn passthrough(text: str) str { ret forward(text); } fn main() void { sink(passthrough(\"literal\")); }",
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }

    #[test]
    fn a_view_cannot_outlive_an_inner_owner_when_assigned_outward() {
        rejects_view(
            "fn main() void { var view: str = \"\"; if (true) { var owner = make(); view = owner.view(); } sink(view); }",
        );
    }

    #[test]
    fn inner_view_scope_releases_its_shared_loan() {
        let report = analyze(
            "fn main() void { var owner = make(); if (true) { var view = owner.view(); sink(view); } owner.free(); }",
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }

    #[test]
    fn owner_rooted_views_cannot_escape_into_aggregates_arrays_or_closures() {
        for body in [
            "struct Holder { text: str } fn main() void { var owner = make(); var holder = Holder { text: owner.view() }; }",
            "fn main() void { var owner = make(); var array = [owner.view()]; }",
            "fn main() void { var owner = make(); var value = Some(owner.view()); }",
            "fn main() void { var owner = make(); var view = owner.view(); var closure: fn() str = || view; }",
            "fn main() void { var closure: fn() str = || make().view(); }",
            "struct Holder { text: str } fn main() void { var holder = Holder { text: \"\" }; holder.text = make().view(); }",
        ] {
            rejects_view(body);
        }
    }

    #[test]
    fn direct_call_arguments_hold_views_until_sibling_arguments_finish() {
        rejects_view(
            "fn release(owner: Owner) i32 { owner.free(); ret 0; } fn pair(text: str, ignored: i32) void {} fn main() void { var owner = make(); pair(owner.view(), release(owner)); }",
        );
    }

    #[test]
    fn shadowed_bindings_preserve_the_original_owner_identity() {
        rejects_view(
            "fn leak() str { var owner = make(); if (true) { var owner = owner.view(); ret owner; } ret \"\"; } fn main() void {}",
        );
    }

    #[test]
    fn destructor_field_arguments_do_not_freeze_the_receiver_after_the_call() {
        let report = analyze(
            r#"
@intrinsic("quazi.free") fn release_data(data: str) void;
struct Buffer { data: str }
impl Buffer { fn free(self: Buffer) void { release_data(self.data); self.data = ""; } }
fn main() void {}
"#,
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }

    #[test]
    fn raw_allocated_views_can_still_construct_a_new_owned_result() {
        let report = analyze(
            r#"
@intrinsic("quazi.str_concat") fn allocate(first: str, second: str) str;
fn copy(text: str) Owner { const data = allocate(text, ""); ret Owner { text: data }; }
fn main() void { var copied = copy("literal"); copied.free(); }
"#,
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }
}
