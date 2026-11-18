// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Operand control shared by physical lowering and source control-flow evidence.
//! Unclassified expressions remain opaque; this is not an ownership effect model.

use crate::parser::ast::{
    BinOpKind, CompoundAssignOp, Expr, ExprKind, IncDecOp, Literal, UnaryOpKind,
};
use crate::semantic::OwnershipArgumentRole;

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
    /// `name = value`: the named target is not evaluated; the value completes
    /// before the target's previous value is replaced.
    NamedAssignment {
        name: &'a str,
        target: &'a Expr,
        value: &'a Expr,
    },
    /// `name op= value`: the named target's previous value is read before the
    /// value is evaluated; the combined result is stored afterwards.
    NamedCompoundAssignment {
        target: &'a Expr,
        op: &'a CompoundAssignOp,
        value: &'a Expr,
    },
    /// `name++`, `--name`: reads, adjusts and stores the named target without
    /// evaluating an operand.
    NamedIncDec {
        target: &'a Expr,
        op: &'a IncDecOp,
        prefix: bool,
    },
    /// `[a, b, c]`: the elements in source order, then the array value.
    ArrayLiteral(&'a [Expr]),
    /// `object.field` read whose object is an expression, not a name chain
    /// (names may be module, enum or type namespaces): the object, then the
    /// field load.
    FieldRead {
        object: &'a Expr,
    },
    /// `object[index]` read: the object, then the index, then bounds and load.
    IndexRead {
        object: &'a Expr,
        index: &'a Expr,
    },
    /// An update of a dereference, field or single-index place. Lowering
    /// evaluates `operands` (see [`place_target_operands`]), then the value
    /// when there is one, then reads (for compound forms) and stores the place.
    PlaceUpdate {
        target: &'a Expr,
        operands: Vec<&'a Expr>,
        value: Option<&'a Expr>,
        form: PlaceUpdateForm<'a>,
    },
}

#[derive(Clone, Copy)]
pub(crate) enum PlaceUpdateForm<'a> {
    Assign,
    Compound(&'a CompoundAssignOp),
    IncDec { op: &'a IncDecOp, prefix: bool },
}

/// The subexpressions lowering evaluates to address a place target, in order:
/// a dereferenced pointer, a field's object, or an indexed object and then its
/// single index. Bare names and other targets return `None`.
pub(crate) fn place_target_operands(target: &Expr) -> Option<Vec<&Expr>> {
    match &target.node {
        ExprKind::Group(inner) => place_target_operands(inner),
        ExprKind::Unary {
            op: UnaryOpKind::Deref,
            expr,
        } => Some(vec![expr]),
        ExprKind::Field { object, .. } => Some(vec![object]),
        ExprKind::Index { object, indices } if indices.len() == 1 => {
            Some(vec![object, &indices[0]])
        }
        _ => None,
    }
}

fn place_update<'a>(
    target: &'a Expr,
    value: Option<&'a Expr>,
    form: PlaceUpdateForm<'a>,
) -> ExpressionControl<'a> {
    match place_target_operands(target) {
        Some(operands) => ExpressionControl::PlaceUpdate {
            target,
            operands,
            value,
            form,
        },
        None => ExpressionControl::Opaque,
    }
}

/// `a`, `a.b.c` or a grouped form: a path that may name a namespace.
fn is_name_chain(expression: &Expr) -> bool {
    match &expression.node {
        ExprKind::Ident(_) => true,
        ExprKind::Group(inner) | ExprKind::Field { object: inner, .. } => is_name_chain(inner),
        _ => false,
    }
}

/// The local or global name an assignment target spells, through groups.
pub(crate) fn assignment_target_name(target: &Expr) -> Option<&str> {
    match &target.node {
        ExprKind::Ident(name) => Some(name),
        ExprKind::Group(inner) => assignment_target_name(inner),
        _ => None,
    }
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
        ExprKind::ArrayLit(elements) => ExpressionControl::ArrayLiteral(elements),
        ExprKind::Field { object, .. } if !is_name_chain(object) => {
            ExpressionControl::FieldRead { object }
        }
        ExprKind::Index { object, indices } if indices.len() == 1 => ExpressionControl::IndexRead {
            object,
            index: &indices[0],
        },
        ExprKind::Assign { target, value } => match assignment_target_name(target) {
            Some(name) => ExpressionControl::NamedAssignment {
                name,
                target,
                value,
            },
            None => place_update(target, Some(value), PlaceUpdateForm::Assign),
        },
        ExprKind::CompoundAssign { target, op, value }
            if assignment_target_name(target).is_some() =>
        {
            ExpressionControl::NamedCompoundAssignment { target, op, value }
        }
        ExprKind::CompoundAssign { target, op, value } => {
            place_update(target, Some(value), PlaceUpdateForm::Compound(op))
        }
        ExprKind::IncDec { expr, op, prefix } if assignment_target_name(expr).is_some() => {
            ExpressionControl::NamedIncDec {
                target: expr,
                op,
                prefix: *prefix,
            }
        }
        ExprKind::IncDec { expr, op, prefix } => place_update(
            expr,
            None,
            PlaceUpdateForm::IncDec {
                op,
                prefix: *prefix,
            },
        ),
        _ => ExpressionControl::Opaque,
    }
}

/// Schedule by declared slot, keeping multiple variadic values in source
/// order. Method lowering evaluates a receiver separately before this schedule.
pub(crate) fn ordered_call_arguments<'a, 'slots>(
    slots: impl Iterator<Item = (&'slots OwnershipArgumentRole, Option<usize>)>,
    positional: &'a [Expr],
    named: &'a [(String, Expr)],
) -> Result<Vec<&'a Expr>, String> {
    let supplied =
        positional
            .iter()
            .enumerate()
            .map(|(index, expression)| (OwnershipArgumentRole::Positional(index), expression))
            .chain(named.iter().map(|(name, expression)| {
                (OwnershipArgumentRole::Named(name.clone()), expression)
            }));
    let expected = slots
        .filter(|(role, _)| **role != OwnershipArgumentRole::Receiver)
        .collect::<Vec<_>>();
    if expected.len() != positional.len() + named.len() {
        return Err(
            "named call declaration-slot mapping does not cover every supplied argument"
                .to_string(),
        );
    }
    let mut ordered = Vec::with_capacity(expected.len());
    for ((role, expression), (expected_role, slot)) in supplied.zip(expected) {
        if &role != expected_role {
            return Err(
                "named call declaration-slot mapping does not match its supplied arguments"
                    .to_string(),
            );
        }
        let slot =
            slot.ok_or_else(|| "named call argument has no resolved declaration slot".to_string())?;
        ordered.push((slot, expression));
    }
    // Stable sorting keeps all supplied variadic elements in their existing
    // order. Receiver arguments are emitted separately by method dispatch.
    ordered.sort_by_key(|(slot, _)| *slot);
    Ok(ordered
        .into_iter()
        .map(|(_, expression)| expression)
        .collect())
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
        ExprKind::Binary { left, op, right }
            if !matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr) =>
        {
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
