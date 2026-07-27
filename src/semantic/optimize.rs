// quazi - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::{BTreeSet, HashSet};

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
        let absorbed: Option<(ConstValue, &str)> = match (op, &left.const_value, &right.const_value)
        {
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

    pub(super) fn run_tree_shake_pass(&mut self, program: &Program) {
        // Collect all locally-defined function names.
        let all_fns: BTreeSet<String> = program
            .items
            .iter()
            .filter_map(|item| match &item.node {
                ItemKind::Fn { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();

        // Library mode: no main → can't determine reachability, skip.
        if !all_fns.contains("main") {
            return;
        }

        // BFS from main using Call edges.
        let mut reachable: BTreeSet<String> = BTreeSet::new();
        reachable.insert("main".to_string());
        let mut queue: Vec<String> = vec!["main".to_string()];

        while let Some(fn_name) = queue.pop() {
            for (kind, from, to) in &self.dependency_edges {
                if *kind == DependencyKind::Call
                    && from == &fn_name
                    && all_fns.contains(to)
                    && reachable.insert(to.clone())
                {
                    queue.push(to.clone());
                }
            }
        }

        // Warn on functions that are called but not reachable from main
        // (i.e., called only by other dead functions).
        // Functions with zero callers are already caught by "unused function".
        let global_scope = self
            .scopes
            .first()
            .expect("global scope must exist")
            .clone();

        for fn_name in all_fns.difference(&reachable) {
            let is_called_by_someone = self
                .dependency_edges
                .iter()
                .any(|(k, _, to)| *k == DependencyKind::Call && to == fn_name);
            if is_called_by_someone && let Some(sym) = global_scope.get(fn_name) {
                self.push_warning_with_suggestion(
                    sym.span,
                    "W07",
                    format!(
                        "dead function '{}': only reachable from dead code, never from main",
                        fn_name
                    ),
                    format!("remove '{}' or make it reachable from main", fn_name),
                );
            }
            self.unreachable_functions.insert(fn_name.clone());
        }
    }

    pub(super) fn run_inline_candidate_pass(&mut self, program: &Program) {
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
                    body: Some(body),
                    attributes,
                    ..
                } => {
                    self.maybe_add_inline_candidate(name, body, attributes, item.span);
                }
                ItemKind::Impl {
                    for_ty, methods, ..
                } => {
                    let type_name = crate::semantic::declare::type_kind_base_name(&for_ty.node);
                    for method in methods {
                        if let ItemKind::Fn {
                            name,
                            body: Some(body),
                            attributes,
                            ..
                        } = &method.node
                        {
                            let mangled = format!("{}.{}", type_name, name);
                            self.maybe_add_inline_candidate(
                                &mangled,
                                body,
                                attributes,
                                method.span,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn maybe_add_inline_candidate(
        &mut self,
        name: &str,
        body: &Block,
        attributes: &[Attribute],
        span: Span,
    ) {
        if name == "main" {
            return;
        }

        if attributes
            .iter()
            .any(|a| matches!(a.name.as_str(), "syscall" | "api"))
        {
            return;
        }

        let inline_hint = attributes.iter().any(|a| a.name == "inline");
        if self.is_recursive(name) {
            return;
        }

        let is_small = self.is_small_inline_body(body);
        let (is_hot, call_count, called_from_main) = self.is_hot_call_target(name);

        if !inline_hint && (!is_small || !is_hot) {
            return;
        }
        let reason = if inline_hint {
            "inline attribute".to_string()
        } else {
            let mut parts = Vec::new();
            parts.push("small body".to_string());
            if called_from_main {
                parts.push("direct main call".to_string());
            } else {
                parts.push(format!(
                    "{} call{}",
                    call_count,
                    if call_count == 1 { "" } else { "s" }
                ));
            }
            parts.join(", ")
        };

        let candidate = InlineCandidate {
            name: name.to_string(),
            span,
            reason,
        };
        self.inline_candidates.push(candidate.clone());
    }

    fn is_small_inline_body(&self, body: &Block) -> bool {
        if body.stmts.len() > 2 {
            return false;
        }

        !body.stmts.iter().any(|stmt| {
            match &stmt.node {
                StmtKind::If { .. }
                | StmtKind::For { .. }
                | StmtKind::UnsafeBlock { .. }
                | StmtKind::CfgBlock { .. } => true,
                // match expressions compile to conditional jumps that are not
                // remapped by the inline pass — exclude them from inlining.
                StmtKind::Return(Some(expr)) | StmtKind::ExprStmt(expr) => {
                    matches!(expr.node, ExprKind::Match { .. })
                }
                _ => false,
            }
        })
    }

    fn is_recursive(&self, name: &str) -> bool {
        let mut stack = vec![name.to_string()];
        let mut visited: HashSet<String> = HashSet::new();

        while let Some(current) = stack.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            for (kind, from, to) in &self.dependency_edges {
                if *kind != DependencyKind::Call || from != &current {
                    continue;
                }
                if to == name {
                    return true;
                }
                stack.push(to.clone());
            }
        }

        false
    }

    fn is_hot_call_target(&self, name: &str) -> (bool, usize, bool) {
        let call_count = self.call_counts.get(name).copied().unwrap_or(0);
        let called_from_main = self
            .dependency_edges
            .iter()
            .any(|(kind, from, to)| *kind == DependencyKind::Call && from == "main" && to == name);
        let hot = call_count >= 1;
        (hot, call_count, called_from_main)
    }

    pub(super) fn run_import_optimization_pass(&mut self) {
        // Unused imports are already reported per-import via W03 warnings.
        // No additional aggregate suggestion needed.
    }

    pub(super) fn run_exhaustiveness_pass(&mut self) {
        let candidates = std::mem::take(&mut self.match_candidates);

        for candidate in candidates {
            let Some(TypeKind::Named {
                name: scrutinee_enum,
                ..
            }) = candidate.scrutinee_ty
            else {
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
                        if arm.has_guard {
                            // Guarded wildcard does not count as full coverage —
                            // the guard may fail, so later arms are still reachable.
                        } else if wildcard_seen {
                            self.push_warning(
                                arm.span,
                                "W05",
                                "unreachable wildcard match arm".to_string(),
                            );
                        }
                        if !arm.has_guard {
                            wildcard_seen = true;
                        }
                    }
                    MatchArmKindInfo::Variant { enum_name, variant } => {
                        let already_covered = wildcard_seen || covered.contains(variant.as_str());
                        if already_covered && !arm.has_guard {
                            self.push_warning(
                                arm.span,
                                "W05",
                                format!("unreachable match arm '{}', already covered", variant),
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

                        if !arm.has_guard && !covered.insert(variant.clone()) {
                            self.push_warning(
                                arm.span,
                                "W05",
                                format!(
                                    "duplicate/unreachable match arm '{}.{}'",
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
