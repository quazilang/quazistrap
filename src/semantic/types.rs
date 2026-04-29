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
}

#[derive(Debug, Clone)]
pub struct SemanticError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for SemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} at {}:{} [{}..{}]",
            self.message, self.span.line, self.span.col, self.span.start, self.span.end
        )
    }
}

#[derive(Debug, Clone)]
pub struct SemanticWarning {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for SemanticWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} at {}:{} [{}..{}]",
            self.message, self.span.line, self.span.col, self.span.start, self.span.end
        )
    }
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
}
