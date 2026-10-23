// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! D-016 source power requirements, including declaration-backed template slots.

use super::*;

const MAX_SITES: usize = 65_536;
const MAX_WORK: usize = 1_000_000;
const MAX_TEXT: usize = 16 * 1024 * 1024;

/// Operands must already have aliases resolved and implicit scalar loads applied.
pub(crate) fn power_result_type(base: &TypeKind, exponent: &TypeKind) -> Option<TypeKind> {
    if !(Analyzer::is_integer(base) || Analyzer::is_float(base))
        || !(Analyzer::is_integer(exponent) || Analyzer::is_float(exponent))
    {
        None
    } else if Analyzer::is_float(base) || Analyzer::is_float(exponent) {
        Some(TypeKind::Float64)
    } else {
        Some(base.clone())
    }
}

#[derive(Clone, Debug)]
enum Operand {
    Number(TypeKind),
    Parameter(usize),
}

#[derive(Debug)]
struct Site {
    callable: String,
    span: Span,
    base: Operand,
    exponent: Operand,
    constant_base: Option<i64>,
    constant_exponent: Option<i64>,
    floating_result: bool,
}

#[derive(Default)]
pub(super) struct PowerFacts {
    sites: Vec<Site>,
    seen: HashSet<(usize, usize)>,
    retained_text: usize,
    exhausted: bool,
}

impl PowerFacts {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
}

fn integer_shape(ty: &TypeKind) -> Option<(u32, bool)> {
    Some(match ty {
        TypeKind::Int8 => (8, true),
        TypeKind::Int16 => (16, true),
        TypeKind::Int32 => (32, true),
        TypeKind::Int64 | TypeKind::Isize => (64, true),
        TypeKind::Uint8 => (8, false),
        TypeKind::Uint16 => (16, false),
        TypeKind::Uint32 => (32, false),
        TypeKind::Uint64 | TypeKind::Usize => (64, false),
        _ => return None,
    })
}

/// At most 64 squaring steps, independent of the exponent's magnitude.
pub(super) fn checked_integer_power(base: i64, exponent: u64, ty: &TypeKind) -> Option<i64> {
    let (width, signed) = integer_shape(ty)?;
    let (minimum, maximum) = if signed {
        (-(1_i128 << (width - 1)), (1_i128 << (width - 1)) - 1)
    } else {
        (0, (1_i128 << width) - 1)
    };
    let mut factor = if signed {
        i128::from(base)
    } else {
        i128::from(base as u64)
    };
    let mut remaining = exponent;
    let mut result = 1_i128;
    let multiply = |left: i128, right: i128| {
        left.checked_mul(right)
            .filter(|value| (minimum..=maximum).contains(value))
    };
    while remaining != 0 {
        if remaining & 1 != 0 {
            result = multiply(result, factor)?;
        }
        remaining >>= 1;
        if remaining != 0 {
            factor = multiply(factor, factor)?;
        }
    }
    Some(result as i64)
}

fn constant_integer_error(
    base_ty: &TypeKind,
    exponent_ty: &TypeKind,
    base: Option<i64>,
    exponent: Option<i64>,
) -> Result<Option<i64>, String> {
    if !Analyzer::is_integer(base_ty) || !Analyzer::is_integer(exponent_ty) {
        return Ok(None);
    }
    let Some(exponent) = exponent else {
        return Ok(None);
    };
    if integer_shape(exponent_ty).is_some_and(|(_, signed)| signed) && exponent < 0 {
        return Err("integer power requires a nonnegative exponent".into());
    }
    let Some(base) = base else { return Ok(None) };
    checked_integer_power(base, exponent as u64, base_ty)
        .map(Some)
        .ok_or_else(|| format!("integer power overflows `{base_ty}`"))
}

impl Analyzer {
    fn power_operand(&mut self, ty: Option<&TypeKind>, span: Span) -> Option<Operand> {
        let Some(ty) = ty else {
            self.push_error(
                span,
                "S14",
                "cannot infer a power operand type; add an explicit annotation".into(),
            );
            return None;
        };
        let ty = Self::autoderef_type(self.resolve_type_aliases(ty));
        if let TypeKind::Named { name, type_args } = &ty
            && type_args.is_empty()
            && self
                .current_generic_params
                .iter()
                .any(|scope| scope.contains(name))
            && let Some(slot) = self
                .current_function
                .last()
                .map(|callable| self.generic_params_for_function(callable))
                .and_then(|parameters| parameters.iter().rposition(|parameter| parameter == name))
        {
            return Some(Operand::Parameter(slot));
        }
        if Self::is_integer(&ty) || Self::is_float(&ty) {
            Some(Operand::Number(ty))
        } else {
            if !matches!(ty, TypeKind::Error) {
                self.push_error(
                    span,
                    "S06",
                    format!("power requires numeric operands, got `{ty}`"),
                );
            }
            None
        }
    }

    /// Mutable bindings are runtime inputs even when initialized by a literal.
    fn power_constant_expression(&self, expr: &Expr) -> bool {
        match &expr.node {
            ExprKind::Literal(_) => true,
            ExprKind::Ident(name) => self.resolve_symbol(name).is_some_and(|symbol| {
                matches!(symbol.kind, SymbolKind::Variable { mutable: false })
                    && symbol.const_value.is_some()
            }),
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Unary {
                op: UnaryOpKind::Neg | UnaryOpKind::Not,
                expr: inner,
            } => self.power_constant_expression(inner),
            ExprKind::Binary { left, right, .. } => {
                self.power_constant_expression(left) && self.power_constant_expression(right)
            }
            _ => false,
        }
    }

    pub(super) fn check_power_expression(
        &mut self,
        span: Span,
        base_expr: &Expr,
        exponent_expr: &Expr,
        base: &ExprEval,
        exponent: &ExprEval,
    ) -> ExprEval {
        let Some(base_operand) = self.power_operand(base.ty.as_ref(), span) else {
            return ExprEval::default();
        };
        let Some(exponent_operand) = self.power_operand(exponent.ty.as_ref(), span) else {
            return ExprEval::default();
        };
        let constant = |expr: &Expr, eval: &ExprEval| {
            if self.power_constant_expression(expr) {
                match eval.const_value {
                    Some(ConstValue::Int(value)) => Some(value),
                    _ => None,
                }
            } else {
                None
            }
        };
        let constant_base = constant(base_expr, base);
        let constant_exponent = constant(exponent_expr, exponent);
        let floating_result = [&base_operand, &exponent_operand]
            .into_iter()
            .any(|operand| matches!(operand, Operand::Number(ty) if Self::is_float(ty)));
        let result_ty = if floating_result {
            TypeKind::Float64
        } else {
            match &base_operand {
                Operand::Number(ty) => ty.clone(),
                Operand::Parameter(_) => self.resolve_type_aliases(base.ty.as_ref().unwrap()),
            }
        };
        let mut constant_value = None;
        if let (Operand::Number(base_ty), Operand::Number(exponent_ty)) =
            (&base_operand, &exponent_operand)
        {
            match constant_integer_error(base_ty, exponent_ty, constant_base, constant_exponent) {
                Ok(value) => constant_value = value.map(ConstValue::Int),
                Err(message) => self.push_error(span, "S06", message),
            }
        } else if let Some(callable) = self.current_function.last() {
            let facts = &mut self.power;
            if !facts.exhausted && !facts.seen.contains(&(span.start, span.end)) {
                if facts.sites.len() >= MAX_SITES
                    || facts
                        .retained_text
                        .checked_add(callable.len())
                        .is_none_or(|total| total > MAX_TEXT)
                {
                    facts.exhausted = true;
                    self.push_error(span, "S14", "power source-fact budget exceeded".into());
                } else {
                    facts.retained_text += callable.len();
                    facts.seen.insert((span.start, span.end));
                    facts.sites.push(Site {
                        callable: callable.clone(),
                        span,
                        base: base_operand,
                        exponent: exponent_operand,
                        constant_base,
                        constant_exponent,
                        floating_result,
                    });
                }
            }
        } else {
            self.push_error(
                span,
                "S14",
                "generic power has no source declaration context".into(),
            );
        }
        if !floating_result {
            let from = self
                .current_function
                .last()
                .cloned()
                .unwrap_or_else(|| "__program__".into());
            self.add_dependency_edge(DependencyKind::Call, &from, "panic.panic");
            self.add_dependency_edge(DependencyKind::Call, &from, "panic");
        }
        ExprEval {
            ty: Some(result_ty),
            const_value: constant_value,
        }
    }

    pub(super) fn validate_power_specializations(
        &mut self,
        concrete: &BTreeSet<OwnershipCallableId>,
        checkpoint: &mut impl FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<(), crate::cancel::Cancelled> {
        let mut by_callable: HashMap<&str, Vec<&Site>> = HashMap::new();
        for site in &self.power.sites {
            checkpoint()?;
            by_callable.entry(&site.callable).or_default().push(site);
        }
        let mut work = MAX_WORK;
        let mut checked = BTreeSet::new();
        let mut rejected = HashSet::new();
        let mut diagnostics = Vec::new();
        'instances: for mono in &self.monomorphizations {
            checkpoint()?;
            let Some(sites) = by_callable.get(mono.fn_name.as_str()) else {
                continue;
            };
            let id = self.ownership_callable_id(&mono.fn_name, &mono.type_args);
            if !concrete.contains(&id) || !checked.insert(id) {
                continue;
            }
            for site in sites {
                checkpoint()?;
                if work == 0 {
                    diagnostics.push(SemanticError {
                        span: site.span,
                        code: "S14",
                        message: "power specialization work budget exceeded".into(),
                    });
                    break 'instances;
                }
                work -= 1;
                let resolve = |operand: &Operand| -> Result<TypeKind, (&'static str, String)> {
                    let ty = match operand {
                        Operand::Number(ty) => return Ok(ty.clone()),
                        Operand::Parameter(slot) => mono
                            .type_args
                            .get(*slot)
                            .ok_or_else(|| ("S14", "power parameter slot is missing".into()))?,
                    };
                    let ty = crate::type_resolution::resolve_type(
                        ty,
                        &self.type_aliases,
                        &HashMap::new(),
                        &HashSet::new(),
                        |_| None,
                    )
                    .map_err(|error| ("S14", error.to_string()))?;
                    if Self::is_integer(&ty) || Self::is_float(&ty) {
                        Ok(ty)
                    } else {
                        Err((
                            "S06",
                            format!("power requires numeric operands, got `{ty}`"),
                        ))
                    }
                };
                let outcome = resolve(&site.base).and_then(|base| resolve(&site.exponent).and_then(|exponent| {
                    if !site.floating_result && Self::is_integer(&base) && Self::is_float(&exponent) {
                        return Err(("S14", "generic power changes its provisional integer result to f64; use an explicitly floating operand".into()));
                    }
                    constant_integer_error(&base, &exponent, site.constant_base, site.constant_exponent)
                        .map(|_| ()).map_err(|message| ("S06", message))
                }));
                if let Err((code, message)) = outcome
                    && rejected.insert((site.span.start, site.span.end))
                {
                    diagnostics.push(SemanticError {
                        span: site.span,
                        code,
                        message: format!("specialization `{}`: {message}", mono.mangled_name),
                    });
                }
            }
        }
        self.errors.extend(diagnostics);
        Ok(())
    }
}
