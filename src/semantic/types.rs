// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::HashMap;

use crate::parser::ast::{Span, TypeKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Variable { mutable: bool },
    Parameter,
    TypeName,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
}

impl std::fmt::Display for ConstValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConstValue::Int(v) => write!(f, "{}", v),
            ConstValue::Float(v) => write!(f, "{}", v),
            ConstValue::String(v) => write!(f, "\"{}\"", v),
            ConstValue::Bool(v) => write!(f, "{}", v),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub ty: Option<TypeKind>,
    pub span: Span,
    pub params: Vec<TypeKind>,
    pub used: bool,
    pub initialized: bool,
    pub is_import: bool,
    pub import_path: Option<String>,
    pub const_value: Option<ConstValue>,
    pub variadic: bool,
    pub attributes: Vec<String>,
    pub public: bool,
}

#[derive(Debug, Clone)]
pub struct SemanticError {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
}

impl SemanticError {
    pub fn render(&self, source: &str) -> String {
        render_diag("error", self.code, &self.message, self.span, source)
    }
}

impl std::fmt::Display for SemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error[{}]: {} at {}:{}",
            self.code, self.message, self.span.line, self.span.col
        )
    }
}

#[derive(Debug, Clone)]
pub struct SemanticWarning {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
    pub suggestions: Vec<String>,
}

impl SemanticWarning {
    pub fn render(&self, source: &str) -> String {
        let mut out = render_diag("warning", self.code, &self.message, self.span, source);
        for s in &self.suggestions {
            out.push_str(&format!("\n  \x1b[2m=\x1b[0m \x1b[36m{}\x1b[0m", s));
        }
        out
    }
}

impl std::fmt::Display for SemanticWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "warning[{}]: {} at {}:{}",
            self.code, self.message, self.span.line, self.span.col
        )
    }
}

fn render_diag(label: &str, code: &str, message: &str, span: Span, source: &str) -> String {
    let (lc, cc) = match label {
        "error" => ("\x1b[1;31m", "\x1b[1;31m"),
        "warning" => ("\x1b[1;33m", "\x1b[1;33m"),
        _ => ("\x1b[1m", "\x1b[1m"),
    };
    let mut out = format!(
        "{lc}{label}\x1b[0m\x1b[1m[{code}]\x1b[0m: {message}\n  \x1b[1;34m-->\x1b[0m {line}:{col}",
        line = span.line,
        col = span.col
    );
    if let Some(line_text) = source.lines().nth(span.line.saturating_sub(1)) {
        let lnum = span.line.to_string();
        let w = lnum.len();
        let blank = " ".repeat(w);
        let caret_off = span.col.saturating_sub(1);
        let caret_w = (span.end.saturating_sub(span.start)).max(1);
        out.push_str(&format!("\n{blank} \x1b[1;34m|\x1b[0m"));
        out.push_str(&format!(
            "\n\x1b[1;34m{lnum}\x1b[0m \x1b[1;34m|\x1b[0m {line_text}"
        ));
        out.push_str(&format!(
            "\n{blank} \x1b[1;34m|\x1b[0m {}{cc}{}\x1b[0m",
            " ".repeat(caret_off),
            "^".repeat(caret_w)
        ));
    }
    out
}

#[derive(Debug, Clone)]
pub struct SemanticSuggestion {
    pub message: String,
    pub span: Option<Span>,
}

#[derive(Debug, Clone)]
pub struct ExprAnnotation {
    pub span: Span,
    pub ty: Option<TypeKind>,
    pub const_value: Option<ConstValue>,
    pub reachable: bool,
}

#[derive(Debug, Clone)]
pub struct ConstantEvaluation {
    pub span: Span,
    pub value: ConstValue,
}

#[derive(Debug, Clone)]
pub struct InlineCandidate {
    pub name: String,
    pub span: Span,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct ImportInfo {
    pub local_name: String,
    pub full_path: String,
    pub used: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct MathOptimization {
    pub span: Span,
    pub description: String,
    pub result_value: Option<ConstValue>,
}

#[derive(Debug, Clone)]
pub struct LazyImportHint {
    pub import_span: Span,
    pub broad_path: String,
    pub accessed_subpaths: Vec<String>,
    pub suggested_imports: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AnnotatedProgram {
    pub span: Option<Span>,
    pub item_count: usize,
    pub expr_annotations: Vec<ExprAnnotation>,
    pub constant_evaluations: Vec<ConstantEvaluation>,
}

#[derive(Debug, Clone)]
pub struct SymbolTableEntry {
    pub scope_depth: usize,
    pub name: String,
    pub symbol: Symbol,
}

#[derive(Debug, Clone, Default)]
pub struct SymbolTable {
    pub entries: Vec<SymbolTableEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct OptimizationHints {
    pub constant_evaluations: Vec<ConstantEvaluation>,
    pub inline_candidates: Vec<InlineCandidate>,
    pub removable_imports: Vec<String>,
    pub exhaustiveness_checks: usize,
    pub math_optimizations: Vec<MathOptimization>,
    pub lazy_import_hints: Vec<LazyImportHint>,
    /// Functions that are defined and called, but not reachable from `main`.
    pub dead_functions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DependencyKind {
    Import,
    Call,
}

#[derive(Debug, Clone)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub kind: DependencyKind,
}

#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    pub edges: Vec<DependencyEdge>,
}

#[derive(Debug, Clone)]
pub struct MatchExhaustivenessIssue {
    pub span: Span,
    pub enum_name: String,
    pub missing_variants: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SemanticReport {
    pub errors: Vec<SemanticError>,
    pub warnings: Vec<SemanticWarning>,
    pub suggestions: Vec<SemanticSuggestion>,
    pub used_imports: Vec<String>,
    pub used_imports_map: HashMap<String, ImportInfo>,
    pub unused_imports: Vec<String>,
    pub annotated_exprs: Vec<ExprAnnotation>,
    pub annotated_program: AnnotatedProgram,
    pub symbol_table: SymbolTable,
    pub constant_evaluations: Vec<ConstantEvaluation>,
    pub inline_candidates: Vec<InlineCandidate>,
    pub optimization_hints: OptimizationHints,
    pub dependency_graph: DependencyGraph,
    pub exhaustiveness_checks: usize,
    pub non_exhaustive_matches: Vec<MatchExhaustivenessIssue>,
    pub lazy_import_hints: Vec<LazyImportHint>,
    /// Functions that are defined and called, but not reachable from `main`.
    pub dead_functions: Vec<String>,
}
