// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::BTreeSet;

use crate::parser::ast::*;

use super::*;

impl Analyzer {
    pub(super) fn extract_field_chain(expr: &Expr) -> Option<(String, Vec<String>)> {
        match &expr.node {
            ExprKind::Ident(name) => Some((name.clone(), vec![])),
            ExprKind::Field { object, name } => {
                let (base, mut path) = Self::extract_field_chain(object)?;
                path.push(name.clone());
                Some((base, path))
            }
            _ => None,
        }
    }

    pub(super) fn check_math_identities(
        &mut self,
        span: Span,
        op: &BinOpKind,
        left: &ExprEval,
        right: &ExprEval,
    ) -> Option<ConstValue> {
        // Absorbing elements: result is constant regardless of the unknown operand.
        let absorbed: Option<(ConstValue, &str)> = match (op, &left.const_value, &right.const_value) {
            (BinOpKind::Mul, _, Some(ConstValue::Int(0)))
            | (BinOpKind::Mul, Some(ConstValue::Int(0)), _) => {
                Some((ConstValue::Int(0), "x * 0 = 0"))
            }
            (BinOpKind::Mul, _, Some(ConstValue::Float(f))) if *f == 0.0 => {
                Some((ConstValue::Float(0.0), "x * 0.0 = 0.0"))
            }
            (BinOpKind::Mul, Some(ConstValue::Float(f)), _) if *f == 0.0 => {
                Some((ConstValue::Float(0.0), "0.0 * x = 0.0"))
            }
            (BinOpKind::AndAnd, _, Some(ConstValue::Bool(false)))
            | (BinOpKind::AndAnd, Some(ConstValue::Bool(false)), _) => {
                Some((ConstValue::Bool(false), "x && false = false"))
            }
            (BinOpKind::OrOr, _, Some(ConstValue::Bool(true)))
            | (BinOpKind::OrOr, Some(ConstValue::Bool(true)), _) => {
                Some((ConstValue::Bool(true), "x || true = true"))
            }
            _ => None,
        };

        if let Some((val, desc)) = absorbed {
            self.math_optimizations.push(MathOptimization {
                span,
                description: desc.to_string(),
                result_value: Some(val.clone()),
            });
            self.push_suggestion(Some(span), format!("math optimization: {}", desc));
            // Return the folded value so ExprAnnotation.const_value carries it.
            return Some(val);
        }

        // Identity elements: result equals the non-constant operand — emit suggestion only.
        let identity_desc: Option<&str> = match (op, &left.const_value, &right.const_value) {
            (BinOpKind::Add, _, Some(ConstValue::Int(0)))
            | (BinOpKind::Add, Some(ConstValue::Int(0)), _) => Some("x + 0 = x"),
            (BinOpKind::Sub, _, Some(ConstValue::Int(0))) => Some("x - 0 = x"),
            (BinOpKind::Add, _, Some(ConstValue::Float(f))) if *f == 0.0 => Some("x + 0.0 = x"),
            (BinOpKind::Add, Some(ConstValue::Float(f)), _) if *f == 0.0 => Some("0.0 + x = x"),
            (BinOpKind::Sub, _, Some(ConstValue::Float(f))) if *f == 0.0 => Some("x - 0.0 = x"),
            (BinOpKind::Mul, _, Some(ConstValue::Int(1)))
            | (BinOpKind::Mul, Some(ConstValue::Int(1)), _) => Some("x * 1 = x"),
            (BinOpKind::Div, _, Some(ConstValue::Int(1))) => Some("x / 1 = x"),
            (BinOpKind::Mul, _, Some(ConstValue::Float(f))) if *f == 1.0 => Some("x * 1.0 = x"),
            (BinOpKind::Mul, Some(ConstValue::Float(f)), _) if *f == 1.0 => Some("1.0 * x = x"),
            (BinOpKind::Div, _, Some(ConstValue::Float(f))) if *f == 1.0 => Some("x / 1.0 = x"),
            (BinOpKind::AndAnd, _, Some(ConstValue::Bool(true)))
            | (BinOpKind::AndAnd, Some(ConstValue::Bool(true)), _) => Some("x && true = x"),
            (BinOpKind::OrOr, _, Some(ConstValue::Bool(false)))
            | (BinOpKind::OrOr, Some(ConstValue::Bool(false)), _) => Some("x || false = x"),
            _ => None,
        };

        if let Some(desc) = identity_desc {
            self.math_optimizations.push(MathOptimization {
                span,
                description: desc.to_string(),
                result_value: None,
            });
            self.push_suggestion(Some(span), format!("math optimization: {}", desc));
        }

        None
    }

    pub(super) fn run_lazy_import_pass(&mut self) {
        let accesses = self.lazy_import_accesses.clone();

        for (local_name, paths) in &accesses {
            let sym = match self.resolve_symbol(local_name) {
                Some(s) if s.is_import => s,
                _ => continue,
            };

            let import_path = sym.import_path.as_deref().unwrap_or(local_name).to_string();

            // Keep only deepest paths (remove any that are a strict prefix of another).
            let mut paths_vec: Vec<String> = paths.iter().cloned().collect();
            paths_vec.sort();
            let deepest: Vec<String> = paths_vec
                .iter()
                .filter(|p| {
                    !paths_vec
                        .iter()
                        .any(|other| other != *p && other.starts_with(&format!("{}.", p)))
                })
                .cloned()
                .collect();

            // Only emit when the accessed paths are strictly deeper than the import itself.
            let narrower: Vec<String> = deepest
                .into_iter()
                .filter(|p| p.len() > import_path.len())
                .collect();

            if narrower.is_empty() {
                continue;
            }

            let suggested: Vec<String> =
                narrower.iter().map(|p| format!("import {};", p)).collect();

            self.push_suggestion(
                Some(sym.span),
                format!(
                    "lazy import: '{}' only accesses [{}]; consider narrowing",
                    import_path,
                    narrower.join(", ")
                ),
            );

            self.lazy_import_hints.push(LazyImportHint {
                import_span: sym.span,
                broad_path: import_path,
                accessed_subpaths: narrower,
                suggested_imports: suggested,
            });
        }
    }

    pub(super) fn run_inline_candidate_pass(&mut self, program: &Program) {
        for item in &program.items {
            match &item.node {
                ItemKind::Fn { name, body, .. } => {
                    self.maybe_add_inline_candidate(name, body, item.span);
                }
                ItemKind::Impl { methods, .. } => {
                    for method in methods {
                        if let ItemKind::Fn { name, body, .. } = &method.node {
                            self.maybe_add_inline_candidate(name, body, method.span);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn maybe_add_inline_candidate(&mut self, name: &str, body: &Block, span: Span) {
        if name == "main" {
            return;
        }

        if body.stmts.len() > 2 {
            return;
        }

        if body
            .stmts
            .iter()
            .any(|stmt| matches!(stmt.node, StmtKind::If { .. } | StmtKind::While { .. }))
        {
            return;
        }

        let candidate = InlineCandidate {
            name: name.to_string(),
            span,
            reason: "small function body".to_string(),
        };
        self.inline_candidates.push(candidate.clone());
        self.push_suggestion(
            Some(span),
            format!(
                "function '{}' is an inline candidate ({})",
                candidate.name, candidate.reason
            ),
        );
    }

    pub(super) fn run_import_optimization_pass(&mut self) {
        if self.unused_import_paths.is_empty() {
            return;
        }

        let list = self
            .unused_import_paths
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");

        self.push_suggestion(
            None,
            format!("import optimization: remove unused imports [{}]", list),
        );
    }

    pub(super) fn run_exhaustiveness_pass(&mut self) {
        let candidates = std::mem::take(&mut self.match_candidates);

        for candidate in candidates {
            let Some(TypeKind::Named { name: scrutinee_enum, .. }) = candidate.scrutinee_ty else {
                continue;
            };

            let Some(enum_info) = self.enums.get(&scrutinee_enum).cloned() else {
                continue;
            };

            self.exhaustiveness_checks += 1;

            let mut wildcard_seen = false;
            let mut covered: BTreeSet<String> = BTreeSet::new();

            for arm in &candidate.arms {
                match &arm.kind {
                    MatchArmKindInfo::Wildcard => {
                        if wildcard_seen {
                            self.push_warning(
                                arm.span,
                                "W05",
                                "unreachable wildcard match arm".to_string(),
                            );
                        }
                        wildcard_seen = true;
                    }
                    MatchArmKindInfo::Variant { enum_name, variant } => {
                        if wildcard_seen {
                            self.push_warning(
                                arm.span,
                                "W05",
                                format!(
                                    "unreachable match arm '{}', wildcard already covers it",
                                    variant
                                ),
                            );
                            continue;
                        }

                        let target_enum =
                            enum_name.clone().unwrap_or_else(|| scrutinee_enum.clone());

                        if target_enum != scrutinee_enum {
                            self.push_error(
                                arm.span,
                                "S09",
                                format!(
                                    "match arm enum '{}' does not match '{}'",
                                    target_enum, scrutinee_enum
                                ),
                            );
                            continue;
                        }

                        if !enum_info.variants.contains_key(variant) {
                            self.push_error(
                                arm.span,
                                "S04",
                                format!(
                                    "unknown variant '{}.{}' in match arm",
                                    scrutinee_enum, variant
                                ),
                            );
                            continue;
                        }

                        if !covered.insert(variant.clone()) {
                            self.push_warning(
                                arm.span,
                                "W05",
                                format!("duplicate/unreachable match arm '{}.{}'", scrutinee_enum, variant),
                            );
                            self.push_suggestion(
                                Some(arm.span),
                                format!(
                                    "remove duplicate match arm for '{}.{}'",
                                    scrutinee_enum, variant
                                ),
                            );
                        }
                    }
                }
            }

            if !wildcard_seen {
                let missing_variants: Vec<String> = enum_info
                    .variants
                    .keys()
                    .filter(|name| !covered.contains(*name))
                    .cloned()
                    .collect();

                if !missing_variants.is_empty() {
                    let missing_joined = missing_variants
                        .iter()
                        .map(|v| format!("{}.{}", scrutinee_enum, v))
                        .collect::<Vec<_>>()
                        .join(", ");

                    self.push_error(
                        candidate.span,
                        "S09",
                        format!(
                            "non-exhaustive match for enum '{}': missing [{}]",
                            scrutinee_enum, missing_joined
                        ),
                    );
                    self.push_suggestion(
                        Some(candidate.span),
                        format!(
                            "add missing arms [{}] or add wildcard '_' arm",
                            missing_joined
                        ),
                    );

                    self.non_exhaustive_matches.push(MatchExhaustivenessIssue {
                        span: candidate.span,
                        enum_name: scrutinee_enum,
                        missing_variants,
                    });
                }
            }
        }
    }
}
