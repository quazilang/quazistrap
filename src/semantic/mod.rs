// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::collections::{BTreeSet, HashMap};

use crate::parser::ast::*;
use crate::semantic::typecheck::substitute_type_kind;

pub mod types;
pub use types::*;
mod borrow;
mod declare;
mod optimize;
pub(crate) mod typecheck;
mod unused;

// Private internal types used across sub-modules via `use super::*;`

#[derive(Debug, Clone, Default)]
pub(super) struct ExprEval {
    pub(super) ty: Option<TypeKind>,
    pub(super) const_value: Option<ConstValue>,
}

#[derive(Debug, Clone)]
pub(super) struct EnumInfo {
    pub(super) variants: HashMap<String, usize>, // variant → arity (payload count)
    pub(super) variant_fields: HashMap<String, Vec<TypeKind>>, // variant → field types
    pub(super) order: Vec<String>,               // declaration order → discriminant index
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
    pub(super) has_guard: bool,
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
    /// Call-only adjacency index maintained with `dependency_edges`.
    pub(super) call_dependencies: HashMap<String, BTreeSet<String>>,
    pub(super) called_functions: BTreeSet<String>,
    pub(super) call_counts: HashMap<String, usize>,
    pub(super) current_function: Vec<String>,
    /// Generic parameters of the function currently being type-checked.
    pub(super) current_generic_params: Vec<Vec<String>>,
    pub(super) math_optimizations: Vec<MathOptimization>,
    pub(super) lazy_import_accesses: HashMap<String, BTreeSet<String>>,
    pub(super) lazy_import_hints: Vec<LazyImportHint>,
    /// Functions not reachable from `main` via the call graph.
    pub(super) unreachable_functions: BTreeSet<String>,
    /// Nesting depth of `unsafe` blocks/functions (0 = safe context).
    pub(super) unsafe_depth: usize,
    /// Nesting depth of loops (0 = outside any loop).
    pub(super) loop_depth: usize,
    /// Nesting depth of trait definitions (0 = outside any trait).
    /// `any` in trait method signatures is exempt from W05.
    pub(super) trait_depth: usize,
    /// Character-index ranges in the merged source that belong to library files.
    /// Diagnostics whose span falls inside these ranges are suppressed.
    pub(super) library_char_ranges: Vec<std::ops::Range<usize>>,
    /// Per-file source map for merged sources.
    pub(super) source_files: Vec<SourceFile>,
    /// Files whose top-level definitions are mangled with their module name.
    /// Populated from LoadResult; empty when analyzing raw source without a loader.
    pub(super) namespaced_paths: std::collections::HashSet<String>,
    /// Function names that live in library (dependency) files.
    /// Set once from LoadResult; not reset between analyses.
    pub(super) library_fn_names: std::collections::HashSet<String>,
    /// Symbols declared by library files that are not part of the parsed user program.
    /// Used by tooling paths such as the LSP, where open-buffer analysis should not
    /// merge dependency source and shift user spans.
    pub(super) library_symbols: Vec<(String, Symbol)>,
    /// Function names explicitly imported by leaf name → full import path.
    /// Empty string value = wildcard import (no conflict detection).
    /// Reset at the start of each analysis; populated during the declare pass.
    pub(super) explicitly_imported_fns: std::collections::HashMap<String, String>,
    /// Struct field layouts: struct name → ordered list of (field_name, field_type).
    pub(super) struct_defs: HashMap<String, Vec<(String, TypeKind)>>,
    pub(super) struct_field_bit_widths: HashMap<String, Vec<(String, Option<u8>)>>,
    /// Generic params per struct: struct name → ordered generic param names.
    pub(super) struct_generic_params: HashMap<String, Vec<String>>,
    /// Structs explicitly requesting the target C memory layout.
    pub(super) repr_c_structs: std::collections::HashSet<String>,
    pub(super) repr_c_unions: std::collections::HashSet<String>,
    pub(super) repr_c_packed: std::collections::HashSet<String>,
    pub(super) repr_c_alignments: HashMap<String, usize>,
    pub(super) flexible_array_structs: std::collections::HashSet<String>,
    /// Derived traits: struct name → list of trait names from @derive.
    pub(super) derived_traits: HashMap<String, Vec<String>>,
    /// Trait implementations: type name → set of trait names explicitly implemented via `impl Trait for Type`.
    pub(super) trait_impls: HashMap<String, std::collections::HashSet<String>>,
    /// Method slot order per trait: trait name → ordered method names (index = vtable slot).
    pub(super) trait_method_slots: HashMap<String, Vec<String>>,
    /// When type-checking an impl method, this holds the mangled name (e.g. "Counter.get")
    /// so that dependency edges use the mangled name rather than the bare method name.
    pub(super) current_fn_name_override: Option<String>,
    /// Module path of the function whose body is currently being type-checked.
    /// `None` for entry-file functions that keep their bare names.
    pub(super) current_module_path: Option<String>,
    /// Monomorphization requests recorded during type checking.
    pub(super) monomorphizations: Vec<MonomorphizationInfo>,
    /// Type aliases: alias name → (generic_params, aliased TypeKind).
    pub(super) type_aliases: std::collections::HashMap<String, (Vec<String>, TypeKind)>,
    /// Ordered parameter names per function: fn name (or mangled) → param names (excl. self).
    pub(super) fn_param_names: HashMap<String, Vec<String>>,
    /// Internal function name → stable native symbol requested by @export.
    pub(super) exported_symbols: HashMap<String, String>,
    /// Resolved Quazi binding name → imported C data symbol metadata.
    pub(super) foreign_globals: HashMap<String, ForeignGlobalInfo>,
    /// Whether the entry point is `fn main(args: Array[str])`.
    pub(super) main_takes_args: bool,
}

pub(super) fn unwrap_type(ty: &Type) -> TypeKind {
    ty.node.clone()
}

pub(super) fn extract_attribute_names(attributes: &[Attribute]) -> Vec<String> {
    attributes.iter().map(|a| a.name.clone()).collect()
}

fn resolve_layout_alias(
    ty: &TypeKind,
    aliases: &std::collections::HashMap<String, (Vec<String>, TypeKind)>,
) -> TypeKind {
    if let TypeKind::Named { name, type_args } = ty
        && type_args.is_empty()
        && let Some((params, target)) = aliases.get(name)
        && params.is_empty()
    {
        return resolve_layout_alias(target, aliases);
    }
    ty.clone()
}

fn ffi_type_size_align(
    ty: &TypeKind,
    aliases: &std::collections::HashMap<String, (Vec<String>, TypeKind)>,
) -> (usize, usize) {
    match resolve_layout_alias(ty, aliases) {
        TypeKind::Int8 | TypeKind::Uint8 | TypeKind::Bool => (1, 1),
        TypeKind::Int16 | TypeKind::Uint16 => (2, 2),
        TypeKind::Int32 | TypeKind::Uint32 | TypeKind::Float32 => (4, 4),
        TypeKind::Int64
        | TypeKind::Uint64
        | TypeKind::Isize
        | TypeKind::Usize
        | TypeKind::Float64
        | TypeKind::RawPtr { .. } => (8, 8),
        TypeKind::Array { elem_ty, len } => {
            let (size, align) = ffi_type_size_align(&elem_ty.node, aliases);
            (size.saturating_mul(len as usize), align)
        }
        TypeKind::FlexibleArray { elem_ty } => {
            let (_, align) = ffi_type_size_align(&elem_ty.node, aliases);
            (0, align)
        }
        _ => (8, 8),
    }
}

#[derive(Default)]
struct AggregateLayout {
    size: usize,
    align: usize,
    offsets: Vec<(String, usize)>,
    bit_fields: HashMap<String, BitFieldLayout>,
}

fn ffi_aggregate_layout(
    fields: &[(String, TypeKind)],
    bit_widths: &[(String, Option<u8>)],
    aliases: &std::collections::HashMap<String, (Vec<String>, TypeKind)>,
    is_union: bool,
    packed: bool,
    explicit_align: Option<usize>,
) -> AggregateLayout {
    let mut offset = 0usize;
    let mut struct_align = 1usize;
    let mut offsets = Vec::with_capacity(fields.len());
    let mut bit_fields = HashMap::new();
    let mut active_bits: Option<(usize, usize, u8)> = None;
    let mut union_size = 0usize;
    for ((name, ty), (_, bit_width)) in fields.iter().zip(bit_widths) {
        let (size, align) = ffi_type_size_align(ty, aliases);
        let field_align = if packed { 1 } else { align };
        struct_align = struct_align.max(field_align);
        if let Some(width) = bit_width {
            let signed = matches!(
                resolve_layout_alias(ty, aliases),
                TypeKind::Int8
                    | TypeKind::Int16
                    | TypeKind::Int32
                    | TypeKind::Int64
                    | TypeKind::Isize
            );
            let (byte_offset, bit_offset) = if is_union {
                union_size = union_size.max(size);
                (0, 0)
            } else if let Some((unit_offset, unit_size, used)) = active_bits
                && unit_size == size
                && usize::from(used) + usize::from(*width) <= size * 8
            {
                active_bits = Some((unit_offset, unit_size, used + *width));
                (unit_offset, used)
            } else {
                offset = align_up(offset, field_align);
                let unit_offset = offset;
                offset += size;
                active_bits = Some((unit_offset, size, *width));
                (unit_offset, 0)
            };
            offsets.push((name.clone(), byte_offset));
            bit_fields.insert(
                name.clone(),
                BitFieldLayout {
                    byte_offset,
                    storage_bytes: size as u8,
                    bit_offset,
                    bit_width: *width,
                    signed,
                },
            );
        } else {
            active_bits = None;
            if is_union {
                offsets.push((name.clone(), 0));
                union_size = union_size.max(size);
            } else {
                offset = align_up(offset, field_align);
                offsets.push((name.clone(), offset));
                offset += size;
            }
        }
    }
    struct_align = struct_align.max(explicit_align.unwrap_or(1));
    let raw_size = if is_union { union_size } else { offset };
    AggregateLayout {
        size: align_up(raw_size, struct_align),
        align: struct_align,
        offsets,
        bit_fields,
    }
}

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

/// Returns `true` if the item should be included on this host based on @cfg attributes.
pub fn item_should_include(attributes: &[Attribute]) -> bool {
    #[cfg(target_os = "windows")]
    let host_abi = "win64";
    #[cfg(not(target_os = "windows"))]
    let host_abi = "sysv";
    item_should_include_for(
        attributes,
        std::env::consts::OS,
        std::env::consts::ARCH,
        host_abi,
    )
}

/// Returns `true` if the item should be included for an explicit compilation target.
pub fn item_should_include_for(
    attributes: &[Attribute],
    target_os: &str,
    target_arch: &str,
    target_abi: &str,
) -> bool {
    use crate::parser::ast::{AttrArg, AttrVal};
    for attr in attributes {
        if attr.name == "cfg" {
            let mut matched = true;
            for arg in &attr.args {
                match arg {
                    AttrArg::KeyValue(key, AttrVal::Str(val)) if key == "target_os" => {
                        if val.as_str() != target_os {
                            matched = false;
                        }
                    }
                    AttrArg::KeyValue(key, AttrVal::Str(val)) if key == "target_arch" => {
                        if val.as_str() != target_arch {
                            matched = false;
                        }
                    }
                    AttrArg::KeyValue(key, AttrVal::Str(val)) if key == "target_abi" => {
                        if val.as_str() != target_abi {
                            matched = false;
                        }
                    }
                    _ => {}
                }
            }
            if !matched {
                return false;
            }
        }
    }
    true
}

/// Strip @cfg-disabled items and CfgBlock statements from a Program before analysis.
/// Called once after loading; all subsequent passes see the clean AST.
pub fn strip_cfg(program: &Program) -> Program {
    #[cfg(target_os = "windows")]
    let host_abi = "win64";
    #[cfg(not(target_os = "windows"))]
    let host_abi = "sysv";
    strip_cfg_for(
        program,
        std::env::consts::OS,
        std::env::consts::ARCH,
        host_abi,
    )
}

/// Strip @cfg-disabled syntax for an explicit target before semantic analysis.
pub fn strip_cfg_for(
    program: &Program,
    target_os: &str,
    target_arch: &str,
    target_abi: &str,
) -> Program {
    fn strip_block(block: &Block, target_os: &str, target_arch: &str, target_abi: &str) -> Block {
        let mut stmts = Vec::new();
        for stmt in &block.stmts {
            match &stmt.node {
                StmtKind::CfgBlock { condition, body } => {
                    if item_should_include_for(
                        std::slice::from_ref(condition),
                        target_os,
                        target_arch,
                        target_abi,
                    ) {
                        stmts.extend(strip_block(body, target_os, target_arch, target_abi).stmts);
                    }
                }
                StmtKind::If {
                    condition,
                    then_block,
                    else_if,
                    else_block,
                } => {
                    stmts.push(Spanned::new(
                        StmtKind::If {
                            condition: condition.clone(),
                            then_block: strip_block(then_block, target_os, target_arch, target_abi),
                            else_if: else_if
                                .iter()
                                .map(|(c, b)| {
                                    (
                                        c.clone(),
                                        strip_block(b, target_os, target_arch, target_abi),
                                    )
                                })
                                .collect(),
                            else_block: else_block.as_ref().map(|block| {
                                strip_block(block, target_os, target_arch, target_abi)
                            }),
                        },
                        stmt.span,
                    ));
                }
                StmtKind::For { kind, body } => {
                    stmts.push(Spanned::new(
                        StmtKind::For {
                            kind: kind.clone(),
                            body: strip_block(body, target_os, target_arch, target_abi),
                        },
                        stmt.span,
                    ));
                }
                StmtKind::UnsafeBlock { body } => {
                    stmts.push(Spanned::new(
                        StmtKind::UnsafeBlock {
                            body: strip_block(body, target_os, target_arch, target_abi),
                        },
                        stmt.span,
                    ));
                }
                _ => stmts.push(stmt.clone()),
            }
        }
        Block {
            stmts,
            span: block.span,
        }
    }

    fn strip_fn(node: &ItemKind, target_os: &str, target_arch: &str, target_abi: &str) -> ItemKind {
        match node {
            ItemKind::Fn {
                name,
                return_ty,
                params,
                body,
                attributes,
                pub_fn,
                unsafe_fn,
                generic_params,
                c_variadic,
            } => ItemKind::Fn {
                name: name.clone(),
                return_ty: return_ty.clone(),
                params: params.clone(),
                body: body
                    .as_ref()
                    .map(|block| strip_block(block, target_os, target_arch, target_abi)),
                attributes: attributes
                    .iter()
                    .filter(|attribute| attribute.name != "cfg")
                    .cloned()
                    .collect(),
                pub_fn: *pub_fn,
                unsafe_fn: *unsafe_fn,
                generic_params: generic_params.clone(),
                c_variadic: *c_variadic,
            },
            other => other.clone(),
        }
    }

    fn remove_cfg_attributes(node: &mut ItemKind) {
        let attributes = match node {
            ItemKind::Fn { attributes, .. }
            | ItemKind::Struct { attributes, .. }
            | ItemKind::Trait { attributes, .. }
            | ItemKind::Enum { attributes, .. }
            | ItemKind::TypeAlias { attributes, .. }
            | ItemKind::ForeignGlobal { attributes, .. } => Some(attributes),
            _ => None,
        };
        if let Some(attributes) = attributes {
            attributes.retain(|attribute| attribute.name != "cfg");
        }
    }

    let mut items = Vec::new();
    for item in &program.items {
        let attrs: Option<&[Attribute]> = match &item.node {
            ItemKind::Fn { attributes, .. }
            | ItemKind::Struct { attributes, .. }
            | ItemKind::Trait { attributes, .. }
            | ItemKind::Enum { attributes, .. }
            | ItemKind::TypeAlias { attributes, .. }
            | ItemKind::ForeignGlobal { attributes, .. } => Some(attributes),
            _ => None,
        };
        if attrs.is_some_and(|attributes| {
            !item_should_include_for(attributes, target_os, target_arch, target_abi)
        }) {
            continue;
        }
        let mut node = match &item.node {
            ItemKind::Impl {
                trait_ty,
                for_ty,
                methods,
            } => {
                let methods: Vec<Item> = methods
                    .iter()
                    .filter(|m| {
                        if let ItemKind::Fn { attributes, .. } = &m.node {
                            item_should_include_for(attributes, target_os, target_arch, target_abi)
                        } else {
                            true
                        }
                    })
                    .map(|m| {
                        Spanned::new(
                            strip_fn(&m.node, target_os, target_arch, target_abi),
                            m.span,
                        )
                    })
                    .collect();
                ItemKind::Impl {
                    trait_ty: trait_ty.clone(),
                    for_ty: for_ty.clone(),
                    methods,
                }
            }
            ItemKind::Fn { .. } => strip_fn(&item.node, target_os, target_arch, target_abi),
            other => other.clone(),
        };
        remove_cfg_attributes(&mut node);
        items.push(Spanned::new(node, item.span));
    }
    Program {
        items,
        span: program.span,
    }
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
            call_dependencies: HashMap::new(),
            called_functions: BTreeSet::new(),
            call_counts: HashMap::new(),
            current_function: Vec::new(),
            current_generic_params: Vec::new(),
            math_optimizations: Vec::new(),
            lazy_import_accesses: HashMap::new(),
            lazy_import_hints: Vec::new(),
            unreachable_functions: BTreeSet::new(),
            unsafe_depth: 0,
            loop_depth: 0,
            trait_depth: 0,
            main_takes_args: false,
            library_char_ranges: Vec::new(),
            source_files: Vec::new(),
            namespaced_paths: std::collections::HashSet::new(),
            library_fn_names: std::collections::HashSet::new(),
            library_symbols: Vec::new(),
            explicitly_imported_fns: std::collections::HashMap::new(),
            struct_defs: HashMap::new(),
            struct_field_bit_widths: HashMap::new(),
            struct_generic_params: HashMap::new(),
            repr_c_structs: std::collections::HashSet::new(),
            repr_c_unions: std::collections::HashSet::new(),
            repr_c_packed: std::collections::HashSet::new(),
            repr_c_alignments: HashMap::new(),
            flexible_array_structs: std::collections::HashSet::new(),
            derived_traits: HashMap::new(),
            trait_impls: HashMap::new(),
            trait_method_slots: HashMap::new(),
            current_fn_name_override: None,
            current_module_path: None,
            monomorphizations: Vec::new(),
            type_aliases: std::collections::HashMap::new(),
            fn_param_names: HashMap::new(),
            exported_symbols: HashMap::new(),
            foreign_globals: HashMap::new(),
        }
    }

    pub fn set_library_fns(&mut self, names: std::collections::HashSet<String>) {
        self.library_fn_names = names;
    }

    pub fn set_library_char_ranges(&mut self, ranges: Vec<std::ops::Range<usize>>) {
        self.library_char_ranges = ranges;
    }

    pub fn set_source_files(&mut self, files: Vec<SourceFile>) {
        self.source_files = files;
    }

    pub fn set_namespaced_paths(&mut self, paths: std::collections::HashSet<String>) {
        self.namespaced_paths = paths;
    }

    pub(super) fn is_library_span(&self, span: Span) -> bool {
        self.library_char_ranges
            .iter()
            .any(|r| r.contains(&span.start))
    }

    pub(super) fn describe_span(&self, span: Span) -> String {
        self.source_files
            .iter()
            .find(|file| file.contains(span))
            .map(|file| file.label(span))
            .unwrap_or_else(|| format!("{}:{}", span.line, span.col))
    }

    /// Derive the module name for a source file path, matching the loader's
    /// `path_module_name`: `bar.qz` → `bar`, `mod.qz` → parent dir name.
    pub(super) fn module_name_for_path(&self, path: &str) -> Option<String> {
        let p = std::path::Path::new(path);
        let stem = p.file_stem().and_then(|s| s.to_str())?;
        if stem == "mod" {
            p.parent()
                .and_then(|parent| parent.file_name())
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        } else {
            Some(stem.to_string())
        }
    }

    /// Return the module path for the source file containing `span`, if that
    /// file is marked as namespaced.
    pub(super) fn module_path_for_span(&self, span: Span) -> Option<String> {
        let file = self.source_files.iter().find(|file| file.contains(span))?;
        if !self.namespaced_paths.contains(&file.path) {
            return None;
        }
        file.module_name
            .clone()
            .or_else(|| self.module_name_for_path(&file.path))
    }

    /// Return true if the source file containing `span` is a namespaced module.
    pub(super) fn is_namespaced_span(&self, span: Span) -> bool {
        self.source_files
            .iter()
            .find(|f| f.contains(span))
            .map(|f| self.namespaced_paths.contains(&f.path))
            .unwrap_or(false)
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
            struct_defs: self.struct_defs.clone(),
            struct_sizes: self
                .struct_defs
                .iter()
                .map(|(name, fields)| {
                    let size = if self.repr_c_structs.contains(name) {
                        ffi_aggregate_layout(
                            fields,
                            self.struct_field_bit_widths
                                .get(name)
                                .map(Vec::as_slice)
                                .unwrap_or(&[]),
                            &self.type_aliases,
                            self.repr_c_unions.contains(name),
                            self.repr_c_packed.contains(name),
                            self.repr_c_alignments.get(name).copied(),
                        )
                        .size
                    } else {
                        fields.len() * 8
                    };
                    (name.clone(), size)
                })
                .collect(),
            struct_field_offsets: self
                .struct_defs
                .iter()
                .map(|(name, fields)| {
                    let offsets = if self.repr_c_structs.contains(name) {
                        ffi_aggregate_layout(
                            fields,
                            self.struct_field_bit_widths
                                .get(name)
                                .map(Vec::as_slice)
                                .unwrap_or(&[]),
                            &self.type_aliases,
                            self.repr_c_unions.contains(name),
                            self.repr_c_packed.contains(name),
                            self.repr_c_alignments.get(name).copied(),
                        )
                        .offsets
                    } else {
                        fields
                            .iter()
                            .enumerate()
                            .map(|(i, (fname, _))| (fname.clone(), i * 8))
                            .collect()
                    };
                    (name.clone(), offsets)
                })
                .collect(),
            bit_field_layouts: self
                .struct_defs
                .iter()
                .filter(|(name, _)| self.repr_c_structs.contains(*name))
                .map(|(name, fields)| {
                    let layout = ffi_aggregate_layout(
                        fields,
                        self.struct_field_bit_widths
                            .get(name)
                            .map(Vec::as_slice)
                            .unwrap_or(&[]),
                        &self.type_aliases,
                        self.repr_c_unions.contains(name),
                        self.repr_c_packed.contains(name),
                        self.repr_c_alignments.get(name).copied(),
                    );
                    (name.clone(), layout.bit_fields)
                })
                .collect(),
            struct_alignments: self
                .struct_defs
                .iter()
                .map(|(name, fields)| {
                    let align = if self.repr_c_structs.contains(name) {
                        ffi_aggregate_layout(
                            fields,
                            self.struct_field_bit_widths
                                .get(name)
                                .map(Vec::as_slice)
                                .unwrap_or(&[]),
                            &self.type_aliases,
                            self.repr_c_unions.contains(name),
                            self.repr_c_packed.contains(name),
                            self.repr_c_alignments.get(name).copied(),
                        )
                        .align
                    } else {
                        8
                    };
                    (name.clone(), align)
                })
                .collect(),
            trait_impls: self.trait_impls.clone(),
            trait_method_slots: self.trait_method_slots.clone(),
            enum_defs: self
                .enums
                .iter()
                .map(|(k, v)| {
                    let disc_map = v
                        .order
                        .iter()
                        .enumerate()
                        .map(|(i, name)| (name.clone(), i))
                        .collect();
                    (k.clone(), disc_map)
                })
                .collect(),
            struct_generic_params: self.struct_generic_params.clone(),
            monomorphizations: std::mem::take(&mut self.monomorphizations),
            type_aliases: self.type_aliases.clone(),
            fn_param_names: self.fn_param_names.clone(),
            exported_symbols: self.exported_symbols.clone(),
            foreign_globals: self.foreign_globals.clone(),
            repr_c_structs: self.repr_c_structs.clone(),
            repr_c_unions: self.repr_c_unions.clone(),
            flexible_array_structs: self.flexible_array_structs.clone(),
            namespaced_paths: self.namespaced_paths.clone(),
            main_takes_args: self.main_takes_args,
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
        self.call_dependencies.clear();
        self.called_functions.clear();
        self.call_counts.clear();
        self.current_function.clear();
        self.current_generic_params.clear();
        self.math_optimizations.clear();
        self.lazy_import_accesses.clear();
        self.lazy_import_hints.clear();
        self.unreachable_functions.clear();
        self.unsafe_depth = 0;
        self.trait_depth = 0;
        self.explicitly_imported_fns.clear(); // HashMap::clear
        self.struct_defs.clear();
        self.struct_field_bit_widths.clear();
        self.struct_generic_params.clear();
        self.derived_traits.clear();
        self.trait_impls.clear();
        self.trait_method_slots.clear();
        self.monomorphizations.clear();
        self.type_aliases.clear();
        self.fn_param_names.clear();
        self.exported_symbols.clear();
        self.foreign_globals.clear();
        self.repr_c_structs.clear();
        self.repr_c_unions.clear();
        self.repr_c_packed.clear();
        self.repr_c_alignments.clear();
        self.flexible_array_structs.clear();
        self.main_takes_args = false;
        self.current_fn_name_override = None;
        self.current_module_path = None;
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
        option_variants.insert("Some".to_string(), 1usize); // arity 1
        option_variants.insert("None".to_string(), 0usize); // arity 0
        self.enums.insert(
            "Option".to_string(),
            EnumInfo {
                variants: option_variants,
                variant_fields: HashMap::new(), // generic T — resolved at call site
                order: vec!["None".to_string(), "Some".to_string()], // None=0, Some=1
            },
        );

        let mut result_variants = HashMap::new();
        result_variants.insert("Ok".to_string(), 1usize); // arity 1
        result_variants.insert("Err".to_string(), 1usize); // arity 1
        self.enums.insert(
            "Result".to_string(),
            EnumInfo {
                variants: result_variants,
                variant_fields: HashMap::new(), // generic T/E — resolved at call site
                order: vec!["Err".to_string(), "Ok".to_string()], // Err=0, Ok=1
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
                    unsafe_fn: false,
                    generic_params: vec![],
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
                    unsafe_fn: false,
                    generic_params: vec![],
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
                unsafe_fn: false,
                generic_params: vec![],
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
        let inserted = self
            .dependency_edges
            .insert((kind, from.to_string(), to.to_string()));
        if inserted && kind == DependencyKind::Call {
            self.call_dependencies
                .entry(from.to_string())
                .or_default()
                .insert(to.to_string());
            self.called_functions.insert(to.to_string());
        }
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

        let calls_from = self
            .call_dependencies
            .iter()
            .map(|(from, targets)| (from.clone(), targets.iter().cloned().collect()))
            .collect();

        DependencyGraph { edges, calls_from }
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
                // Same-path duplicate import is a no-op (two modules both import std.io, etc.)
                if symbol.import_path == prev.import_path {
                    return;
                }
                self.push_error(
                    symbol.span,
                    "S05",
                    format!(
                        "import name conflict for '{}' (previous import at {})",
                        name,
                        self.describe_span(prev.span)
                    ),
                );
                return;
            }

            let prev_location = self.describe_span(prev.span);
            self.push_error(
                symbol.span,
                "S05",
                format!(
                    "duplicate declaration '{}' (previous declaration at {})",
                    name, prev_location
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

    /// Recursively expand type aliases in a TypeKind.
    pub(super) fn resolve_type_aliases(&self, ty: &TypeKind) -> TypeKind {
        match ty {
            TypeKind::Named { name, type_args } => {
                let alias_entry = self.type_aliases.get(name).or_else(|| {
                    // Dotted name like "ffi.c_int": also try the leaf segment "c_int".
                    if name.contains('.') {
                        name.rsplit('.')
                            .next()
                            .and_then(|leaf| self.type_aliases.get(leaf))
                    } else {
                        None
                    }
                });
                if let Some((generic_params, aliased)) = alias_entry {
                    // Substitute generic params if this alias has type args
                    if type_args.is_empty() || generic_params.is_empty() {
                        let resolved = self.resolve_type_aliases(aliased);
                        return self.resolve_type_aliases(&resolved);
                    }
                    let mut map = std::collections::HashMap::new();
                    for (gp, ta) in generic_params.iter().zip(type_args.iter()) {
                        map.insert(gp.clone(), self.resolve_type_aliases(&ta.node));
                    }
                    let substituted = substitute_type_kind(aliased, &map);
                    self.resolve_type_aliases(&substituted)
                } else {
                    TypeKind::Named {
                        name: name.clone(),
                        type_args: type_args
                            .iter()
                            .map(|t| Spanned::new(self.resolve_type_aliases(&t.node), t.span))
                            .collect(),
                    }
                }
            }

            TypeKind::Ref { inner } => TypeKind::Ref {
                inner: Box::new(Spanned::new(
                    self.resolve_type_aliases(&inner.node),
                    inner.span,
                )),
            },
            TypeKind::RawPtr { inner } => TypeKind::RawPtr {
                inner: Box::new(Spanned::new(
                    self.resolve_type_aliases(&inner.node),
                    inner.span,
                )),
            },
            TypeKind::Array { elem_ty, len } => TypeKind::Array {
                elem_ty: Box::new(Spanned::new(
                    self.resolve_type_aliases(&elem_ty.node),
                    elem_ty.span,
                )),
                len: *len,
            },
            TypeKind::FlexibleArray { elem_ty } => TypeKind::FlexibleArray {
                elem_ty: Box::new(Spanned::new(
                    self.resolve_type_aliases(&elem_ty.node),
                    elem_ty.span,
                )),
            },
            TypeKind::Slice { elem_ty } => TypeKind::Slice {
                elem_ty: Box::new(Spanned::new(
                    self.resolve_type_aliases(&elem_ty.node),
                    elem_ty.span,
                )),
            },
            TypeKind::Fn { params, return_ty } => TypeKind::Fn {
                params: params
                    .iter()
                    .map(|p| Spanned::new(self.resolve_type_aliases(&p.node), p.span))
                    .collect(),
                return_ty: Box::new(Spanned::new(
                    self.resolve_type_aliases(&return_ty.node),
                    return_ty.span,
                )),
            },
            TypeKind::CFn { params, return_ty } => TypeKind::CFn {
                params: params
                    .iter()
                    .map(|p| Spanned::new(self.resolve_type_aliases(&p.node), p.span))
                    .collect(),
                return_ty: Box::new(Spanned::new(
                    self.resolve_type_aliases(&return_ty.node),
                    return_ty.span,
                )),
            },
            other => other.clone(),
        }
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
                if symbol.is_import
                    && let Some(path) = &symbol.import_path
                {
                    self.used_import_paths.insert(path.clone());
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
        if self.is_library_span(span) {
            return;
        }
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
        if self.is_library_span(span) {
            return;
        }
        self.warnings.push(SemanticWarning {
            code,
            message,
            span,
            suggestions: vec![suggestion],
        });
    }

    pub(super) fn push_suggestion(&mut self, span: Option<Span>, message: String) {
        if let Some(s) = span
            && self.is_library_span(s)
        {
            return;
        }
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
    use crate::parser::{Parser, ast::ItemKind};

    use super::{Analyzer, ConstValue, DependencyKind, SemanticReport, strip_cfg_for};

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
    fn strips_cfg_for_an_explicit_target() {
        let program = parse_program(
            r#"
@cfg(target_os="linux") type platform_word = i64;
@cfg(target_os="windows") type platform_word = i32;
@cfg(target_abi="sysv") fn linux_only() void {}
@cfg(target_abi="win64") fn windows_only() void {}
"#,
        );
        let linux = strip_cfg_for(&program, "linux", "x86_64", "sysv");
        let names: Vec<_> = linux
            .items
            .iter()
            .map(|item| match &item.node {
                ItemKind::TypeAlias { name, .. } | ItemKind::Fn { name, .. } => name.as_str(),
                _ => "",
            })
            .collect();
        assert_eq!(names, ["platform_word", "linux_only"]);
        assert!(linux.items.iter().all(|item| match &item.node {
            ItemKind::TypeAlias { attributes, .. } | ItemKind::Fn { attributes, .. } =>
                attributes.iter().all(|attribute| attribute.name != "cfg"),
            _ => true,
        }));
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
    fn rejects_nonsensical_string_and_struct_comparisons() {
        let string_report = analyze("fn bad(s: str) bool { ret s == 1; }");
        assert!(string_report.errors.iter().any(|error| error.code == "S01"));

        let ordering_report =
            analyze("struct Value { n: i32, } fn bad(a: Value, b: Value) bool { ret a < b; }");
        assert!(
            ordering_report
                .errors
                .iter()
                .any(|error| error.code == "S06")
        );
    }

    #[test]
    fn permits_ordering_inside_generic_functions() {
        let report = analyze("fn max[T](a: T, b: T) T { if (a > b) { ret a; } ret b; }");
        assert!(
            report.errors.is_empty(),
            "generic ordering must remain available to generic helper bodies: {:?}",
            report.errors
        );
    }

    #[test]
    fn reports_type_mismatch_in_var() {
        let report = analyze(
            r#"
fn main() void {
    var x: bool = "not a bool";
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
    fn accepts_bitwise_operators_on_integers() {
        let report = analyze(
            r#"
fn main() void {
    var a: i32 = 1 & 2;
    var b: i32 = 3 | 4;
    var c: i32 = 5 ^ 6;
    var d: i32 = 7 << 1;
    var e: i32 = 8 >> 1;
    ret;
}
"#,
        );
        assert!(report.errors.is_empty());
    }

    #[test]
    fn accepts_bitwise_operators_on_bools() {
        let report = analyze(
            r#"
fn main() void {
    var a: bool = true & false;
    var b: bool = true | false;
    var c: bool = true ^ false;
    ret;
}
"#,
        );
        assert!(report.errors.is_empty());
    }

    #[test]
    fn reports_type_mismatch_in_bitwise_op() {
        let report = analyze(
            r#"
fn main() void {
    var a: i32 = 1 & "oops";
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
    fn accepts_else_if_chain() {
        let report = analyze(
            r#"
fn main() void {
    var x: i32 = 1;
    if (x == 1) {
        x = 2;
    } else if (x == 2) {
        x = 3;
    } else {
        x = 4;
    }
}
"#,
        );
        assert!(report.errors.is_empty());
    }

    #[test]
    fn reports_non_bool_else_if_condition() {
        let report = analyze(
            r#"
fn main() void {
    if (true) {
        ret;
    } else if ("not a bool") {
        ret;
    }
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("if condition must be bool"))
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
                .any(|w| w.message.contains("already covered"))
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
        assert_eq!(
            report
                .dependency_graph
                .calls_from
                .get("main")
                .map(Vec::as_slice),
            Some(["helper".to_string()].as_slice())
        );
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
    fn compound_assign_on_index_expression_is_valid() {
        let report = analyze(
            r#"
fn main() void {
    var arr = [1, 2, 3];
    arr[0] += 10;
    (arr[1]) -= 1;
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
    fn inc_dec_on_parenthesized_deref_is_valid() {
        let report = analyze(
            r#"
unsafe fn bump(p: *i32) void {
    (*p)++;
    --(*p);
}

fn main() void {
    var x: i32 = 0;
    unsafe { bump(&x); }
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
    fn foreach_over_explicit_named_array_moves_iterable() {
        let report = analyze(
            r#"
struct Array[T] { ptr: i32, }

fn consume(a: Array[i32]) void { ret; }

fn main() void {
    var arr: Array[i32];
    for item : arr {
        var x: i32 = 1;
    }
    consume(arr);
}
"#,
        );
        let borrow_errors: Vec<_> = report.errors.iter().filter(|e| e.code == "S10").collect();
        assert!(
            !borrow_errors.is_empty(),
            "foreach should move explicitly typed Array[i32]: {:?}",
            borrow_errors
        );
    }

    #[test]
    fn generic_receiver_method_checks_substituted_arg_type() {
        let report = analyze(
            r#"
struct Array[T] { ptr: i32, }

impl Array[T] {
    fn push(self: Array[T], val: T) void { ret; }
}

fn main() void {
    var arr: Array[i32];
    arr.push("hello");
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.code == "S08" && e.message.contains("expected i32, got &str")),
            "Array[i32].push(str) should be rejected, got: {:?}",
            report.errors
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

    #[test]
    fn main_with_array_str_args_is_valid() {
        let report = analyze(
            r#"
fn main(args: Array[str]) i32 {
    ret args.len() as i32;
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "main(args: Array[str]) should be accepted, got {:?}",
            report.errors
        );
        assert!(report.main_takes_args, "main_takes_args flag should be set");
    }

    #[test]
    fn main_with_invalid_param_is_error() {
        let report = analyze(
            r#"
fn main(x: i32) i32 {
    ret x;
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("main() must take either no parameters")),
            "expected main parameter error, got {:?}",
            report.errors
        );
    }

    #[test]
    fn array_to_slice_coercion_is_rejected_with_clear_error() {
        let report = analyze(
            r#"
fn sum(s: [i32]) i32 { ret 0; }

fn main() i32 {
    var arr = [1, 2, 3];
    ret sum(arr);
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("passing fixed-size array")
                    && e.message.contains("slice parameter")),
            "expected clear array-to-slice error, got {:?}",
            report.errors
        );
    }

    #[test]
    fn impl_method_call_resolves_to_method_return_type() {
        let report = analyze(
            r#"
struct Counter { val: i32, }

impl Counter {
    fn get(self: Counter) i32 { ret self.val; }
    fn inc(self: Counter, n: i32) Counter { ret Counter { val: self.val + n }; }
}

fn main() void {
    var c: Counter = Counter { val: 0 };
    var n: i32 = c.get();
    var c2: Counter = c.inc(1);
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "impl method call should not produce errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn inherent_impl_no_trait_is_accepted() {
        let report = analyze(
            r#"
struct Point { x: i32, y: i32, }

impl Point {
    fn x_val(self: Point) i32 { ret self.x; }
}

fn main() void {
    var p: Point = Point { x: 1, y: 2 };
    var x: i32 = p.x_val();
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "inherent impl should not produce errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn trait_impl_method_call_resolves_type() {
        let report = analyze(
            r#"
trait Display { fn label(self: str) str; }

struct Tag { name: str, }

impl Display for Tag {
    fn label(self: Tag) str { ret self.name; }
}

fn main() void {
    var t: Tag = Tag { name: "hello" };
    var s: str = t.label();
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "trait impl method call should not produce errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn impl_method_duplicate_is_error() {
        let report = analyze(
            r#"
struct Foo { x: i32, }

impl Foo {
    fn get(self: Foo) i32 { ret self.x; }
}

impl Foo {
    fn get(self: Foo) i32 { ret self.x; }
}

fn main() void { ret; }
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("duplicate declaration 'Foo.get'")),
            "duplicate impl method should produce error, got: {:?}",
            report.errors
        );
    }

    #[test]
    fn type_alias_expands_at_call_site() {
        let report = analyze(
            r#"
type Rune = u32;

fn main() void {
    var x: Rune = 42;
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "type alias Rune = u32 should accept assignments: {:?}",
            report.errors
        );
    }

    #[test]
    fn type_alias_rejects_wrong_type() {
        let report = analyze(
            r#"
type Rune = u32;

fn main() void {
    var x: Rune = "hello";
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("type mismatch")),
            "type alias should enforce underlying type: {:?}",
            report.errors
        );
    }

    #[test]
    fn function_name_used_as_value_has_fn_type() {
        let report = analyze(
            r#"
fn add(a: i32, b: i32) i32 { ret a + b; }

fn main() void {
    var f: fn(i32, i32) i32 = add;
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "function name as value should type-check: {:?}",
            report.errors
        );
    }

    #[test]
    fn function_pointer_call_checks_arg_count() {
        let report = analyze(
            r#"
fn add(a: i32, b: i32) i32 { ret a + b; }

fn main() void {
    var f: fn(i32, i32) i32 = add;
    var x = f(1, 2);
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "calling function pointer should type-check: {:?}",
            report.errors
        );
    }

    #[test]
    fn closure_expression_parses_and_typechecks() {
        let report = analyze(
            r#"
fn main() void {
    var f = |x, y| x + y;
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "closure expression should type-check without errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn closure_has_fn_type() {
        let report = analyze(
            r#"
fn main() void {
    var f = |x| x + 1;
    var g: fn(i32) i32 = f;
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "closure should have fn(i32) i32 type: {:?}",
            report.errors
        );
    }

    #[test]
    fn generic_struct_field_access_returns_substituted_type() {
        let report = analyze(
            r#"
struct Pair[A, B] { first: A, second: B, }

fn main() void {
    var p: Pair[i32, str] = Pair { first: 1, second: "hi" };
    var x: i32 = p.first;
    var y: str = p.second;
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "generic struct field access should type-check: {:?}",
            report.errors
        );
    }

    #[test]
    fn generic_struct_field_wrong_type_is_error() {
        let report = analyze(
            r#"
struct Box[T] { val: T, }

fn main() void {
    var b: Box[i32] = Box { val: 1 };
    var s: str = b.val;
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.message.contains("type mismatch")),
            "accessing i32 field as str should be an error: {:?}",
            report.errors
        );
    }

    #[test]
    fn trait_method_slots_are_recorded() {
        let report = analyze(
            r#"
trait Drawable { fn draw(self: str) void; fn area(self: str) i32; }
fn main() void { }
"#,
        );
        let slots = report
            .trait_method_slots
            .get("Drawable")
            .expect("Drawable slots must exist");
        assert_eq!(slots[0], "draw", "draw should be slot 0");
        assert_eq!(slots[1], "area", "area should be slot 1");
    }

    #[test]
    fn ffi_attributes_and_c_layout_are_recorded() {
        let report = analyze(
            r#"
type c_char = i8;
type c_int = i32;
@repr(C)
struct Record { tag: c_char, value: c_int, next: *Record, }
@api("native_read") unsafe fn native_read(out: *Record) c_int;
@export("quazi_answer") pub fn answer() c_int { ret 42; }
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "FFI declarations: {:?}",
            report.errors
        );
        assert_eq!(report.struct_sizes.get("Record"), Some(&16));
        assert_eq!(
            report.struct_field_offsets.get("Record"),
            Some(&vec![
                ("tag".into(), 0),
                ("value".into(), 4),
                ("next".into(), 8)
            ])
        );
        assert_eq!(
            report.exported_symbols.get("answer"),
            Some(&"quazi_answer".to_string())
        );
    }

    #[test]
    fn ffi_rejects_safe_calls_but_accepts_aggregate_by_value() {
        let report = analyze(
            r#"
@repr(C) struct Point { x: i32, y: i32, }
@api unsafe fn consume(point: Point) i32;
fn main() void { consume(Point { x: 1, y: 2 }); }
"#,
        );
        assert!(report.errors.iter().any(|e| e.code == "S11"));
        assert!(!report.errors.iter().any(|e| e.code == "S14"));
    }

    // ── @api ──────────────────────────────────────────────────────────────────

    #[test]
    fn ffi_bare_api_uses_function_name() {
        // @api with no argument is valid; the function name is used as the symbol
        let report = analyze(
            r#"
@api unsafe fn puts(text: *i8) i32;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "bare @api should be valid: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_api_with_explicit_symbol_is_valid() {
        let report = analyze(
            r#"
@api("write") unsafe fn sys_write(fd: i32, buf: *u8, count: usize) isize;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "explicit @api(\"symbol\") should be valid: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_api_with_body_is_rejected() {
        let report = analyze(
            r#"
@api("c_func") unsafe fn c_func(x: i32) i32 { ret x; }
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S06"),
            "S06 expected for @api with body: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_api_must_be_unsafe() {
        // @api functions are bodyless; calling them without unsafe => S11
        let report = analyze(
            r#"
@api("c_func") unsafe fn c_func(x: i32) i32;
fn main() void {
    var r: i32 = c_func(1);
}
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S11"),
            "S11 expected when calling @api outside unsafe: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_api_call_inside_unsafe_block_is_valid() {
        let report = analyze(
            r#"
@api("c_add") unsafe fn c_add(a: i32, b: i32) i32;
fn main() void {
    var r: i32 = 0;
    unsafe { r = c_add(1, 2); }
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "calling @api inside unsafe block must be valid: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_api_invalid_attribute_arg_rejected() {
        // @api(42) — integer not valid for @api
        let report = analyze(
            r#"
@api(42) unsafe fn bad(x: i32) i32;
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S06"),
            "S06 expected for invalid @api arg: {:?}",
            report.errors
        );
    }

    // ── @export ───────────────────────────────────────────────────────────────

    #[test]
    fn ffi_export_requires_pub() {
        let report = analyze(
            r#"
@export("quazi_fn") fn not_public(x: i32) i32 { ret x; }
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S06"),
            "S06 expected for non-pub @export: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_export_requires_body() {
        let report = analyze(
            r#"
@export("quazi_fn") pub unsafe fn no_body(x: i32) i32;
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S06"),
            "S06 expected for bodyless @export: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_export_bare_uses_function_name() {
        // Bare @export (no symbol argument) should be valid and use fn name
        let report = analyze(
            r#"
@export pub fn quazi_add(a: i32, b: i32) i32 { ret a + b; }
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "bare @export should be valid: {:?}",
            report.errors
        );
        assert_eq!(
            report.exported_symbols.get("quazi_add"),
            Some(&"quazi_add".to_string()),
        );
    }

    #[test]
    fn ffi_export_with_explicit_symbol_is_recorded() {
        let report = analyze(
            r#"
@export("my_lib_add") pub fn add(a: i32, b: i32) i32 { ret a + b; }
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "explicit @export should be valid: {:?}",
            report.errors
        );
        assert_eq!(
            report.exported_symbols.get("add"),
            Some(&"my_lib_add".to_string()),
        );
    }

    #[test]
    fn ffi_export_cannot_combine_with_api() {
        let report = analyze(
            r#"
@api("x") @export("y") pub fn bad(x: i32) i32 { ret x; }
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S06"),
            "S06 expected for @export + @api combination: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_syscall_and_api_cannot_be_combined() {
        let report = analyze(
            r#"
@syscall("write") @api("write") unsafe fn bad_write(fd: i32, buf: *u8, n: usize) isize;
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S06"),
            "S06 expected for @syscall + @api: {:?}",
            report.errors
        );
    }

    // ── FFI signature restrictions ────────────────────────────────────────────

    #[test]
    fn ffi_accepts_float_parameter() {
        let report = analyze(
            r#"
@api("c_fn") unsafe fn c_fn(x: f32) f32;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "float FFI param: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_accepts_float64_return() {
        let report = analyze(
            r#"
@api("get_pi") unsafe fn get_pi() f64;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "f64 FFI return: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_rejects_str_parameter() {
        let report = analyze(
            r#"
@api("c_fn") unsafe fn c_fn(s: str) void;
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S14"),
            "S14 expected for str FFI param: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_rejects_variadic_parameter() {
        let report = analyze(
            r#"
@api("printf") unsafe fn c_printf(fmt: *i8, ...args: i32) i32;
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S14"),
            "S14 expected for variadic FFI: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_accepts_stack_parameters_after_six_registers() {
        let report = analyze(
            r#"
@api("too_many") unsafe fn too_many(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32, g: i32) i32;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "7+ FFI params: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_allows_exactly_six_params() {
        let report = analyze(
            r#"
@api("six_args") unsafe fn six_args(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32) i32;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "six params should be allowed: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_rejects_generic_api_function() {
        let report = analyze(
            r#"
@api("gen_fn") unsafe fn gen_fn[T](x: i32) i32;
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S14"),
            "S14 expected for generic FFI function: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_rejects_generic_export_function() {
        let report = analyze(
            r#"
@export("gen_export") pub fn gen_export[T](x: i32) i32 { ret x; }
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S14"),
            "S14 expected for generic @export: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_allows_void_return() {
        let report = analyze(
            r#"
@api("free") unsafe fn c_free(ptr: *u8) void;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "void return from FFI should be valid: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_allows_bool_parameter_and_return() {
        let report = analyze(
            r#"
@api("c_flag") unsafe fn c_flag(enabled: bool) bool;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "bool FFI param/return should be valid: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_allows_all_integer_widths() {
        let report = analyze(
            r#"
@api("i8_fn") unsafe fn i8_fn(a: i8) i8;
@api("i16_fn") unsafe fn i16_fn(a: i16) i16;
@api("i32_fn") unsafe fn i32_fn(a: i32) i32;
@api("i64_fn") unsafe fn i64_fn(a: i64) i64;
@api("u8_fn") unsafe fn u8_fn(a: u8) u8;
@api("u16_fn") unsafe fn u16_fn(a: u16) u16;
@api("u32_fn") unsafe fn u32_fn(a: u32) u32;
@api("u64_fn") unsafe fn u64_fn(a: u64) u64;
@api("usize_fn") unsafe fn usize_fn(a: usize) usize;
@api("isize_fn") unsafe fn isize_fn(a: isize) isize;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "all integer widths should be valid FFI types: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_allows_raw_pointer_parameter() {
        let report = analyze(
            r#"
@api("memcpy") unsafe fn c_memcpy(dst: *u8, src: *u8, n: usize) *u8;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "raw pointer FFI params/return should be valid: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_type_alias_resolves_for_validation() {
        // Type aliases (like c_int = i32) must be resolved before FFI validation
        let report = analyze(
            r#"
type c_int = i32;
type c_char = i8;
@api("strlen") unsafe fn c_strlen(s: *c_char) usize;
@export("quazi_sum") pub fn qz_sum(a: c_int, b: c_int) c_int { ret a + b; }
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "type aliases should resolve correctly for FFI: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_type_alias_to_float_is_accepted() {
        let report = analyze(
            r#"
type c_float = f32;
@api("c_fn") unsafe fn c_fn(x: c_float) c_float;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "float alias in FFI: {:?}",
            report.errors
        );
    }

    // ── @repr(C) ──────────────────────────────────────────────────────────────

    #[test]
    fn ffi_repr_c_basic_layout() {
        // struct { i8, i32 } → size 8 with padding, offsets [0, 4]
        let report = analyze(
            r#"
@repr(C) struct S { a: i8, b: i32, }
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "basic @repr(C) should be valid: {:?}",
            report.errors
        );
        assert_eq!(report.struct_sizes.get("S"), Some(&8));
        assert_eq!(
            report.struct_field_offsets.get("S"),
            Some(&vec![("a".into(), 0), ("b".into(), 4)])
        );
    }

    #[test]
    fn ffi_repr_c_rejects_generic_struct() {
        let report = analyze(
            r#"
@repr(C) struct Pair[T] { first: i32, second: i32, }
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S14"),
            "S14 expected for generic @repr(C): {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_repr_c_rejects_str_field() {
        let report = analyze(
            r#"
@repr(C) struct Bad { name: str, value: i32, }
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S14"),
            "S14 expected for str field in @repr(C): {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_repr_c_accepts_float_fields() {
        let report = analyze(
            r#"
@repr(C) struct Bad { x: f32, y: f32, }
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "float @repr(C) fields: {:?}",
            report.errors
        );
        assert_eq!(report.struct_sizes.get("Bad"), Some(&8));
    }

    #[test]
    fn ffi_repr_c_pointer_fields_valid() {
        // All fields are raw pointers — valid in @repr(C) phase one
        let report = analyze(
            r#"
@repr(C) struct Node { next: *Node, prev: *Node, data: *u8, }
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "pointer-only @repr(C) struct should be valid: {:?}",
            report.errors
        );
        // All pointers = 8 bytes each, naturally aligned → size 24, offsets 0,8,16
        assert_eq!(report.struct_sizes.get("Node"), Some(&24));
        assert_eq!(
            report.struct_field_offsets.get("Node"),
            Some(&vec![
                ("next".into(), 0),
                ("prev".into(), 8),
                ("data".into(), 16),
            ])
        );
    }

    #[test]
    fn ffi_repr_c_all_integer_fields_layout() {
        // struct { i8, i8, i16, i32 } → should be 8 bytes total with C layout
        let report = analyze(
            r#"
@repr(C) struct Packed { a: i8, b: i8, c: i16, d: i32, }
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "all-integer @repr(C) struct: {:?}",
            report.errors
        );
        assert_eq!(report.struct_sizes.get("Packed"), Some(&8));
        assert_eq!(
            report.struct_field_offsets.get("Packed"),
            Some(&vec![
                ("a".into(), 0),
                ("b".into(), 1),
                ("c".into(), 2),
                ("d".into(), 4),
            ])
        );
    }

    #[test]
    fn ffi_repr_wrong_arg_rejected() {
        let report = analyze(
            r#"
@repr(Rust) struct Bad { x: i32, }
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S14"),
            "S14 expected for @repr(Rust): {:?}",
            report.errors
        );
    }

    // ── @opaque ───────────────────────────────────────────────────────────────

    #[test]
    fn ffi_opaque_empty_struct_valid() {
        let report = analyze(
            r#"
@opaque pub struct sqlite3 {}
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "empty @opaque struct should be valid: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_opaque_with_fields_rejected() {
        let report = analyze(
            r#"
@opaque pub struct Bad { x: i32, }
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S14"),
            "S14 expected for non-empty @opaque: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_opaque_generic_rejected() {
        let report = analyze(
            r#"
@opaque pub struct GenHandle[T] {}
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S14"),
            "S14 expected for generic @opaque: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_opaque_used_as_pointer_param() {
        // @opaque types should be usable as raw-pointer FFI params
        let report = analyze(
            r#"
@opaque pub struct sqlite3 {}
@api("sqlite3_close") unsafe fn sqlite3_close(db: *sqlite3) i32;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "@opaque pointer param should be valid: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_opaque_double_pointer_param_via_double_star() {
        // sqlite3_open uses **sqlite3 — double pointer via StarStar token is now
        // supported: the parser treats **T as *(*T).
        let report = analyze(
            r#"
@opaque pub struct sqlite3 {}
@api("sqlite3_open") unsafe fn sqlite3_open(filename: *i8, database: **sqlite3) i32;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "**opaque param should be valid after StarStar parser fix: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_double_star_parses_as_nested_pointer() {
        // Verify that **T parses as RawPtr { inner: RawPtr { inner: T } } without
        // a panic. The semantic check then accepts *(*T) as a valid FFI type.
        let report = analyze(
            r#"
@api("f") unsafe fn f(p: **i32) **i32;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "**T should parse and be a valid FFI type after fix: {:?}",
            report.errors
        );
    }

    // ── @repr(C) struct through pointer in FFI ────────────────────────────────

    #[test]
    fn ffi_repr_c_struct_through_pointer_is_valid() {
        let report = analyze(
            r#"
@repr(C) struct Record { tag: i8, value: i32, }
@api("read_record") unsafe fn read_record(r: *Record) i32;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "@repr(C) struct through pointer should be a valid FFI param: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_repr_c_struct_by_value_is_accepted() {
        let report = analyze(
            r#"
@repr(C) struct Vec2 { x: i32, y: i32, }
@api("use_vec") unsafe fn use_vec(v: Vec2) i32;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "@repr(C) by value: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_repr_c_struct_return_by_value_is_accepted() {
        let report = analyze(
            r#"
@repr(C) struct Vec2 { x: i32, y: i32, }
@api("make_vec") unsafe fn make_vec() Vec2;
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "@repr(C) return: {:?}",
            report.errors
        );
    }

    // ── nullptr ───────────────────────────────────────────────────────────────

    #[test]
    fn ffi_nullptr_usage_valid() {
        let report = analyze(
            r#"
pub unsafe fn nullptr[T]() *T { ret 0; }
@opaque pub struct Ctx {}
fn main() void {
    unsafe {
        var p: *Ctx = nullptr[Ctx]();
    }
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "nullptr[T]() should be usable: {:?}",
            report.errors
        );
    }

    // ── @export symbol recording ──────────────────────────────────────────────

    #[test]
    fn ffi_multiple_exports_all_recorded() {
        let report = analyze(
            r#"
@export("quazi_add") pub fn add(a: i32, b: i32) i32 { ret a + b; }
@export("quazi_sub") pub fn sub(a: i32, b: i32) i32 { ret a - b; }
@export("quazi_mul") pub fn mul(a: i32, b: i32) i32 { ret a * b; }
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "multiple exports should all be valid: {:?}",
            report.errors
        );
        assert_eq!(
            report.exported_symbols.get("add"),
            Some(&"quazi_add".to_string())
        );
        assert_eq!(
            report.exported_symbols.get("sub"),
            Some(&"quazi_sub".to_string())
        );
        assert_eq!(
            report.exported_symbols.get("mul"),
            Some(&"quazi_mul".to_string())
        );
    }

    #[test]
    fn ffi_export_pointer_return_valid() {
        // @export does not exempt from S12: a raw-pointer return still requires
        // `unsafe fn` on the Quazi side. The exported symbol is recorded correctly.
        let report = analyze(
            r#"
@export("quazi_buf") pub unsafe fn get_buf() *u8 { ret 0; }
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "pointer-returning @export with unsafe fn should be valid: {:?}",
            report.errors
        );
        assert_eq!(
            report.exported_symbols.get("get_buf"),
            Some(&"quazi_buf".to_string())
        );
    }

    // ── Calling @api inside unsafe fn ────────────────────────────────────────

    #[test]
    fn ffi_api_callable_inside_unsafe_fn() {
        let report = analyze(
            r#"
@api("c_work") unsafe fn c_work(x: i32) i32;
unsafe fn do_work(x: i32) i32 {
    ret c_work(x);
}
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "@api callable from unsafe fn: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_foreign_global_supports_unsafe_reads_and_writes() {
        let report = analyze(
            r#"
@api("native_counter") pub var counter: i32;
fn main() i32 {
    var result: i32 = 0;
    unsafe {
        counter += 1;
        result = counter;
    }
    ret result;
}
"#,
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let global = report.foreign_globals.get("counter").unwrap();
        assert_eq!(global.symbol, "native_counter");
        assert!(matches!(global.ty, crate::parser::ast::TypeKind::Int32));
    }

    #[test]
    fn ffi_foreign_global_requires_api_and_unsafe_access() {
        let report = analyze(
            r#"
var missing_api: i32;
@api("native_counter") var counter: i32;
fn main() i32 {
    counter = 1;
    ret counter;
}
"#,
        );
        assert!(report.errors.iter().any(|error| {
            error.code == "S14" && error.message.contains("requires exactly one @api")
        }));
        assert!(
            report
                .errors
                .iter()
                .any(|error| { error.code == "S11" && error.message.contains("foreign global") })
        );
    }

    // ── S14 from export, not just @api ────────────────────────────────────────

    #[test]
    fn ffi_export_with_float_param_accepted() {
        let report = analyze(
            r#"
@export("quazi_fn") pub fn qz_fn(x: f64) i32 { ret 0; }
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "float @export param: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_export_with_str_return_rejected() {
        let report = analyze(
            r#"
@export("quazi_fn") pub fn qz_fn() str { ret "hello"; }
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S14"),
            "S14 expected for str return in @export: {:?}",
            report.errors
        );
    }

    // ── repr(C) layout: tail padding ─────────────────────────────────────────

    #[test]
    fn ffi_repr_c_tail_padding_applied() {
        // struct { i32, i8 } → size should be 8 (tail padding to i32 align)
        let report = analyze(
            r#"
@repr(C) struct TailPad { x: i32, y: i8, }
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "tail-padded @repr(C) should be valid: {:?}",
            report.errors
        );
        assert_eq!(report.struct_sizes.get("TailPad"), Some(&8));
        assert_eq!(
            report.struct_field_offsets.get("TailPad"),
            Some(&vec![("x".into(), 0), ("y".into(), 4)])
        );
    }

    #[test]
    fn ffi_repr_c_single_pointer_field() {
        let report = analyze(
            r#"
@repr(C) struct Handle { ptr: *u8, }
fn main() void { }
"#,
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.struct_sizes.get("Handle"), Some(&8));
        assert_eq!(
            report.struct_field_offsets.get("Handle"),
            Some(&vec![("ptr".into(), 0)])
        );
    }
    // ── C variadics: bare `...` in @api ──────────────────────────────────────

    #[test]
    fn ffi_c_variadic_bare_dots_accepted() {
        let report = analyze(
            r#"
@api("printf") unsafe fn c_printf(fmt: *i8, ...) i32;
unsafe fn caller() i32 {
    ret c_printf(0, 1 as i32, 2 as i32);
}
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "bare `...` in @api should be accepted: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_c_variadic_quazi_style_rejected_in_api() {
        let report = analyze(
            r#"
@api("printf") unsafe fn c_printf(fmt: *i8, ...args: i32) i32;
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S14"),
            "Quazi-style variadic in @api should be rejected with S14: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_c_variadic_rejects_non_c_extra_argument() {
        let report = analyze(
            r#"
@api("printf") unsafe fn c_printf(fmt: *i8, ...) i32;
unsafe fn caller() i32 { ret c_printf(0, "not a C string pointer"); }
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|error| error.code == "S14"),
            "C variadic extras need a concrete C ABI type: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_repr_c_empty_struct_is_rejected() {
        let report = analyze(
            r#"
@repr(C) struct Empty { }
fn main() void { }
"#,
        );
        assert!(
            report.errors.iter().any(|error| error.code == "S14"),
            "portable C layout cannot represent an empty struct: {:?}",
            report.errors
        );
    }

    #[test]
    fn byte_strings_have_length_index_and_pointer_methods() {
        let report = analyze(
            r#"
fn main() void {
    var value: bytes = b"A\xFF\0";
    var length: usize = value.len();
    var byte: u8 = value[1];
    var pointer: *u8 = value.as_ptr();
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "byte-string operations should typecheck: {:?}",
            report.errors
        );
    }

    #[test]
    fn ffi_union_packed_alignment_and_bitfield_layouts_are_recorded() {
        let report = analyze(
            r#"
@repr(C) union Number { integer: i32, decimal: f64 }
@repr(C, packed, align=8) struct Header { tag: u8, value: u32 }
@repr(C) struct Flags { low: u32:3, high: u32:5, tail: u16:4 }

fn main() void {
    var number = Number { integer: 7 };
    unsafe {
        var value: i32 = number.integer;
        number.decimal = 1.0;
    }
}
"#,
        );
        assert!(
            report.errors.is_empty(),
            "aggregate layouts: {:?}",
            report.errors
        );
        assert!(report.repr_c_unions.contains("Number"));
        assert_eq!(report.struct_sizes.get("Number"), Some(&8));
        assert_eq!(
            report.struct_field_offsets.get("Header"),
            Some(&vec![("tag".to_string(), 0), ("value".to_string(), 1),])
        );
        assert_eq!(report.struct_sizes.get("Header"), Some(&8));
        assert_eq!(report.struct_alignments.get("Header"), Some(&8));
        let flags = report.bit_field_layouts.get("Flags").unwrap();
        assert_eq!(flags["low"].bit_offset, 0);
        assert_eq!(flags["high"].bit_offset, 3);
        assert_eq!(flags["tail"].byte_offset, 4);
        assert_eq!(report.struct_sizes.get("Flags"), Some(&8));
    }

    #[test]
    fn ffi_c_function_pointer_alias_accepts_exports_and_is_unsafe_to_call() {
        let report = analyze(
            r#"
@repr(C) pub type CompareFn = fn(*u8, *u8) i32;

@export("compare_bytes")
pub unsafe fn compare_bytes(left: *u8, right: *u8) i32 { ret 0; }

@api("get_compare") unsafe fn get_compare() CompareFn;
@api("set_compare") unsafe fn set_compare(callback: CompareFn);

fn main() void {
    unsafe {
        var local: CompareFn = compare_bytes;
        set_compare(local);
        var foreign: CompareFn = get_compare();
        foreign(0, 0);
    }
}
"#,
        );
        assert!(report.errors.is_empty(), "C callbacks: {:?}", report.errors);

        let unsafe_call = analyze(
            r#"
@repr(C) type Callback = fn(i32) i32;
fn invoke(callback: Callback) i32 { ret callback(1); }
fn main() void { }
"#,
        );
        assert!(
            unsafe_call
                .errors
                .iter()
                .any(|error| { error.message.contains("C function pointer requires unsafe") })
        );
    }

    #[test]
    fn ffi_c_function_pointer_rejects_non_exported_function_coercion() {
        let report = analyze(
            r#"
@repr(C) type Callback = fn(i32) i32;
fn ordinary(value: i32) i32 { ret value; }
fn main() void { var callback: Callback = ordinary; }
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|error| { error.message.contains("type mismatch") })
        );
    }

    #[test]
    fn ffi_c_function_pointer_address_cast_requires_unsafe() {
        let safe = analyze(
            r#"
@repr(C) type Callback = fn(i32) i32;
fn cast(address: usize) Callback { ret address as Callback; }
"#,
        );
        assert!(safe.errors.iter().any(|error| error.code == "S11"));

        let unsafe_cast = analyze(
            r#"
@repr(C) type Callback = fn(i32) i32;
unsafe fn cast(address: usize) Callback { ret address as Callback; }
"#,
        );
        assert!(
            unsafe_cast.errors.is_empty(),
            "unsafe callback cast: {:?}",
            unsafe_cast.errors
        );
    }

    #[test]
    fn ffi_flexible_array_is_final_pointer_only_and_has_zero_size_contribution() {
        let report = analyze(
            r#"
@repr(C) struct Packet { length: u32, data: [u8; ..] }
@api("consume") unsafe fn consume(packet: *Packet);
unsafe fn first(packet: *Packet) u8 { ret (*packet).data[0]; }
fn main() void { }
"#,
        );
        assert!(
            report.errors.is_empty(),
            "flexible array layout: {:?}",
            report.errors
        );
        assert_eq!(report.struct_sizes.get("Packet"), Some(&4));
        assert_eq!(
            report.struct_field_offsets.get("Packet"),
            Some(&vec![("length".to_string(), 0), ("data".to_string(), 4),])
        );

        let by_value = analyze(
            r#"
@repr(C) struct Packet { length: u32, data: [u8; ..] }
@api("consume") unsafe fn consume(packet: Packet);
fn main() void { }
"#,
        );
        assert!(
            by_value
                .errors
                .iter()
                .any(|error| { error.message.contains("unsupported C ABI type") })
        );
    }

    #[test]
    fn byte_strings_are_immutable() {
        let report = analyze(
            r#"
fn main() void {
    var value = b"abc";
    value[0] = 1;
}
"#,
        );
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.message.contains("byte strings are immutable")),
            "byte-string writes should be rejected: {:?}",
            report.errors
        );
    }
}
