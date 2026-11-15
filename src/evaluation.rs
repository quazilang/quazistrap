// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Operand control shared by physical lowering and source control-flow evidence.
//! Unclassified expressions remain opaque; this is not an ownership effect model.

use crate::parser::ast::{BinOpKind, Expr, ExprKind, Literal, UnaryOpKind};

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

/// Integer literals are typed `i32` without contextual inference, so an
/// expression built only from integer literals has no declared width: `1 << 40`
/// in a `u64` context must not wrap at 32 bits.
pub(crate) fn is_integer_literal_expression(expression: &Expr) -> bool {
    match &expression.node {
        ExprKind::Literal(Literal::Int(_)) => true,
        ExprKind::Group(inner)
        | ExprKind::Unary {
            op: UnaryOpKind::Neg,
            expr: inner,
        } => is_integer_literal_expression(inner),
        ExprKind::Binary { left, op, right } if !matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr) => {
            is_integer_literal_expression(left) && is_integer_literal_expression(right)
        }
        _ => false,
    }
}

/// A float literal, possibly negated or grouped (`-0.5`): like integer
/// literals it takes the float type its context needs.
pub(crate) fn is_float_literal_expression(expression: &Expr) -> bool {
    match &expression.node {
        ExprKind::Literal(Literal::Float(_)) => true,
        ExprKind::Group(inner)
        | ExprKind::Unary {
            op: UnaryOpKind::Neg,
            expr: inner,
        } => is_float_literal_expression(inner),
        _ => false,
    }
}

/// The operand whose integer type gives an operation its width and
/// signedness: the left one unless it is literal-only, then the right one (a
/// shift keeps the type of its left operand, so a literal-only left has
/// none). `None` means both sides are literal-only: the full 64-bit result is
/// kept.
pub(crate) fn integer_width_operand<'a>(
    op: &BinOpKind,
    left: &'a Expr,
    right: &'a Expr,
) -> Option<&'a Expr> {
    if !is_integer_literal_expression(left) {
        Some(left)
    } else if matches!(op, BinOpKind::Shl | BinOpKind::Shr) || is_integer_literal_expression(right)
    {
        None
    } else {
        Some(right)
    }
}

/// The exact value of a literal-only integer expression (see
/// [`is_integer_literal_expression`]), or `None` when it is not one, divides
/// by zero or leaves the i128 range.
pub(crate) fn integer_literal_value(expression: &Expr) -> Option<i128> {
    match &expression.node {
        ExprKind::Literal(Literal::Int(value)) => Some(i128::from(*value)),
        ExprKind::Group(inner) => integer_literal_value(inner),
        ExprKind::Unary {
            op: UnaryOpKind::Neg,
            expr: inner,
        } => integer_literal_value(inner)?.checked_neg(),
        ExprKind::Binary { left, op, right } => {
            let (left, right) = (integer_literal_value(left)?, integer_literal_value(right)?);
            match op {
                BinOpKind::Add => left.checked_add(right),
                BinOpKind::Sub => left.checked_sub(right),
                BinOpKind::Mul => left.checked_mul(right),
                BinOpKind::Div => left.checked_div(right),
                BinOpKind::Mod => left.checked_rem(right),
                BinOpKind::BitAnd => Some(left & right),
                BinOpKind::BitOr => Some(left | right),
                BinOpKind::BitXor => Some(left ^ right),
                BinOpKind::Shl => u32::try_from(right)
                    .ok()
                    .filter(|shift| *shift < 64)
                    .and_then(|shift| left.checked_mul(1i128 << shift)),
                BinOpKind::Shr => u32::try_from(right)
                    .ok()
                    .filter(|shift| *shift < 64)
                    .map(|shift| left >> shift),
                _ => None,
            }
        }
        _ => None,
    }
}
