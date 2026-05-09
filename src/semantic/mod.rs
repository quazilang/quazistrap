// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::{BTreeSet, HashMap};

use crate::parser::ast::*;

pub mod types;
pub use types::*;
mod borrow;
mod declare;
mod optimize;
mod typecheck;
mod unused;

// Private internal types used across sub-modules via `use super::*;`

#[derive(Debug, Clone, Default)]
pub(super) struct ExprEval {
    pub(super) ty: Option<TypeKind>,
    pub(super) const_value: Option<ConstValue>,
}

#[derive(Debug, Clone)]
pub(super) struct EnumInfo {
    pub(super) variants: HashMap<String, usize>,
}

#[derive(Debug, Clone)]
pub(super) enum MatchArmKindInfo {
    Wildcard,
    Variant {
        enum_name: Option<String>,
        variant: String,
    },
}

#[derive(Debug, Clone)]
pub(super) struct MatchArmInfo {
    pub(super) span: Span,
    pub(super) kind: MatchArmKindInfo,
}

#[derive(Debug, Clone)]
pub(super) struct MatchCandidate {
    pub(super) span: Span,
    pub(super) scrutinee_ty: Option<TypeKind>,
    pub(super) arms: Vec<MatchArmInfo>,
}

pub struct Analyzer {
    pub(super) scopes: Vec<HashMap<String, Symbol>>,
    pub(super) finished_scopes: Vec<Vec<(String, Symbol)>>,
    pub(super) errors: Vec<SemanticError>,
    pub(super) warnings: Vec<SemanticWarning>,
    pub(super) suggestions: Vec<SemanticSuggestion>,
    pub(super) used_import_paths: BTreeSet<String>,
    pub(super) unused_import_paths: BTreeSet<String>,
    pub(super) annotated_exprs: Vec<ExprAnnotation>,
    pub(super) constant_evaluations: Vec<ConstantEvaluation>,
    pub(super) inline_candidates: Vec<InlineCandidate>,
    pub(super) enums: HashMap<String, EnumInfo>,
    pub(super) match_candidates: Vec<MatchCandidate>,
    pub(super) non_exhaustive_matches: Vec<MatchExhaustivenessIssue>,
    pub(super) exhaustiveness_checks: usize,
    pub(super) dependency_edges: BTreeSet<(DependencyKind, String, String)>,
    pub(super) call_counts: HashMap<String, usize>,
    pub(super) current_function: Vec<String>,
    pub(super) math_optimizations: Vec<MathOptimization>,
    pub(super) lazy_import_accesses: HashMap<String, BTreeSet<String>>,
    pub(super) lazy_import_hints: Vec<LazyImportHint>,
    /// Functions not reachable from `main` via the call graph.
    pub(super) unreachable_functions: BTreeSet<String>,
    /// Nesting depth of `unsafe` blocks/functions (0 = safe context).
    pub(super) unsafe_depth: usize,
    /// Function names that live in library (dependency) files.
    /// Set once from LoadResult; not reset between analyses.
    pub(super) library_fn_names: std::collections::HashSet<String>,
    /// Symbols declared by library files that are not part of the parsed user program.
    /// Used by tooling paths such as the LSP, where open-buffer analysis should not
    /// merge dependency source and shift user spans.
    pub(super) library_symbols: Vec<(String, Symbol)>,
    /// Function names explicitly imported by leaf name (e.g. `import std.core.write`).
    /// Reset at the start of each analysis; populated during the declare pass.
    pub(super) explicitly_imported_fns: std::collections::HashSet<String>,
}

pub(super) fn unwrap_type(ty: &Type) -> TypeKind {
    ty.node.clone()
}

pub(super) fn extract_attribute_names(attributes: &[Attribute]) -> Vec<String> {
    attributes.iter().map(|a| a.name.clone()).collect()
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            finished_scopes: Vec::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            suggestions: Vec::new(),
            used_import_paths: BTreeSet::new(),
            unused_import_paths: BTreeSet::new(),
            annotated_exprs: Vec::new(),
            constant_evaluations: Vec::new(),
            inline_candidates: Vec::new(),
            enums: HashMap::new(),
            match_candidates: Vec::new(),
            non_exhaustive_matches: Vec::new(),
            exhaustiveness_checks: 0,
            dependency_edges: BTreeSet::new(),
            call_counts: HashMap::new(),
            current_function: Vec::new(),
            math_optimizations: Vec::new(),
            lazy_import_accesses: HashMap::new(),
            lazy_import_hints: Vec::new(),
            unreachable_functions: BTreeSet::new(),
            unsafe_depth: 0,
            library_fn_names: std::collections::HashSet::new(),
            library_symbols: Vec::new(),
            explicitly_imported_fns: std::collections::HashSet::new(),
        }
    }

    pub fn set_library_fns(&mut self, names: std::collections::HashSet<String>) {
        self.library_fn_names = names;
    }

    pub fn set_library_symbols(&mut self, symbols: Vec<(String, Symbol)>) {
        self.library_symbols = symbols;
    }

    pub fn analyze_program(&mut self, program: &Program) -> SemanticReport {
        self.reset_state();

        // Pass 1: gather top-level declarations and imports.
        for item in &program.items {
            self.declare_top_level_item(item);
        }

        // Pass 2: type checking + usage tracking + initialization checks + annotations.
        for item in &program.items {
            self.type_check_item(item);
        }

        // Pass 3: unused symbol/import analysis.
        self.run_unused_pass();

        // Pass 4: dead code detection (reachability).
        self.run_dead_code_pass(program);

        // Pass 5: tree-shaking — find functions not reachable from main.
        self.run_tree_shake_pass(program);

        // Pass 6: optimization hints.
        self.run_inline_candidate_pass(program);
        self.run_exhaustiveness_pass();
        self.run_import_optimization_pass();
        self.run_lazy_import_pass();

        // Pass 7: borrow / move checker.
        self.run_borrow_check_pass(program);

        let symbol_table = self.build_symbol_table();
        let used_imports_vec: Vec<String> = self.used_import_paths.iter().cloned().collect();
        let used_imports_map = self.build_import_usage_map(&symbol_table, true);
        let unused_imports_vec: Vec<String> = self.unused_import_paths.iter().cloned().collect();

        let annotated_exprs = std::mem::take(&mut self.annotated_exprs);
        let constant_evaluations = std::mem::take(&mut self.constant_evaluations);
        let inline_candidates = std::mem::take(&mut self.inline_candidates);
        let math_optimizations = std::mem::take(&mut self.math_optimizations);
        let lazy_import_hints = std::mem::take(&mut self.lazy_import_hints);
        let dead_functions: Vec<String> = self.unreachable_functions.iter().cloned().collect();

        let annotated_program = AnnotatedProgram {
            span: program.span,
            item_count: program.items.len(),
            expr_annotations: annotated_exprs.clone(),
            constant_evaluations: constant_evaluations.clone(),
        };

        let optimization_hints = OptimizationHints {
            constant_evaluations: constant_evaluations.clone(),
            inline_candidates: inline_candidates.clone(),
            removable_imports: unused_imports_vec.clone(),
            exhaustiveness_checks: self.exhaustiveness_checks,
            math_optimizations: math_optimizations.clone(),
            lazy_import_hints: lazy_import_hints.clone(),
            dead_functions: dead_functions.clone(),
        };

        let dependency_graph = self.build_dependency_graph();

        SemanticReport {
            errors: std::mem::take(&mut self.errors),
            warnings: std::mem::take(&mut self.warnings),
            suggestions: std::mem::take(&mut self.suggestions),
            used_imports: used_imports_vec,
            used_imports_map,
            unused_imports: unused_imports_vec,
            annotated_exprs,
            annotated_program,
            symbol_table,
            constant_evaluations,
            inline_candidates,
            optimization_hints,
            dependency_graph,
            exhaustiveness_checks: self.exhaustiveness_checks,
            non_exhaustive_matches: std::mem::take(&mut self.non_exhaustive_matches),
            lazy_import_hints,
            dead_functions,
        }
    }

    fn reset_state(&mut self) {
        self.scopes.clear();
        self.scopes.push(HashMap::new());
        self.finished_scopes.clear();

        self.errors.clear();
        self.warnings.clear();
        self.suggestions.clear();
        self.used_import_paths.clear();
        self.unused_import_paths.clear();
        self.annotated_exprs.clear();
        self.constant_evaluations.clear();
        self.inline_candidates.clear();
        self.enums.clear();
        self.match_candidates.clear();
        self.non_exhaustive_matches.clear();
        self.exhaustiveness_checks = 0;
        self.dependency_edges.clear();
        self.call_counts.clear();
        self.current_function.clear();
        self.math_optimizations.clear();
        self.lazy_import_accesses.clear();
        self.lazy_import_hints.clear();
        self.unreachable_functions.clear();
        self.unsafe_depth = 0;
        self.explicitly_imported_fns.clear();
        self.init_builtins();
        self.init_library_symbols();
    }

    fn init_builtins(&mut self) {
        let span = Span {
            line: 0,
            col: 0,
            start: 0,
            end: 0,
        };

        let mut option_variants = HashMap::new();
        option_variants.insert("Some".to_string(), 1usize);
        option_variants.insert("None".to_string(), 0usize);
        self.enums.insert(
            "Option".to_string(),
            EnumInfo {
                variants: option_variants,
            },
        );

        let mut result_variants = HashMap::new();
        result_variants.insert("Ok".to_string(), 1usize);
        result_variants.insert("Err".to_string(), 1usize);
        self.enums.insert(
            "Result".to_string(),
            EnumInfo {
                variants: result_variants,
            },
        );

        for type_name in &["Option", "Result"] {
            self.declare(
                type_name.to_string(),
                Symbol {
                    kind: SymbolKind::TypeName,
                    span,
                    ty: None,
                    params: vec![],
                    used: true,
                    initialized: true,
                    is_import: false,
                    import_path: None,
                    const_value: None,
                    variadic: false,
                    attributes: Vec::new(),
                    public: true,
                },
            );
        }

        for ctor in &["Some", "Ok", "Err"] {
            self.declare(
                ctor.to_string(),
                Symbol {
                    kind: SymbolKind::Function,
                    span,
                    ty: Some(TypeKind::Any),
                    params: vec![TypeKind::Any],
                    used: true,
                    initialized: true,
                    is_import: false,
                    import_path: None,
                    const_value: None,
                    variadic: false,
                    attributes: Vec::new(),
                    public: true,
                },
            );
        }

        self.declare(
            "None".to_string(),
            Symbol {
                kind: SymbolKind::Function,
                span,
                ty: Some(TypeKind::Any),
                params: vec![],
                used: true,
                initialized: true,
                is_import: false,
                import_path: None,
                const_value: None,
                variadic: false,
                attributes: Vec::new(),
                public: true,
            },
        );
    }

    fn init_library_symbols(&mut self) {
        for (name, symbol) in self.library_symbols.clone() {
            if self.resolve_symbol(&name).is_none() {
                self.declare(name, symbol);
            }
        }
    }

    pub(super) fn add_dependency_edge(&mut self, kind: DependencyKind, from: &str, to: &str) {
        self.dependency_edges
            .insert((kind, from.to_string(), to.to_string()));
    }

    fn build_symbol_table(&self) -> SymbolTable {
        let mut entries = Vec::new();

        if let Some(global_scope) = self.scopes.first() {
            for (name, symbol) in global_scope {
                entries.push(SymbolTableEntry {
                    scope_depth: 0,
                    name: name.clone(),
                    symbol: symbol.clone(),
                });
            }
        }

        for (idx, scope) in self.finished_scopes.iter().enumerate() {
            for (name, symbol) in scope {
                entries.push(SymbolTableEntry {
                    scope_depth: idx + 1,
                    name: name.clone(),
                    symbol: symbol.clone(),
                });
            }
        }

        entries.sort_by(|a, b| {
            a.scope_depth
                .cmp(&b.scope_depth)
                .then_with(|| a.name.cmp(&b.name))
        });

        SymbolTable { entries }
    }

    fn build_import_usage_map(
        &self,
        symbol_table: &SymbolTable,
        include_only_used: bool,
    ) -> HashMap<String, ImportInfo> {
        let mut map = HashMap::new();

        for entry in &symbol_table.entries {
            if !entry.symbol.is_import {
                continue;
            }

            if include_only_used && !entry.symbol.used {
                continue;
            }

            let full_path = entry
                .symbol
                .import_path
                .clone()
                .unwrap_or_else(|| entry.name.clone());

            map.insert(
                full_path.clone(),
                ImportInfo {
                    local_name: entry.name.clone(),
                    full_path,
                    used: entry.symbol.used,
                    span: entry.symbol.span,
                },
            );
        }

        map
    }

    fn build_dependency_graph(&self) -> DependencyGraph {
        let edges = self
            .dependency_edges
            .iter()
            .map(|(kind, from, to)| DependencyEdge {
                from: from.clone(),
                to: to.clone(),
                kind: *kind,
            })
            .collect();

        DependencyGraph { edges }
    }

    pub(super) fn declare(&mut self, name: String, symbol: Symbol) {
        let existing = self
            .scopes
            .last()
            .expect("semantic analyzer must always have at least one scope")
            .get(&name)
            .cloned();

        if let Some(prev) = existing {
            if symbol.is_import && prev.is_import {
                self.push_error(
                    symbol.span,
                    "S05",
                    format!(
                        "import name conflict for '{}' (previous import at {}:{} [{}..{}])",
                        name, prev.span.line, prev.span.col, prev.span.start, prev.span.end
                    ),
                );
                return;
            }

            self.push_error(
                symbol.span,
                "S05",
                format!(
                    "duplicate declaration '{}' (previous at {}:{} [{}..{}])",
                    name, prev.span.line, prev.span.col, prev.span.start, prev.span.end
                ),
            );
            return;
        }

        let current_scope = self
            .scopes
            .last_mut()
            .expect("semantic analyzer must always have at least one scope");
        current_scope.insert(name, symbol);
    }

    pub(super) fn resolve_symbol(&self, name: &str) -> Option<Symbol> {
        for scope in self.scopes.iter().rev() {
            if let Some(symbol) = scope.get(name) {
                return Some(symbol.clone());
            }
        }
        None
    }

    pub(super) fn resolve_for_read(&mut self, name: &str) -> Option<Symbol> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(symbol) = scope.get_mut(name) {
                symbol.used = true;
                if symbol.is_import {
                    if let Some(path) = &symbol.import_path {
                        self.used_import_paths.insert(path.clone());
                    }
                }
                return Some(symbol.clone());
            }
        }
        None
    }

    pub(super) fn mark_initialized(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(symbol) = scope.get_mut(name) {
                symbol.initialized = true;
                return;
            }
        }
    }

    pub(super) fn set_symbol_const_value(&mut self, name: &str, value: Option<ConstValue>) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(symbol) = scope.get_mut(name) {
                symbol.const_value = value;
                return;
            }
        }
    }

    pub(super) fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub(super) fn exit_scope_collect(&mut self) {
        let scope = self
            .scopes
            .pop()
            .expect("semantic analyzer must always have at least one scope");
        self.finished_scopes.push(scope.into_iter().collect());
    }

    pub(super) fn push_error(&mut self, span: Span, code: &'static str, message: String) {
        self.errors.push(SemanticError {
            code,
            message,
            span,
        });
    }

    pub(super) fn push_warning(&mut self, span: Span, code: &'static str, message: String) {
        self.warnings.push(SemanticWarning {
            code,
            message,
            span,
            suggestions: Vec::new(),
        });
    }

    pub(super) fn push_warning_with_suggestion(
        &mut self,
        span: Span,
        code: &'static str,
        message: String,
        suggestion: String,
    ) {
        self.warnings.push(SemanticWarning {
            code,
            message,
            span,
            suggestions: vec![suggestion],
        });
    }

    pub(super) fn push_suggestion(&mut self, span: Option<Span>, message: String) {
        self.suggestions.push(SemanticSuggestion { message, span });
    }
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    use super::{Analyzer, ConstValue, DependencyKind, SemanticReport};

    fn parse_program(src: &str) -> crate::parser::ast::Program {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        parser.parse().expect("source must parse")
    }

    fn analyze(src: &str) -> SemanticReport {
        let program = parse_program(src);
        let mut analyzer = Analyzer::new();
        analyzer.analyze_program(&program)
    }

    #[test]
    fn reports_type_mismatch_in_const() {
        let report = analyze(
            r#"
fn main() void {
    const x: i32 = "";
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("type mismatch"))
        );
    }

    #[test]
    fn reports_type_mismatch_in_var() {
        let report = analyze(
            r#"
fn main() void {
    var x: bool = 123;
}
    "#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("type mismatch"))
        );
    }

    #[test]
    fn reports_readable_type_names_in_errors() {
        let report = analyze(
            r#"
fn main() void {
    const x: i32 = "";
}
"#,
        );

        let combined = report
            .errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(combined.contains("declared i32"));
        assert!(combined.contains("got &str"));
        assert!(!combined.contains("Int32"));
        assert!(!combined.contains("Str"));
    }

    #[test]
    fn reports_unknown_identifier() {
        let report = analyze(
            r#"
fn main() void {
    x = 1;
}
"#,
        );

        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("unknown identifier 'x'"))
        );
    }

    #[test]
    fn reports_duplicate_local_declaration() {
        let report = analyze(
            r#"
fn main() void {
    var x: i32 = 1;
    var x: i32 = 2;
}
"#,
        );

        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("duplicate declaration 'x'"))
        );
    }

    #[test]
    fn warns_for_unused_import_with_dot_path() {
        let report = analyze(
            r#"
import std.io.stdout;

fn main() void {
    const x: i32 = 1;
}
"#,
        );

        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.message.contains("unused import 'stdout'")
                    && w.message.contains("std.io.stdout"))
        );
        assert!(report.unused_imports.contains(&"std.io.stdout".to_string()));
    }

    #[test]
    fn tracks_used_imports_with_dot_path() {
        let report = analyze(
            r#"
import std.io.stdout;

fn main() void {
    stdout.println("ok");
}
"#,
        );

        assert!(report.used_imports.contains(&"std.io.stdout".to_string()));
    }

    #[test]
    fn module_qualified_call_marks_import_used() {
        let report = analyze(
            r#"
import mymod;

fn foo(x: i32) i32 {
    ret x;
}

fn main() void {
    const y: i32 = mymod.foo(1);
}
"#,
        );

        assert!(report.errors.is_empty());
        assert!(report.used_imports.contains(&"mymod".to_string()));
    }

    #[test]
    fn reports_use_before_initialization() {
        let report = analyze(
            r#"
fn main() void {
    var x: i32;
    const y: i32 = x;
}
"#,
        );

        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("before initialization"))
        );
    }

    #[test]
    fn warns_about_unreachable_code_after_return() {
        let report = analyze(
            r#"
fn main() void {
    ret;
    var x: i32 = 1;
}
"#,
        );

        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.message.contains("unreachable code"))
        );
    }

    #[test]
    fn warns_about_unreachable_after_if_else_both_return() {
        let report = analyze(
            r#"
fn main() void {
    if (true) {
        ret;
    } else {
        ret;
    }
    var x: i32 = 1;
}
"#,
        );

        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.message.contains("unreachable code"))
        );
    }

    #[test]
    fn warns_about_unused_local_variable() {
        let report = analyze(
            r#"
fn main() void {
    var x: i32 = 1;
}
"#,
        );

        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.message.contains("unused variable 'x'"))
        );
    }

    #[test]
    fn warns_about_unused_function() {
        let report = analyze(
            r#"
fn helper() void {
    ret;
}

fn main() void {
    ret;
}
"#,
        );

        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.message.contains("unused function 'helper'"))
        );
    }

    #[test]
    fn records_expression_annotations_and_const_eval() {
        let report = analyze(
            r#"
fn main() void {
    const x: i32 = 1 + 2;
}
"#,
        );

        assert!(!report.annotated_exprs.is_empty());
        assert!(
            report
                .constant_evaluations
                .iter()
                .any(|entry| entry.value == ConstValue::Int(3))
        );
    }

    #[test]
    fn detects_inline_candidates() {
        let report = analyze(
            r#"
fn helper(a: i32) i32 {
    ret a;
}

fn main() void {
    helper(1);
        helper(2);
}
"#,
        );

        assert!(report.inline_candidates.iter().any(|c| c.name == "helper"));
    }

    #[test]
    fn skips_inline_candidates_for_recursive_function() {
        let report = analyze(
            r#"
fn recurse(x: i32) i32 {
    ret recurse(x);
}

fn main() void {
    recurse(1);
}
"#,
        );

        assert!(!report.inline_candidates.iter().any(|c| c.name == "recurse"));
    }

    #[test]
    fn reports_non_exhaustive_match_for_enum() {
        let report = analyze(
            r#"
enum Option[T] {
    Some(T),
    None,
}

fn unwrap_or_fail(x: Option[i32]) i32 {
    ret match x {
        Some(v) => v,
    };
}
"#,
        );

        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("non-exhaustive match"))
        );
        assert_eq!(report.exhaustiveness_checks, 1);
        assert_eq!(report.non_exhaustive_matches.len(), 1);
    }

    #[test]
    fn accepts_exhaustive_match_for_enum() {
        let report = analyze(
            r#"
enum Option[T] {
    Some(T),
    None,
}

fn unwrap_or_zero(x: Option[i32]) i32 {
    ret match x {
        Some(v) => v,
        None => 0,
    };
}
"#,
        );

        assert!(
            !report
                .errors
                .iter()
                .any(|e| e.message.contains("non-exhaustive match"))
        );
        assert_eq!(report.exhaustiveness_checks, 1);
    }

    #[test]
    fn warns_on_duplicate_match_arm() {
        let report = analyze(
            r#"
enum Color {
    Red,
    Blue,
}

fn color_value(c: Color) i32 {
    ret match c {
        Red => 1,
        Red => 2,
        Blue => 3,
    };
}
"#,
        );

        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.message.contains("duplicate/unreachable match arm"))
        );
    }

    #[test]
    fn exposes_structured_semantic_output_sections() {
        let report = analyze(
            r#"
import std.io.stdout;

fn helper(a: i32) i32 {
    ret a + 1;
}

fn main() void {
    const y: i32 = helper(41);
    stdout.println("{}", y);
}
"#,
        );

        assert_eq!(report.annotated_program.item_count, 3);
        assert_eq!(
            report.annotated_program.expr_annotations.len(),
            report.annotated_exprs.len()
        );
        assert!(!report.symbol_table.entries.is_empty());
        assert!(
            report
                .symbol_table
                .entries
                .iter()
                .any(|e| e.name == "helper")
        );

        let import = report
            .used_imports_map
            .get("std.io.stdout")
            .expect("used import map should contain stdout import");
        assert_eq!(import.local_name, "stdout");
        assert!(import.used);

        assert!(
            report
                .optimization_hints
                .inline_candidates
                .iter()
                .any(|c| c.name == "helper")
        );
        assert!(
            report
                .dependency_graph
                .edges
                .iter()
                .any(|edge| edge.kind == DependencyKind::Import
                    && edge.from == "__program__"
                    && edge.to == "std.io.stdout")
        );
        assert!(report.dependency_graph.edges.iter().any(|edge| {
            edge.kind == DependencyKind::Call && edge.from == "main" && edge.to == "helper"
        }));
    }

    #[test]
    fn reports_import_name_conflicts_explicitly() {
        let report = analyze(
            r#"
import std.io.stdout;
import std.net.stdout;

fn main() void {
    ret;
}
"#,
        );

        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("import name conflict for 'stdout'"))
        );
    }

    #[test]
    fn builtin_option_some_and_none_resolve_without_declaration() {
        let report = analyze(
            r#"
fn wrap(x: i32) Option[i32] {
    ret Some(x);
}

fn empty() Option[i32] {
    ret None();
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "unexpected errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn builtin_none_bare_ident_resolves() {
        let report = analyze(
            r#"
fn get_none() Option[i32] {
    const n = None;
    ret n;
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "unexpected errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn builtin_result_ok_and_err_resolve_without_declaration() {
        let report = analyze(
            r#"
fn succeed(x: i32) Result[i32, str] {
    ret Ok(x);
}

fn fail(msg: str) Result[i32, str] {
    ret Err(msg);
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "unexpected errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn builtin_option_exhaustiveness_check_works() {
        let report = analyze(
            r#"
fn unwrap_or_zero(x: Option[i32]) i32 {
    ret match x {
        Some(v) => v,
        None => 0,
    };
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "unexpected errors: {:?}",
            report.errors
        );
        assert_eq!(report.exhaustiveness_checks, 1);
    }

    #[test]
    fn builtin_option_non_exhaustive_match_is_caught() {
        let report = analyze(
            r#"
fn bad_match(x: Option[i32]) i32 {
    ret match x {
        Some(v) => v,
    };
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("non-exhaustive match")),
            "expected non-exhaustive match error"
        );
    }

    #[test]
    fn builtin_result_exhaustiveness_check_works() {
        let report = analyze(
            r#"
fn handle(x: Result[i32, str]) i32 {
    ret match x {
        Ok(v) => v,
        Err(e) => 0,
    };
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "unexpected errors: {:?}",
            report.errors
        );
        assert_eq!(report.exhaustiveness_checks, 1);
    }

    #[test]
    fn redefining_builtin_type_is_an_error() {
        let report = analyze(
            r#"
enum Option[T] {
    Some(T),
    None,
}

fn main() void {}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("duplicate declaration 'Option'")),
            "expected duplicate declaration error for Option"
        );
    }

    #[test]
    fn compound_assign_ops_are_valid() {
        let report = analyze(
            r#"
fn main() void {
    var x: i32 = 0;
    x += 1;
    x -= 1;
    x *= 2;
    x /= 2;
    x %= 3;
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "unexpected errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn compound_assign_on_const_is_error() {
        let report = analyze(
            r#"
fn main() void {
    const x: i32 = 1;
    x += 1;
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("cannot assign to const")),
            "expected const assign error"
        );
    }

    #[test]
    fn postfix_increment_is_valid() {
        let report = analyze(
            r#"
fn main() void {
    var x: i32 = 0;
    x++;
    x--;
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "unexpected errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn prefix_increment_is_valid() {
        let report = analyze(
            r#"
fn main() void {
    var x: i32 = 0;
    ++x;
    --x;
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "unexpected errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn inc_dec_on_bool_is_error() {
        let report = analyze(
            r#"
fn main() void {
    var flag: bool = false;
    flag++;
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("++ / -- not valid")),
            "expected inc/dec type error"
        );
    }

    #[test]
    fn inc_dec_on_const_is_error() {
        let report = analyze(
            r#"
fn main() void {
    const x: i32 = 1;
    x++;
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("cannot assign to const")),
            "expected const assign error"
        );
    }

    #[test]
    fn math_absorber_mul_zero_folds_to_zero_in_annotated_tree() {
        let report = analyze(
            r#"
fn mul(x: i32) i32 {
    ret x * 0;
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "unexpected errors: {:?}",
            report.errors
        );
        // The binary expr annotation must carry const_value = Int(0)
        assert!(
            report
                .annotated_exprs
                .iter()
                .any(|a| a.const_value == Some(ConstValue::Int(0))),
            "x * 0 should fold to Int(0) in annotated tree"
        );
        assert!(
            report
                .optimization_hints
                .math_optimizations
                .iter()
                .any(|m| m.description.contains("x * 0 = 0")),
        );
    }

    #[test]
    fn math_absorber_mul_zero_float_folds_in_annotated_tree() {
        let report = analyze(
            r#"
fn scale(x: f64) f64 {
    ret 0.0 * x;
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "unexpected errors: {:?}",
            report.errors
        );
        assert!(
            report
                .annotated_exprs
                .iter()
                .any(|a| matches!(a.const_value, Some(ConstValue::Float(f)) if f == 0.0)),
            "0.0 * x should fold to Float(0.0) in annotated tree"
        );
        assert!(
            report
                .optimization_hints
                .math_optimizations
                .iter()
                .any(|m| m.description.contains("0.0 * x = 0.0")),
        );
    }

    #[test]
    fn math_identity_add_zero_emits_suggestion() {
        let report = analyze(
            r#"
fn add(x: i32) i32 {
    ret x + 0;
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "unexpected errors: {:?}",
            report.errors
        );
        assert!(
            report
                .optimization_hints
                .math_optimizations
                .iter()
                .any(|m| m.description.contains("x + 0 = x")),
            "x + 0 should produce identity optimization hint"
        );
        assert!(
            report
                .suggestions
                .iter()
                .any(|s| s.message.contains("x + 0 = x")),
        );
    }

    #[test]
    fn lazy_import_broad_import_suggests_narrower_path() {
        let report = analyze(
            r#"
import std;

fn main() void {
    std.io.stdout.println("hello");
}
"#,
        );
        assert!(
            report.lazy_import_hints.iter().any(|h| {
                h.broad_path == "std" && h.accessed_subpaths.iter().any(|p| p == "std.io.stdout")
            }),
            "expected lazy import hint for std -> std.io.stdout, got {:?}",
            report.lazy_import_hints
        );
        assert!(
            report
                .optimization_hints
                .lazy_import_hints
                .iter()
                .any(|h| h.broad_path == "std"),
        );
    }

    #[test]
    fn lazy_import_exact_import_produces_no_hint() {
        let report = analyze(
            r#"
import std.io.stdout;

fn main() void {
    stdout.println("hello");
}
"#,
        );
        // stdout is the leaf symbol — no deeper sub-path accessed, no hint expected
        assert!(
            report.lazy_import_hints.is_empty(),
            "exact import should not produce a lazy import hint"
        );
    }

    #[test]
    fn dead_function_in_call_chain_is_warned() {
        // helper1 is unused (no callers), helper2 is called only by helper1.
        // helper2 should appear in dead_functions because it's only reachable from dead code.
        let report = analyze(
            r#"
fn helper2(x: i32) i32 {
    ret x;
}

fn helper1() void {
    helper2(1);
}

fn main() void {
    ret;
}
"#,
        );
        // helper1: unused function (directly uncalled)
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.message.contains("unused function 'helper1'")),
            "expected unused function warning for helper1"
        );
        // helper2: dead function (called only from dead code)
        assert!(
            report.dead_functions.contains(&"helper2".to_string()),
            "helper2 should be in dead_functions, got {:?}",
            report.dead_functions
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.message.contains("dead function 'helper2'")),
            "expected dead function warning for helper2"
        );
        // main is reachable
        assert!(!report.dead_functions.contains(&"main".to_string()));
    }

    #[test]
    fn reachable_functions_not_in_dead_set() {
        let report = analyze(
            r#"
fn helper(x: i32) i32 {
    ret x + 1;
}

fn main() void {
    helper(1);
}
"#,
        );
        assert!(
            report.dead_functions.is_empty(),
            "helper is reachable, should not be dead"
        );
        assert!(
            report.errors.is_empty(),
            "unexpected errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn for_range_bounds_must_be_integers() {
        let report = analyze(
            r#"
fn main() void {
    for var i : "hello"..10 {
        ret;
    }
}
"#,
        );
        assert!(
            report.errors.iter().any(|e| e
                .message
                .contains("for range start must be an integer type")),
            "expected integer type error for str range start, got: {:?}",
            report.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn for_range_loop_var_has_integer_type() {
        let report = analyze(
            r#"
fn main() void {
    for var i : 0..10 {
        var s: str = i;
    }
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("type mismatch")),
            "for loop var typed as i32 — assigning to str should error"
        );
    }

    // ── Borrow checker tests ──────────────────────────────────────────────────

    #[test]
    fn use_after_move_is_error() {
        let report = analyze(
            r#"
struct Point { x: i32, y: i32, }

fn consume(p: Point) i32 { ret p.x; }

fn main() void {
    var p: Point;
    consume(p);
    consume(p);
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.code == "S10" && e.message.contains("use of moved value 'p'")),
            "expected use-after-move error, got: {:?}",
            report.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn copy_type_not_moved() {
        let report = analyze(
            r#"
fn double(x: i32) i32 { ret x + x; }

fn main() void {
    var x: i32 = 5;
    double(x);
    double(x);
}
"#,
        );
        let borrow_errors: Vec<_> = report.errors.iter().filter(|e| e.code == "S10").collect();
        assert!(
            borrow_errors.is_empty(),
            "i32 is Copy — should not produce S10 errors: {:?}",
            borrow_errors
        );
    }

    #[test]
    fn reassign_clears_moved_state() {
        let report = analyze(
            r#"
struct Box { val: i32, }

fn consume(b: Box) i32 { ret b.val; }

fn make() Box { var b: Box; ret b; }

fn main() void {
    var b: Box;
    consume(b);
    b = make();
    consume(b);
}
"#,
        );
        let borrow_errors: Vec<_> = report.errors.iter().filter(|e| e.code == "S10").collect();
        assert!(
            borrow_errors.is_empty(),
            "reassign should clear moved state: {:?}",
            borrow_errors
        );
    }

    #[test]
    fn move_in_loop_is_error() {
        let report = analyze(
            r#"
struct Obj { id: i32, }

fn consume(o: Obj) void { ret; }

fn main() void {
    var o: Obj;
    var i: i32 = 0;
    for i < 3 {
        consume(o);
        i += 1;
    }
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.code == "S10" && e.message.contains("cannot move 'o' inside a loop")),
            "expected move-in-loop error, got: {:?}",
            report.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn move_in_if_branch_conservatively_blocks_later_use() {
        let report = analyze(
            r#"
struct Val { n: i32, }

fn consume(v: Val) void { ret; }

fn main() void {
    var v: Val;
    var cond: bool = 1 == 1;
    if (cond) {
        consume(v);
    }
    consume(v);
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.code == "S10" && e.message.contains("use of moved value 'v'")),
            "conservative branch merge: move in if-branch should block post-if use, got: {:?}",
            report.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn main_may_return_never() {
        let report = analyze(
            r#"
fn main() ! {
}
"#,
        );
        assert!(
            !report
                .errors
                .iter()
                .any(|e| e.message.contains("main() return type must")),
            "main returning ! should be accepted, got {:?}",
            report.errors
        );
    }
}
