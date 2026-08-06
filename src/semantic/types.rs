// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::collections::HashMap;

use crate::parser::ast::{Span, TypeKind};

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub line_start: usize,
}

impl SourceFile {
    pub fn contains(&self, span: Span) -> bool {
        self.start <= span.start && span.start < self.end
    }

    pub fn line_col(&self, span: Span) -> (usize, usize) {
        (span.line.saturating_sub(self.line_start) + 1, span.col)
    }

    pub fn label(&self, span: Span) -> String {
        let (line, col) = self.line_col(span);
        format!("{}:{}:{}", self.path, line, col)
    }
}

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
    pub unsafe_fn: bool,
    pub generic_params: Vec<String>,
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

    pub fn render_with_source_files(&self, source: &str, files: &[SourceFile]) -> String {
        render_diag_with_source_files("error", self.code, &self.message, self.span, source, files)
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
            out.push_str(&format!("\n  \x1b[2mhint:\x1b[0m \x1b[36m{}\x1b[0m", s));
        }
        out
    }

    pub fn render_with_source_files(&self, source: &str, files: &[SourceFile]) -> String {
        let mut out = render_diag_with_source_files(
            "warning",
            self.code,
            &self.message,
            self.span,
            source,
            files,
        );
        for s in &self.suggestions {
            out.push_str(&format!("\n  \x1b[2mhint:\x1b[0m \x1b[36m{}\x1b[0m", s));
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
    render_diag_at(
        label,
        code,
        message,
        span,
        source,
        &format!("{}:{}", span.line, span.col),
        span.line,
    )
}

fn render_diag_with_source_files(
    label: &str,
    code: &str,
    message: &str,
    span: Span,
    source: &str,
    files: &[SourceFile],
) -> String {
    if let Some(file) = files.iter().find(|file| file.contains(span)) {
        let (line, _) = file.line_col(span);
        render_diag_at(label, code, message, span, source, &file.label(span), line)
    } else {
        render_diag(label, code, message, span, source)
    }
}

fn render_diag_at(
    label: &str,
    code: &str,
    message: &str,
    span: Span,
    source: &str,
    location: &str,
    display_line: usize,
) -> String {
    let (lc, cc) = match label {
        "error" => ("\x1b[1;31m", "\x1b[1;31m"),
        "warning" => ("\x1b[1;33m", "\x1b[1;33m"),
        _ => ("\x1b[1m", "\x1b[1m"),
    };
    let mut out = format!(
        "{lc}{label}\x1b[0m\x1b[1m[{code}]\x1b[0m: {message}\n  \x1b[1;34m-->\x1b[0m {location}",
    );
    if let Some(line_text) = source.lines().nth(span.line.saturating_sub(1)) {
        let lnum = display_line.to_string();
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
    /// If the expression resolves to a specific function name (e.g. a module-qualified
    /// mangled name), codegen should use this instead of the raw AST identifier.
    pub resolved_fn: Option<String>,
    /// If true, codegen should load the value pointed to by this reference expression.
    /// Set when a `&T` expression is used in a context that expects the value `T`.
    pub auto_deref: bool,
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

/// Target-independent description of a named C bitfield inside its storage unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitFieldLayout {
    pub byte_offset: usize,
    pub storage_bytes: u8,
    pub bit_offset: u8,
    pub bit_width: u8,
    pub signed: bool,
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
    /// Struct field layouts: struct name → ordered list of (field_name, field_type).
    pub struct_defs: HashMap<String, Vec<(String, TypeKind)>>,
    /// Struct total byte sizes: struct name → total byte size.
    pub struct_sizes: HashMap<String, usize>,
    /// Struct field byte offsets: struct name → vec of (field_name, byte_offset).
    pub struct_field_offsets: HashMap<String, Vec<(String, usize)>>,
    /// C bitfield metadata, keyed by aggregate and field name.
    pub bit_field_layouts: HashMap<String, HashMap<String, BitFieldLayout>>,
    /// Effective alignment of every aggregate.
    pub struct_alignments: HashMap<String, usize>,
    /// Trait implementations: type name → set of trait names explicitly implemented.
    pub trait_impls: HashMap<String, std::collections::HashSet<String>>,
    /// Method slot order per trait: trait name → ordered method names (index = vtable slot).
    pub trait_method_slots: HashMap<String, Vec<String>>,
    /// Enum variant tags: enum name → variant name → discriminant index.
    pub enum_defs: HashMap<String, HashMap<String, usize>>,
    /// Generic param names per struct: struct name → ordered generic param names.
    pub struct_generic_params: HashMap<String, Vec<String>>,
    /// Monomorphization requests: function name → list of concrete type args used at call sites.
    pub monomorphizations: Vec<MonomorphizationInfo>,
    /// Type aliases: alias name → (generic_params, aliased TypeKind).
    pub type_aliases: std::collections::HashMap<String, (Vec<String>, TypeKind)>,
    /// Ordered parameter names per function (mangled or plain): used for named-arg resolution.
    pub fn_param_names: HashMap<String, Vec<String>>,
    /// Internal function name → stable C ABI symbol requested by @export.
    pub exported_symbols: HashMap<String, String>,
    /// Struct names declared with `@repr(C)`.
    pub repr_c_structs: std::collections::HashSet<String>,
    /// `@repr(C) union` declarations (also present in `repr_c_structs`).
    pub repr_c_unions: std::collections::HashSet<String>,
    /// Aggregates whose final field is a C flexible array member.
    pub flexible_array_structs: std::collections::HashSet<String>,
    /// Files whose top-level definitions were mangled with their module name.
    pub namespaced_paths: std::collections::HashSet<String>,
    /// Whether the entry point is `fn main(args: Array[str])`.
    pub main_takes_args: bool,
}

/// Records a call to a generic function with concrete type arguments.
/// Used by codegen to create specialized function entries.
#[derive(Debug, Clone)]
pub struct MonomorphizationInfo {
    /// The original generic function name.
    pub fn_name: String,
    /// The concrete type arguments supplied at the call site.
    pub type_args: Vec<TypeKind>,
    /// The mangled name for the specialized copy (computed during typecheck).
    pub mangled_name: String,
}
