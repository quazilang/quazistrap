// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Operand control shared by physical lowering and source control-flow evidence.
//! Unclassified expressions remain opaque; this is not an ownership effect model.

use crate::parser::ast::{BinOpKind, Expr, ExprKind};

pub(crate) enum ExpressionControl<'a> {
    Opaque,
    UnaryChild(&'a Expr),
    EagerBinary {
        left: &'a Expr,
        right: &'a Expr,
    },
    ShortCircuit {
        left: &'a Expr,
        right: &'a Expr,
        evaluate_right_on: bool,
    },
    TryOperand(&'a Expr),
}

pub(crate) fn expression_control(expression: &Expr) -> ExpressionControl<'_> {
    match &expression.node {
        ExprKind::Group(inner)
        | ExprKind::Cast { expr: inner, .. }
        | ExprKind::Unary { expr: inner, .. } => ExpressionControl::UnaryChild(inner),
        ExprKind::Binary {
            left,
            op: BinOpKind::AndAnd,
            right,
        } => ExpressionControl::ShortCircuit {
            left,
            right,
            evaluate_right_on: true,
        },
        ExprKind::Binary {
            left,
            op: BinOpKind::OrOr,
            right,
        } => ExpressionControl::ShortCircuit {
            left,
            right,
            evaluate_right_on: false,
        },
        ExprKind::Binary { left, right, .. } => ExpressionControl::EagerBinary { left, right },
        ExprKind::Try { expr } => ExpressionControl::TryOperand(expr),
        _ => ExpressionControl::Opaque,
    }
}
