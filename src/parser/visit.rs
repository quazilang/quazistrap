// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Exhaustive mutable traversal of the syntax tree.
//!
//! Passes that derive a new program from parsed syntax (target views, span
//! relocation) override the hooks they need; the `walk_*` functions visit
//! every child, so a new AST field only has to be added here once.

use super::ast::*;

pub trait MutVisitor {
    fn visit_span(&mut self, _span: &mut Span) {}

    fn visit_item(&mut self, item: &mut Item) {
        walk_item(self, item);
    }

    fn visit_block(&mut self, block: &mut Block) {
        walk_block(self, block);
    }

    fn visit_stmt(&mut self, stmt: &mut Stmt) {
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &mut Expr) {
        walk_expr(self, expr);
    }

    fn visit_type(&mut self, ty: &mut Type) {
        walk_type(self, ty);
    }

    fn visit_pattern(&mut self, pattern: &mut Pattern) {
        walk_pattern(self, pattern);
    }

    fn visit_attribute(&mut self, attribute: &mut Attribute) {
        self.visit_span(&mut attribute.span);
    }
}

fn visit_attributes<V: MutVisitor + ?Sized>(visitor: &mut V, attributes: &mut [Attribute]) {
    for attribute in attributes {
        visitor.visit_attribute(attribute);
    }
}

fn visit_params<V: MutVisitor + ?Sized>(visitor: &mut V, params: &mut [Param]) {
    for param in params {
        visitor.visit_span(&mut param.name_span);
        visitor.visit_type(&mut param.ty);
        visit_attributes(visitor, &mut param.attributes);
    }
}

pub fn walk_item<V: MutVisitor + ?Sized>(visitor: &mut V, item: &mut Item) {
    visitor.visit_span(&mut item.span);
    match &mut item.node {
        ItemKind::Fn {
            name_span,
            params,
            return_ty,
            body,
            attributes,
            ..
        } => {
            if let Some(span) = name_span {
                visitor.visit_span(span);
            }
            visit_params(visitor, params);
            visitor.visit_type(return_ty);
            if let Some(body) = body {
                visitor.visit_block(body);
            }
            visit_attributes(visitor, attributes);
        }
        ItemKind::Struct {
            fields, attributes, ..
        } => {
            for field in fields {
                visitor.visit_type(&mut field.ty);
                visit_attributes(visitor, &mut field.attributes);
            }
            visit_attributes(visitor, attributes);
        }
        ItemKind::Trait {
            methods,
            attributes,
            ..
        } => {
            for method in methods {
                for param in &mut method.params {
                    visitor.visit_type(param);
                }
                visitor.visit_type(&mut method.return_ty);
                visit_attributes(visitor, &mut method.attributes);
                visitor.visit_span(&mut method.span);
            }
            visit_attributes(visitor, attributes);
        }
        ItemKind::Enum {
            variants,
            attributes,
            ..
        } => {
            for variant in variants {
                for payload in &mut variant.payload_types {
                    visitor.visit_type(payload);
                }
                visit_attributes(visitor, &mut variant.attributes);
                visitor.visit_span(&mut variant.span);
            }
            visit_attributes(visitor, attributes);
        }
        ItemKind::Impl {
            trait_ty,
            for_ty,
            methods,
        } => {
            if let Some(trait_ty) = trait_ty {
                visitor.visit_type(trait_ty);
            }
            visitor.visit_type(for_ty);
            for method in methods {
                visitor.visit_item(method);
            }
        }
        ItemKind::Import(import) => {
            for span in &mut import.path_spans {
                visitor.visit_span(span);
            }
            for span in &mut import.selector_spans {
                visitor.visit_span(span);
            }
            if let Some(span) = &mut import.alias_span {
                visitor.visit_span(span);
            }
            visit_attributes(visitor, &mut import.attributes);
            visitor.visit_span(&mut import.span);
        }
        ItemKind::TypeAlias {
            aliased_type,
            attributes,
            ..
        } => {
            visitor.visit_type(aliased_type);
            visit_attributes(visitor, attributes);
        }
        ItemKind::ForeignGlobal { ty, attributes, .. } => {
            visitor.visit_type(ty);
            visit_attributes(visitor, attributes);
        }
        ItemKind::Const {
            ty,
            value,
            attributes,
            ..
        } => {
            visitor.visit_type(ty);
            visitor.visit_expr(value);
            visit_attributes(visitor, attributes);
        }
    }
}

pub fn walk_block<V: MutVisitor + ?Sized>(visitor: &mut V, block: &mut Block) {
    visitor.visit_span(&mut block.span);
    for stmt in &mut block.stmts {
        visitor.visit_stmt(stmt);
    }
}

pub fn walk_stmt<V: MutVisitor + ?Sized>(visitor: &mut V, stmt: &mut Stmt) {
    visitor.visit_span(&mut stmt.span);
    match &mut stmt.node {
        StmtKind::Var {
            ty,
            value,
            attributes,
            ..
        } => {
            if let Some(ty) = ty {
                visitor.visit_type(ty);
            }
            if let Some(value) = value {
                visitor.visit_expr(value);
            }
            visit_attributes(visitor, attributes);
        }
        StmtKind::Const {
            ty,
            value,
            attributes,
            ..
        } => {
            if let Some(ty) = ty {
                visitor.visit_type(ty);
            }
            visitor.visit_expr(value);
            visit_attributes(visitor, attributes);
        }
        StmtKind::Return(value) => {
            if let Some(value) = value {
                visitor.visit_expr(value);
            }
        }
        StmtKind::If {
            condition,
            then_block,
            else_if,
            else_block,
        } => {
            visitor.visit_expr(condition);
            visitor.visit_block(then_block);
            for (condition, block) in else_if {
                visitor.visit_expr(condition);
                visitor.visit_block(block);
            }
            if let Some(block) = else_block {
                visitor.visit_block(block);
            }
        }
        StmtKind::For { kind, body } => {
            match kind {
                ForLoop::Each {
                    var_spans, iter, ..
                } => {
                    for span in var_spans {
                        visitor.visit_span(span);
                    }
                    match iter {
                        ForIter::Range { start, end, .. } => {
                            visitor.visit_expr(start);
                            visitor.visit_expr(end);
                        }
                        ForIter::Iter(expr) => visitor.visit_expr(expr),
                    }
                }
                ForLoop::CStyle {
                    init,
                    condition,
                    update,
                } => {
                    if let Some(init) = init {
                        visitor.visit_stmt(init);
                    }
                    if let Some(condition) = condition {
                        visitor.visit_expr(condition);
                    }
                    if let Some(update) = update {
                        visitor.visit_expr(update);
                    }
                }
                ForLoop::Cond { condition } => {
                    if let Some(condition) = condition {
                        visitor.visit_expr(condition);
                    }
                }
            }
            visitor.visit_block(body);
        }
        StmtKind::Break | StmtKind::Continue => {}
        StmtKind::ExprStmt(expr) => visitor.visit_expr(expr),
        StmtKind::CfgBlock { condition, body } => {
            visitor.visit_attribute(condition);
            visitor.visit_block(body);
        }
        StmtKind::UnsafeBlock { body } => visitor.visit_block(body),
    }
}

pub fn walk_expr<V: MutVisitor + ?Sized>(visitor: &mut V, expr: &mut Expr) {
    visitor.visit_span(&mut expr.span);
    match &mut expr.node {
        ExprKind::Literal(_) | ExprKind::Ident(_) => {}
        ExprKind::Group(inner) | ExprKind::Spread(inner) => visitor.visit_expr(inner),
        ExprKind::Unary { expr, .. } | ExprKind::Try { expr } => visitor.visit_expr(expr),
        ExprKind::IncDec { expr, .. } => visitor.visit_expr(expr),
        ExprKind::Cast { expr, ty } => {
            visitor.visit_expr(expr);
            visitor.visit_type(ty);
        }
        ExprKind::Binary { left, right, .. } => {
            visitor.visit_expr(left);
            visitor.visit_expr(right);
        }
        ExprKind::Assign { target, value } | ExprKind::CompoundAssign { target, value, .. } => {
            visitor.visit_expr(target);
            visitor.visit_expr(value);
        }
        ExprKind::Call {
            callee,
            type_args,
            args,
            named_args,
        } => {
            visitor.visit_expr(callee);
            for ty in type_args {
                visitor.visit_type(ty);
            }
            for arg in args {
                visitor.visit_expr(arg);
            }
            for (_, arg) in named_args {
                visitor.visit_expr(arg);
            }
        }
        ExprKind::MethodCall {
            object,
            type_args,
            args,
            named_args,
            ..
        } => {
            visitor.visit_expr(object);
            for ty in type_args {
                visitor.visit_type(ty);
            }
            for arg in args {
                visitor.visit_expr(arg);
            }
            for (_, arg) in named_args {
                visitor.visit_expr(arg);
            }
        }
        ExprKind::Field { object, .. } => visitor.visit_expr(object),
        ExprKind::StructInit { fields, .. } => {
            for (_, value) in fields {
                visitor.visit_expr(value);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            visitor.visit_expr(scrutinee);
            for arm in arms {
                visitor.visit_pattern(&mut arm.pattern);
                if let Some(guard) = &mut arm.guard {
                    visitor.visit_expr(guard);
                }
                visitor.visit_expr(&mut arm.expr);
                visitor.visit_span(&mut arm.span);
            }
        }
        ExprKind::ArrayLit(values) => {
            for value in values {
                visitor.visit_expr(value);
            }
        }
        ExprKind::Index { object, indices } => {
            visitor.visit_expr(object);
            for index in indices {
                visitor.visit_expr(index);
            }
        }
        ExprKind::Closure { body, .. } => visitor.visit_expr(body),
    }
}

pub fn walk_type<V: MutVisitor + ?Sized>(visitor: &mut V, ty: &mut Type) {
    visitor.visit_span(&mut ty.span);
    match &mut ty.node {
        TypeKind::Named { type_args, .. } => {
            for argument in type_args {
                visitor.visit_type(argument);
            }
        }
        TypeKind::Array { elem_ty, .. }
        | TypeKind::FlexibleArray { elem_ty }
        | TypeKind::Slice { elem_ty } => visitor.visit_type(elem_ty),
        TypeKind::Ref { inner } | TypeKind::MutRef { inner } | TypeKind::RawPtr { inner } => {
            visitor.visit_type(inner)
        }
        TypeKind::Fn { params, return_ty } | TypeKind::CFn { params, return_ty } => {
            for param in params {
                visitor.visit_type(param);
            }
            visitor.visit_type(return_ty);
        }
        _ => {}
    }
}

pub fn walk_pattern<V: MutVisitor + ?Sized>(visitor: &mut V, pattern: &mut Pattern) {
    visitor.visit_span(&mut pattern.span);
    match &mut pattern.node {
        PatternKind::Variant { sub_patterns, .. } => {
            for sub in sub_patterns {
                visitor.visit_pattern(sub);
            }
        }
        PatternKind::Struct { fields, .. } => {
            for (_, field) in fields {
                visitor.visit_pattern(field);
            }
        }
        PatternKind::Wildcard | PatternKind::Bind(_) | PatternKind::Literal(_) => {}
    }
}
