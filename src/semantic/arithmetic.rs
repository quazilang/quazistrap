// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Source arithmetic obligations. Template parameters retain declaration
//! slots; only proven concrete specializations can discharge those obligations.

use super::*;

const MAX_SITES: usize = 65_536;
const MAX_WORK: usize = 1_000_000;
const MAX_RETAINED_TEXT: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(super) enum ArithmeticOp {
    Increment,
    Negate,
    Binary(BinOpKind),
    Ordering(BinOpKind),
    Compound(CompoundAssignOp),
}

impl ArithmeticOp {
    fn name(&self) -> &'static str {
        match self {
            Self::Increment => "++ / --",
            Self::Negate => "unary -",
            Self::Binary(BinOpKind::Add) => "+",
            Self::Binary(BinOpKind::Sub) => "-",
            Self::Binary(BinOpKind::Mul) => "*",
            Self::Binary(BinOpKind::Div) => "/",
            Self::Binary(BinOpKind::Mod) => "%",
            Self::Binary(BinOpKind::BitAnd) => "&",
            Self::Binary(BinOpKind::BitOr) => "|",
            Self::Binary(BinOpKind::BitXor) => "^",
            Self::Binary(BinOpKind::Shl) => "<<",
            Self::Binary(BinOpKind::Shr) => ">>",
            Self::Ordering(BinOpKind::Lt) => "<",
            Self::Ordering(BinOpKind::LtEq) => "<=",
            Self::Ordering(BinOpKind::Gt) => ">",
            Self::Ordering(BinOpKind::GtEq) => ">=",
            Self::Binary(_) | Self::Ordering(_) => unreachable!("unsupported operator"),
            Self::Compound(CompoundAssignOp::Add) => "+=",
            Self::Compound(CompoundAssignOp::Sub) => "-=",
            Self::Compound(CompoundAssignOp::Mul) => "*=",
            Self::Compound(CompoundAssignOp::Div) => "/=",
            Self::Compound(CompoundAssignOp::Mod) => "%=",
        }
    }

    fn permits_offset(&self) -> bool {
        matches!(
            self,
            Self::Compound(CompoundAssignOp::Add | CompoundAssignOp::Sub)
                | Self::Binary(BinOpKind::Add | BinOpKind::Sub)
        )
    }
    fn permits_string_views(&self) -> bool {
        matches!(self, Self::Binary(BinOpKind::Add) | Self::Ordering(_))
    }
    fn permits_bool(&self) -> bool {
        matches!(
            self,
            Self::Binary(BinOpKind::BitAnd | BinOpKind::BitOr | BinOpKind::BitXor)
        )
    }
    fn permits_float(&self) -> bool {
        !matches!(
            self,
            Self::Binary(
                BinOpKind::BitAnd
                    | BinOpKind::BitOr
                    | BinOpKind::BitXor
                    | BinOpKind::Shl
                    | BinOpKind::Shr
            )
        )
    }
    fn is_ordering(&self) -> bool {
        matches!(self, Self::Ordering(_))
    }
    fn requirements(&self) -> &'static str {
        if self.is_ordering() {
            "integer pairs, float pairs or supported strings"
        } else if self.permits_bool() {
            "integer pairs or bool pairs"
        } else if !self.permits_float() {
            "integer operands"
        } else {
            "numeric operands, str concatenation or a permitted pointer offset"
        }
    }
}

#[derive(Clone, Debug)]
enum Operand {
    // Only integer/float primitive leaves are retained here, never a type tree.
    Number(TypeKind),
    Pointer,
    StringView,
    OwnedString,
    Bool,
    Parameter(usize),
}

impl Operand {
    fn description(&self) -> String {
        match self {
            Self::Number(ty) => ty.to_string(),
            Self::Pointer => "raw pointer".into(),
            Self::StringView => "str".into(),
            Self::OwnedString => "String".into(),
            Self::Bool => "bool".into(),
            Self::Parameter(slot) => format!("type parameter slot {slot}"),
        }
    }
}

#[derive(Debug)]
struct Site {
    callable: String,
    span: Span,
    kind: ArithmeticOp,
    target: Operand,
    value: Option<Operand>,
}

pub(super) struct ArithmeticFacts {
    sites: Vec<Site>,
    seen: HashSet<(usize, usize)>,
    retained_text: usize,
    exhausted: bool,
    site_limit: usize,
    work_limit: usize,
}

impl Default for ArithmeticFacts {
    fn default() -> Self {
        Self {
            sites: Vec::new(),
            seen: HashSet::new(),
            retained_text: 0,
            exhausted: false,
            site_limit: MAX_SITES,
            work_limit: MAX_WORK,
        }
    }
}

impl ArithmeticFacts {
    pub(super) fn clear(&mut self) {
        self.sites.clear();
        self.seen.clear();
        self.retained_text = 0;
        self.exhausted = false;
    }
}

impl Analyzer {
    /// Shared string references preserve their view representation. An
    /// exclusive reference adds a VM slot; implicit emission loads one slot.
    pub(super) fn arithmetic_string_load(ty: &TypeKind) -> Option<bool> {
        let mut cursor = ty;
        let mut load = false;
        loop {
            match cursor {
                TypeKind::Str => return Some(load),
                TypeKind::Ref { inner } => cursor = &inner.node,
                TypeKind::MutRef { inner } if !load => {
                    load = true;
                    cursor = &inner.node;
                }
                _ => return None,
            }
        }
    }

    fn arithmetic_operand(
        &mut self,
        kind: &ArithmeticOp,
        ty: Option<&TypeKind>,
        span: Span,
        parameters: &[String],
        rhs: bool,
    ) -> Option<Operand> {
        let Some(ty) = ty else {
            self.push_error(
                span,
                "S14",
                format!(
                    "cannot prove the operand type for {}; add an explicit type annotation",
                    kind.name()
                ),
            );
            return None;
        };
        let mut ty = self.resolve_type_aliases(ty);
        if rhs || matches!(kind, ArithmeticOp::Binary(_) | ArithmeticOp::Ordering(_)) {
            // Match checked binary operand autoderef. A symbolic &T is not
            // an implicit numeric load until that lowering is implemented.
            ty = if matches!(kind, ArithmeticOp::Binary(_) | ArithmeticOp::Ordering(_))
                && Self::arithmetic_string_load(&ty).is_some()
            {
                TypeKind::Str
            } else {
                self.checked_string_operand_type(ty)
            };
        }
        if let TypeKind::Named { name, type_args } = &ty
            && type_args.is_empty()
            && self
                .current_generic_params
                .iter()
                .any(|scope| scope.contains(name))
            && let Some(slot) = parameters.iter().rposition(|parameter| parameter == name)
        {
            return Some(Operand::Parameter(slot));
        }
        if Self::is_integer(&ty) || (kind.permits_float() && Self::is_float(&ty)) {
            return Some(Operand::Number(ty));
        }
        if kind.permits_string_views() && matches!(ty, TypeKind::Str) {
            return Some(Operand::StringView);
        }
        if kind.is_ordering()
            && matches!(ty, TypeKind::Named { .. })
            && self.is_checked_owned_string(&ty)
        {
            return Some(Operand::OwnedString);
        }
        if kind.permits_bool() && matches!(ty, TypeKind::Bool) {
            return Some(Operand::Bool);
        }
        if matches!(ty, TypeKind::RawPtr { .. }) && !rhs && kind.permits_offset() {
            return Some(Operand::Pointer);
        }
        if !matches!(ty, TypeKind::Error) {
            self.push_error(
                span,
                "S06",
                format!(
                    "{} not valid for {}; {} are required",
                    kind.name(),
                    ty,
                    kind.requirements()
                ),
            );
        }
        None
    }

    pub(super) fn arithmetic_op_is_deferred(&self, span: Span) -> bool {
        self.arithmetic.seen.contains(&(span.start, span.end))
    }

    pub(super) fn check_arithmetic_op(
        &mut self,
        span: Span,
        kind: ArithmeticOp,
        target: Option<&TypeKind>,
        value: Option<&TypeKind>,
    ) {
        let parameters = self
            .current_function
            .last()
            .map(|callable| self.generic_params_for_function(callable))
            .unwrap_or_default();
        let Some(target) = self.arithmetic_operand(&kind, target, span, &parameters, false) else {
            return;
        };
        let value = if matches!(
            kind,
            ArithmeticOp::Compound(_) | ArithmeticOp::Binary(_) | ArithmeticOp::Ordering(_)
        ) {
            let Some(value) = self.arithmetic_operand(&kind, value, span, &parameters, true) else {
                return;
            };
            Some(value)
        } else {
            None
        };
        if let (Operand::Pointer, Some(Operand::Number(ty))) = (&target, &value)
            && !Self::is_integer(ty)
        {
            self.push_error(
                span,
                "S06",
                format!(
                    "{} on a raw pointer requires an integer offset",
                    kind.name()
                ),
            );
            return;
        }
        if (matches!(target, Operand::StringView | Operand::OwnedString)
            || matches!(value, Some(Operand::StringView | Operand::OwnedString)))
            && !matches!(
                (&target, &value),
                (
                    Operand::StringView | Operand::OwnedString | Operand::Parameter(_),
                    Some(Operand::StringView | Operand::OwnedString | Operand::Parameter(_))
                )
            )
        {
            self.push_error(
                span,
                "S06",
                "string operations require two supported strings".into(),
            );
            return;
        }
        if !matches!(target, Operand::Parameter(_)) && !matches!(value, Some(Operand::Parameter(_)))
        {
            // Existing inference checks concrete numeric compatibility.
            return;
        }
        let Some(callable) = self.current_function.last() else {
            self.push_error(
                span,
                "S14",
                "arithmetic has no source declaration context".into(),
            );
            return;
        };
        let facts = &mut self.arithmetic;
        if facts.exhausted || facts.seen.contains(&(span.start, span.end)) {
            return;
        }
        if facts.sites.len() >= facts.site_limit
            || facts
                .retained_text
                .checked_add(callable.len())
                .is_none_or(|size| size > MAX_RETAINED_TEXT)
        {
            facts.exhausted = true;
            self.push_error(span, "S14", "arithmetic source-fact budget exceeded".into());
            return;
        }
        facts.retained_text += callable.len();
        facts.seen.insert((span.start, span.end));
        facts.sites.push(Site {
            callable: callable.clone(),
            span,
            kind,
            target,
            value,
        });
    }

    pub(super) fn validate_arithmetic_specializations(
        &mut self,
        concrete: &BTreeSet<OwnershipCallableId>,
        checkpoint: &mut impl FnMut() -> Result<(), crate::cancel::Cancelled>,
    ) -> Result<(), crate::cancel::Cancelled> {
        let mut sites: HashMap<&str, Vec<&Site>> = HashMap::new();
        for site in &self.arithmetic.sites {
            checkpoint()?;
            sites.entry(&site.callable).or_default().push(site);
        }
        if sites.is_empty() {
            return Ok(());
        }
        let mut work = self.arithmetic.work_limit;
        let mut checked = BTreeSet::new();
        let mut rejected = HashSet::new();
        let mut diagnostics = Vec::new();
        'specializations: for mono in &self.monomorphizations {
            checkpoint()?;
            let Some(obligations) = sites.get(mono.fn_name.as_str()) else {
                continue;
            };
            let id = self.ownership_callable_id(&mono.fn_name, &mono.type_args);
            if !concrete.contains(&id) || !checked.insert(id) {
                continue;
            }
            for site in obligations {
                checkpoint()?;
                if work == 0 {
                    diagnostics.push(SemanticError {
                        span: site.span,
                        code: "S14",
                        message: "arithmetic specialization work budget exceeded".into(),
                    });
                    break 'specializations;
                }
                work -= 1;
                let resolved = |operand: &Operand| -> Result<Operand, (&'static str, String)> {
                    match operand {
                        Operand::Parameter(slot) => {
                            let actual = mono.type_args.get(*slot).ok_or_else(|| {
                                (
                                    "S14",
                                    "arithmetic type-parameter slot is missing".to_string(),
                                )
                            })?;
                            let actual = crate::type_resolution::resolve_type(
                                actual,
                                &self.type_aliases,
                                &HashMap::new(),
                                &HashSet::new(),
                                |_| None,
                            )
                            .map_err(|error| ("S14", error.to_string()))?;
                            if Self::is_integer(&actual)
                                || (site.kind.permits_float() && Self::is_float(&actual))
                            {
                                Ok(Operand::Number(actual))
                            } else if site.kind.permits_string_views()
                                && Self::arithmetic_string_load(&actual) == Some(false)
                            {
                                Ok(Operand::StringView)
                            } else if site.kind.is_ordering()
                                && self.is_checked_owned_string(&actual)
                                && matches!(actual, TypeKind::Named { .. })
                            {
                                Ok(Operand::OwnedString)
                            } else if site.kind.permits_bool() && matches!(actual, TypeKind::Bool) {
                                Ok(Operand::Bool)
                            } else if matches!(actual, TypeKind::RawPtr { .. }) {
                                Ok(Operand::Pointer)
                            } else {
                                Err(("S06", format!("unsupported operand type `{actual}`")))
                            }
                        }
                        other => Ok(other.clone()),
                    }
                };
                let target = resolved(&site.target);
                let value = site.value.as_ref().map(resolved).transpose();
                let valid = match (&target, &value) {
                    (Ok(Operand::Number(_)), Ok(None)) => true,
                    (
                        Ok(Operand::StringView | Operand::OwnedString),
                        Ok(Some(Operand::StringView | Operand::OwnedString)),
                    ) => site.kind.permits_string_views(),
                    (Ok(Operand::Bool), Ok(Some(Operand::Bool))) => site.kind.permits_bool(),
                    (Ok(Operand::Number(left)), Ok(Some(Operand::Number(right)))) => {
                        if site.kind.is_ordering() {
                            (Self::is_integer(left) && Self::is_integer(right))
                                || (Self::is_float(left) && Self::is_float(right))
                        } else {
                            self.types_compatible(left, right)
                        }
                    }
                    (Ok(Operand::Pointer), Ok(Some(Operand::Number(right)))) => {
                        site.kind.permits_offset() && Self::is_integer(right)
                    }
                    _ => false,
                };
                if !valid && rejected.insert((site.span.start, site.span.end)) {
                    let describe = |operand: &Result<Operand, (&'static str, String)>| match operand
                    {
                        Ok(value) => value.description(),
                        Err((_, error)) => error.clone(),
                    };
                    let code = if matches!(&target, Err(("S14", _)))
                        || matches!(&value, Err(("S14", _)))
                    {
                        "S14"
                    } else if matches!(
                        (&target, &value),
                        (Ok(Operand::Number(_)), Ok(Some(Operand::Number(_))))
                    ) && !site.kind.is_ordering()
                    {
                        "S01"
                    } else {
                        "S06"
                    };
                    let target = describe(&target);
                    let value = match value {
                        Ok(Some(value)) => format!(" and {}", value.description()),
                        Err((_, error)) => format!(" and {error}"),
                        _ => String::new(),
                    };
                    diagnostics.push(SemanticError { span: site.span, code, message: format!("specialization `{}`: {} not valid for {target}{value}; {} are required", mono.mangled_name, site.kind.name(), site.kind.requirements()) });
                }
            }
        }
        self.errors.extend(diagnostics);
        Ok(())
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    use crate::{lexer::Lexer, parser::Parser};

    fn program(source: &str) -> Program {
        Parser::new(Lexer::new(source).tokenize()).parse().unwrap()
    }

    #[test]
    fn numeric_mutation_source_budget_rejects_and_resets() {
        let mut analyzer = Analyzer::new();
        analyzer.arithmetic.site_limit = 0;
        let report = analyzer.analyze_program(&program("fn bump[T](value:T)T{value++;ret value;}"));
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.code == "S14" && error.message.contains("source-fact budget"))
        );
        assert!(
            analyzer
                .analyze_program(&program("fn main()void{}"))
                .errors
                .is_empty()
        );
    }

    #[test]
    fn numeric_mutation_specialization_work_budget_rejects() {
        let mut analyzer = Analyzer::new();
        analyzer.arithmetic.work_limit = 0;
        let report = analyzer.analyze_program(&program(
            "fn bump[T](value:T)T{value++;ret value;}fn main()void{bump[i32](1);}",
        ));
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.code == "S14" && error.message.contains("work budget"))
        );
    }

    #[test]
    fn numeric_mutation_specialization_validation_polls_cancellation() {
        let mut analyzer = Analyzer::new();
        let report = analyzer.analyze_program(&program(
            "fn bump[T](value:T)T{value++;ret value;}fn main()void{bump[i32](1);}",
        ));
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let concrete = report
            .ownership_call_graph
            .iter()
            .map(|node| node.id.clone())
            .collect();
        analyzer.monomorphizations = report.monomorphizations;
        let mut polls = 0;
        let result = analyzer.validate_arithmetic_specializations(&concrete, &mut || {
            polls += 1;
            if polls >= 2 {
                Err(crate::cancel::Cancelled)
            } else {
                Ok(())
            }
        });
        assert_eq!(result, Err(crate::cancel::Cancelled));
        assert!(analyzer.errors.is_empty());
    }
}
