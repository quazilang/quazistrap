// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::collections::HashMap;

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
    /// First address-of operation that keeps this stack slot borrowed for the
    /// remainder of the conservative function-local analysis.
    borrowed_at: Option<Span>,
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
}

impl MoveEnv {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            loop_depth: 0,
            control_depth: 0,
            reassign_targets: std::collections::HashSet::new(),
        }
    }

    fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn exit_scope(&mut self) {
        self.scopes.pop();
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
                    borrowed_at: None,
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

    fn mark_moved(&mut self, name: &str, at: Span) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(v) = scope.get_mut(name) {
                v.moved_at = Some(at);
                return;
            }
        }
    }

    fn mark_borrowed(&mut self, name: &str, at: Span) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(variable) = scope.get_mut(name) {
                variable.borrowed_at.get_or_insert(at);
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
                if let Some(at) = var.borrowed_at {
                    self.mark_borrowed(name, at);
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
                    params,
                    body: Some(body),
                    ..
                } => {
                    let mut env = MoveEnv::new();
                    for p in params {
                        env.declare(p.name.clone(), Some(p.ty.node.clone()));
                    }
                    self.bc_block(body, &mut env);
                }
                ItemKind::Impl { methods, .. } => {
                    for m in methods {
                        if let ItemKind::Fn {
                            params,
                            body: Some(body),
                            ..
                        } = &m.node
                        {
                            let mut env = MoveEnv::new();
                            for p in params {
                                env.declare(p.name.clone(), Some(p.ty.node.clone()));
                            }
                            self.bc_block(body, &mut env);
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
                env.declare(name.clone(), var_ty);
            }
            StmtKind::Const {
                name, ty, value, ..
            } => {
                let var_ty = ty
                    .as_ref()
                    .map(|t| t.node.clone())
                    .or_else(|| self.bc_annotated_type(value));
                self.bc_expr(value, env, true);
                env.declare(name.clone(), var_ty);
            }
            StmtKind::Return(Some(expr)) => {
                // Return exits the function; moves here don't affect post-return code.
                // Use a cloned env with loop_depth=0 to suppress the "move in loop" error
                // (the loop never runs again after a return) and don't apply back.
                let mut ret_env = env.clone();
                ret_env.loop_depth = 0;
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
    fn bc_is_move_type(&self, ty: &TypeKind) -> bool {
        match ty {
            // Primitives and references are Copy — no move tracking needed.
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
            | TypeKind::Float32
            | TypeKind::Float64
            | TypeKind::Str
            | TypeKind::Bytes
            | TypeKind::RawPtr { .. }
            | TypeKind::CFn { .. }
            | TypeKind::Ref { .. } => false,
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
                    if let Some(borrowed_at) = var.borrowed_at {
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
                } else {
                    self.bc_expr(target, env, false);
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
                for arg in args {
                    self.bc_expr(arg, env, true);
                }
                for (_, arg) in named_args {
                    self.bc_expr(arg, env, true);
                }
            }

            ExprKind::MethodCall {
                object,
                args,
                named_args,
                ..
            } => {
                // Quazi does not yet distinguish shared and mutable method
                // receivers. Conservatively treat every method call as capable
                // of mutation while an outstanding shared borrow exists.
                self.bc_reject_borrowed_write(object, env);
                self.bc_expr(object, env, false);
                for arg in args {
                    self.bc_expr(arg, env, true);
                }
                for (_, arg) in named_args {
                    self.bc_expr(arg, env, true);
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
                op: UnaryOpKind::Ref,
            } => {
                let mut place = inner;
                while let ExprKind::Group(grouped) = &place.node {
                    place = grouped;
                }
                if let ExprKind::Ident(name) = &place.node {
                    env.mark_borrowed(name, expr.span);
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
                // Field access borrows the object — no whole-struct move.
                // (Partial moves not tracked until reference types are added.)
                self.bc_expr(object, env, false);
            }

            ExprKind::Index { object, indices } => {
                self.bc_expr(object, env, false);
                for idx in indices {
                    self.bc_expr(idx, env, false);
                }
            }

            ExprKind::ArrayLit(elems) => {
                // Array construction consumes each element.
                for elem in elems {
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
                closure_env.enter_scope();
                for (index, name) in params.iter().enumerate() {
                    closure_env.declare(
                        name.clone(),
                        parameter_types.get(index).map(|ty| ty.node.clone()),
                    );
                }
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

    fn bc_reject_borrowed_write(&mut self, target: &Expr, env: &MoveEnv) {
        let Some(name) = assignment_root_ident(target) else {
            return;
        };
        if let Some(borrowed_at) = env.lookup(name).and_then(|variable| variable.borrowed_at) {
            self.push_error(
                target.span,
                "S10",
                format!(
                    "cannot mutate `{name}` while it is shared-borrowed (borrowed at {})",
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
