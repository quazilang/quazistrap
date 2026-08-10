// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use super::instruction::{
    FLOAT_FLAG, MemWidth, call_c_reg, field_load, field_load_typed, field_store,
    field_store_typed, mem_lea, mem_load, mem_load_w, mem_store, mem_store_w, ri16, rrr, rrr_f,
};
use super::{Chunk, ConstPoolEntry, Opcode};
use crate::abi::{
    AbiField, AbiSignature, AbiType, ForeignGlobal as AbiForeignGlobal, ForeignSymbol,
};
use crate::parser::ast::*;
use crate::semantic::types::{SourceFile, SymbolKind};
use crate::semantic::{ConstValue, DependencyKind, SemanticReport};

/// Find the source file path for a given span.
fn source_file_for_span(span: Span, source_files: &[SourceFile]) -> String {
    source_files
        .iter()
        .find(|sf| sf.contains(span))
        .map(|sf| sf.path.clone())
        .unwrap_or_else(|| "<unknown>".to_string())
}

/// Derive module name from a span's source file (e.g. "unix" from "std/src/unix.qz").
/// For "mod.qz", uses the parent directory name (e.g. "prelude" from "prelude/mod.qz").
fn module_name_for_span(span: Span, source_files: &[SourceFile]) -> Option<String> {
    source_files
        .iter()
        .find(|sf| sf.contains(span))
        .and_then(|sf| {
            let path = std::path::Path::new(&sf.path);
            let stem = path.file_stem()?.to_str()?;
            if stem == "mod" {
                path.parent()?.file_name()?.to_str().map(|s| s.to_string())
            } else {
                Some(stem.to_string())
            }
        })
}

// Enum heap layout invariants:
//   - discriminant (tag) is stored at ENUM_DISCRIM_OFFSET (8 bytes)
//   - variant payloads start at ENUM_PAYLOAD_OFFSET (8 bytes each)
const ENUM_DISCRIM_OFFSET: u8 = 0;
const ENUM_PAYLOAD_OFFSET: u16 = 8;

/// Compute the allocation size (in bytes) for an enum variant with `payload_count` payloads.
/// The layout is: discriminant (8 bytes) + payload_count * 8 bytes per payload,
/// with a minimum of 16 bytes.
fn enum_variant_alloc_size(payload_count: usize) -> u16 {
    ((payload_count + 1) * 8).max(16) as u16
}

fn abi_type_from_layout(
    ty: &TypeKind,
    struct_defs: &HashMap<String, Vec<(String, TypeKind)>>,
    struct_sizes: &HashMap<String, usize>,
    struct_field_offsets: &HashMap<String, Vec<(String, usize)>>,
    struct_alignments: &HashMap<String, usize>,
    bit_field_layouts: &HashMap<String, HashMap<String, crate::semantic::BitFieldLayout>>,
    repr_c_structs: &HashSet<String>,
    type_aliases: &HashMap<String, (Vec<String>, TypeKind)>,
) -> Option<AbiType> {
    let resolved = match ty {
        TypeKind::Named { name, type_args } if type_args.is_empty() => {
            if let Some((params, target)) = type_aliases.get(name)
                && params.is_empty()
            {
                return abi_type_from_layout(
                    target,
                    struct_defs,
                    struct_sizes,
                    struct_field_offsets,
                    struct_alignments,
                    bit_field_layouts,
                    repr_c_structs,
                    type_aliases,
                );
            }
            ty
        }
        _ => ty,
    };

    let integer = |bytes, signed| AbiType::Integer { bytes, signed };
    match resolved {
        TypeKind::Int8 => Some(integer(1, true)),
        TypeKind::Int16 => Some(integer(2, true)),
        TypeKind::Int32 => Some(integer(4, true)),
        TypeKind::Int64 | TypeKind::Isize => Some(integer(8, true)),
        TypeKind::Uint8 | TypeKind::Bool => Some(integer(1, false)),
        TypeKind::Uint16 => Some(integer(2, false)),
        TypeKind::Uint32 => Some(integer(4, false)),
        TypeKind::Uint64 | TypeKind::Usize => Some(integer(8, false)),
        TypeKind::Float32 => Some(AbiType::Float32),
        TypeKind::Float64 => Some(AbiType::Float64),
        TypeKind::RawPtr { .. } => Some(AbiType::Pointer),
        TypeKind::CFn { .. } => Some(AbiType::Pointer),
        TypeKind::Array { elem_ty, len } => {
            let elem = abi_type_from_layout(
                &elem_ty.node,
                struct_defs,
                struct_sizes,
                struct_field_offsets,
                struct_alignments,
                bit_field_layouts,
                repr_c_structs,
                type_aliases,
            )?;
            let elem_size = elem.size();
            let fields = (0..*len)
                .map(|index| {
                    Some(AbiField {
                        offset: u16::try_from(index as usize * elem_size).ok()?,
                        ty: elem.clone(),
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(AbiType::Aggregate {
                size: u16::try_from(elem_size * *len as usize).ok()?,
                align: u8::try_from(elem.align()).ok()?,
                fields,
            })
        }
        TypeKind::Void | TypeKind::Never => Some(AbiType::Void),
        TypeKind::Named { name, type_args }
            if type_args.is_empty() && repr_c_structs.contains(name) =>
        {
            let defs = struct_defs.get(name)?;
            let offsets = struct_field_offsets.get(name)?;
            let size = u16::try_from(*struct_sizes.get(name)?).ok()?;
            let mut fields = Vec::with_capacity(defs.len());
            let bit_layouts = bit_field_layouts.get(name);
            let mut emitted_bit_units = HashSet::new();
            for ((field_name, field_ty), (offset_name, offset)) in defs.iter().zip(offsets) {
                if field_name != offset_name {
                    return None;
                }
                if matches!(field_ty, TypeKind::FlexibleArray { .. }) {
                    continue;
                }
                if let Some(bit) = bit_layouts.and_then(|layouts| layouts.get(field_name)) {
                    if emitted_bit_units.insert((bit.byte_offset, bit.storage_bytes)) {
                        fields.push(AbiField {
                            offset: u16::try_from(bit.byte_offset).ok()?,
                            ty: AbiType::Integer {
                                bytes: bit.storage_bytes,
                                signed: false,
                            },
                        });
                    }
                    continue;
                }
                let abi_ty = abi_type_from_layout(
                    field_ty,
                    struct_defs,
                    struct_sizes,
                    struct_field_offsets,
                    struct_alignments,
                    bit_field_layouts,
                    repr_c_structs,
                    type_aliases,
                )?;
                fields.push(AbiField {
                    offset: u16::try_from(*offset).ok()?,
                    ty: abi_ty,
                });
            }
            Some(AbiType::Aggregate {
                size,
                align: u8::try_from(*struct_alignments.get(name)?).ok()?,
                fields,
            })
        }
        _ => None,
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

pub struct Codegen<'a> {
    report: &'a SemanticReport,
    fn_index: HashMap<String, u16>,
    const_map: HashMap<(usize, usize), ConstValue>,
    type_map: HashMap<(usize, usize), TypeKind>,
    /// Spans marked for auto-deref by semantic analysis.
    autoderef_map: HashMap<(usize, usize), bool>,
    import_names: HashSet<String>,
    /// Maps variadic function name → number of fixed (non-variadic) params.
    variadic_fn_info: HashMap<String, usize>,
    /// Variadic-str functions: compiler auto-coerces args to str at call sites.
    str_variadic_fns: HashSet<String>,
    /// Variadic @intrinsic functions: call with coerced args directly (no pre-format step).
    variadic_intrinsic_fns: HashSet<String>,
    /// Resolved Quazi function name -> portable C import description.
    foreign_imports: HashMap<String, ForeignSymbol>,
    /// Resolved Quazi function name -> portable C export description.
    foreign_exports: HashMap<String, ForeignSymbol>,
    source_files: Vec<SourceFile>,
}

impl<'a> Codegen<'a> {
    pub fn new(report: &'a SemanticReport) -> Self {
        let mut const_map = HashMap::new();
        let mut type_map = HashMap::new();
        let mut autoderef_map = HashMap::new();
        for ann in &report.annotated_exprs {
            let key = (ann.span.start, ann.span.end);
            if let Some(cv) = &ann.const_value {
                const_map.insert(key, cv.clone());
            }
            if let Some(ty) = &ann.ty {
                type_map.insert(key, ty.clone());
            }
            if ann.auto_deref {
                autoderef_map.insert(key, true);
            }
        }
        let mut import_names = HashSet::new();
        for entry in &report.symbol_table.entries {
            if entry.symbol.is_import {
                import_names.insert(entry.name.clone());
            }
        }
        let mut variadic_fn_info = HashMap::new();
        for entry in &report.symbol_table.entries {
            if entry.symbol.variadic {
                let fixed = entry.symbol.params.len().saturating_sub(1);
                variadic_fn_info.insert(entry.name.clone(), fixed);
            }
        }
        Self {
            report,
            fn_index: HashMap::new(),
            const_map,
            type_map,
            autoderef_map,
            import_names,
            variadic_fn_info,
            str_variadic_fns: HashSet::new(),
            variadic_intrinsic_fns: HashSet::new(),
            foreign_imports: HashMap::new(),
            foreign_exports: HashMap::new(),
            source_files: Vec::new(),
        }
    }

    /// Return the resolved symbol name for a top-level item defined at `span`.
    /// Namespaced files use `module.name`; entry files use the bare `name`.
    /// Internal runtime symbols (`__quazi_*`) keep their bare names.
    /// `@no_mangle` functions keep their bare name regardless of file namespace.
    /// `@export` functions keep their source identity here; a synthetic adapter
    /// receives the external symbol and C ABI metadata after body compilation.
    fn resolve_item_name(&self, span: Span, name: &str, attributes: &[Attribute]) -> String {
        if name.starts_with("__quazi_") {
            return name.to_string();
        }
        if attributes.iter().any(|a| a.name == "no_mangle") {
            return name.to_string();
        }
        if let Some(sf) = self.source_files.iter().find(|f| f.contains(span))
            && self.report.namespaced_paths.contains(&sf.path)
            && let Some(module) = module_name_for_span(span, &self.source_files)
        {
            return format!("{}.{}", module, name);
        }
        name.to_string()
    }

    fn abi_type(&self, ty: &TypeKind) -> Option<AbiType> {
        abi_type_from_layout(
            ty,
            &self.report.struct_defs,
            &self.report.struct_sizes,
            &self.report.struct_field_offsets,
            &self.report.struct_alignments,
            &self.report.bit_field_layouts,
            &self.report.repr_c_structs,
            &self.report.type_aliases,
        )
    }

    pub fn compile_program(
        &mut self,
        program: &Program,
        source_files: &[SourceFile],
    ) -> Result<Vec<Chunk>, String> {
        self.source_files = source_files.to_vec();
        self.foreign_imports.clear();
        self.foreign_exports.clear();
        for item in &program.items {
            let ItemKind::Fn {
                name,
                return_ty,
                params,
                attributes,
                c_variadic,
                ..
            } = &item.node
            else {
                continue;
            };
            if !item_cfg_active(attributes) {
                continue;
            }
            let Some(kind) = attributes
                .iter()
                .find(|attr| attr.name == "api" || attr.name == "export")
            else {
                continue;
            };
            let Some(param_types) = params
                .iter()
                .map(|param| self.abi_type(&param.ty.node))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let Some(return_type) = self.abi_type(&return_ty.node) else {
                continue;
            };
            let resolved_name = self.resolve_item_name(item.span, name, attributes);
            let symbol = api_symbol(kind).unwrap_or_else(|| name.clone());
            let foreign = ForeignSymbol {
                symbol,
                signature: AbiSignature {
                    params: param_types,
                    return_type,
                    variadic: *c_variadic,
                },
            };
            if kind.name == "api" {
                self.foreign_imports.insert(resolved_name, foreign);
            } else {
                self.foreign_exports.insert(resolved_name, foreign);
            }
        }
        // Pre-pass: collect str_variadic fns, variadic @intrinsic, and @panic_handler names.
        let mut user_panic_handler: Option<String> = None;
        for item in &program.items {
            // Skip @cfg-disabled items.
            if let ItemKind::Fn { attributes, .. } = &item.node
                && !item_cfg_active(attributes)
            {
                continue;
            }
            match &item.node {
                ItemKind::Fn {
                    name,
                    attributes,
                    params,
                    ..
                } => {
                    let has_str_variadic_param = params
                        .last()
                        .map(|p| {
                            if !p.variadic {
                                return false;
                            }
                            if matches!(
                                &p.ty.node,
                                crate::parser::ast::TypeKind::Str
                                    | crate::parser::ast::TypeKind::Ref { .. }
                            ) {
                                return true;
                            }
                            matches!(&p.ty.node, crate::parser::ast::TypeKind::Any)
                                && params.iter().filter(|q| !q.variadic).any(|q| {
                                    matches!(
                                        &q.ty.node,
                                        crate::parser::ast::TypeKind::Str
                                            | crate::parser::ast::TypeKind::Ref { .. }
                                    )
                                })
                        })
                        .unwrap_or(false);
                    let resolved_name = self.resolve_item_name(item.span, name, attributes);
                    if has_str_variadic_param {
                        self.str_variadic_fns.insert(resolved_name.clone());
                    }
                    if attributes.iter().any(|a| a.name == "intrinsic")
                        && params.last().map(|p| p.variadic).unwrap_or(false)
                    {
                        self.variadic_intrinsic_fns.insert(resolved_name.clone());
                    }
                    if attributes.iter().any(|a| a.name == "panic_handler") {
                        user_panic_handler = Some(resolved_name.clone());
                    }
                }
                ItemKind::Impl {
                    for_ty, methods, ..
                } => {
                    let type_name = type_kind_base_name(&for_ty.node);
                    for method in methods {
                        if let ItemKind::Fn {
                            name,
                            attributes: _,
                            params,
                            ..
                        } = &method.node
                        {
                            let has_str_var = params
                                .last()
                                .map(|p| {
                                    if !p.variadic {
                                        return false;
                                    }
                                    if matches!(
                                        &p.ty.node,
                                        crate::parser::ast::TypeKind::Str
                                            | crate::parser::ast::TypeKind::Ref { .. }
                                    ) {
                                        return true;
                                    }
                                    matches!(&p.ty.node, crate::parser::ast::TypeKind::Any)
                                        && params.iter().filter(|q| !q.variadic).any(|q| {
                                            matches!(
                                                &q.ty.node,
                                                crate::parser::ast::TypeKind::Str
                                                    | crate::parser::ast::TypeKind::Ref { .. }
                                            )
                                        })
                                })
                                .unwrap_or(false);
                            if has_str_var {
                                self.str_variadic_fns
                                    .insert(format!("{}.{}", type_name, name));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Propagate alias imports: `import foo.bar as baz` → add baz to str_variadic_fns
        // and variadic_fn_info if the original is there. The import_path stores the
        // module-qualified target name (e.g. "core.write"), so look up by that.
        for entry in &self.report.symbol_table.entries {
            let sym = &entry.symbol;
            if matches!(sym.kind, SymbolKind::Function)
                && sym.is_import
                && let Some(path) = &sym.import_path
                && path != &entry.name
            {
                if self.str_variadic_fns.contains(path) {
                    self.str_variadic_fns.insert(entry.name.clone());
                }
                if let Some(&fixed) = self.variadic_fn_info.get(path) {
                    self.variadic_fn_info.insert(entry.name.clone(), fixed);
                }
            }
        }

        // Compute the set of functions reachable from main via the call graph.
        // Library mode (no main) compiles everything.
        let has_main = program
            .items
            .iter()
            .any(|item| matches!(&item.node, ItemKind::Fn { name, .. } if name == "main"));

        let destructor_roots = collect_destructor_roots(program);
        let reachable: Option<std::collections::HashSet<String>> = if has_main {
            let mut set = std::collections::HashSet::new();
            set.insert("main".to_string());
            set.extend(self.foreign_exports.keys().cloned());
            for root in &destructor_roots {
                set.insert(root.clone());
            }
            let mut queue = set.iter().cloned().collect::<Vec<_>>();
            while let Some(fn_name) = queue.pop() {
                for edge in &self.report.dependency_graph.edges {
                    if edge.kind == DependencyKind::Call
                        && edge.from == fn_name
                        && set.insert(edge.to.clone())
                    {
                        queue.push(edge.to.clone());
                    }
                }
            }
            // @panic_handler: the user's function is compiled under __quazi_panic_handler but
            // its own body's Call edges are indexed by the original function name. Seed BFS
            // from the original name so its dependencies are included.
            if set.contains("__quazi_panic_handler")
                && let Some(ph_name) = &user_panic_handler
                && set.insert(ph_name.clone())
            {
                let mut q2 = vec![ph_name.clone()];
                while let Some(fn_name) = q2.pop() {
                    for edge in &self.report.dependency_graph.edges {
                        if edge.kind == DependencyKind::Call
                            && edge.from == fn_name
                            && set.insert(edge.to.clone())
                        {
                            q2.push(edge.to.clone());
                        }
                    }
                }
            }
            Some(set)
        } else {
            None
        };

        let is_live = |name: &str| -> bool { reachable.as_ref().is_none_or(|r| r.contains(name)) };

        // Pass 1: assign each live function a table index.
        // Namespaced modules use `module.name`; entry files keep the bare name.
        let mut idx = 0usize;
        for item in &program.items {
            if let ItemKind::Fn {
                name, attributes, ..
            } = &item.node
            {
                let is_ph = attributes.iter().any(|a| a.name == "panic_handler");
                let is_export = attributes.iter().any(|a| a.name == "export");
                // When user has @panic_handler, skip the stdlib default.
                if name == "__quazi_panic_handler" && user_panic_handler.is_some() {
                    continue;
                }
                // @panic_handler fn is registered under the handler slot name.
                let index_name = if is_ph {
                    "__quazi_panic_handler".to_string()
                } else {
                    self.resolve_item_name(item.span, name, attributes)
                };
                if is_live(&index_name) || is_ph || is_export {
                    let table_index = u16::try_from(idx).map_err(|_| {
                        "program exceeds the QZI function-table limit".to_string()
                    })?;
                    self.fn_index.insert(index_name.clone(), table_index);
                    idx += 1;
                }
            }
        }
        // Index live impl methods as "TypeName.method_name".
        for item in &program.items {
            if let ItemKind::Impl {
                for_ty, methods, ..
            } = &item.node
            {
                let type_name = type_kind_base_name(&for_ty.node);
                for method in methods {
                    if let ItemKind::Fn { name, .. } = &method.node {
                        let mangled = format!("{}.{}", type_name, name);
                        if is_live(&mangled) {
                            let table_index = u16::try_from(idx).map_err(|_| {
                                "program exceeds the QZI function-table limit".to_string()
                            })?;
                            self.fn_index.insert(mangled.clone(), table_index);
                            idx += 1;
                        }
                    }
                }
            }
        }

        // Index monomorphized specializations.
        for mono in &self.report.monomorphizations {
            let mono_name = &mono.mangled_name;
            // Only add if the specialized name is reachable.
            if is_live(mono_name) && !self.fn_index.contains_key(mono_name) {
                let table_index = u16::try_from(idx)
                    .map_err(|_| "program exceeds the QZI function-table limit".to_string())?;
                self.fn_index.insert(mono_name.clone(), table_index);
                idx += 1;
            }
        }

        // Add alias entries to fn_index: `import foo.bar as baz` → baz → same idx as bar.
        // These are extra name entries; the primary chunk name stays unchanged.
        let alias_entries: Vec<(String, u16)> = self
            .report
            .symbol_table
            .entries
            .iter()
            .filter_map(|entry| {
                let sym = &entry.symbol;
                if matches!(sym.kind, SymbolKind::Function)
                    && sym.is_import
                    && let Some(path) = &sym.import_path
                {
                    let leaf = path.rsplit('.').next().unwrap_or(path.as_str());
                    if leaf != entry.name {
                        // Namespaced imports live under their module-qualified name.
                        let lookup = {
                            let segs: Vec<&str> = path.split('.').collect();
                            if segs.len() >= 2 {
                                format!("{}.{}", segs[segs.len() - 2], segs[segs.len() - 1])
                            } else {
                                leaf.to_string()
                            }
                        };
                        if let Some(&orig_idx) = self.fn_index.get(&lookup)
                            && !self.fn_index.contains_key(&entry.name)
                        {
                            return Some((entry.name.clone(), orig_idx));
                        }
                    }
                }
                None
            })
            .collect();
        for (alias, orig_idx) in alias_entries {
            self.fn_index.insert(alias, orig_idx);
        }

        // Pass 2: compile each live function body.
        let mut chunks = Vec::new();
        let mut next_closure_idx = 0u16;
        for item in &program.items {
            if let ItemKind::Fn {
                name,
                params,
                body,
                attributes,
                c_variadic,
                ..
            } = &item.node
            {
                if !item_cfg_active(attributes) {
                    continue;
                }
                let is_ph = attributes.iter().any(|a| a.name == "panic_handler");
                // Skip stdlib default handler when user has their own.
                if name == "__quazi_panic_handler" && user_panic_handler.is_some() {
                    continue;
                }
                // @panic_handler fn is compiled under the handler slot name.
                let compile_name = if is_ph {
                    "__quazi_panic_handler".to_string()
                } else {
                    self.resolve_item_name(item.span, name, attributes)
                };
                let is_export = attributes.iter().any(|a| a.name == "export");
                if (is_live(&compile_name) || is_ph || is_export)
                    && let Some(chunk) = self.compile_fn(
                        &compile_name,
                        params,
                        body.as_ref().map(|b| b as &Block),
                        attributes,
                        *c_variadic,
                        &mut chunks,
                        &mut next_closure_idx,
                    )?
                {
                    chunks.push(chunk);
                    if let Some(foreign) = self.foreign_exports.get(&compile_name).cloned() {
                        let fn_idx = *self
                            .fn_index
                            .get(&compile_name)
                            .expect("exported function must have a function-table index");
                        let adapter_name = export_adapter_name(&compile_name, fn_idx);
                        let mut adapter = Chunk::with_params(adapter_name, params.len());
                        adapter.export = Some(foreign);
                        adapter.reg_count = params.len().max(1) as u8;
                        for index in 0..params.len() {
                            adapter.emit(rrr(Opcode::CallArg, index as u8, 0, 0));
                        }
                        adapter.emit(ri16(Opcode::CallIdx, 0, fn_idx));
                        adapter.emit(rrr(Opcode::Ret, 0, 0, 0));
                        chunks.push(adapter);
                    }
                }
            }
        }
        // Compile live impl methods.
        for item in &program.items {
            if let ItemKind::Impl {
                for_ty, methods, ..
            } = &item.node
            {
                let type_name = type_kind_base_name(&for_ty.node);
                for method in methods {
                    if let ItemKind::Fn {
                        name,
                        params,
                        body,
                        attributes,
                        c_variadic,
                        ..
                    } = &method.node
                    {
                        if !item_cfg_active(attributes) {
                            continue;
                        }
                        let mangled = format!("{}.{}", type_name, name);
                        if is_live(&mangled)
                            && let Some(chunk) = self.compile_fn(
                                &mangled,
                                params,
                                body.as_ref().map(|b| b as &Block),
                                attributes,
                                *c_variadic,
                                &mut chunks,
                                &mut next_closure_idx,
                            )?
                        {
                            chunks.push(chunk);
                        }
                    }
                }
            }
        }

        // Compile monomorphized specializations (top-level fns and impl methods).
        let monos: Vec<_> = self.report.monomorphizations.clone();
        for mono in &monos {
            let mono_name = &mono.mangled_name;
            if !self.fn_index.contains_key(mono_name) {
                continue;
            }
            // Already compiled? Skip duplicate.
            if chunks.iter().any(|c| c.name == *mono_name) {
                continue;
            }

            if mono.fn_name.contains('.') {
                // Could be an impl method ("TypeName.method") or a module-namespaced
                // top-level generic function ("module.fn"). Try impl blocks first.
                let (type_part, method_part) = mono.fn_name.split_once('.').unwrap();
                let mut found = false;
                for item in &program.items {
                    if let ItemKind::Impl {
                        for_ty, methods, ..
                    } = &item.node
                    {
                        if type_kind_base_name(&for_ty.node) != type_part {
                            continue;
                        }
                        for m in methods {
                            if let ItemKind::Fn {
                                name,
                                params,
                                body,
                                attributes,
                                generic_params,
                                c_variadic,
                                ..
                            } = &m.node
                            {
                                if name != method_part {
                                    continue;
                                }
                                // Build substitution: struct-level generic params first, then method-level.
                                let struct_params = self
                                    .report
                                    .struct_generic_params
                                    .get(type_part)
                                    .cloned()
                                    .unwrap_or_default();
                                let all_params: Vec<String> = struct_params
                                    .into_iter()
                                    .chain(generic_params.iter().cloned())
                                    .collect();
                                let subst: HashMap<String, TypeKind> = all_params
                                    .iter()
                                    .zip(mono.type_args.iter())
                                    .map(|(p, t)| (p.clone(), t.clone()))
                                    .collect();
                                if let Some(chunk) = self.compile_fn_with_subst(
                                    mono_name,
                                    params,
                                    body.as_ref().map(|b| b as &Block),
                                    attributes,
                                    *c_variadic,
                                    &mut chunks,
                                    &mut next_closure_idx,
                                    subst,
                                )? {
                                    chunks.push(chunk);
                                }
                                found = true;
                                break;
                            }
                        }
                        if found {
                            break;
                        }
                    }
                }
                // If no impl block matched, fall through to top-level lookup. This handles
                // module-namespaced generic functions, e.g. `ffi.nullptr` whose fn_name
                // contains a dot but is not an impl method.
                if !found {
                    let original = program.items.iter().find(|item| {
                        if let ItemKind::Fn {
                            name, attributes, ..
                        } = &item.node
                        {
                            self.resolve_item_name(item.span, name, attributes) == mono.fn_name
                        } else {
                            false
                        }
                    });
                    if let Some(Item {
                        node:
                            ItemKind::Fn {
                                params,
                                body,
                                attributes,
                                generic_params,
                                c_variadic,
                                ..
                            },
                        ..
                    }) = original
                    {
                        let subst: HashMap<String, TypeKind> = generic_params
                            .iter()
                            .zip(mono.type_args.iter())
                            .map(|(p, t)| (p.clone(), t.clone()))
                            .collect();
                        if let Some(chunk) = self.compile_fn_with_subst(
                            mono_name,
                            params,
                            body.as_ref().map(|b| b as &Block),
                            attributes,
                            *c_variadic,
                            &mut chunks,
                            &mut next_closure_idx,
                            subst,
                        )? {
                            chunks.push(chunk);
                        }
                    }
                }
            } else {
                // Top-level function (bare name, no dot).
                let original = program.items.iter().find(|item| {
                    if let ItemKind::Fn {
                        name, attributes, ..
                    } = &item.node
                    {
                        self.resolve_item_name(item.span, name, attributes) == mono.fn_name
                    } else {
                        false
                    }
                });
                if let Some(Item {
                    node:
                        ItemKind::Fn {
                            params,
                            body,
                            attributes,
                            generic_params,
                            c_variadic,
                            ..
                        },
                    ..
                }) = original
                {
                    let subst: HashMap<String, TypeKind> = generic_params
                        .iter()
                        .zip(mono.type_args.iter())
                        .map(|(p, t)| (p.clone(), t.clone()))
                        .collect();
                    if let Some(chunk) = self.compile_fn_with_subst(
                        mono_name,
                        params,
                        body.as_ref().map(|b| b as &Block),
                        attributes,
                        *c_variadic,
                        &mut chunks,
                        &mut next_closure_idx,
                        subst,
                    )? {
                        chunks.push(chunk);
                    }
                }
            }
        }

        // Post-pass: inline small functions that are inline candidates.
        let inline_set: std::collections::HashSet<String> = self
            .report
            .inline_candidates
            .iter()
            .map(|c| c.name.clone())
            .collect();

        // Variadic intrinsics must always be inlined so the caller's actual arg count
        // propagates into the Intrinsic instruction's flags field.  They have no AST
        // body so the semantic pass never adds them to inline_candidates; we include
        // them here unconditionally.
        let has_variadic_intrinsics = chunks.iter().any(|c| c.variadic && c.intrinsic);

        if !inline_set.is_empty() || has_variadic_intrinsics {
            // Build a snapshot of callee chunks before mutating.
            let callee_map: std::collections::HashMap<String, Chunk> = chunks
                .iter()
                .filter(|c| inline_set.contains(&c.name) || (c.variadic && c.intrinsic))
                .map(|c| (c.name.clone(), c.clone()))
                .collect();

            // Build reverse index: fn_idx -> name
            // Build reverse index: fn_idx → primary chunk name.
            // When aliases share an idx, prefer the name that matches an actual chunk.
            let chunk_name_set: std::collections::HashSet<&str> =
                chunks.iter().map(|c| c.name.as_str()).collect();
            let idx_to_name: std::collections::HashMap<u16, String> = {
                let mut map = std::collections::HashMap::new();
                for (name, &idx) in &self.fn_index {
                    let is_primary = chunk_name_set.contains(name.as_str());
                    if is_primary || !map.contains_key(&idx) {
                        map.insert(idx, name.clone());
                    }
                }
                map
            };

            for chunk in &mut chunks {
                let mut i = 0;
                while i < chunk.code.len() {
                    let instr = chunk.code[i];
                    if instr.opcode != Opcode::CallIdx as u8 {
                        i += 1;
                        continue;
                    }

                    let (dst, fn_idx) = instr.ri16();
                    let Some(callee_name) = idx_to_name.get(&fn_idx) else {
                        i += 1;
                        continue;
                    };
                    let Some(callee) = callee_map.get(callee_name) else {
                        i += 1;
                        continue;
                    };

                    // Skip inlining if callee contains jump instructions — jump targets are
                    // callee-relative offsets and the inline pass does not remap them.
                    let has_jumps = callee.code.iter().any(|ins| {
                        let op = ins.opcode;
                        op == Opcode::Jmp as u8
                            || op == Opcode::Je as u8
                            || op == Opcode::Jne as u8
                            || op == Opcode::Jz as u8
                            || op == Opcode::Jnz as u8
                    });
                    if has_jumps {
                        i += 1;
                        continue;
                    }

                    // Collect preceding CallArg instructions.
                    let (call_start, arg_regs) = if callee.variadic {
                        // Variadic: scan backward for all consecutive CallArgs.
                        let mut start = i;
                        while start > 0 && chunk.code[start - 1].opcode == Opcode::CallArg as u8 {
                            start -= 1;
                        }
                        let min_args = callee.param_count.saturating_sub(1);
                        if i - start < min_args {
                            i += 1;
                            continue;
                        }
                        let regs: Vec<u8> =
                            chunk.code[start..i].iter().map(|ins| ins.ops[0]).collect();
                        (start, regs)
                    } else {
                        let arg_count = callee.param_count;
                        if i < arg_count {
                            i += 1;
                            continue;
                        }
                        let start = i - arg_count;
                        let all_callargs = chunk.code[start..i]
                            .iter()
                            .all(|ins| ins.opcode == Opcode::CallArg as u8);
                        if !all_callargs {
                            i += 1;
                            continue;
                        }
                        let regs: Vec<u8> =
                            chunk.code[start..i].iter().map(|ins| ins.ops[0]).collect();
                        (start, regs)
                    };

                    if arg_regs.len() > u8::MAX as usize {
                        return Err(format!(
                            "call in `{}` passes more than {} QZI register arguments",
                            chunk.name,
                            u8::MAX
                        ));
                    }
                    let base = chunk.reg_count;
                    let needed = callee.reg_count.max(arg_regs.len() as u8);
                    let Some(inlined_reg_count) = base.checked_add(needed) else {
                        // Inlining is optional. Keep the call when the combined virtual
                        // frame would exceed the QZI register encoding.
                        i += 1;
                        continue;
                    };
                    if chunk.constants.len() + callee.constants.len() > u16::MAX as usize {
                        i += 1;
                        continue;
                    }

                    // Merge callee's constant pool into caller's, recording index offset.
                    let const_base = chunk.constants.len() as u16;
                    for entry in &callee.constants {
                        chunk.constants.push(entry.clone());
                    }

                    // Remap and inline.
                    let remap = |r: u8| base + r;

                    let mut inlined: Vec<crate::bytecode::instruction::Instruction> = Vec::new();

                    // Copy args into callee's param registers (base+0, base+1, ...).
                    for (k, &arg) in arg_regs.iter().enumerate() {
                        let param_reg = remap(k as u8);
                        if param_reg != arg {
                            inlined.push(rrr(Opcode::Mov, param_reg, arg, 0));
                        }
                    }

                    // Remap callee body, drop final Ret.
                    let body: &[_] = if callee
                        .code
                        .last()
                        .map(|x| x.opcode == Opcode::Ret as u8)
                        .unwrap_or(false)
                    {
                        &callee.code[..callee.code.len() - 1]
                    } else {
                        &callee.code[..]
                    };

                    for &ins in body {
                        let mut r = ins;
                        remap_instr_regs(&mut r, remap);
                        // Remap MovConst constant pool index.
                        if r.opcode == Opcode::MovConst as u8 {
                            let old_idx = u16::from_le_bytes([r.ops[1], r.ops[2]]);
                            let new_idx = old_idx + const_base;
                            let bytes = new_idx.to_le_bytes();
                            r.ops[1] = bytes[0];
                            r.ops[2] = bytes[1];
                        }
                        // For variadic intrinsics, update flags to the actual call-site
                        // arg count (flags was fixed at declaration param_count).
                        if r.opcode == Opcode::Intrinsic as u8 && callee.variadic {
                            r.flags = arg_regs.len() as u8;
                        }
                        inlined.push(r);
                    }

                    // Move return value (base+0 = remapped r0) to dst.
                    let ret_reg = remap(0);
                    if ret_reg != dst {
                        inlined.push(rrr(Opcode::Mov, dst, ret_reg, 0));
                    }

                    // Replace the CallArg* + CallIdx range.
                    let old_len = i - call_start + 1;
                    let new_len = inlined.len();
                    chunk.code.splice(call_start..=i, inlined);

                    // After the splice, any absolute jump targets that pointed past the
                    // replaced range must be adjusted by the size delta.  Targets inside
                    // the old range (the callarg/callidx instructions themselves) should
                    // never be jump destinations, so we leave them as-is.
                    let delta = new_len as isize - old_len as isize;
                    if delta != 0 {
                        let splice_end = call_start + old_len;
                        for instr in chunk.code.iter_mut() {
                            let is_jump = matches!(
                                instr.opcode,
                                x if x == Opcode::Jmp as u8
                                    || x == Opcode::Je as u8
                                    || x == Opcode::Jne as u8
                                    || x == Opcode::Jg as u8
                                    || x == Opcode::Jge as u8
                                    || x == Opcode::Jl as u8
                                    || x == Opcode::Jle as u8
                                    || x == Opcode::Ja as u8
                                    || x == Opcode::Jb as u8
                                    || x == Opcode::Jz as u8
                                    || x == Opcode::Jnz as u8
                            );
                            if is_jump {
                                let target =
                                    u16::from_le_bytes([instr.ops[1], instr.ops[2]]) as isize;
                                if target >= splice_end as isize {
                                    let adjusted = target + delta;
                                    let new_target = u16::try_from(adjusted).map_err(|_| {
                                        format!(
                                            "inlining `{}` makes a jump target unrepresentable",
                                            callee.name
                                        )
                                    })?;
                                    let [lo, hi] = new_target.to_le_bytes();
                                    instr.ops[1] = lo;
                                    instr.ops[2] = hi;
                                }
                            }
                        }
                    }

                    // For variadic callees the actual slot count is arg_regs.len()
                    // (may exceed callee.reg_count which was fixed at declaration time).
                    chunk.reg_count = inlined_reg_count;
                    // Adjust i: we removed (arg_count + 1) instrs, restart from call_start.
                    i = call_start;
                }
            }
        }

        // Variadic intrinsic chunks were fully inlined at every call site above.
        // Replace their bodies with a single Ret so the encoder emits a tiny stub
        // (~20 bytes) instead of the full expanded implementation (~300+ bytes).
        for chunk in &mut chunks {
            if chunk.variadic && chunk.intrinsic {
                chunk.code = vec![rrr(Opcode::Ret, 0, 0, 0)];
                chunk.constants.clear();
                chunk.reg_count = 0;
            }
        }

        // Dead chunk elimination: inline candidates with no remaining call sites
        // and no fn-pointer references are removed entirely, shrinking the binary.
        // fn_index stores both primary names and module-qualified aliases for the
        // same chunk (same idx value). We work on idx values, not names, so that
        // all aliases of a dead chunk are removed together.
        if !inline_set.is_empty() {
            let live_idx: HashSet<u16> = chunks
                .iter()
                .flat_map(|c| c.code.iter())
                .filter(|ins| ins.opcode == Opcode::CallIdx as u8)
                .map(|ins| u16::from_le_bytes([ins.ops[1], ins.ops[2]]))
                .collect();

            let fn_addr_refs: HashSet<&str> = chunks
                .iter()
                .flat_map(|c| c.constants.iter())
                .filter_map(|e| {
                    if let crate::bytecode::chunk::ConstPoolEntry::FnAddr(n) = e {
                        Some(n.as_str())
                    } else {
                        None
                    }
                })
                .collect();

            // Dead idx set: inline candidate, no remaining CallIdx, no FnAddr ref.
            let dead_indices: HashSet<u16> = inline_set
                .iter()
                .filter_map(|name| self.fn_index.get(name).copied())
                .filter(|&idx| !live_idx.contains(&idx))
                .filter(|idx| {
                    // Guard: no name mapping to this idx is referenced via fn-pointer.
                    !self
                        .fn_index
                        .iter()
                        .any(|(n, &i)| i == *idx && fn_addr_refs.contains(n.as_str()))
                })
                .collect();

            if !dead_indices.is_empty() {
                // Drop dead chunks (by primary name lookup).
                chunks.retain(|c| {
                    self.fn_index
                        .get(&c.name)
                        .is_none_or(|&idx| !dead_indices.contains(&idx))
                });

                // Remove ALL fn_index entries (primaries + aliases) for dead indices.
                self.fn_index.retain(|_, idx| !dead_indices.contains(idx));

                // Build old→new mapping from unique live indices (deduplicates aliases).
                let old_to_new: HashMap<u16, u16> = {
                    let mut seen = HashSet::new();
                    let mut sorted: Vec<u16> = self
                        .fn_index
                        .values()
                        .copied()
                        .filter(|i| seen.insert(*i))
                        .collect();
                    sorted.sort_unstable();
                    sorted
                        .iter()
                        .enumerate()
                        .map(|(new, &old)| (old, new as u16))
                        .collect()
                };

                // Patch all CallIdx operands.
                for chunk in &mut chunks {
                    for ins in &mut chunk.code {
                        if ins.opcode == Opcode::CallIdx as u8 {
                            let old = u16::from_le_bytes([ins.ops[1], ins.ops[2]]);
                            if let Some(&new) = old_to_new.get(&old) {
                                let [lo, hi] = new.to_le_bytes();
                                ins.ops[1] = lo;
                                ins.ops[2] = hi;
                            }
                        }
                    }
                }

                // Re-densify fn_index values.
                for idx in self.fn_index.values_mut() {
                    if let Some(&new) = old_to_new.get(idx) {
                        *idx = new;
                    }
                }
            }
        }

        // Cross-basic-block constant propagation and folding (P2 optimisation).
        // Runs after inline expansion so inlined constants are visible across BBs.
        for chunk in &mut chunks {
            crate::bytecode::constprop::const_prop_fold(chunk);
        }

        // Dead register elimination then linear-scan slot sharing.
        for chunk in &mut chunks {
            crate::bytecode::regalloc::elim_dead_regs(chunk);
            crate::bytecode::regalloc::linear_scan_alloc(chunk);
        }

        // Reorder chunks so regular functions occupy their fn_index slot.
        // Closures created during Pass 2 are inserted inline into `chunks` before
        // the enclosing function's chunk, which shifts subsequent entries and breaks
        // the fn_index ↔ fn_table position correspondence used by CallIdx.
        // Fix: place each chunk that has an fn_index entry at that position; append
        // all closure/anonymous chunks (no fn_index entry) at the end.
        {
            let max_idx = self
                .fn_index
                .values()
                .copied()
                .max()
                .map(|v| v as usize)
                .unwrap_or(0);
            let mut ordered: Vec<Option<Chunk>> = (0..=max_idx).map(|_| None).collect();
            let mut closures: Vec<Chunk> = Vec::new();
            for chunk in chunks {
                if let Some(&idx) = self.fn_index.get(&chunk.name) {
                    ordered[idx as usize] = Some(chunk);
                } else {
                    closures.push(chunk);
                }
            }
            chunks = ordered.into_iter().flatten().chain(closures).collect();
        }

        for chunk in &chunks {
            if chunk.code.len() > u16::MAX as usize {
                return Err(format!(
                    "function `{}` has {} instructions, exceeding the QZI jump-address limit",
                    chunk.name,
                    chunk.code.len()
                ));
            }
            if chunk.constants.len() > u16::MAX as usize {
                return Err(format!(
                    "function `{}` has too many constants for QZI",
                    chunk.name
                ));
            }
        }
        crate::bytecode::chunk::validate_qzi_chunks(&chunks)
            .map_err(|error| format!("invalid generated bytecode: {error}"))?;
        Ok(chunks)
    }

    fn compile_fn(
        &self,
        name: &str,
        params: &[crate::parser::ast::Param],
        body: Option<&Block>,
        attributes: &[crate::parser::ast::Attribute],
        c_variadic: bool,
        output_chunks: &mut Vec<Chunk>,
        next_closure_idx: &mut u16,
    ) -> Result<Option<Chunk>, String> {
        self.compile_fn_with_subst(
            name,
            params,
            body,
            attributes,
            c_variadic,
            output_chunks,
            next_closure_idx,
            HashMap::new(),
        )
    }

    fn compile_fn_with_subst(
        &self,
        name: &str,
        params: &[crate::parser::ast::Param],
        body: Option<&Block>,
        attributes: &[crate::parser::ast::Attribute],
        c_variadic: bool,
        output_chunks: &mut Vec<Chunk>,
        next_closure_idx: &mut u16,
        type_subst: HashMap<String, TypeKind>,
    ) -> Result<Option<Chunk>, String> {
        if params.len() > u8::MAX as usize {
            return Err(format!(
                "function `{name}` has {} parameters, exceeding the QZI limit of {}",
                params.len(),
                u8::MAX
            ));
        }
        // @intrinsic: emit a platform-neutral Intrinsic instruction.
        if let Some(attr) = attributes.iter().find(|a| a.name == "intrinsic") {
            return Ok(Some(self.compile_intrinsic_fn(name, params, attr)?));
        }

        // @syscall: emit a single Syscall instruction instead of the function body.
        if let Some(attr) = attributes.iter().find(|a| a.name == "syscall") {
            return Ok(Some(self.compile_syscall_fn(name, params, attr)));
        }

        // @api: emit a single CallExt instruction instead of the function body.
        if let Some(attr) = attributes.iter().find(|a| a.name == "api") {
            return Ok(Some(self.compile_api_fn(name, params, attr, c_variadic)));
        }

        // Bodyless declaration — no code to emit; linker must resolve calls.
        let Some(body) = body else {
            return Ok(None);
        };

        // Variadic param needs 2 registers: ptr (the param name) + len (__len_<name>).
        let has_variadic = params.last().map(|p| p.variadic).unwrap_or(false);
        let effective_param_count = params.len() + if has_variadic { 1 } else { 0 };
        let mut fc = FnCompiler::new(
            name,
            effective_param_count,
            &self.fn_index,
            &self.const_map,
            &self.type_map,
            &self.autoderef_map,
            &self.import_names,
            &self.report.struct_defs,
            &self.report.struct_sizes,
            &self.report.struct_field_offsets,
            &self.report.struct_alignments,
            &self.report.bit_field_layouts,
            &self.report.repr_c_structs,
            &self.report.type_aliases,
            &self.foreign_imports,
            &self.report.foreign_globals,
            &self.report.trait_impls,
            &self.variadic_fn_info,
            &self.report.enum_defs,
            &self.str_variadic_fns,
            &self.variadic_intrinsic_fns,
            &self.report.monomorphizations,
            &self.report.trait_method_slots,
            output_chunks,
            next_closure_idx,
            type_subst,
            &self.report.fn_param_names,
            &self.source_files,
            &self.report.annotated_exprs,
        );
        // The function owns ordinary by-value parameters. Keep a function-wide
        // cleanup scope outside the body scope so parameters are destroyed on
        // every return and on fallthrough. Method `self` remains borrowed,
        // matching the language's receiver semantics.
        fc.drop_scopes.push(Vec::new());
        for p in params {
            if p.variadic {
                fc.bind(p.name.clone());
                fc.local_types
                    .insert(p.name.clone(), fc.resolve_type(&p.ty.node));
                fc.bind(format!("__len_{}", p.name));
            } else {
                let reg = fc.bind(p.name.clone());
                let param_ty = fc.resolve_type(&p.ty.node);
                fc.local_types.insert(p.name.clone(), param_ty.clone());
                if p.name != "self" {
                    fc.register_drop_local(&p.name, reg, Some(param_ty));
                }
            }
        }
        fc.compile_block(body);
        // Guarantee every path ends with Ret.
        if fc.chunk.code.last().map(|i| i.opcode) != Some(Opcode::Ret as u8) {
            fc.emit_scope_cleanup();
            fc.chunk.emit(ri16(Opcode::MovI, 0, 0));
            fc.chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
        }
        fc.drop_scopes.pop();
        if let Some(error) = fc.codegen_error {
            return Err(error);
        }
        fc.chunk.reg_count = fc.next_reg as u8;
        Ok(Some(fc.chunk))
    }

    fn compile_syscall_fn(
        &self,
        name: &str,
        params: &[crate::parser::ast::Param],
        attr: &crate::parser::ast::Attribute,
    ) -> Chunk {
        use crate::parser::ast::{AttrArg, AttrVal};
        let mut chunk = Chunk::with_params(name, params.len());
        // Store name or raw number in const pool — arch-neutral QZI.
        let entry = match attr.args.first() {
            Some(AttrArg::Positional(AttrVal::Int(n))) => ConstPoolEntry::Int(*n),
            Some(AttrArg::Positional(AttrVal::Str(s))) => ConstPoolEntry::Str(s.clone()),
            _ => ConstPoolEntry::Str(String::new()),
        };
        let idx = chunk.add_constant(entry);
        let arg_count = params.len() as u8;
        let mut instr = ri16(Opcode::Syscall, 0, idx);
        instr.flags = arg_count;
        chunk.emit(instr);
        chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
        chunk
    }

    fn compile_intrinsic_fn(
        &self,
        name: &str,
        params: &[crate::parser::ast::Param],
        attr: &crate::parser::ast::Attribute,
    ) -> Result<Chunk, String> {
        let mut chunk = Chunk::with_params(name, params.len());
        let instr_name = attr
            .args
            .first()
            .and_then(|a| match a {
                crate::parser::ast::AttrArg::Positional(crate::parser::ast::AttrVal::Str(s)) => {
                    Some(s.as_str())
                }
                _ => None,
            })
            .unwrap_or("");
        // Intrinsics with dedicated opcodes (not routed through Intrinsic case_id).
        {
            static INTRINSIC_OPCODE_MAP: LazyLock<HashMap<&'static str, Opcode>> =
                LazyLock::new(|| {
                    let mut m = HashMap::new();
                    m.insert("quazi.array.store", Opcode::ArrayStore);
                    m.insert("quazi.array.load", Opcode::ArrayLoad);
                    m.insert("quazi.str.from_ptr", Opcode::StrAsStr);
                    m
                });
            if let Some(&op) = INTRINSIC_OPCODE_MAP.get(instr_name) {
                // RRR: ops[0]=val/dst, ops[1]=base_ptr, ops[2]=idx
                // Params are bound to r0, r1, r2 in declaration order.
                match op {
                    Opcode::ArrayStore => {
                        // fn __ptr_store(base: *u8, idx: usize, val: usize)
                        // ArrayStore: val=ops[0], base=ops[1], idx=ops[2]
                        chunk.emit(rrr(op, 2, 0, 1));
                    }
                    Opcode::ArrayLoad => {
                        // fn __ptr_load(base: *u8, idx: usize) -> usize
                        // ArrayLoad: dst=ops[0], base=ops[1], idx=ops[2]
                        chunk.emit(rrr(op, 0, 0, 1));
                    }
                    _ => {
                        chunk.emit(rrr(op, 0, 0, 0));
                    }
                }
                chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
                chunk.reg_count = params.len() as u8;
                return Ok(chunk);
            }
        }
        let Some(id) = intrinsic_id(attr) else {
            return Err(format!("unknown intrinsic `{instr_name}` on function `{name}`"));
        };
        let arg_count = params.len() as u8;
        let mut instr = ri16(Opcode::Intrinsic, 0, id);
        instr.flags = arg_count;
        chunk.emit(instr);
        chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
        chunk.intrinsic = true;
        chunk.reg_count = arg_count; // ensure frame covers all param slots
        chunk.variadic = params.last().map(|p| p.variadic).unwrap_or(false);
        Ok(chunk)
    }

    fn compile_api_fn(
        &self,
        name: &str,
        params: &[crate::parser::ast::Param],
        attr: &crate::parser::ast::Attribute,
        c_variadic: bool,
    ) -> Chunk {
        let mut chunk = Chunk::with_params(name, params.len());
        chunk.c_variadic = c_variadic;
        // Keep the Quazi wrapper private and distinct from the undefined C
        // symbol it calls, otherwise the relocation resolves recursively to
        // the wrapper itself when local and foreign names match.
        chunk.intrinsic = true;
        let foreign = self
            .foreign_imports
            .get(name)
            .cloned()
            .unwrap_or_else(|| ForeignSymbol {
                symbol: api_symbol(attr).unwrap_or_else(|| name.to_string()),
                signature: AbiSignature {
                    params: vec![AbiType::Pointer; params.len()],
                    return_type: AbiType::Pointer,
                    variadic: c_variadic,
                },
            });
        let sym_idx = chunk.add_constant(ConstPoolEntry::ForeignSymbol(foreign));
        // Normalize fixed Quazi parameters into an explicit foreign-call argument list.
        for index in 0..params.len() {
            chunk.emit(rrr(Opcode::CallArg, index as u8, 0, 0));
        }
        let mut instr = ri16(Opcode::CallExt, 0, sym_idx);
        instr.flags = 0;
        chunk.emit(instr);
        chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
        chunk
    }
}

// ── Per-function compiler ─────────────────────────────────────────────────────

struct FnCompiler<'a> {
    chunk: Chunk,
    regs: HashMap<String, u8>,
    /// Declared or inferred local types. Expression annotations can be `Any`
    /// after a generic helper call, but an explicit local annotation remains
    /// authoritative for direct method dispatch.
    local_types: HashMap<String, TypeKind>,
    /// Monotonic virtual-register counter.  Keep this wider than the QZI
    /// physical register encoding so exhaustion is diagnosed instead of
    /// silently wrapping back to r0.
    next_reg: u16,
    codegen_error: Option<String>,
    drop_scopes: Vec<Vec<DropLocal>>,
    fn_index: &'a HashMap<String, u16>,
    const_map: &'a HashMap<(usize, usize), ConstValue>,
    type_map: &'a HashMap<(usize, usize), TypeKind>,
    /// Spans marked for auto-deref by semantic analysis.
    autoderef_map: &'a HashMap<(usize, usize), bool>,
    import_names: &'a HashSet<String>,
    struct_defs: &'a HashMap<String, Vec<(String, TypeKind)>>,
    struct_sizes: &'a HashMap<String, usize>,
    struct_field_offsets: &'a HashMap<String, Vec<(String, usize)>>,
    struct_alignments: &'a HashMap<String, usize>,
    bit_field_layouts: &'a HashMap<String, HashMap<String, crate::semantic::BitFieldLayout>>,
    repr_c_structs: &'a HashSet<String>,
    type_aliases: &'a HashMap<String, (Vec<String>, TypeKind)>,
    foreign_imports: &'a HashMap<String, ForeignSymbol>,
    foreign_globals: &'a HashMap<String, crate::semantic::ForeignGlobalInfo>,
    trait_impls: &'a HashMap<String, std::collections::HashSet<String>>,
    /// Maps variadic function name → number of fixed (non-variadic) params.
    variadic_fn_info: &'a HashMap<String, usize>,
    /// Enum variant tags: enum name → variant name → discriminant.
    enum_defs: &'a HashMap<String, HashMap<String, usize>>,
    /// Variadic-str functions: auto-coerce args to str at call sites.
    str_variadic_fns: &'a HashSet<String>,
    /// Variadic @intrinsic functions: coerce args and call directly.
    variadic_intrinsic_fns: &'a HashSet<String>,
    /// Monomorphization info: used to resolve mangled names for generic calls.
    monomorphizations: &'a [crate::semantic::MonomorphizationInfo],
    /// Vtable method slot order per trait: trait name → ordered method names.
    trait_method_slots: &'a HashMap<String, Vec<String>>,
    /// Output chunks accumulator — closure chunks are pushed here.
    output_chunks: &'a mut Vec<Chunk>,
    /// Counter for generating unique closure names.
    next_closure_idx: &'a mut u16,
    /// Tracks which local variable registers hold closure environment struct pointers.
    /// Used at call sites to dispatch through the env-wrapper convention.
    closure_env_regs: HashSet<u8>,
    /// Type substitution map for monomorphized functions: generic param name → concrete type.
    type_subst: HashMap<String, TypeKind>,
    /// Ordered parameter names per function: used to resolve named arguments at call sites.
    fn_param_names: &'a HashMap<String, Vec<String>>,
    source_files: &'a [SourceFile],
    /// Expression annotations from semantic analysis (resolved function names, types, etc.)
    annotated_exprs: &'a [crate::semantic::ExprAnnotation],
    loop_stack: Vec<LoopFrame>,
}

#[derive(Clone)]
struct DropLocal {
    name: String,
    reg: u8,
    drop_fn: String,
    active: bool,
}

struct LoopFrame {
    break_jumps: Vec<usize>,
    continue_jumps: Vec<usize>,
}

/// Address of an lvalue so it can be loaded and stored without re-evaluating
/// the base expression twice. This makes compound assignment and inc/dec on
/// non-identifier targets correct even when register allocation reuses slots.
enum LvalueAddr {
    Ident {
        name: String,
        span: Span,
    },
    Deref {
        ptr: u8,
        width: MemWidth,
        signed: bool,
    },
    ForeignGlobal {
        ptr: u8,
        width: MemWidth,
        signed: bool,
        float32: bool,
    },
    Field {
        obj: u8,
        offset: u16,
    },
    BitField {
        obj: u8,
        layout: crate::semantic::BitFieldLayout,
    },
    IndexArray {
        obj: u8,
        idx: u8,
        index_target: String,
        set_target: String,
    },
    IndexSlice {
        ptr: u8,
        idx: u8,
    },
    IndexFixed {
        base: u8,
        idx: u8,
        literal: Option<i64>,
    },
}

impl LoopFrame {
    fn new() -> Self {
        Self {
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
        }
    }
}

impl<'a> FnCompiler<'a> {
    fn new(
        name: &str,
        param_count: usize,
        fn_index: &'a HashMap<String, u16>,
        const_map: &'a HashMap<(usize, usize), ConstValue>,
        type_map: &'a HashMap<(usize, usize), TypeKind>,
        autoderef_map: &'a HashMap<(usize, usize), bool>,
        import_names: &'a HashSet<String>,
        struct_defs: &'a HashMap<String, Vec<(String, TypeKind)>>,
        struct_sizes: &'a HashMap<String, usize>,
        struct_field_offsets: &'a HashMap<String, Vec<(String, usize)>>,
        struct_alignments: &'a HashMap<String, usize>,
        bit_field_layouts: &'a HashMap<String, HashMap<String, crate::semantic::BitFieldLayout>>,
        repr_c_structs: &'a HashSet<String>,
        type_aliases: &'a HashMap<String, (Vec<String>, TypeKind)>,
        foreign_imports: &'a HashMap<String, ForeignSymbol>,
        foreign_globals: &'a HashMap<String, crate::semantic::ForeignGlobalInfo>,
        trait_impls: &'a HashMap<String, std::collections::HashSet<String>>,
        variadic_fn_info: &'a HashMap<String, usize>,
        enum_defs: &'a HashMap<String, HashMap<String, usize>>,
        str_variadic_fns: &'a HashSet<String>,
        variadic_intrinsic_fns: &'a HashSet<String>,
        monomorphizations: &'a [crate::semantic::MonomorphizationInfo],
        trait_method_slots: &'a HashMap<String, Vec<String>>,
        output_chunks: &'a mut Vec<Chunk>,
        next_closure_idx: &'a mut u16,
        type_subst: HashMap<String, TypeKind>,
        fn_param_names: &'a HashMap<String, Vec<String>>,
        source_files: &'a [SourceFile],
        annotated_exprs: &'a [crate::semantic::ExprAnnotation],
    ) -> Self {
        Self {
            chunk: Chunk::with_params(name, param_count),
            regs: HashMap::new(),
            local_types: HashMap::new(),
            next_reg: 0,
            codegen_error: None,
            drop_scopes: Vec::new(),
            fn_index,
            const_map,
            type_map,
            autoderef_map,
            import_names,
            struct_defs,
            struct_sizes,
            struct_field_offsets,
            struct_alignments,
            bit_field_layouts,
            repr_c_structs,
            type_aliases,
            foreign_imports,
            foreign_globals,
            trait_impls,
            variadic_fn_info,
            enum_defs,
            str_variadic_fns,
            variadic_intrinsic_fns,
            monomorphizations,
            trait_method_slots,
            output_chunks,
            next_closure_idx,
            closure_env_regs: HashSet::new(),
            type_subst,
            fn_param_names,
            source_files,
            annotated_exprs,
            loop_stack: Vec::new(),
        }
    }

    /// Merge positional args + named args into correct param order (cloned).
    /// Named args are inserted at their declared parameter positions.
    fn merge_named_args(
        &self,
        callee_name: &str,
        positional: &[Expr],
        named: &[(String, Expr)],
    ) -> Vec<Expr> {
        let param_names = self.fn_param_names.get(callee_name);
        let total = param_names
            .map(|n| n.len())
            .unwrap_or(positional.len() + named.len());
        let mut result: Vec<Option<Expr>> = vec![None; total];
        for (i, e) in positional.iter().enumerate() {
            if i < total {
                result[i] = Some(e.clone());
            }
        }
        if let Some(names) = param_names {
            for (arg_name, arg_expr) in named {
                if let Some(pos) = names.iter().position(|n| n == arg_name)
                    && pos < total
                {
                    result[pos] = Some(arg_expr.clone());
                }
            }
        } else {
            // Unknown function — append named args after positional.
            let start = positional.len();
            for (i, (_, arg_expr)) in named.iter().enumerate() {
                let pos = start + i;
                if pos < total {
                    result[pos] = Some(arg_expr.clone());
                }
            }
        }
        result.into_iter().flatten().collect()
    }

    /// Resolve a type through the monomorphization substitution map.
    /// Generic param references `T` → concrete type; all others pass through.
    fn resolve_type(&self, ty: &TypeKind) -> TypeKind {
        match ty {
            TypeKind::Named { name, type_args } if type_args.is_empty() => self
                .type_subst
                .get(name)
                .cloned()
                .unwrap_or_else(|| ty.clone()),
            TypeKind::Named { name, type_args } => {
                let resolved_args: Vec<Type> = type_args
                    .iter()
                    .map(|a| Spanned::new(self.resolve_type(&a.node), a.span))
                    .collect();
                TypeKind::Named {
                    name: name.clone(),
                    type_args: resolved_args,
                }
            }
            TypeKind::Ref { inner } => TypeKind::Ref {
                inner: Box::new(Spanned::new(self.resolve_type(&inner.node), inner.span)),
            },
            TypeKind::RawPtr { inner } => TypeKind::RawPtr {
                inner: Box::new(Spanned::new(self.resolve_type(&inner.node), inner.span)),
            },
            TypeKind::Array { elem_ty, len } => TypeKind::Array {
                elem_ty: Box::new(Spanned::new(self.resolve_type(&elem_ty.node), elem_ty.span)),
                len: *len,
            },
            TypeKind::FlexibleArray { elem_ty } => TypeKind::FlexibleArray {
                elem_ty: Box::new(Spanned::new(self.resolve_type(&elem_ty.node), elem_ty.span)),
            },
            TypeKind::Slice { elem_ty } => TypeKind::Slice {
                elem_ty: Box::new(Spanned::new(self.resolve_type(&elem_ty.node), elem_ty.span)),
            },
            TypeKind::Fn { params, return_ty } => TypeKind::Fn {
                params: params
                    .iter()
                    .map(|param| Spanned::new(self.resolve_type(&param.node), param.span))
                    .collect(),
                return_ty: Box::new(Spanned::new(
                    self.resolve_type(&return_ty.node),
                    return_ty.span,
                )),
            },
            TypeKind::CFn { params, return_ty } => TypeKind::CFn {
                params: params
                    .iter()
                    .map(|param| Spanned::new(self.resolve_type(&param.node), param.span))
                    .collect(),
                return_ty: Box::new(Spanned::new(
                    self.resolve_type(&return_ty.node),
                    return_ty.span,
                )),
            },
            _ => ty.clone(),
        }
    }

    /// Look up the type for a span from the type_map, resolving generic params through
    /// the monomorphization substitution map.
    fn type_of_span(&self, key: (usize, usize)) -> Option<TypeKind> {
        self.type_map.get(&key).map(|ty| self.resolve_type(ty))
    }

    fn type_of_expr(&self, expr: &Expr) -> Option<TypeKind> {
        let annotated = self.type_of_span((expr.span.start, expr.span.end));
        if !matches!(annotated, None | Some(TypeKind::Any)) {
            return annotated;
        }
        if let ExprKind::Ident(name) = &expr.node
            && let Some(ty) = self.local_types.get(name)
        {
            return Some(self.resolve_type(ty));
        }
        annotated
    }

    /// Resolve the physical access required by an explicit raw-pointer
    /// dereference. Other memory paths intentionally retain the VM's 8-byte
    /// slot layout.
    fn raw_pointee_access(&self, ptr_expr: &Expr) -> (MemWidth, bool) {
        let Some(TypeKind::RawPtr { inner }) =
            self.type_of_span((ptr_expr.span.start, ptr_expr.span.end))
        else {
            return (MemWidth::Qword, false);
        };
        match self.resolve_type(&inner.node) {
            TypeKind::Int8 => (MemWidth::Byte, true),
            TypeKind::Int16 => (MemWidth::Word, true),
            TypeKind::Int32 => (MemWidth::Dword, true),
            TypeKind::Uint8 | TypeKind::Bool => (MemWidth::Byte, false),
            TypeKind::Uint16 => (MemWidth::Word, false),
            TypeKind::Uint32 => (MemWidth::Dword, false),
            _ => (MemWidth::Qword, false),
        }
    }

    fn c_memory_access(&self, ty: &TypeKind) -> (MemWidth, bool, u64, bool) {
        match self.resolve_type(ty) {
            TypeKind::Int8 => (MemWidth::Byte, true, 1, false),
            TypeKind::Uint8 | TypeKind::Bool => (MemWidth::Byte, false, 1, false),
            TypeKind::Int16 => (MemWidth::Word, true, 2, false),
            TypeKind::Uint16 => (MemWidth::Word, false, 2, false),
            TypeKind::Int32 => (MemWidth::Dword, true, 4, false),
            TypeKind::Uint32 => (MemWidth::Dword, false, 4, false),
            TypeKind::Float32 => (MemWidth::Dword, false, 4, true),
            _ => (MemWidth::Qword, false, 8, false),
        }
    }

    fn emit_c_load(&mut self, address: u8, width: MemWidth, signed: bool, float32: bool) -> u8 {
        let dst = self.alloc_reg();
        let mut instruction = mem_load_w(address, dst, 0, width, signed);
        if float32 {
            instruction.flags |= FLOAT_FLAG;
        }
        self.chunk.emit(instruction);
        dst
    }

    fn emit_c_store(&mut self, address: u8, source: u8, width: MemWidth, float32: bool) {
        let mut instruction = mem_store_w(address, source, 0, width);
        if float32 {
            instruction.flags |= FLOAT_FLAG;
        }
        self.chunk.emit(instruction);
    }

    fn emit_indexed_c_address(&mut self, base: u8, index: u8, elem_size: u64) -> u8 {
        if elem_size == 1 {
            let address = self.alloc_reg();
            self.chunk.emit(rrr(Opcode::Add, address, base, index));
            return address;
        }
        let size = self.emit_u64_constant(elem_size);
        let offset = self.alloc_reg();
        self.chunk.emit(rrr(Opcode::Mul, offset, index, size));
        let address = self.alloc_reg();
        self.chunk.emit(rrr(Opcode::Add, address, base, offset));
        address
    }

    /// Look up the resolved function name annotation for a span, if any.
    fn resolved_fn_for_span(&self, span: crate::parser::ast::Span) -> Option<String> {
        self.annotated_exprs
            .iter()
            .rev()
            .find(|ann| ann.span.start == span.start && ann.span.end == span.end)
            .and_then(|ann| ann.resolved_fn.clone())
    }

    fn foreign_global_for_span(
        &self,
        span: crate::parser::ast::Span,
    ) -> Option<crate::semantic::ForeignGlobalInfo> {
        let resolved = self
            .annotated_exprs
            .iter()
            .rev()
            .find(|annotation| {
                annotation.span.start == span.start && annotation.span.end == span.end
            })?
            .resolved_global
            .as_ref()?;
        self.foreign_globals.get(resolved).cloned()
    }

    fn emit_foreign_global_address(&mut self, global: &crate::semantic::ForeignGlobalInfo) -> u8 {
        let ty = abi_type_from_layout(
            &global.ty,
            self.struct_defs,
            self.struct_sizes,
            self.struct_field_offsets,
            self.struct_alignments,
            self.bit_field_layouts,
            self.repr_c_structs,
            self.type_aliases,
        )
        .expect("semantic analysis validated the foreign global ABI type");
        let constant = self
            .chunk
            .add_constant(ConstPoolEntry::ForeignGlobal(AbiForeignGlobal {
                symbol: global.symbol.clone(),
                ty,
            }));
        let address = self.alloc_reg();
        self.chunk.emit(ri16(Opcode::MovConst, address, constant));
        address
    }

    fn foreign_global_lvalue(&mut self, span: crate::parser::ast::Span) -> Option<LvalueAddr> {
        let global = self.foreign_global_for_span(span)?;
        let address = self.emit_foreign_global_address(&global);
        let (width, signed, _, float32) = self.c_memory_access(&global.ty);
        Some(LvalueAddr::ForeignGlobal {
            ptr: address,
            width,
            signed,
            float32,
        })
    }

    fn is_c_abi_function_span(&self, span: crate::parser::ast::Span) -> bool {
        self.annotated_exprs
            .iter()
            .rev()
            .find(|annotation| {
                annotation.span.start == span.start && annotation.span.end == span.end
            })
            .is_some_and(|annotation| annotation.c_abi_function)
    }

    fn c_callback_signature(&self, ty: &TypeKind) -> Option<AbiSignature> {
        let TypeKind::CFn { params, return_ty } = self.resolve_type(ty) else {
            return None;
        };
        Some(AbiSignature {
            params: params
                .iter()
                .map(|param| {
                    abi_type_from_layout(
                        &param.node,
                        self.struct_defs,
                        self.struct_sizes,
                        self.struct_field_offsets,
                        self.struct_alignments,
                        self.bit_field_layouts,
                        self.repr_c_structs,
                        self.type_aliases,
                    )
                })
                .collect::<Option<Vec<_>>>()?,
            return_type: abi_type_from_layout(
                &return_ty.node,
                self.struct_defs,
                self.struct_sizes,
                self.struct_field_offsets,
                self.struct_alignments,
                self.bit_field_layouts,
                self.repr_c_structs,
                self.type_aliases,
            )?,
            variadic: false,
        })
    }

    /// True if semantic analysis marked this expression span for auto-deref.
    fn should_autoderef(&self, span: crate::parser::ast::Span) -> bool {
        self.autoderef_map
            .get(&(span.start, span.end))
            .copied()
            .unwrap_or(false)
    }

    /// If `span` is marked auto-deref, load the value at the pointer in `ptr_reg`
    /// and return the register holding the loaded value. Otherwise returns `ptr_reg`.
    fn emit_autoderef_load(&mut self, span: crate::parser::ast::Span, ptr_reg: u8) -> u8 {
        if !self.should_autoderef(span) {
            return ptr_reg;
        }
        let key = (span.start, span.end);
        let Some(ty) = self.type_of_span(key) else {
            return ptr_reg;
        };
        // The VM Load instruction reads one 8-byte slot. Value-like types that fit
        // in a single slot can be auto-dereferenced; `str` is currently a thin null-terminated pointer
        // and must be passed/stored as a reference.
        if matches!(ty, TypeKind::Str) {
            return ptr_reg;
        }
        let dst = self.alloc_reg();
        self.chunk.emit(mem_load(ptr_reg, dst, 0));
        dst
    }

    fn is_float_span(&self, key: (usize, usize)) -> bool {
        matches!(
            self.type_of_span(key),
            Some(TypeKind::Float16 | TypeKind::Float32 | TypeKind::Float64)
        )
    }

    fn is_str_span(&self, key: (usize, usize)) -> bool {
        matches!(
            self.type_of_span(key),
            Some(TypeKind::Str | TypeKind::Ref { .. })
        )
    }

    /// Look up the monomorphized name for a generic function call.
    /// When inside a monomorphized function (type_subst non-empty), applies the substitution
    /// to resolve type variables (e.g. T→i32) before looking up the concrete specialization.
    fn resolve_monomorphized_name(&self, fn_name: &str, type_args: &[Type]) -> Option<String> {
        let raw_kinds: Vec<TypeKind> = type_args.iter().map(|t| t.node.clone()).collect();
        // Fast path: exact match on raw type args.
        if let Some(m) = self
            .monomorphizations
            .iter()
            .find(|m| m.fn_name == fn_name && types_equal_slice(&m.type_args, &raw_kinds))
        {
            // Verify the raw match resolves to a live name; if so, return it.
            if self.fn_index.contains_key(&m.mangled_name) {
                return Some(m.mangled_name.clone());
            }
        }
        // If inside a monomorphized context, substitute type vars and retry.
        if !self.type_subst.is_empty() {
            let subst_kinds: Vec<TypeKind> =
                raw_kinds.iter().map(|t| self.resolve_type(t)).collect();
            if !types_equal_slice(&subst_kinds, &raw_kinds) {
                // Look for a concrete monomorphization with the substituted types.
                if let Some(m) = self
                    .monomorphizations
                    .iter()
                    .find(|m| m.fn_name == fn_name && types_equal_slice(&m.type_args, &subst_kinds))
                {
                    return Some(m.mangled_name.clone());
                }
                // Not in the mono list; check if the mangled name is already in fn_index
                // (e.g. registered by a direct call from another site).
                let mangled =
                    crate::semantic::typecheck::mangle_monomorphized(fn_name, &subst_kinds);
                if self.fn_index.contains_key(&mangled) {
                    return Some(mangled);
                }
            }
        }
        // Fall back to a raw match. emit_call_by_name reports a codegen error if
        // the resulting specialization was not registered in the function table.
        self.monomorphizations
            .iter()
            .find(|m| m.fn_name == fn_name && types_equal_slice(&m.type_args, &raw_kinds))
            .map(|m| m.mangled_name.clone())
    }

    fn enum_ctor_tag(&self, name: &str) -> Option<usize> {
        for variants in self.enum_defs.values() {
            if let Some(&tag) = variants.get(name) {
                return Some(tag);
            }
        }
        None
    }

    fn variant_tag(&self, enum_name: Option<&str>, variant: &str) -> usize {
        if let Some(ename) = enum_name
            && let Some(variants) = self.enum_defs.get(ename)
            && let Some(&tag) = variants.get(variant)
        {
            return tag;
        }
        for variants in self.enum_defs.values() {
            if let Some(&tag) = variants.get(variant) {
                return tag;
            }
        }
        0
    }

    fn field_offset_by_name(&mut self, struct_name: &str, field_name: &str) -> u16 {
        if let Some(offsets) = self.struct_field_offsets.get(struct_name) {
            for (fname, offset) in offsets {
                if fname == field_name {
                    return u16::try_from(*offset).unwrap_or_else(|_| {
                        self.codegen_error.get_or_insert_with(|| {
                            format!(
                                "field `{struct_name}.{field_name}` has byte offset {offset}, exceeding the QZI limit of {}",
                                u16::MAX
                            )
                        });
                        0
                    });
                }
            }
        }
        0
    }

    fn ffi_field_access_by_name(
        &self,
        struct_name: &str,
        field_name: &str,
    ) -> (MemWidth, bool, bool) {
        if !self.repr_c_structs.contains(struct_name) {
            return (MemWidth::Qword, false, false);
        }
        let Some((_, ty)) = self
            .struct_defs
            .get(struct_name)
            .and_then(|fields| fields.iter().find(|(name, _)| name == field_name))
        else {
            return (MemWidth::Qword, false, false);
        };
        let mut resolved = ty;
        while let TypeKind::Named { name, type_args } = resolved {
            if !type_args.is_empty() {
                break;
            }
            let Some((params, target)) = self.type_aliases.get(name) else {
                break;
            };
            if !params.is_empty() {
                break;
            }
            resolved = target;
        }
        match resolved {
            TypeKind::Int8 => (MemWidth::Byte, true, false),
            TypeKind::Uint8 | TypeKind::Bool => (MemWidth::Byte, false, false),
            TypeKind::Int16 => (MemWidth::Word, true, false),
            TypeKind::Uint16 => (MemWidth::Word, false, false),
            TypeKind::Int32 => (MemWidth::Dword, true, false),
            TypeKind::Uint32 => (MemWidth::Dword, false, false),
            TypeKind::Float32 => (MemWidth::Dword, false, true),
            _ => (MemWidth::Qword, false, false),
        }
    }

    fn ffi_field_access(&self, object: &Expr, field_name: &str) -> (MemWidth, bool, bool) {
        let key = (object.span.start, object.span.end);
        if let Some(TypeKind::Named { name, .. }) = self.type_of_span(key) {
            self.ffi_field_access_by_name(&name, field_name)
        } else {
            (MemWidth::Qword, false, false)
        }
    }

    fn bit_field_layout_by_name(
        &self,
        aggregate_name: &str,
        field_name: &str,
    ) -> Option<crate::semantic::BitFieldLayout> {
        self.bit_field_layouts
            .get(aggregate_name)
            .and_then(|fields| fields.get(field_name))
            .copied()
    }

    fn bit_field_layout(
        &self,
        object: &Expr,
        field_name: &str,
    ) -> Option<crate::semantic::BitFieldLayout> {
        let key = (object.span.start, object.span.end);
        let TypeKind::Named { name, .. } = self.type_of_span(key)? else {
            return None;
        };
        self.bit_field_layout_by_name(&name, field_name)
    }

    fn emit_u64_constant(&mut self, value: u64) -> u8 {
        let reg = self.alloc_reg();
        if value <= u16::MAX as u64 {
            self.chunk.emit(ri16(Opcode::MovI, reg, value as u16));
        } else {
            let index = self.chunk.add_constant(ConstPoolEntry::Int(value as i64));
            self.chunk.emit(ri16(Opcode::MovConst, reg, index));
        }
        reg
    }

    fn qzi_u16(&mut self, value: usize, description: &str) -> u16 {
        match u16::try_from(value) {
            Ok(value) => value,
            Err(_) => {
                if self.codegen_error.is_none() {
                    self.codegen_error = Some(format!(
                        "{description} {value} exceeds the QZI u16 encoding limit"
                    ));
                }
                0
            }
        }
    }

    fn qzi_u16_from_u64(&mut self, value: u64, description: &str) -> u16 {
        match u16::try_from(value) {
            Ok(value) => value,
            Err(_) => {
                if self.codegen_error.is_none() {
                    self.codegen_error = Some(format!(
                        "{description} {value} exceeds the QZI u16 encoding limit"
                    ));
                }
                0
            }
        }
    }

    fn qzi_u8(&mut self, value: usize, description: &str) -> u8 {
        match u8::try_from(value) {
            Ok(value) => value,
            Err(_) => {
                if self.codegen_error.is_none() {
                    self.codegen_error = Some(format!(
                        "{description} {value} exceeds the QZI u8 encoding limit"
                    ));
                }
                0
            }
        }
    }

    fn bit_storage_width(bytes: u8) -> MemWidth {
        match bytes {
            1 => MemWidth::Byte,
            2 => MemWidth::Word,
            4 => MemWidth::Dword,
            _ => MemWidth::Qword,
        }
    }

    fn emit_bit_field_load(&mut self, object: u8, layout: crate::semantic::BitFieldLayout) -> u8 {
        let mut value = self.alloc_reg();
        self.chunk.emit(field_load_typed(
            value,
            object,
            u16::try_from(layout.byte_offset).unwrap_or_else(|_| {
                self.codegen_error.get_or_insert_with(|| {
                    format!("bit-field byte offset {} exceeds the QZI limit", layout.byte_offset)
                });
                0
            }),
            Self::bit_storage_width(layout.storage_bytes),
            false,
            false,
        ));
        if layout.bit_offset != 0 {
            let shift = self.emit_u64_constant(layout.bit_offset as u64);
            let shifted = self.alloc_reg();
            self.chunk.emit(rrr(Opcode::Shr, shifted, value, shift));
            value = shifted;
        }
        let storage_bits = u32::from(layout.storage_bytes) * 8;
        if u32::from(layout.bit_width) < storage_bits {
            let mask = (1u64 << layout.bit_width) - 1;
            let mask_reg = self.emit_u64_constant(mask);
            let masked = self.alloc_reg();
            self.chunk.emit(rrr(Opcode::And, masked, value, mask_reg));
            value = masked;
            if layout.signed {
                let amount = 64 - u64::from(layout.bit_width);
                let shift = self.emit_u64_constant(amount);
                let extended = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::Shl, extended, value, shift));
                self.chunk.emit(rrr(Opcode::Sar, extended, extended, shift));
                value = extended;
            }
        }
        value
    }

    fn emit_bit_field_store(
        &mut self,
        object: u8,
        source: u8,
        layout: crate::semantic::BitFieldLayout,
    ) {
        let storage_bits = u32::from(layout.storage_bytes) * 8;
        let value_mask = if u32::from(layout.bit_width) == storage_bits {
            u64::MAX
        } else {
            (1u64 << layout.bit_width) - 1
        };
        let positioned_mask = value_mask << layout.bit_offset;
        let old = self.alloc_reg();
        self.chunk.emit(field_load_typed(
            old,
            object,
            u16::try_from(layout.byte_offset).unwrap_or_else(|_| {
                self.codegen_error.get_or_insert_with(|| {
                    format!("bit-field byte offset {} exceeds the QZI limit", layout.byte_offset)
                });
                0
            }),
            Self::bit_storage_width(layout.storage_bytes),
            false,
            false,
        ));
        let clear_mask = self.emit_u64_constant(!positioned_mask);
        let cleared = self.alloc_reg();
        self.chunk.emit(rrr(Opcode::And, cleared, old, clear_mask));
        let value_mask_reg = self.emit_u64_constant(value_mask);
        let mut positioned = self.alloc_reg();
        self.chunk
            .emit(rrr(Opcode::And, positioned, source, value_mask_reg));
        if layout.bit_offset != 0 {
            let shift = self.emit_u64_constant(layout.bit_offset as u64);
            let shifted = self.alloc_reg();
            self.chunk
                .emit(rrr(Opcode::Shl, shifted, positioned, shift));
            positioned = shifted;
        }
        let merged = self.alloc_reg();
        self.chunk
            .emit(rrr(Opcode::Or, merged, cleared, positioned));
        self.chunk.emit(field_store_typed(
            merged,
            object,
            u16::try_from(layout.byte_offset).unwrap_or_else(|_| {
                self.codegen_error.get_or_insert_with(|| {
                    format!("bit-field byte offset {} exceeds the QZI limit", layout.byte_offset)
                });
                0
            }),
            Self::bit_storage_width(layout.storage_bytes),
            false,
        ));
    }

    /// Scan the body of an `ExprKind::Closure` for identifiers that reference
    /// outer-scope local variables. Returns the deduplicated list of capture names.
    fn capture_ident_names(&self, body: &Expr, closure_params: &[String]) -> Vec<String> {
        let mut names = Vec::new();
        collect_idents(body, &mut names);
        names.sort();
        names.dedup();
        names.retain(|n| self.regs.contains_key(n) && !closure_params.contains(n));
        names
    }

    fn field_offset(&mut self, object: &Expr, field_name: &str) -> u16 {
        let key = (object.span.start, object.span.end);
        if let Some(TypeKind::Named {
            name: struct_name, ..
        }) = self.type_of_span(key)
            && let Some(offsets) = self.struct_field_offsets.get(&struct_name)
        {
            for (fname, offset) in offsets {
                if fname == field_name {
                    return u16::try_from(*offset).unwrap_or_else(|_| {
                        self.codegen_error.get_or_insert_with(|| {
                            format!(
                                "field `{struct_name}.{field_name}` has byte offset {offset}, exceeding the QZI limit of {}",
                                u16::MAX
                            )
                        });
                        0
                    });
                }
            }
        }
        0
    }

    /// Load the value of an lvalue expression (identifier, deref, field, index).
    fn emit_lvalue_load(&mut self, target: &Expr) -> u8 {
        match &target.node {
            ExprKind::Ident(name) => self.reg_of(name),
            ExprKind::Unary {
                op: UnaryOpKind::Deref,
                expr: ptr_expr,
            } => {
                let ptr = self.compile_expr(ptr_expr);
                let dst = self.alloc_reg();
                let (width, signed) = self.raw_pointee_access(ptr_expr);
                self.chunk.emit(mem_load_w(ptr, dst, 0, width, signed));
                dst
            }
            ExprKind::Field {
                object,
                name: field_name,
            } => {
                let byte_offset = self.field_offset(object, field_name);
                let obj = self.compile_expr(object);
                let dst = self.alloc_reg();
                self.chunk.emit(field_load(dst, obj, byte_offset));
                dst
            }
            ExprKind::Index { object, indices } => {
                let key = (object.span.start, object.span.end);
                if let Some(TypeKind::Named {
                    name: type_name,
                    type_args,
                }) = self.type_of_span(key)
                {
                    let implements_index = self
                        .trait_impls
                        .get(type_name.as_str())
                        .map(|ts| ts.contains("Index"))
                        .unwrap_or(false);
                    if implements_index {
                        let type_kinds: Vec<TypeKind> =
                            type_args.iter().map(|t| t.node.clone()).collect();
                        let mangled = if type_kinds.is_empty() {
                            format!("{}.index", type_name)
                        } else {
                            crate::semantic::typecheck::mangle_monomorphized(
                                &format!("{}.index", type_name),
                                &type_kinds,
                            )
                        };
                        if self.fn_index.contains_key(&mangled) {
                            let obj = self.compile_expr(object);
                            let idx_regs: Vec<u8> =
                                indices.iter().map(|i| self.compile_expr(i)).collect();
                            let dst = self.alloc_reg();
                            let mut all_args = vec![obj];
                            all_args.extend_from_slice(&idx_regs);
                            self.emit_call_by_name(&mangled, &all_args, dst);
                            return dst;
                        }
                    }
                }
                let index = indices
                    .first()
                    .expect("index expr must have at least one index");
                let obj_ty = self.type_of_span(key);
                if matches!(obj_ty, Some(TypeKind::Slice { .. })) {
                    let ptr = self.compile_expr(object);
                    let idx = self.compile_expr(index);
                    let eight = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                    let offset = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Mul, offset, idx, eight));
                    let addr = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Sub, addr, ptr, offset));
                    let dst = self.alloc_reg();
                    self.chunk.emit(mem_load(addr, dst, 0));
                    return dst;
                }
                let base = self.compile_expr(object);
                if let ExprKind::Literal(Literal::Int(n)) = &index.node
                    && *n >= 0
                {
                    return base + *n as u8;
                }
                let idx = self.compile_expr(index);
                let ptr = self.alloc_reg();
                self.chunk.emit(mem_lea(base, ptr, 0));
                let eight = self.alloc_reg();
                self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                let offset = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::Mul, offset, idx, eight));
                self.chunk.emit(rrr(Opcode::Sub, ptr, ptr, offset));
                let dst = self.alloc_reg();
                self.chunk.emit(mem_load(ptr, dst, 0));
                dst
            }
            _ => self.compile_expr(target),
        }
    }

    /// Store `src` into an lvalue expression (identifier, deref, field, index).
    fn emit_lvalue_store(&mut self, target: &Expr, src: u8) -> u8 {
        match &target.node {
            ExprKind::Ident(name) => {
                self.drop_local_now(name);
                let dst = self.reg_of(name);
                if dst != src {
                    self.chunk.emit(rrr(Opcode::Mov, dst, src, 0));
                }
                let local_ty = self.type_of_span((target.span.start, target.span.end));
                self.reactivate_drop_local(name, dst, local_ty);
                dst
            }
            ExprKind::Unary {
                op: UnaryOpKind::Deref,
                expr: ptr_expr,
            } => {
                let ptr = self.compile_expr(ptr_expr);
                let (width, _) = self.raw_pointee_access(ptr_expr);
                self.chunk.emit(mem_store_w(ptr, src, 0, width));
                src
            }
            ExprKind::Field {
                object,
                name: field_name,
            } => {
                let byte_offset = self.field_offset(object, field_name);
                let obj = self.compile_expr(object);
                self.chunk.emit(field_store(src, obj, byte_offset));
                src
            }
            ExprKind::Index { object, indices } => {
                let obj_key = (object.span.start, object.span.end);
                let obj_ty = self.type_of_span(obj_key);
                let index = indices.first().expect("index must have at least one index");

                if let Some(TypeKind::FlexibleArray { elem_ty }) = &obj_ty {
                    let base = self.compile_expr(object);
                    let idx = self.compile_expr(index);
                    let (width, _, elem_size, float32) = self.c_memory_access(&elem_ty.node);
                    let ptr = self.emit_indexed_c_address(base, idx, elem_size);
                    self.emit_c_store(ptr, src, width, float32);
                    src
                } else if matches!(&obj_ty, Some(TypeKind::Named { name, .. }) if name == "Array") {
                    let type_kinds: Vec<TypeKind> =
                        if let Some(TypeKind::Named { type_args, .. }) = &obj_ty {
                            type_args.iter().map(|t| t.node.clone()).collect()
                        } else {
                            vec![]
                        };
                    let set_target = if type_kinds.is_empty() {
                        "Array.set".to_string()
                    } else {
                        let mangled = crate::semantic::typecheck::mangle_monomorphized(
                            "Array.set",
                            &type_kinds,
                        );
                        if self.fn_index.contains_key(&mangled) {
                            mangled
                        } else {
                            "Array.set".to_string()
                        }
                    };
                    let obj_reg = self.compile_expr(object);
                    let idx_reg = self.compile_expr(index);
                    let _dst = self.alloc_reg();
                    self.emit_call_by_name(&set_target, &[obj_reg, idx_reg, src], _dst);
                    src
                } else if matches!(obj_ty, Some(TypeKind::Slice { .. })) {
                    let ptr = self.compile_expr(object);
                    let idx_reg = self.compile_expr(index);
                    let eight = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                    let offset = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Mul, offset, idx_reg, eight));
                    let addr = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Sub, addr, ptr, offset));
                    self.chunk.emit(mem_store(addr, src, 0));
                    src
                } else {
                    let base = self.compile_expr(object);
                    if let ExprKind::Literal(Literal::Int(n)) = &index.node
                        && *n >= 0
                    {
                        let elem_reg = base + *n as u8;
                        self.chunk.emit(rrr(Opcode::Mov, elem_reg, src, 0));
                        return src;
                    }
                    let idx_reg = self.compile_expr(index);
                    let ptr = self.alloc_reg();
                    self.chunk.emit(mem_lea(base, ptr, 0));
                    let eight = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                    let offset = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Mul, offset, idx_reg, eight));
                    self.chunk.emit(rrr(Opcode::Sub, ptr, ptr, offset));
                    self.chunk.emit(mem_store(ptr, src, 0));
                    src
                }
            }
            _ => src,
        }
    }

    /// Compute the address of an lvalue once so it can be loaded and stored
    /// without re-evaluating side-effect-free base/index expressions.
    fn compute_lvalue_addr(&mut self, target: &Expr) -> LvalueAddr {
        // Strip grouping parentheses so e.g. `(arr[0]) += 1` and `(*p)++` work.
        let mut target = target;
        while let ExprKind::Group(inner) = &target.node {
            target = inner;
        }
        match &target.node {
            ExprKind::Ident(name) => {
                if let Some(addr) = self.foreign_global_lvalue(target.span) {
                    addr
                } else {
                    LvalueAddr::Ident {
                        name: name.clone(),
                        span: target.span,
                    }
                }
            }
            ExprKind::Unary {
                op: UnaryOpKind::Deref,
                expr: ptr_expr,
            } => {
                let ptr = self.compile_expr(ptr_expr);
                let (width, signed) = self.raw_pointee_access(ptr_expr);
                LvalueAddr::Deref { ptr, width, signed }
            }
            ExprKind::Field {
                object,
                name: field_name,
            } => {
                let obj = self.compile_expr(object);
                if let Some(layout) = self.bit_field_layout(object, field_name) {
                    LvalueAddr::BitField { obj, layout }
                } else {
                    let byte_offset = self.field_offset(object, field_name);
                    LvalueAddr::Field {
                        obj,
                        offset: byte_offset,
                    }
                }
            }
            ExprKind::Index { object, indices } => {
                let obj_key = (object.span.start, object.span.end);
                let obj_ty = self.type_of_span(obj_key);
                let index = indices.first().expect("index must have at least one index");

                if let Some(TypeKind::FlexibleArray { elem_ty }) = &obj_ty {
                    let base = self.compile_expr(object);
                    let idx = self.compile_expr(index);
                    let (width, signed, elem_size, _) = self.c_memory_access(&elem_ty.node);
                    let ptr = self.emit_indexed_c_address(base, idx, elem_size);
                    LvalueAddr::Deref { ptr, width, signed }
                } else if matches!(&obj_ty, Some(TypeKind::Named { name, .. }) if name == "Array") {
                    let type_kinds: Vec<TypeKind> =
                        if let Some(TypeKind::Named { type_args, .. }) = &obj_ty {
                            type_args.iter().map(|t| t.node.clone()).collect()
                        } else {
                            vec![]
                        };
                    let (set_target, index_target) = if type_kinds.is_empty() {
                        ("Array.set".to_string(), "Array.index".to_string())
                    } else {
                        let set_mangled = crate::semantic::typecheck::mangle_monomorphized(
                            "Array.set",
                            &type_kinds,
                        );
                        let index_mangled = crate::semantic::typecheck::mangle_monomorphized(
                            "Array.index",
                            &type_kinds,
                        );
                        let set_target = if self.fn_index.contains_key(&set_mangled) {
                            set_mangled
                        } else {
                            "Array.set".to_string()
                        };
                        let index_target = if self.fn_index.contains_key(&index_mangled) {
                            index_mangled
                        } else {
                            "Array.index".to_string()
                        };
                        (set_target, index_target)
                    };
                    let obj = self.compile_expr(object);
                    let idx = self.compile_expr(index);
                    LvalueAddr::IndexArray {
                        obj,
                        idx,
                        index_target,
                        set_target,
                    }
                } else if matches!(obj_ty, Some(TypeKind::Slice { .. })) {
                    let ptr = self.compile_expr(object);
                    let idx = self.compile_expr(index);
                    LvalueAddr::IndexSlice { ptr, idx }
                } else {
                    let base = self.compile_expr(object);
                    if let ExprKind::Literal(Literal::Int(n)) = &index.node
                        && *n >= 0
                    {
                        LvalueAddr::IndexFixed {
                            base,
                            idx: 0,
                            literal: Some(*n),
                        }
                    } else {
                        let idx = self.compile_expr(index);
                        LvalueAddr::IndexFixed {
                            base,
                            idx,
                            literal: None,
                        }
                    }
                }
            }
            _ => LvalueAddr::Ident {
                name: String::new(),
                span: target.span,
            },
        }
    }

    /// Load the current value from an lvalue address.
    fn load_lvalue(&mut self, addr: &LvalueAddr) -> u8 {
        match addr {
            LvalueAddr::Ident { name, .. } => self.reg_of(name),
            LvalueAddr::Deref { ptr, width, signed } => {
                let dst = self.alloc_reg();
                self.chunk.emit(mem_load_w(*ptr, dst, 0, *width, *signed));
                dst
            }
            LvalueAddr::ForeignGlobal {
                ptr,
                width,
                signed,
                float32,
            } => self.emit_c_load(*ptr, *width, *signed, *float32),
            LvalueAddr::Field { obj, offset } => {
                let dst = self.alloc_reg();
                self.chunk.emit(field_load(dst, *obj, *offset));
                dst
            }
            LvalueAddr::BitField { obj, layout } => self.emit_bit_field_load(*obj, *layout),
            LvalueAddr::IndexArray {
                obj,
                idx,
                index_target,
                ..
            } => {
                let dst = self.alloc_reg();
                self.emit_call_by_name(index_target, &[*obj, *idx], dst);
                dst
            }
            LvalueAddr::IndexSlice { ptr, idx } => {
                let eight = self.alloc_reg();
                self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                let offset = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::Mul, offset, *idx, eight));
                let addr_reg = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::Sub, addr_reg, *ptr, offset));
                let dst = self.alloc_reg();
                self.chunk.emit(mem_load(addr_reg, dst, 0));
                dst
            }
            LvalueAddr::IndexFixed { base, idx, literal } => {
                if let Some(n) = literal {
                    return *base + *n as u8;
                }
                let ptr = self.alloc_reg();
                self.chunk.emit(mem_lea(*base, ptr, 0));
                let eight = self.alloc_reg();
                self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                let offset = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::Mul, offset, *idx, eight));
                self.chunk.emit(rrr(Opcode::Sub, ptr, ptr, offset));
                let dst = self.alloc_reg();
                self.chunk.emit(mem_load(ptr, dst, 0));
                dst
            }
        }
    }

    /// Store `src` into an lvalue address.
    fn store_lvalue(&mut self, addr: &LvalueAddr, src: u8) {
        match addr {
            LvalueAddr::Ident { name, span } => {
                self.drop_local_now(name);
                let dst = self.reg_of(name);
                if dst != src {
                    self.chunk.emit(rrr(Opcode::Mov, dst, src, 0));
                }
                let local_ty = self.type_of_span((span.start, span.end));
                self.reactivate_drop_local(name, dst, local_ty);
            }
            LvalueAddr::Deref { ptr, width, .. } => {
                self.chunk.emit(mem_store_w(*ptr, src, 0, *width));
            }
            LvalueAddr::ForeignGlobal {
                ptr,
                width,
                float32,
                ..
            } => self.emit_c_store(*ptr, src, *width, *float32),
            LvalueAddr::Field { obj, offset } => {
                self.chunk.emit(field_store(src, *obj, *offset));
            }
            LvalueAddr::BitField { obj, layout } => {
                self.emit_bit_field_store(*obj, src, *layout);
            }
            LvalueAddr::IndexArray {
                obj,
                idx,
                set_target,
                ..
            } => {
                let _dst = self.alloc_reg();
                self.emit_call_by_name(set_target, &[*obj, *idx, src], _dst);
            }
            LvalueAddr::IndexSlice { ptr, idx } => {
                let eight = self.alloc_reg();
                self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                let offset = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::Mul, offset, *idx, eight));
                let addr_reg = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::Sub, addr_reg, *ptr, offset));
                self.chunk.emit(mem_store(addr_reg, src, 0));
            }
            LvalueAddr::IndexFixed { base, idx, literal } => {
                if let Some(n) = literal {
                    let elem_reg = *base + *n as u8;
                    self.chunk.emit(rrr(Opcode::Mov, elem_reg, src, 0));
                } else {
                    let ptr = self.alloc_reg();
                    self.chunk.emit(mem_lea(*base, ptr, 0));
                    let eight = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                    let offset = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Mul, offset, *idx, eight));
                    self.chunk.emit(rrr(Opcode::Sub, ptr, ptr, offset));
                    self.chunk.emit(mem_store(ptr, src, 0));
                }
            }
        }
    }

    fn alloc_reg(&mut self) -> u8 {
        if self.next_reg >= u8::MAX as u16 {
            self.codegen_error.get_or_insert_with(|| {
                format!(
                    "function `{}` needs more than 255 QZI register slots; split the function or reduce expression complexity",
                    self.chunk.name
                )
            });
            return 0;
        }
        let r = self.next_reg as u8;
        self.next_reg += 1;
        r
    }

    fn next_reg_slot(&mut self) -> u8 {
        if self.next_reg >= u8::MAX as u16 {
            let _ = self.alloc_reg();
            0
        } else {
            self.next_reg as u8
        }
    }

    fn reserve_reg_block(&mut self, count: usize) -> u8 {
        if count == 0 {
            return self.next_reg_slot();
        }
        let base = self.next_reg_slot();
        for _ in 0..count {
            let _ = self.alloc_reg();
        }
        base
    }

    /// Recursively compile a pattern match against `value_reg`.
    /// Jumps that skip to the next arm on mismatch are pushed into `skip_patches`.
    /// Successful bindings (PatternKind::Bind) are added to the current scope.
    fn compile_pattern_match(
        &mut self,
        pattern: &Pattern,
        value_reg: u8,
        skip_patches: &mut Vec<usize>,
    ) {
        match &pattern.node.clone() {
            PatternKind::Wildcard => { /* no check, no bind */ }
            PatternKind::Bind(name) => {
                let bound = self.bind(name.clone());
                self.chunk.emit(rrr(Opcode::Mov, bound, value_reg, 0));
            }
            PatternKind::Literal(lit) => {
                match lit {
                    LiteralValue::Int(n) => {
                        let tag_reg = self.alloc_reg();
                        if *n >= 0 && *n <= u16::MAX as i64 {
                            self.chunk.emit(ri16(Opcode::MovI, tag_reg, *n as u16));
                        } else {
                            let idx = self.chunk.add_constant(ConstPoolEntry::Int(*n));
                            self.chunk.emit(ri16(Opcode::MovConst, tag_reg, idx));
                        }
                        self.chunk.emit(rrr(Opcode::Cmp, 0, value_reg, tag_reg));
                        skip_patches.push(self.chunk.emit(ri16(Opcode::Jne, 0, 0)));
                    }
                    LiteralValue::Float(f) => {
                        let tag_reg = self.alloc_reg();
                        let idx = self.chunk.add_constant(ConstPoolEntry::Float(*f));
                        self.chunk.emit(ri16(Opcode::MovConst, tag_reg, idx));
                        self.chunk.emit(rrr(Opcode::Cmp, 0, value_reg, tag_reg));
                        skip_patches.push(self.chunk.emit(ri16(Opcode::Jne, 0, 0)));
                    }
                    LiteralValue::Bool(b) => {
                        let tag_reg = self.alloc_reg();
                        self.chunk.emit(ri16(Opcode::MovI, tag_reg, *b as u16));
                        self.chunk.emit(rrr(Opcode::Cmp, 0, value_reg, tag_reg));
                        skip_patches.push(self.chunk.emit(ri16(Opcode::Jne, 0, 0)));
                    }
                    LiteralValue::Str(s) => {
                        // Load the literal constant.
                        let lit_reg = self.alloc_reg();
                        let const_idx = self.chunk.add_constant(ConstPoolEntry::Str(s.clone()));
                        self.chunk.emit(ri16(Opcode::MovConst, lit_reg, const_idx));
                        // Compare lengths first — fast-path for mismatches.
                        let len_a = self.alloc_reg();
                        self.chunk.emit(rrr(Opcode::StrLen, len_a, value_reg, 0));
                        let len_b = self.alloc_reg();
                        self.chunk.emit(rrr(Opcode::StrLen, len_b, lit_reg, 0));
                        self.chunk.emit(rrr(Opcode::Cmp, 0, len_a, len_b));
                        skip_patches.push(self.chunk.emit(ri16(Opcode::Jne, 0, 0)));
                        // Byte-by-byte comparison loop.
                        let idx = self.alloc_reg();
                        self.chunk.emit(ri16(Opcode::MovI, idx, 0));
                        let loop_start = self.chunk.len() as u16;
                        // if idx == len_a → all bytes matched, fall through
                        self.chunk.emit(rrr(Opcode::Cmp, 0, idx, len_a));
                        let done_equal = self.chunk.emit(ri16(Opcode::Je, 0, 0));
                        // byte_a = quazi.str.byte_at(value_reg, idx): args must be in r, r+1
                        let r_a = self.alloc_reg();
                        let r_a_idx = self.alloc_reg(); // must be r_a + 1
                        self.chunk.emit(rrr(Opcode::Mov, r_a, value_reg, 0));
                        self.chunk.emit(rrr(Opcode::Mov, r_a_idx, idx, 0));
                        let mut intr_a = ri16(Opcode::Intrinsic, r_a, 23);
                        intr_a.flags = 2;
                        self.chunk.emit(intr_a);
                        // byte_b = quazi.str.byte_at(lit_reg, idx)
                        let r_b = self.alloc_reg();
                        let r_b_idx = self.alloc_reg(); // must be r_b + 1
                        self.chunk.emit(rrr(Opcode::Mov, r_b, lit_reg, 0));
                        self.chunk.emit(rrr(Opcode::Mov, r_b_idx, idx, 0));
                        let mut intr_b = ri16(Opcode::Intrinsic, r_b, 23);
                        intr_b.flags = 2;
                        self.chunk.emit(intr_b);
                        // Compare bytes; jump to next arm if different.
                        self.chunk.emit(rrr(Opcode::Cmp, 0, r_a, r_b));
                        skip_patches.push(self.chunk.emit(ri16(Opcode::Jne, 0, 0)));
                        // idx++
                        let one = self.alloc_reg();
                        self.chunk.emit(ri16(Opcode::MovI, one, 1));
                        self.chunk.emit(rrr(Opcode::Add, idx, idx, one));
                        // Backward jump to loop start.
                        self.chunk.emit(ri16(Opcode::Jmp, 0, loop_start));
                        // Patch done_equal to here (strings are equal).
                        self.chunk.patch_jump(done_equal, self.chunk.len() as u16);
                    }
                }
            }
            PatternKind::Variant {
                enum_name,
                variant,
                sub_patterns,
            } => {
                let tag = self.variant_tag(enum_name.as_deref(), variant);
                let disc = self.alloc_reg();
                self.chunk
                    .emit(rrr(Opcode::FieldLoad, disc, value_reg, ENUM_DISCRIM_OFFSET));
                let tag_reg = self.alloc_reg();
                let encoded_tag = self.qzi_u16(tag, "enum variant tag");
                self.chunk.emit(ri16(Opcode::MovI, tag_reg, encoded_tag));
                self.chunk.emit(rrr(Opcode::Cmp, 0, disc, tag_reg));
                skip_patches.push(self.chunk.emit(ri16(Opcode::Jne, 0, 0)));
                for (i, sub) in sub_patterns.iter().enumerate() {
                    let payload_reg = self.alloc_reg();
                    let off = ENUM_PAYLOAD_OFFSET + (i as u16 * 8);
                    self.chunk.emit(field_load(payload_reg, value_reg, off));
                    self.compile_pattern_match(sub, payload_reg, skip_patches);
                }
            }
        }
    }

    fn bind(&mut self, name: String) -> u8 {
        let r = self.alloc_reg();
        self.regs.insert(name, r);
        r
    }

    fn reg_of(&mut self, name: &str) -> u8 {
        match self.regs.get(name).copied() {
            Some(register) => register,
            None => {
                if self.codegen_error.is_none() {
                    self.codegen_error = Some(format!(
                        "internal codegen error: no register is bound for `{name}`"
                    ));
                }
                0
            }
        }
    }

    fn drop_fn_for_type(&self, ty: &TypeKind) -> Option<String> {
        let TypeKind::Named { name, type_args } = self.resolve_type(ty) else {
            return None;
        };
        let base = format!("{}.free", name);
        if !type_args.is_empty() {
            let type_kinds: Vec<TypeKind> = type_args.iter().map(|t| t.node.clone()).collect();
            let mangled = crate::semantic::typecheck::mangle_monomorphized(&base, &type_kinds);
            if self.fn_index.contains_key(&mangled) {
                return Some(mangled);
            }
        }
        self.fn_index.contains_key(&base).then_some(base)
    }

    fn register_drop_local(&mut self, name: &str, reg: u8, ty: Option<TypeKind>) {
        let Some(drop_fn) = ty.as_ref().and_then(|t| self.drop_fn_for_type(t)) else {
            return;
        };
        if let Some(scope) = self.drop_scopes.last_mut() {
            scope.push(DropLocal {
                name: name.to_string(),
                reg,
                drop_fn,
                active: true,
            });
        }
    }

    fn deactivate_drop_local(&mut self, name: &str) {
        for scope in self.drop_scopes.iter_mut().rev() {
            if let Some(local) = scope.iter_mut().rev().find(|local| local.name == name) {
                local.active = false;
                return;
            }
        }
    }

    fn reactivate_drop_local(&mut self, name: &str, reg: u8, ty: Option<TypeKind>) {
        let Some(drop_fn) = ty.as_ref().and_then(|t| self.drop_fn_for_type(t)) else {
            return;
        };
        for scope in self.drop_scopes.iter_mut().rev() {
            if let Some(local) = scope.iter_mut().rev().find(|local| local.name == name) {
                local.reg = reg;
                local.drop_fn = drop_fn;
                local.active = true;
                return;
            }
        }
    }

    fn drop_local_now(&mut self, name: &str) {
        let mut to_drop: Option<(u8, String)> = None;
        for scope in self.drop_scopes.iter_mut().rev() {
            if let Some(local) = scope.iter_mut().rev().find(|local| local.name == name) {
                if local.active {
                    local.active = false;
                    to_drop = Some((local.reg, local.drop_fn.clone()));
                }
                break;
            }
        }
        if let Some((reg, drop_fn)) = to_drop {
            let dst = self.alloc_reg();
            self.emit_call_by_name(&drop_fn, &[reg], dst);
        }
    }

    fn mark_consumed_expr(&mut self, expr: &Expr) {
        match &expr.node {
            ExprKind::Ident(name) => self.deactivate_drop_local(name),
            ExprKind::Group(inner) | ExprKind::Try { expr: inner } => {
                self.mark_consumed_expr(inner)
            }
            ExprKind::ArrayLit(elems) => {
                for elem in elems {
                    self.mark_consumed_expr(elem);
                }
            }
            ExprKind::StructInit { fields, .. } => {
                for (_, value) in fields {
                    self.mark_consumed_expr(value);
                }
            }
            _ => {}
        }
    }

    fn emit_scope_cleanup(&mut self) {
        let Some(scope) = self.drop_scopes.last_mut() else {
            return;
        };
        let drops = scope
            .iter_mut()
            .rev()
            .filter_map(|local| {
                if local.active {
                    local.active = false;
                    Some((local.reg, local.drop_fn.clone()))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for (reg, drop_fn) in drops {
            let dst = self.alloc_reg();
            self.emit_call_by_name(&drop_fn, &[reg], dst);
        }
    }

    fn emit_all_cleanup(&mut self) {
        for depth in (0..self.drop_scopes.len()).rev() {
            let drops = self.drop_scopes[depth]
                .iter()
                .rev()
                .filter_map(|local| {
                    if local.active {
                        Some((local.reg, local.drop_fn.clone()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            for (reg, drop_fn) in drops {
                let dst = self.alloc_reg();
                self.emit_call_by_name(&drop_fn, &[reg], dst);
            }
        }
    }

    fn has_active_drops(&self) -> bool {
        self.drop_scopes
            .iter()
            .any(|scope| scope.iter().any(|local| local.active))
    }

    /// Emit an indirect call through a register, dispatching via env-wrapper convention
    /// when `callee_reg` is a closure environment struct pointer.
    fn emit_indirect_call(&mut self, dst: u8, callee_reg: u8, arg_regs: &[u8]) {
        // All fn-ptr values use env struct representation: {fn_ptr at offset 0, captures...}.
        // Always do env dispatch: load fn_ptr from env[0], pass env as hidden first arg.
        let fn_ptr_reg = self.alloc_reg();
        self.chunk.emit(rrr(
            Opcode::FieldLoad,
            fn_ptr_reg,
            callee_reg,
            ENUM_DISCRIM_OFFSET,
        ));
        self.chunk.emit(rrr(Opcode::CallArg, callee_reg, 0, 0));
        for &r in arg_regs {
            self.chunk.emit(rrr(Opcode::CallArg, r, 0, 0));
        }
        self.chunk.emit(rrr(Opcode::CallReg, dst, fn_ptr_reg, 0));
    }

    fn emit_c_indirect_call(
        &mut self,
        dst: u8,
        callee_reg: u8,
        arg_regs: &[u8],
        signature: AbiSignature,
    ) {
        for &reg in arg_regs {
            self.chunk.emit(rrr(Opcode::CallArg, reg, 0, 0));
        }
        let signature_index =
            self.chunk
                .add_constant(ConstPoolEntry::ForeignSymbol(ForeignSymbol {
                    symbol: "<function-pointer>".to_string(),
                    signature,
                }));
        self.chunk
            .emit(call_c_reg(dst, callee_reg, signature_index));
    }

    fn emit_c_callback_address(&mut self, resolved_name: &str) -> Option<u8> {
        let fn_idx = *self.fn_index.get(resolved_name)?;
        let adapter_name = export_adapter_name(resolved_name, fn_idx);
        let dst = self.alloc_reg();
        let constant = self
            .chunk
            .add_constant(ConstPoolEntry::FnAddr(adapter_name));
        self.chunk.emit(ri16(Opcode::MovConst, dst, constant));
        Some(dst)
    }

    // ── Block / statement ──

    /// Compile an else-if / else chain.  `end_jumps` collects the `Jmp`
    /// instructions emitted after each branch's then-block; they are all
    /// patched to the final end-of-chain address by the caller.
    fn compile_else_if_chain(
        &mut self,
        else_if: &[(Expr, Block)],
        else_block: &Option<Block>,
        end_jumps: &mut Vec<usize>,
    ) -> bool {
        if else_if.is_empty() {
            return if let Some(eb) = else_block {
                self.compile_block(eb)
            } else {
                false
            };
        }
        let (cond, block) = &else_if[0];
        let rest = &else_if[1..];
        let cond_key = (cond.span.start, cond.span.end);
        if let Some(ConstValue::Bool(b)) = self.const_map.get(&cond_key).cloned() {
            if b {
                let returns = self.compile_block(block);
                let jmp = self.chunk.emit(ri16(Opcode::Jmp, 0, 0));
                end_jumps.push(jmp);
                return returns;
            }
            return self.compile_else_if_chain(rest, else_block, end_jumps);
        }
        let jump_else = self.compile_condition_jump(cond, true);
        let block_returns = self.compile_block(block);
        let jmp = self.chunk.emit(ri16(Opcode::Jmp, 0, 0));
        end_jumps.push(jmp);
        self.chunk.patch_jump(jump_else, self.chunk.len() as u16);
        let rest_returns = self.compile_else_if_chain(rest, else_block, end_jumps);
        block_returns && rest_returns
    }

    fn compile_block(&mut self, block: &Block) -> bool {
        self.drop_scopes.push(Vec::new());
        for stmt in &block.stmts {
            if self.compile_stmt(stmt) {
                self.drop_scopes.pop();
                return true;
            }
        }
        self.emit_scope_cleanup();
        self.drop_scopes.pop();
        false
    }

    /// Returns true if the statement guarantees exit (return).
    fn compile_stmt(&mut self, stmt: &Stmt) -> bool {
        match &stmt.node {
            StmtKind::Var {
                name, ty, value, ..
            } => {
                if let Some(expr) = value {
                    self.mark_consumed_expr(expr);
                    let src = self.compile_expr(expr);
                    // Coerce concrete Named type → dyn Trait fat pointer when declared type is Dyn.
                    let coerced = if let Some(ty_ann) = ty {
                        if let TypeKind::Dyn { trait_name } = &ty_ann.node {
                            let key = (expr.span.start, expr.span.end);
                            if let Some(TypeKind::Named {
                                name: type_name, ..
                            }) = self.type_of_span(key)
                            {
                                self.coerce_to_dyn(src, &type_name, trait_name)
                            } else {
                                src
                            }
                        } else {
                            src
                        }
                    } else {
                        src
                    };
                    self.regs.insert(name.clone(), coerced);
                    let local_ty = ty
                        .as_ref()
                        .map(|t| self.resolve_type(&t.node))
                        .or_else(|| self.type_of_span((expr.span.start, expr.span.end)));
                    if let Some(local_ty) = local_ty.clone() {
                        self.local_types.insert(name.clone(), local_ty);
                    }
                    self.register_drop_local(name, coerced, local_ty);
                } else {
                    let reg = self.bind(name.clone());
                    let local_ty = ty.as_ref().map(|t| self.resolve_type(&t.node));
                    if let Some(local_ty) = local_ty.clone() {
                        self.local_types.insert(name.clone(), local_ty);
                    }
                    self.register_drop_local(name, reg, local_ty);
                }
                false
            }
            StmtKind::Const { name, value, .. } => {
                self.mark_consumed_expr(value);
                let src = self.compile_expr(value);
                self.regs.insert(name.clone(), src);
                let local_ty = self.type_of_span((value.span.start, value.span.end));
                if let Some(local_ty) = local_ty.clone() {
                    self.local_types.insert(name.clone(), local_ty);
                }
                self.register_drop_local(name, src, local_ty);
                false
            }
            StmtKind::Return(expr) => {
                if let Some(expr) = expr {
                    self.mark_consumed_expr(expr);
                    let src = self.compile_expr(expr);
                    if self.has_active_drops() {
                        let tmp = self.alloc_reg();
                        self.chunk.emit(rrr(Opcode::Mov, tmp, src, 0));
                        self.emit_all_cleanup();
                        if tmp != 0 {
                            self.chunk.emit(rrr(Opcode::Mov, 0, tmp, 0));
                        }
                    } else if src != 0 {
                        self.chunk.emit(rrr(Opcode::Mov, 0, src, 0));
                    }
                } else {
                    self.emit_all_cleanup();
                    self.chunk.emit(ri16(Opcode::MovI, 0, 0));
                }
                self.chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
                true
            }
            StmtKind::ExprStmt(expr) => {
                self.compile_expr(expr);
                false
            }
            StmtKind::CfgBlock { body, condition } => {
                if cfg_condition_matches(condition) {
                    self.compile_block(body)
                } else {
                    false
                }
            }
            StmtKind::If {
                condition,
                then_block,
                else_if,
                else_block,
            } => {
                // Constant-condition elimination: skip the dead branch entirely.
                let cond_key = (condition.span.start, condition.span.end);
                if let Some(ConstValue::Bool(b)) = self.const_map.get(&cond_key).cloned() {
                    if b {
                        return self.compile_block(then_block);
                    }
                    let mut end_jumps = Vec::new();
                    let returns = self.compile_else_if_chain(else_if, else_block, &mut end_jumps);
                    for jmp in end_jumps {
                        self.chunk.patch_jump(jmp, self.chunk.len() as u16);
                    }
                    return returns;
                }

                // Emit condition + jump-if-false past the then block.
                let jump_else = self.compile_condition_jump(condition, true);
                let then_returns = self.compile_block(then_block);
                let mut end_jumps = vec![self.chunk.emit(ri16(Opcode::Jmp, 0, 0))];
                self.chunk.patch_jump(jump_else, self.chunk.len() as u16);

                let chain_returns = self.compile_else_if_chain(else_if, else_block, &mut end_jumps);
                for jmp in end_jumps {
                    self.chunk.patch_jump(jmp, self.chunk.len() as u16);
                }
                then_returns && chain_returns
            }
            StmtKind::Break => {
                if let Some(frame) = self.loop_stack.last_mut() {
                    let jmp = self.chunk.emit(ri16(Opcode::Jmp, 0, 0));
                    frame.break_jumps.push(jmp);
                }
                true
            }
            StmtKind::Continue => {
                if let Some(frame) = self.loop_stack.last_mut() {
                    let jmp = self.chunk.emit(ri16(Opcode::Jmp, 0, 0));
                    frame.continue_jumps.push(jmp);
                }
                true
            }
            StmtKind::For { kind, body } => {
                match kind {
                    ForLoop::Cond { condition: None } => {
                        let loop_top = self.chunk.len() as u16;
                        self.loop_stack.push(LoopFrame::new());
                        self.compile_block(body);
                        let frame = self.loop_stack.pop().unwrap();
                        for jmp in &frame.continue_jumps {
                            self.chunk.patch_jump(*jmp, loop_top);
                        }
                        self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                        let exit_pos = self.chunk.len() as u16;
                        for jmp in &frame.break_jumps {
                            self.chunk.patch_jump(*jmp, exit_pos);
                        }
                    }
                    ForLoop::Cond {
                        condition: Some(condition),
                    } => {
                        let cond_key = (condition.span.start, condition.span.end);
                        if let Some(ConstValue::Bool(false)) =
                            self.const_map.get(&cond_key).cloned()
                        {
                            return false;
                        }
                        let loop_top = self.chunk.len() as u16;
                        let jump_exit = self.compile_condition_jump(condition, true);
                        self.loop_stack.push(LoopFrame::new());
                        self.compile_block(body);
                        let frame = self.loop_stack.pop().unwrap();
                        for jmp in &frame.continue_jumps {
                            self.chunk.patch_jump(*jmp, loop_top);
                        }
                        self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                        let exit_pos = self.chunk.len() as u16;
                        self.chunk.patch_jump(jump_exit, exit_pos);
                        for jmp in &frame.break_jumps {
                            self.chunk.patch_jump(*jmp, exit_pos);
                        }
                    }
                    ForLoop::CStyle {
                        init,
                        condition,
                        update,
                    } => {
                        if let Some(init_stmt) = init {
                            self.compile_stmt(init_stmt);
                        }
                        let loop_top = self.chunk.len() as u16;
                        let jump_exit = condition
                            .as_ref()
                            .map(|cond| self.compile_condition_jump(cond, true));
                        self.loop_stack.push(LoopFrame::new());
                        self.compile_block(body);
                        let frame = self.loop_stack.pop().unwrap();
                        let continue_pos = self.chunk.len() as u16;
                        for jmp in &frame.continue_jumps {
                            self.chunk.patch_jump(*jmp, continue_pos);
                        }
                        if let Some(upd) = update {
                            self.compile_expr(upd);
                        }
                        self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                        let exit_pos = self.chunk.len() as u16;
                        if let Some(je) = jump_exit {
                            self.chunk.patch_jump(je, exit_pos);
                        }
                        for jmp in &frame.break_jumps {
                            self.chunk.patch_jump(*jmp, exit_pos);
                        }
                    }
                    ForLoop::Each { vars, iter } => match iter {
                        ForIter::Range { start, end } => {
                            let loop_var = vars.first().map(|s| s.as_str()).unwrap_or("_");
                            let r_i = self.bind(loop_var.to_string());
                            let r_start = self.compile_expr(start);
                            if r_start != r_i {
                                self.chunk.emit(rrr(Opcode::Mov, r_i, r_start, 0));
                            }
                            let r_end = self.compile_expr(end);
                            let loop_top = self.chunk.len() as u16;
                            self.chunk.emit(rrr(Opcode::Cmp, 0, r_i, r_end));
                            let jump_exit = self.chunk.emit(ri16(Opcode::Jge, 0, 0));
                            self.loop_stack.push(LoopFrame::new());
                            self.compile_block(body);
                            let frame = self.loop_stack.pop().unwrap();
                            let continue_pos = self.chunk.len() as u16;
                            for jmp in &frame.continue_jumps {
                                self.chunk.patch_jump(*jmp, continue_pos);
                            }
                            self.chunk.emit(rrr(Opcode::Inc, r_i, r_i, 0));
                            self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                            let exit_pos = self.chunk.len() as u16;
                            self.chunk.patch_jump(jump_exit, exit_pos);
                            for jmp in &frame.break_jumps {
                                self.chunk.patch_jump(*jmp, exit_pos);
                            }
                        }
                        ForIter::Iter(expr) => {
                            let iter_key = (expr.span.start, expr.span.end);
                            let mut iter_ty = self.type_of_span(iter_key);
                            let original_expr = expr.clone();
                            let mut expr = expr;
                            let is_borrow = if let Some(TypeKind::Ref { inner }) = &iter_ty {
                                if let ExprKind::Unary {
                                    op: UnaryOpKind::Ref,
                                    expr: inner_expr,
                                } = &expr.node
                                {
                                    expr = inner_expr;
                                    iter_ty = Some(inner.node.clone());
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            };
                            if !is_borrow {
                                self.mark_consumed_expr(&original_expr);
                            }
                            let static_array_len: Option<u16> = match &iter_ty {
                                Some(TypeKind::Array { len, .. }) => {
                                    Some(self.qzi_u16_from_u64(*len, "fixed array length"))
                                }
                                _ => None,
                            };
                            if matches!(
                                iter_ty,
                                Some(TypeKind::Slice { .. }) | Some(TypeKind::Array { .. })
                            ) {
                                let ptr = self.compile_expr(expr);
                                if !is_borrow {
                                    self.register_drop_local("__for_iter", ptr, iter_ty.clone());
                                }
                                let len_reg = if let Some(n) = static_array_len {
                                    let r = self.alloc_reg();
                                    self.chunk.emit(ri16(Opcode::MovI, r, n));
                                    r
                                } else {
                                    if let ExprKind::Ident(vname) = &expr.node {
                                        self.regs
                                            .get(&format!("__len_{}", vname))
                                            .copied()
                                            .unwrap_or(ptr + 1)
                                    } else {
                                        ptr + 1
                                    }
                                };
                                let base_addr = if static_array_len.is_some() {
                                    let r = self.alloc_reg();
                                    self.chunk.emit(mem_lea(ptr, r, 0));
                                    r
                                } else {
                                    ptr
                                };
                                let (r_counter, r_val_opt) = match vars.as_slice() {
                                    [] => (self.alloc_reg(), None),
                                    [v] => (self.alloc_reg(), Some(self.bind(v.clone()))),
                                    [i, v, ..] => {
                                        let ri = self.bind(i.clone());
                                        let rv = self.bind(v.clone());
                                        (ri, Some(rv))
                                    }
                                };
                                self.chunk.emit(ri16(Opcode::MovI, r_counter, 0));
                                let loop_top = self.chunk.len() as u16;
                                self.chunk.emit(rrr(Opcode::Cmp, 0, r_counter, len_reg));
                                let jump_exit = self.chunk.emit(ri16(Opcode::Jge, 0, 0));
                                if let Some(r_val) = r_val_opt {
                                    let eight = self.alloc_reg();
                                    self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                                    let offset = self.alloc_reg();
                                    self.chunk.emit(rrr(Opcode::Mul, offset, r_counter, eight));
                                    let addr = self.alloc_reg();
                                    self.chunk.emit(rrr(Opcode::Sub, addr, base_addr, offset));
                                    self.chunk.emit(mem_load(addr, r_val, 0));
                                }
                                self.loop_stack.push(LoopFrame::new());
                                self.compile_block(body);
                                let frame = self.loop_stack.pop().unwrap();
                                let continue_pos = self.chunk.len() as u16;
                                for jmp in &frame.continue_jumps {
                                    self.chunk.patch_jump(*jmp, continue_pos);
                                }
                                self.chunk.emit(rrr(Opcode::Inc, r_counter, r_counter, 0));
                                self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                                let exit_pos = self.chunk.len() as u16;
                                self.chunk.patch_jump(jump_exit, exit_pos);
                                for jmp in &frame.break_jumps {
                                    self.chunk.patch_jump(*jmp, exit_pos);
                                }
                            } else if matches!(&iter_ty, Some(TypeKind::Named { name, .. }) if name == "Array")
                            {
                                let type_kinds: Vec<TypeKind> =
                                    if let Some(TypeKind::Named { type_args, .. }) = &iter_ty {
                                        type_args.iter().map(|t| t.node.clone()).collect()
                                    } else {
                                        vec![]
                                    };
                                let len_target = if type_kinds.is_empty() {
                                    "Array.len".to_string()
                                } else {
                                    let mangled = crate::semantic::typecheck::mangle_monomorphized(
                                        "Array.len",
                                        &type_kinds,
                                    );
                                    if self.fn_index.contains_key(&mangled) {
                                        mangled
                                    } else {
                                        "Array.len".to_string()
                                    }
                                };
                                let get_target = if type_kinds.is_empty() {
                                    "Array.get".to_string()
                                } else {
                                    let mangled = crate::semantic::typecheck::mangle_monomorphized(
                                        "Array.get",
                                        &type_kinds,
                                    );
                                    if self.fn_index.contains_key(&mangled) {
                                        mangled
                                    } else {
                                        "Array.get".to_string()
                                    }
                                };
                                let arr_reg = self.compile_expr(expr);
                                if !is_borrow {
                                    self.register_drop_local(
                                        "__for_iter",
                                        arr_reg,
                                        iter_ty.clone(),
                                    );
                                }
                                let len_reg = self.alloc_reg();
                                self.emit_call_by_name(&len_target, &[arr_reg], len_reg);

                                let (r_idx, r_val_opt) = match vars.as_slice() {
                                    [] => (self.alloc_reg(), None),
                                    [v] => (self.alloc_reg(), Some(self.bind(v.clone()))),
                                    [i, v, ..] => {
                                        let ri = self.bind(i.clone());
                                        let rv = self.bind(v.clone());
                                        (ri, Some(rv))
                                    }
                                };
                                self.chunk.emit(ri16(Opcode::MovI, r_idx, 0));
                                let loop_top = self.chunk.len() as u16;
                                self.chunk.emit(rrr(Opcode::Cmp, 0, r_idx, len_reg));
                                let jump_exit = self.chunk.emit(ri16(Opcode::Jge, 0, 0));
                                if let Some(r_val) = r_val_opt {
                                    let elem_reg = self.alloc_reg();
                                    self.emit_call_by_name(
                                        &get_target,
                                        &[arr_reg, r_idx],
                                        elem_reg,
                                    );
                                    self.chunk.emit(rrr(Opcode::Mov, r_val, elem_reg, 0));
                                }
                                self.loop_stack.push(LoopFrame::new());
                                self.compile_block(body);
                                let frame = self.loop_stack.pop().unwrap();
                                let continue_pos = self.chunk.len() as u16;
                                for jmp in &frame.continue_jumps {
                                    self.chunk.patch_jump(*jmp, continue_pos);
                                }
                                self.chunk.emit(rrr(Opcode::Inc, r_idx, r_idx, 0));
                                self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                                let exit_pos = self.chunk.len() as u16;
                                self.chunk.patch_jump(jump_exit, exit_pos);
                                for jmp in &frame.break_jumps {
                                    self.chunk.patch_jump(*jmp, exit_pos);
                                }
                            } else {
                                let iter_reg = self.compile_expr(expr);
                                if !is_borrow {
                                    self.register_drop_local(
                                        "__for_iter",
                                        iter_reg,
                                        iter_ty.clone(),
                                    );
                                }
                                let iter_key = (expr.span.start, expr.span.end);
                                let iter_ty = self.type_of_span(iter_key);

                                let loop_var = vars.first().map(|s| s.as_str()).unwrap_or("_");
                                let r_val = self.bind(loop_var.to_string());

                                let loop_top = self.chunk.len() as u16;

                                let r_has_next = self.alloc_reg();
                                match &iter_ty {
                                    Some(TypeKind::Named { name, type_args }) => {
                                        let base = format!("{}.has_next", name);
                                        let mangled = if type_args.is_empty() {
                                            base.clone()
                                        } else {
                                            let type_kinds: Vec<TypeKind> =
                                                type_args.iter().map(|t| t.node.clone()).collect();
                                            crate::semantic::typecheck::mangle_monomorphized(
                                                &base,
                                                &type_kinds,
                                            )
                                        };
                                        let target = if self.fn_index.contains_key(&mangled) {
                                            mangled
                                        } else {
                                            base
                                        };
                                        self.emit_call_by_name(&target, &[iter_reg], r_has_next);
                                    }
                                    Some(TypeKind::Dyn { trait_name }) => {
                                        if let Some(slots) = self.trait_method_slots.get(trait_name)
                                        {
                                            let slot = slots.iter().position(|m| m == "has_next");
                                            let slot_idx = match slot {
                                                Some(slot) => self.qzi_u8(slot, "trait method slot"),
                                                None => {
                                                    if self.codegen_error.is_none() {
                                                        self.codegen_error = Some(format!(
                                                            "trait `{trait_name}` has no `has_next` slot"
                                                        ));
                                                    }
                                                    0
                                                }
                                            };
                                            let vtbl_ptr = self.alloc_reg();
                                            self.chunk.emit(rrr(
                                                Opcode::FieldLoad,
                                                vtbl_ptr,
                                                iter_reg,
                                                8,
                                            ));
                                            let fn_ptr = self.alloc_reg();
                                            self.chunk.emit(rrr(
                                                Opcode::VtblLoad,
                                                fn_ptr,
                                                vtbl_ptr,
                                                slot_idx,
                                            ));
                                            let data_ptr = self.alloc_reg();
                                            self.chunk.emit(rrr(
                                                Opcode::FieldLoad,
                                                data_ptr,
                                                iter_reg,
                                                0,
                                            ));
                                            self.chunk.emit(rrr(Opcode::CallArg, data_ptr, 0, 0));
                                            self.chunk.emit(rrr(
                                                Opcode::CallReg,
                                                r_has_next,
                                                fn_ptr,
                                                0,
                                            ));
                                        }
                                    }
                                    _ => {}
                                }

                                let jump_exit = self.chunk.emit(ri16(Opcode::Jz, r_has_next, 0));

                                let r_next_opt = self.alloc_reg();
                                match &iter_ty {
                                    Some(TypeKind::Named { name, type_args }) => {
                                        let base = format!("{}.next", name);
                                        let mangled = if type_args.is_empty() {
                                            base.clone()
                                        } else {
                                            let type_kinds: Vec<TypeKind> =
                                                type_args.iter().map(|t| t.node.clone()).collect();
                                            crate::semantic::typecheck::mangle_monomorphized(
                                                &base,
                                                &type_kinds,
                                            )
                                        };
                                        let target = if self.fn_index.contains_key(&mangled) {
                                            mangled
                                        } else {
                                            base
                                        };
                                        self.emit_call_by_name(&target, &[iter_reg], r_next_opt);
                                    }
                                    Some(TypeKind::Dyn { trait_name }) => {
                                        if let Some(slots) = self.trait_method_slots.get(trait_name)
                                        {
                                            let slot = slots.iter().position(|m| m == "next");
                                            let slot_idx = match slot {
                                                Some(slot) => self.qzi_u8(slot, "trait method slot"),
                                                None => {
                                                    if self.codegen_error.is_none() {
                                                        self.codegen_error = Some(format!(
                                                            "trait `{trait_name}` has no `next` slot"
                                                        ));
                                                    }
                                                    0
                                                }
                                            };
                                            let vtbl_ptr = self.alloc_reg();
                                            self.chunk.emit(rrr(
                                                Opcode::FieldLoad,
                                                vtbl_ptr,
                                                iter_reg,
                                                8,
                                            ));
                                            let fn_ptr = self.alloc_reg();
                                            self.chunk.emit(rrr(
                                                Opcode::VtblLoad,
                                                fn_ptr,
                                                vtbl_ptr,
                                                slot_idx,
                                            ));
                                            let data_ptr = self.alloc_reg();
                                            self.chunk.emit(rrr(
                                                Opcode::FieldLoad,
                                                data_ptr,
                                                iter_reg,
                                                0,
                                            ));
                                            self.chunk.emit(rrr(Opcode::CallArg, data_ptr, 0, 0));
                                            self.chunk.emit(rrr(
                                                Opcode::CallReg,
                                                r_next_opt,
                                                fn_ptr,
                                                0,
                                            ));
                                        }
                                    }
                                    _ => {}
                                }

                                let r_tag = self.alloc_reg();
                                self.chunk.emit(rrr(
                                    Opcode::FieldLoad,
                                    r_tag,
                                    r_next_opt,
                                    ENUM_DISCRIM_OFFSET,
                                ));
                                let r_one = self.alloc_reg();
                                self.chunk.emit(ri16(Opcode::MovI, r_one, 1));
                                self.chunk.emit(rrr(Opcode::Cmp, 0, r_tag, r_one));
                                let jump_none = self.chunk.emit(ri16(Opcode::Jne, 0, 0));
                                self.chunk.emit(field_load(
                                    r_val,
                                    r_next_opt,
                                    ENUM_PAYLOAD_OFFSET,
                                ));

                                self.loop_stack.push(LoopFrame::new());
                                self.compile_block(body);
                                let frame = self.loop_stack.pop().unwrap();
                                for jmp in &frame.continue_jumps {
                                    self.chunk.patch_jump(*jmp, loop_top);
                                }
                                self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));

                                let exit_pos = self.chunk.len() as u16;
                                self.chunk.patch_jump(jump_exit, exit_pos);
                                self.chunk.patch_jump(jump_none, exit_pos);
                                for jmp in &frame.break_jumps {
                                    self.chunk.patch_jump(*jmp, exit_pos);
                                }
                            }
                        }
                    },
                }
                false
            }
            StmtKind::UnsafeBlock { body } => {
                self.compile_block(body);
                false
            }
        }
    }

    // ── Condition helpers ─────────────────────────────────────────────────────

    /// Emit instructions for a boolean condition and a conditional jump.
    /// `jump_if_false = true` means jump when condition is false (used for if/while).
    /// Returns the index of the emitted jump instruction (caller must patch).
    fn compile_condition_jump(&mut self, expr: &Expr, jump_if_false: bool) -> usize {
        match &expr.node {
            ExprKind::Binary { left, op, right } if is_comparison(op) => {
                let r1 = self.compile_expr(left);
                let r2 = self.compile_expr(right);
                self.chunk.emit(rrr(Opcode::Cmp, 0, r1, r2));
                let jop = if jump_if_false {
                    negate_cmp(op)
                } else {
                    direct_cmp(op)
                };
                self.chunk.emit(ri16(jop, 0, 0))
            }
            ExprKind::Group(inner) => self.compile_condition_jump(inner, jump_if_false),
            _ => {
                let r = self.compile_expr(expr);
                let jop = if jump_if_false {
                    Opcode::Jz
                } else {
                    Opcode::Jnz
                };
                // ri16 layout: ops[0]=register, ops[1..2]=target (patched later)
                self.chunk.emit(ri16(jop, r, 0))
            }
        }
    }

    fn emit_call_by_name(&mut self, name: &str, arg_regs: &[u8], dst: u8) {
        if let Some(&idx) = self.fn_index.get(name) {
            for &r in arg_regs {
                self.chunk.emit(rrr(Opcode::CallArg, r, 0, 0));
            }
            self.chunk.emit(ri16(Opcode::CallIdx, dst, idx));
        } else {
            if self.codegen_error.is_none() {
                self.codegen_error = Some(format!(
                    "internal codegen error: function `{name}` is missing from the function table"
                ));
            }
        }
    }

    fn emit_c_variadic_call(&mut self, name: &str, args: &[Expr], dst: u8) -> bool {
        let Some(declaration) = self.foreign_imports.get(name).cloned() else {
            return false;
        };
        if !declaration.signature.variadic {
            return false;
        }

        let fixed_count = declaration.signature.params.len();
        let arg_regs: Vec<u8> = args
            .iter()
            .map(|arg| {
                self.mark_consumed_expr(arg);
                self.compile_expr(arg)
            })
            .collect();
        let mut params = declaration.signature.params.clone();
        for arg in args.iter().skip(fixed_count) {
            let Some(ty) = self
                .type_of_span((arg.span.start, arg.span.end))
                .and_then(|ty| {
                    abi_type_from_layout(
                        &ty,
                        self.struct_defs,
                        self.struct_sizes,
                        self.struct_field_offsets,
                        self.struct_alignments,
                        self.bit_field_layouts,
                        self.repr_c_structs,
                        self.type_aliases,
                    )
                })
            else {
                if self.codegen_error.is_none() {
                    self.codegen_error = Some(format!(
                        "unsupported C variadic argument type at {}:{}",
                        arg.span.line, arg.span.col
                    ));
                }
                return true;
            };
            params.push(match ty {
                // C default argument promotions.
                AbiType::Float32 => AbiType::Float64,
                AbiType::Integer { bytes: 1 | 2, .. } => AbiType::Integer {
                    bytes: 4,
                    signed: true,
                },
                other => other,
            });
        }
        let foreign = ForeignSymbol {
            symbol: declaration.symbol,
            signature: AbiSignature {
                params,
                return_type: declaration.signature.return_type,
                variadic: true,
            },
        };
        for reg in arg_regs {
            self.chunk.emit(rrr(Opcode::CallArg, reg, 0, 0));
        }
        let symbol_index = self
            .chunk
            .add_constant(ConstPoolEntry::ForeignSymbol(foreign));
        self.chunk.emit(ri16(Opcode::CallExt, dst, symbol_index));
        true
    }

    /// If `object` is a module alias (imported via `import std.X;`), returns its base name.
    fn module_import_base(&self, object: &Expr) -> Option<String> {
        let (base, _) = extract_field_chain(object)?;
        if self.regs.contains_key(&base) {
            return None;
        }
        if self.import_names.contains(&base) {
            Some(base)
        } else {
            None
        }
    }

    // ── Expression ───────────────────────────────────────────────────────────

    /// Uses PrimToStr with a type tag in ops[2]: 0=int, 1=float, 2=bool.
    /// For str/any types returns reg unchanged.
    fn coerce_to_display_str(&mut self, reg: u8, span: crate::parser::ast::Span) -> u8 {
        let key = (span.start, span.end);
        let type_tag: Option<u8> = match self.type_of_span(key) {
            Some(
                TypeKind::Int8
                | TypeKind::Int16
                | TypeKind::Int32
                | TypeKind::Int64
                | TypeKind::Uint8
                | TypeKind::Uint16
                | TypeKind::Uint32
                | TypeKind::Uint64
                | TypeKind::Isize
                | TypeKind::Usize,
            ) => Some(0), // int
            Some(TypeKind::Float32 | TypeKind::Float64) => Some(1), // float
            Some(TypeKind::Bool) => Some(2),                        // bool
            Some(TypeKind::Str) | Some(TypeKind::Ref { .. }) => None, // str/&str — already a pointer
            // Known struct/enum Named types: pass as-is (pointer to heap object).
            // Unresolved generic type params (T, U, etc.) not in struct/enum defs: default to int.
            Some(TypeKind::Named { name, .. }) => {
                if self.struct_defs.contains_key(name.as_str())
                    || self.enum_defs.contains_key(name.as_str())
                {
                    None
                } else {
                    Some(0)
                }
            }
            // RawPtr/Slice: pass as-is (caller formats explicitly)
            Some(TypeKind::RawPtr { .. } | TypeKind::Slice { .. }) => None,
            // Any (unresolved generic T from unwrap/ok/etc.) defaults to int to avoid
            // treating a raw integer register as a string pointer in the format engine.
            Some(TypeKind::Any) | None | Some(_) => Some(0),
        };
        if let Some(tag) = type_tag {
            let dst = self.alloc_reg();
            let instr = rrr(Opcode::PrimToStr, dst, reg, tag);
            self.chunk.emit(instr);
            dst
        } else {
            reg
        }
    }

    /// Coerce `reg` to str applying a format spec extracted from the template.
    /// Falls back to `coerce_to_display_str` for unknown or inapplicable specs.
    fn coerce_with_spec(&mut self, reg: u8, spec: &str, span: crate::parser::ast::Span) -> u8 {
        if spec.is_empty() {
            return self.coerce_to_display_str(reg, span);
        }
        let key = (span.start, span.end);
        let ty = self.type_of_span(key);
        let is_int = matches!(
            ty.as_ref(),
            Some(
                TypeKind::Int8
                    | TypeKind::Int16
                    | TypeKind::Int32
                    | TypeKind::Int64
                    | TypeKind::Uint8
                    | TypeKind::Uint16
                    | TypeKind::Uint32
                    | TypeKind::Uint64
                    | TypeKind::Isize
                    | TypeKind::Usize
            )
        );
        let is_float = matches!(ty.as_ref(), Some(TypeKind::Float32 | TypeKind::Float64));
        let tag: Option<u8> = if is_int {
            match spec {
                "x" | "#x" => Some(3),
                "X" | "#X" => Some(4),
                "o" | "#o" => Some(5),
                "b" | "#b" => Some(6),
                _ => None,
            }
        } else if is_float && spec.starts_with('.') {
            let prec = spec[1..].parse::<u8>().unwrap_or(6).min(9);
            Some(20 + prec)
        } else {
            None
        };
        if let Some(t) = tag {
            let dst = self.alloc_reg();
            self.chunk.emit(rrr(Opcode::PrimToStr, dst, reg, t));
            dst
        } else {
            self.coerce_to_display_str(reg, span)
        }
    }

    /// Box a concrete struct pointer into a 16-byte fat pointer for `dyn Trait`.
    /// Layout: word[0] = concrete ptr, word[1] = vtable ptr.
    fn coerce_to_dyn(&mut self, obj_reg: u8, type_name: &str, trait_name: &str) -> u8 {
        let fat = self.alloc_reg();
        self.chunk.emit(ri16(Opcode::New, fat, 16));
        // fat[0] = concrete ptr
        self.chunk.emit(rrr(Opcode::FieldStore, obj_reg, fat, 0));
        // fat[8] = vtable ptr (loaded via MovConst + VtableAddr)
        let vtbl_reg = self.alloc_reg();
        let idx = self.chunk.add_constant(ConstPoolEntry::VtableAddr(
            type_name.to_string(),
            trait_name.to_string(),
        ));
        self.chunk.emit(ri16(Opcode::MovConst, vtbl_reg, idx));
        self.chunk.emit(rrr(Opcode::FieldStore, vtbl_reg, fat, 8));
        fat
    }

    fn compile_expr(&mut self, expr: &Expr) -> u8 {
        let reg = self.compile_expr_inner(expr);
        self.emit_autoderef_load(expr.span, reg)
    }

    fn compile_expr_inner(&mut self, expr: &Expr) -> u8 {
        // Const-fold: if the semantic pass computed a known value for a non-trivial
        // expression, emit it directly instead of computing it at runtime.
        // Skip Ident (value already in a register) and Literal (emits directly below).
        let key = (expr.span.start, expr.span.end);
        if !matches!(expr.node, ExprKind::Ident(_) | ExprKind::Literal(_))
            && let Some(cv) = self.const_map.get(&key).cloned()
        {
            return self.emit_const_value(cv);
        }

        match &expr.node {
            ExprKind::Literal(lit) => self.emit_literal(lit),

            ExprKind::Ident(name) => {
                if let Some(global) = self.foreign_global_for_span(expr.span) {
                    let address = self.emit_foreign_global_address(&global);
                    let (width, signed, _, float32) = self.c_memory_access(&global.ty);
                    return self.emit_c_load(address, width, signed, float32);
                }
                // Use sema-resolved name when available (handles namespacing).
                let resolved_name = self
                    .resolved_fn_for_span(expr.span)
                    .unwrap_or_else(|| name.clone());
                // Zero-arg enum variant used as a value (e.g. `None`).
                // Only if not bound as a local variable.
                if !self.regs.contains_key(resolved_name.as_str()) {
                    if let Some(tag) = self.enum_ctor_tag(&resolved_name) {
                        let dst = self.alloc_reg();
                        let ptr = self.alloc_reg();
                        self.chunk
                            .emit(ri16(Opcode::New, ptr, enum_variant_alloc_size(0)));
                        let tag_reg = self.alloc_reg();
                        let encoded_tag = self.qzi_u16(tag, "enum variant tag");
                        self.chunk.emit(ri16(Opcode::MovI, tag_reg, encoded_tag));
                        self.chunk
                            .emit(rrr(Opcode::FieldStore, tag_reg, ptr, ENUM_DISCRIM_OFFSET));
                        self.chunk.emit(rrr(Opcode::Mov, dst, ptr, 0));
                        return dst;
                    }
                    // Function name used as a value → wrap in forwarder + env struct.
                    // All fn-ptr values use the env struct representation for uniform dispatch.
                    if self.is_c_abi_function_span(expr.span)
                        && let Some(address) = self.emit_c_callback_address(&resolved_name)
                    {
                        return address;
                    }
                    if let Some(&fn_idx) = self.fn_index.get(resolved_name.as_str()) {
                        let user_param_count =
                            if let Some(TypeKind::Fn { params, .. }) = self.type_map.get(&key) {
                                params.len()
                            } else {
                                0
                            };
                        // Forwarder chunk: (env_ptr_ignored, user_args...) → call named_fn(user_args...)
                        let fwd_name = format!("__quazi_fwd_{}", resolved_name);
                        let mut fwd_chunk = Chunk::with_params(&fwd_name, user_param_count + 1);
                        for i in 0..user_param_count {
                            fwd_chunk.emit(rrr(Opcode::CallArg, (i + 1) as u8, 0, 0));
                        }
                        fwd_chunk.emit(ri16(Opcode::CallIdx, 0, fn_idx));
                        fwd_chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
                        self.output_chunks.push(fwd_chunk);
                        // Env struct: {fn_ptr: forwarder_addr}
                        let env_ptr = self.alloc_reg();
                        self.chunk.emit(ri16(Opcode::New, env_ptr, 16));
                        let fn_addr_reg = self.alloc_reg();
                        let cidx = self.chunk.add_constant(ConstPoolEntry::FnAddr(fwd_name));
                        self.chunk.emit(ri16(Opcode::MovConst, fn_addr_reg, cidx));
                        self.chunk.emit(rrr(
                            Opcode::FieldStore,
                            fn_addr_reg,
                            env_ptr,
                            ENUM_DISCRIM_OFFSET,
                        ));
                        self.closure_env_regs.insert(env_ptr);
                        return env_ptr;
                    }
                }
                self.reg_of(name)
            }

            ExprKind::Group(inner) => self.compile_expr(inner),

            ExprKind::Cast { expr: inner, ty: _ } => {
                // QZI uses 64-bit slots for all values. Integer size changes and
                // float size changes are no-ops at this level; the encoder and ABI
                // handle the actual width. The typechecker has already validated.
                self.compile_expr(inner)
            }

            ExprKind::Unary { op, expr: inner } => match op {
                UnaryOpKind::Ref => {
                    let src = self.compile_expr(inner);
                    let dst = self.alloc_reg();
                    self.chunk.emit(mem_lea(src, dst, 0));
                    dst
                }
                UnaryOpKind::Deref => {
                    let ptr = self.compile_expr(inner);
                    let dst = self.alloc_reg();
                    let (width, signed) = self.raw_pointee_access(inner);
                    self.chunk.emit(mem_load_w(ptr, dst, 0, width, signed));
                    dst
                }
                UnaryOpKind::Neg => {
                    let src = self.compile_expr(inner);
                    let dst = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Neg, dst, src, 0));
                    dst
                }
                UnaryOpKind::Not => {
                    let src = self.compile_expr(inner);
                    let dst = self.alloc_reg();
                    // Logical not: dst = (src == 0) ? 1 : 0
                    self.chunk.emit(ri16(Opcode::MovI, dst, 1));
                    let jump_idx = self.chunk.emit(ri16(Opcode::Jz, src, 0));
                    self.chunk.emit(ri16(Opcode::MovI, dst, 0));
                    self.chunk.patch_jump(jump_idx, self.chunk.len() as u16);
                    dst
                }
            },

            // Short-circuit logical ops — lazy right evaluation.
            ExprKind::Binary {
                left,
                op: BinOpKind::AndAnd,
                right,
            } => {
                let r1 = self.compile_expr(left);
                let dst = self.alloc_reg();
                let false_idx = self.chunk.emit(ri16(Opcode::Jz, r1, 0));
                let r2 = self.compile_expr(right);
                self.chunk.emit(rrr(Opcode::Mov, dst, r2, 0));
                let end_idx = self.chunk.emit(ri16(Opcode::Jmp, 0, 0));
                let false_tgt = self.chunk.len() as u16;
                self.chunk.patch_jump(false_idx, false_tgt);
                self.chunk.emit(ri16(Opcode::MovI, dst, 0));
                self.chunk.patch_jump(end_idx, self.chunk.len() as u16);
                dst
            }
            ExprKind::Binary {
                left,
                op: BinOpKind::OrOr,
                right,
            } => {
                let r1 = self.compile_expr(left);
                let dst = self.alloc_reg();
                let true_idx = self.chunk.emit(ri16(Opcode::Jnz, r1, 0));
                let r2 = self.compile_expr(right);
                self.chunk.emit(rrr(Opcode::Mov, dst, r2, 0));
                let end_idx = self.chunk.emit(ri16(Opcode::Jmp, 0, 0));
                let true_tgt = self.chunk.len() as u16;
                self.chunk.patch_jump(true_idx, true_tgt);
                self.chunk.emit(ri16(Opcode::MovI, dst, 1));
                self.chunk.patch_jump(end_idx, self.chunk.len() as u16);
                dst
            }

            ExprKind::Binary { left, op, right } => {
                let left_key = (left.span.start, left.span.end);
                let is_float = self.is_float_span(left_key);
                let is_str = self.is_str_span(left_key);
                let r1 = self.compile_expr(left);
                let r2 = self.compile_expr(right);
                let dst = self.alloc_reg();
                let arith = if is_float { rrr_f } else { rrr };
                match op {
                    BinOpKind::Add if is_str => {
                        // str + str: call runtime concat intrinsic (ID 14).
                        // Arguments must be in consecutive registers starting at `dst`.
                        let arg2 = self.alloc_reg(); // dst + 1
                        self.chunk.emit(rrr(Opcode::Mov, dst, r1, 0));
                        self.chunk.emit(rrr(Opcode::Mov, arg2, r2, 0));
                        let mut instr = ri16(Opcode::Intrinsic, dst, 14);
                        instr.flags = 2; // 2 arguments
                        self.chunk.emit(instr);
                    }
                    BinOpKind::Add => {
                        self.chunk.emit(arith(Opcode::Add, dst, r1, r2));
                    }
                    BinOpKind::Sub => {
                        self.chunk.emit(arith(Opcode::Sub, dst, r1, r2));
                    }
                    BinOpKind::Mul => {
                        self.chunk.emit(arith(Opcode::Mul, dst, r1, r2));
                    }
                    BinOpKind::Div => {
                        self.chunk.emit(arith(Opcode::Div, dst, r1, r2));
                    }
                    BinOpKind::Mod => {
                        self.chunk.emit(arith(Opcode::Mod, dst, r1, r2));
                    }
                    // Comparisons: materialize bool result into dst.
                    _ if is_comparison(op) => {
                        self.chunk.emit(rrr(Opcode::Cmp, 0, r1, r2));
                        self.chunk.emit(ri16(Opcode::MovI, dst, 1));
                        let skip = self.chunk.emit(ri16(direct_cmp(op), 0, 0));
                        self.chunk.emit(ri16(Opcode::MovI, dst, 0));
                        self.chunk.patch_jump(skip, self.chunk.len() as u16);
                    }
                    BinOpKind::Pow => {
                        self.chunk.emit(rrr(Opcode::Pow, dst, r1, r2));
                    }
                    BinOpKind::BitAnd => {
                        self.chunk.emit(rrr(Opcode::And, dst, r1, r2));
                    }
                    BinOpKind::BitOr => {
                        self.chunk.emit(rrr(Opcode::Or, dst, r1, r2));
                    }
                    BinOpKind::BitXor => {
                        self.chunk.emit(rrr(Opcode::Xor, dst, r1, r2));
                    }
                    BinOpKind::Shl => {
                        self.chunk.emit(rrr(Opcode::Shl, dst, r1, r2));
                    }
                    BinOpKind::Shr => {
                        self.chunk.emit(rrr(Opcode::Shr, dst, r1, r2));
                    }
                    BinOpKind::AndAnd | BinOpKind::OrOr => unreachable!(),
                    _ => {
                        self.chunk.emit(rrr(Opcode::Add, dst, r1, r2));
                    } // fallback
                }
                dst
            }

            ExprKind::Assign { target, value } => {
                self.mark_consumed_expr(value);
                let src = self.compile_expr(value);
                match &target.node {
                    ExprKind::Ident(name) => {
                        if let Some(addr) = self.foreign_global_lvalue(target.span) {
                            self.store_lvalue(&addr, src);
                            return src;
                        }
                        self.drop_local_now(name);
                        let dst = self.reg_of(name);
                        if dst != src {
                            self.chunk.emit(rrr(Opcode::Mov, dst, src, 0));
                        }
                        let local_ty = self.type_of_span((value.span.start, value.span.end));
                        self.reactivate_drop_local(name, dst, local_ty);
                        dst
                    }
                    ExprKind::Unary {
                        op: UnaryOpKind::Deref,
                        expr: ptr_expr,
                    } => {
                        let ptr = self.compile_expr(ptr_expr);
                        let (width, _) = self.raw_pointee_access(ptr_expr);
                        self.chunk.emit(mem_store_w(ptr, src, 0, width));
                        src
                    }
                    ExprKind::Field {
                        object,
                        name: field_name,
                    } => {
                        let obj = self.compile_expr(object);
                        if let Some(layout) = self.bit_field_layout(object, field_name) {
                            self.emit_bit_field_store(obj, src, layout);
                        } else {
                            let byte_offset = self.field_offset(object, field_name);
                            let (width, _, float32) = self.ffi_field_access(object, field_name);
                            self.chunk.emit(field_store_typed(
                                src,
                                obj,
                                byte_offset,
                                width,
                                float32,
                            ));
                        }
                        src
                    }
                    ExprKind::Index { object, indices } => {
                        let obj_key = (object.span.start, object.span.end);
                        let obj_ty = self.type_of_span(obj_key);
                        let index = indices.first().expect("index must have at least one index");

                        // Evaluate object and index first; the value register may otherwise
                        // be overwritten before the store happens.
                        let obj_reg = self.compile_expr(object);
                        let idx_reg = self.compile_expr(index);
                        // Re-evaluate the value after object/index so it survives the store.
                        let src = self.compile_expr(value);

                        if let Some(TypeKind::FlexibleArray { elem_ty }) = &obj_ty {
                            let (width, _, elem_size, float32) =
                                self.c_memory_access(&elem_ty.node);
                            let address = self.emit_indexed_c_address(obj_reg, idx_reg, elem_size);
                            self.emit_c_store(address, src, width, float32);
                            return src;
                        }

                        // Named type with Index trait → dispatch to Type.set if available.
                        if matches!(&obj_ty, Some(TypeKind::Named { name, .. }) if name == "Array")
                        {
                            let type_kinds: Vec<TypeKind> =
                                if let Some(TypeKind::Named { type_args, .. }) = &obj_ty {
                                    type_args.iter().map(|t| t.node.clone()).collect()
                                } else {
                                    vec![]
                                };
                            let set_target = if type_kinds.is_empty() {
                                "Array.set".to_string()
                            } else {
                                let mangled = crate::semantic::typecheck::mangle_monomorphized(
                                    "Array.set",
                                    &type_kinds,
                                );
                                if self.fn_index.contains_key(&mangled) {
                                    mangled
                                } else {
                                    "Array.set".to_string()
                                }
                            };
                            let _dst = self.alloc_reg();
                            self.emit_call_by_name(&set_target, &[obj_reg, idx_reg, src], _dst);
                            src
                        } else if matches!(obj_ty, Some(TypeKind::Slice { .. })) {
                            // Slice: store at stack address ptr - (idx * 8)
                            let eight = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                            let offset = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::Mul, offset, idx_reg, eight));
                            let addr = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::Sub, addr, obj_reg, offset));
                            self.chunk.emit(mem_store(addr, src, 0));
                            src
                        } else {
                            // Fixed-size array: store at base register or computed address.
                            if let ExprKind::Literal(Literal::Int(n)) = &index.node
                                && *n >= 0
                            {
                                let elem_reg = obj_reg + *n as u8;
                                self.chunk.emit(rrr(Opcode::Mov, elem_reg, src, 0));
                                return src;
                            }
                            let ptr = self.alloc_reg();
                            self.chunk.emit(mem_lea(obj_reg, ptr, 0));
                            let eight = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                            let offset = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::Mul, offset, idx_reg, eight));
                            self.chunk.emit(rrr(Opcode::Sub, ptr, ptr, offset));
                            self.chunk.emit(mem_store(ptr, src, 0));
                            src
                        }
                    }
                    _ => src,
                }
            }
            ExprKind::StructInit { name, fields } => {
                // Get field order from struct_defs
                let field_order: Vec<String> = if let Some(defs) = self.struct_defs.get(name) {
                    defs.iter().map(|(fn_, _)| fn_.clone()).collect()
                } else {
                    fields.iter().map(|(fn_, _)| fn_.clone()).collect()
                };
                let struct_size = self
                    .struct_sizes
                    .get(name)
                    .copied()
                    .unwrap_or_else(|| field_order.len() * 8);
                let struct_size = u16::try_from(struct_size).unwrap_or_else(|_| {
                    self.codegen_error.get_or_insert_with(|| {
                        format!(
                            "struct `{name}` is larger than the QZI allocation limit of {} bytes",
                            u16::MAX
                        )
                    });
                    0
                });
                let dst = self.alloc_reg();
                self.chunk.emit(ri16(Opcode::New, dst, struct_size));

                // Compile and store each field in declaration order using computed offsets
                for field_name in &field_order {
                    if let Some((_, fval)) = fields.iter().find(|(fn_, _)| fn_ == field_name) {
                        let val = self.compile_expr(fval);
                        if let Some(layout) = self.bit_field_layout_by_name(name, field_name) {
                            self.emit_bit_field_store(dst, val, layout);
                        } else {
                            let off = self.field_offset_by_name(name, field_name);
                            let (width, _, float32) =
                                self.ffi_field_access_by_name(name, field_name);
                            self.chunk
                                .emit(field_store_typed(val, dst, off, width, float32));
                        }
                    }
                }
                dst
            }

            ExprKind::CompoundAssign { target, op, value } => {
                if let ExprKind::Ident(name) = &target.node {
                    if self.foreign_global_for_span(target.span).is_some() {
                        let addr = self.compute_lvalue_addr(target);
                        let old = self.load_lvalue(&addr);
                        let src = self.compile_expr(value);
                        let opcode = match op {
                            CompoundAssignOp::Add => Opcode::Add,
                            CompoundAssignOp::Sub => Opcode::Sub,
                            CompoundAssignOp::Mul => Opcode::Mul,
                            CompoundAssignOp::Div => Opcode::Div,
                            CompoundAssignOp::Mod => Opcode::Mod,
                        };
                        let new_val = self.alloc_reg();
                        self.chunk.emit(rrr(opcode, new_val, old, src));
                        self.store_lvalue(&addr, new_val);
                        return new_val;
                    }
                    let src = self.compile_expr(value);
                    let dst = self.reg_of(name);
                    let opcode = match op {
                        CompoundAssignOp::Add => Opcode::Add,
                        CompoundAssignOp::Sub => Opcode::Sub,
                        CompoundAssignOp::Mul => Opcode::Mul,
                        CompoundAssignOp::Div => Opcode::Div,
                        CompoundAssignOp::Mod => Opcode::Mod,
                    };
                    self.chunk.emit(rrr(opcode, dst, dst, src));
                    dst
                } else {
                    let addr = self.compute_lvalue_addr(target);
                    let old = self.load_lvalue(&addr);
                    let src = self.compile_expr(value);
                    let opcode = match op {
                        CompoundAssignOp::Add => Opcode::Add,
                        CompoundAssignOp::Sub => Opcode::Sub,
                        CompoundAssignOp::Mul => Opcode::Mul,
                        CompoundAssignOp::Div => Opcode::Div,
                        CompoundAssignOp::Mod => Opcode::Mod,
                    };
                    let new_val = self.alloc_reg();
                    self.chunk.emit(rrr(opcode, new_val, old, src));
                    self.store_lvalue(&addr, new_val);
                    new_val
                }
            }

            ExprKind::IncDec {
                expr: inner,
                op,
                prefix,
            } => {
                if let ExprKind::Ident(name) = &inner.node {
                    if self.foreign_global_for_span(inner.span).is_some() {
                        let addr = self.compute_lvalue_addr(inner);
                        let old = self.load_lvalue(&addr);
                        let opcode = match op {
                            IncDecOp::Inc => Opcode::Add,
                            IncDecOp::Dec => Opcode::Sub,
                        };
                        let one = self.alloc_reg();
                        self.chunk.emit(ri16(Opcode::MovI, one, 1));
                        let new_val = self.alloc_reg();
                        self.chunk.emit(rrr(opcode, new_val, old, one));
                        self.store_lvalue(&addr, new_val);
                        return if *prefix { new_val } else { old };
                    }
                    let r = self.reg_of(name);
                    let opcode = match op {
                        IncDecOp::Inc => Opcode::Inc,
                        IncDecOp::Dec => Opcode::Dec,
                    };
                    if *prefix {
                        // ++n / --n: modify in place and return the new value.
                        self.chunk.emit(rrr(opcode, r, r, 0));
                        r
                    } else {
                        // n++ / n--: return the old value, then modify.
                        let dst = self.alloc_reg();
                        self.chunk.emit(rrr(Opcode::Mov, dst, r, 0));
                        self.chunk.emit(rrr(opcode, r, r, 0));
                        dst
                    }
                } else {
                    let addr = self.compute_lvalue_addr(inner);
                    let old = self.load_lvalue(&addr);
                    let opcode = match op {
                        IncDecOp::Inc => Opcode::Add,
                        IncDecOp::Dec => Opcode::Sub,
                    };
                    let one = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::MovI, one, 1));
                    let new_val = self.alloc_reg();
                    self.chunk.emit(rrr(opcode, new_val, old, one));
                    self.store_lvalue(&addr, new_val);
                    if *prefix { new_val } else { old }
                }
            }

            ExprKind::Call {
                callee,
                type_args,
                args,
                named_args,
                ..
            } => {
                let dst = self.alloc_reg();
                if let ExprKind::Ident(name) = &callee.node {
                    // Use the sema-resolved name when available (handles namespacing).
                    let call_name = self
                        .resolved_fn_for_span(expr.span)
                        .unwrap_or_else(|| name.clone());
                    // Merge positional + named args into correct param order if needed.
                    let merged_owned: Vec<Expr>;
                    let args: &[Expr] = if named_args.is_empty() {
                        args
                    } else {
                        merged_owned = self.merge_named_args(&call_name, args, named_args);
                        &merged_owned
                    };
                    if self.emit_c_variadic_call(&call_name, args, dst) {
                        return dst;
                    }
                    // Panic calls: inject file/line constants from the call site span.
                    // Only use the builtin panic path when the resolved name is the bare
                    // builtin "panic"; a user-defined `fn panic` in a module keeps the
                    // resolved module-qualified name and falls through to normal dispatch.
                    if call_name.rsplit('.').next() == Some("panic") {
                        let mut call_regs: Vec<u8> = Vec::new();
                        // Compile msg arg (first user-provided fixed arg).
                        if !args.is_empty() {
                            call_regs.push(self.compile_expr(&args[0]));
                        } else {
                            let r = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, r, 0));
                            call_regs.push(r);
                        }
                        // Inject file path string constant.
                        let file_path = source_file_for_span(expr.span, self.source_files);
                        let file_idx = self.chunk.add_constant(ConstPoolEntry::Str(file_path));
                        let file_reg = self.alloc_reg();
                        self.chunk.emit(ri16(Opcode::MovConst, file_reg, file_idx));
                        call_regs.push(file_reg);
                        // Inject line number as integer constant (relative to source file).
                        let line = self
                            .source_files
                            .iter()
                            .find(|sf| sf.contains(expr.span))
                            .map(|sf| sf.line_col(expr.span).0 as i64)
                            .unwrap_or(expr.span.line as i64);
                        let line_idx = self.chunk.add_constant(ConstPoolEntry::Int(line));
                        let line_reg = self.alloc_reg();
                        self.chunk.emit(ri16(Opcode::MovConst, line_reg, line_idx));
                        call_regs.push(line_reg);
                        // Compile remaining user args as variadic.
                        let var_args = &args[1..];
                        let (r_ptr, r_len) = if var_args.is_empty() {
                            let rp = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, rp, 0));
                            let rl = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, rl, 0));
                            (rp, rl)
                        } else {
                            let var_regs: Vec<u8> = var_args
                                .iter()
                                .map(|a| {
                                    self.mark_consumed_expr(a);
                                    self.compile_expr(a)
                                })
                                .collect();
                            let first_slot = self.next_reg_slot();
                            for &r in &var_regs {
                                let slot = self.alloc_reg();
                                if r != slot {
                                    self.chunk.emit(rrr(Opcode::Mov, slot, r, 0));
                                }
                            }
                            let rp = self.alloc_reg();
                            self.chunk.emit(mem_lea(first_slot, rp, 0));
                            let rl = self.alloc_reg();
                            self.chunk
                                .emit(ri16(Opcode::MovI, rl, var_regs.len() as u16));
                            (rp, rl)
                        };
                        call_regs.push(r_ptr);
                        call_regs.push(r_len);
                        self.emit_call_by_name(&call_name, &call_regs, dst);
                        return dst;
                    }
                    // Monomorphized generic function: resolve to mangled name.
                    if !type_args.is_empty()
                        && let Some(mono_name) =
                            self.resolve_monomorphized_name(&call_name, type_args)
                    {
                        let arg_regs: Vec<u8> = args
                            .iter()
                            .map(|a| {
                                self.mark_consumed_expr(a);
                                self.compile_expr(a)
                            })
                            .collect();
                        self.emit_call_by_name(&mono_name, &arg_regs, dst);
                        return dst;
                    }
                    // str_variadic dispatch: auto-coerce args to str at call sites.
                    if self.str_variadic_fns.contains(call_name.as_str()) && !args.is_empty() {
                        if let Some(expanded) = crate::parser::format::expand_format_call_args(args)
                        {
                            let idx = self
                                .chunk
                                .add_constant(ConstPoolEntry::Str(expanded.clean_template));
                            let template_reg = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovConst, template_reg, idx));

                            let mut coerced_var: Vec<u8> = Vec::new();
                            for (i, arg) in expanded.args.iter().enumerate() {
                                let reg = self.compile_expr(arg);
                                let spec = expanded.specs.get(i).map(|s| s.as_str()).unwrap_or("");
                                coerced_var.push(self.coerce_with_spec(reg, spec, arg.span));
                            }

                            let fmt_dst = if !coerced_var.is_empty() {
                                let first_slot = self.next_reg_slot();
                                for &r in &coerced_var {
                                    let slot = self.alloc_reg();
                                    if r != slot {
                                        self.chunk.emit(rrr(Opcode::Mov, slot, r, 0));
                                    }
                                }
                                let rp = self.alloc_reg();
                                self.chunk.emit(mem_lea(first_slot, rp, 0));
                                let rl = self.alloc_reg();
                                self.chunk
                                    .emit(ri16(Opcode::MovI, rl, coerced_var.len() as u16));
                                let fd = self.alloc_reg();
                                self.emit_call_by_name("fmt.format", &[template_reg, rp, rl], fd);
                                fd
                            } else {
                                template_reg
                            };

                            let mut call_args = vec![fmt_dst];
                            if self.variadic_fn_info.contains_key(call_name.as_str()) {
                                let rp = self.alloc_reg();
                                self.chunk.emit(ri16(Opcode::MovI, rp, 0));
                                let rl = self.alloc_reg();
                                self.chunk.emit(ri16(Opcode::MovI, rl, 0));
                                call_args.push(rp);
                                call_args.push(rl);
                            }
                            self.emit_call_by_name(&call_name, &call_args, dst);
                            return dst;
                        }
                    }
                    if let Some(&fixed_count) = self.variadic_fn_info.get(call_name.as_str()) {
                        // Variadic call: compile fixed args, pack variadic args into
                        // consecutive stack slots, pass (ptr, len) as hidden trailing args.
                        let fixed_regs: Vec<u8> = args[..fixed_count.min(args.len())]
                            .iter()
                            .map(|a| {
                                self.mark_consumed_expr(a);
                                self.compile_expr(a)
                            })
                            .collect();
                        let var_args = &args[fixed_count.min(args.len())..];
                        let (r_ptr, r_len) = if var_args.is_empty() {
                            // Zero variadic args: pass null ptr + count 0.
                            let rp = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, rp, 0));
                            let rl = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, rl, 0));
                            (rp, rl)
                        } else {
                            // Compile each variadic arg (may land in non-consecutive regs).
                            let var_regs: Vec<u8> = var_args
                                .iter()
                                .map(|a| {
                                    self.mark_consumed_expr(a);
                                    self.compile_expr(a)
                                })
                                .collect();
                            // Copy into fresh consecutive slots so Lea gives a contiguous block.
                            let first_slot = self.next_reg_slot();
                            for (i, &r) in var_regs.iter().enumerate() {
                                let slot = self.alloc_reg(); // = first_slot + i
                                if r != slot {
                                    self.chunk.emit(rrr(Opcode::Mov, slot, r, 0));
                                }
                                let _ = i; // suppress unused warning
                            }
                            // Lea first_slot → pointer to its stack slot on this frame.
                            let rp = self.alloc_reg();
                            self.chunk.emit(mem_lea(first_slot, rp, 0));
                            let rl = self.alloc_reg();
                            self.chunk
                                .emit(ri16(Opcode::MovI, rl, var_regs.len() as u16));
                            (rp, rl)
                        };
                        let mut all_regs = fixed_regs;
                        all_regs.push(r_ptr);
                        all_regs.push(r_len);
                        self.emit_call_by_name(&call_name, &all_regs, dst);
                    } else if let Some(tag) = self.enum_ctor_tag(&call_name) {
                        // Enum variant constructor: allocate heap struct, store discriminant + payloads.
                        let payload_regs: Vec<u8> = args
                            .iter()
                            .map(|a| {
                                self.mark_consumed_expr(a);
                                self.compile_expr(a)
                            })
                            .collect();
                        let ptr = self.alloc_reg();
                        self.chunk.emit(ri16(
                            Opcode::New,
                            ptr,
                            enum_variant_alloc_size(payload_regs.len()),
                        ));
                        let tag_reg = self.alloc_reg();
                        let encoded_tag = self.qzi_u16(tag, "enum variant tag");
                        self.chunk.emit(ri16(Opcode::MovI, tag_reg, encoded_tag));
                        self.chunk
                            .emit(rrr(Opcode::FieldStore, tag_reg, ptr, ENUM_DISCRIM_OFFSET));
                        for (i, &payload) in payload_regs.iter().enumerate() {
                            let off = ENUM_PAYLOAD_OFFSET + (i as u16 * 8);
                            self.chunk.emit(field_store(payload, ptr, off));
                        }
                        if dst != ptr {
                            self.chunk.emit(rrr(Opcode::Mov, dst, ptr, 0));
                        }
                        return dst;
                    } else if self.regs.contains_key(call_name.as_str()) {
                        // Local variable — fn pointer or closure env pointer.
                        let callee_type = self.type_of_span((callee.span.start, callee.span.end));
                        let fn_reg = self.compile_expr(callee);
                        let arg_regs: Vec<u8> = args
                            .iter()
                            .map(|a| {
                                self.mark_consumed_expr(a);
                                self.compile_expr(a)
                            })
                            .collect();
                        if let Some(signature) = callee_type
                            .as_ref()
                            .and_then(|ty| self.c_callback_signature(ty))
                        {
                            self.emit_c_indirect_call(dst, fn_reg, &arg_regs, signature);
                        } else {
                            self.emit_indirect_call(dst, fn_reg, &arg_regs);
                        }
                    } else {
                        let arg_regs: Vec<u8> = args
                            .iter()
                            .map(|a| {
                                self.mark_consumed_expr(a);
                                self.compile_expr(a)
                            })
                            .collect();
                        self.emit_call_by_name(&call_name, &arg_regs, dst);
                    }
                } else {
                    // Indirect call: callee is an expression (variable, closure, etc.)
                    let callee_type = self.type_of_span((callee.span.start, callee.span.end));
                    let fn_reg = self.compile_expr(callee);
                    let arg_regs: Vec<u8> = args
                        .iter()
                        .map(|a| {
                            self.mark_consumed_expr(a);
                            self.compile_expr(a)
                        })
                        .collect();
                    if let Some(signature) = callee_type
                        .as_ref()
                        .and_then(|ty| self.c_callback_signature(ty))
                    {
                        self.emit_c_indirect_call(dst, fn_reg, &arg_regs, signature);
                    } else {
                        self.emit_indirect_call(dst, fn_reg, &arg_regs);
                    }
                }
                dst
            }

            ExprKind::MethodCall {
                object,
                method,
                type_args,
                args,
                named_args,
            } => {
                // Merge positional + named args if needed.
                let merged_owned: Vec<Expr>;
                let args: &[Expr] = if named_args.is_empty() {
                    args
                } else {
                    merged_owned = self.merge_named_args(method, args, named_args);
                    &merged_owned
                };
                if let Some(module_base) = self.module_import_base(object) {
                    let dst = self.alloc_reg();
                    // Use sema-resolved name when available; otherwise form the target from
                    // the module chain. The resolved name already accounts for namespacing.
                    let mut call_target = self
                        .resolved_fn_for_span(expr.span)
                        .unwrap_or_else(|| format!("{}.{}", module_base, method));

                    if self.emit_c_variadic_call(&call_target, args, dst) {
                        return dst;
                    }

                    // Monomorphized generic function: resolve to mangled name.
                    if !type_args.is_empty()
                        && let Some(mono_name) =
                            self.resolve_monomorphized_name(&call_target, type_args)
                    {
                        call_target = mono_name;
                    }
                    let is_fmt_fn = self.str_variadic_fns.contains(call_target.as_str());
                    let is_variadic_intrinsic =
                        self.variadic_intrinsic_fns.contains(call_target.as_str());
                    if (is_fmt_fn || is_variadic_intrinsic) && !args.is_empty() {
                        if let Some(expanded) = crate::parser::format::expand_format_call_args(args)
                        {
                            let idx = self
                                .chunk
                                .add_constant(ConstPoolEntry::Str(expanded.clean_template));
                            let template_reg = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovConst, template_reg, idx));

                            let mut coerced = vec![template_reg];
                            for (i, arg) in expanded.args.iter().enumerate() {
                                let reg = self.compile_expr(arg);
                                let spec = expanded.specs.get(i).map(|s| s.as_str()).unwrap_or("");
                                let cr = if spec.is_empty() {
                                    self.coerce_to_display_str(reg, arg.span)
                                } else {
                                    self.coerce_with_spec(reg, spec, arg.span)
                                };
                                coerced.push(cr);
                            }
                            if is_variadic_intrinsic {
                                self.emit_call_by_name(&call_target, &coerced, dst);
                            } else {
                                let fmt_dst = if coerced.len() > 1 {
                                    let coerced_args = &coerced[1..];
                                    let first_slot = self.next_reg_slot();
                                    for &r in coerced_args {
                                        let slot = self.alloc_reg();
                                        if r != slot {
                                            self.chunk.emit(rrr(Opcode::Mov, slot, r, 0));
                                        }
                                    }
                                    let rp = self.alloc_reg();
                                    self.chunk.emit(mem_lea(first_slot, rp, 0));
                                    let rl = self.alloc_reg();
                                    self.chunk.emit(ri16(
                                        Opcode::MovI,
                                        rl,
                                        coerced_args.len() as u16,
                                    ));
                                    let fd = self.alloc_reg();
                                    self.emit_call_by_name(
                                        "fmt.format",
                                        &[template_reg, rp, rl],
                                        fd,
                                    );
                                    fd
                                } else {
                                    template_reg
                                };
                                self.emit_call_by_name(&call_target, &[fmt_dst], dst);
                            }
                            return dst;
                        }
                    }
                    // Fallthrough: plain call (format expansion not applicable or no template match).
                    {
                        let arg_regs: Vec<u8> = args
                            .iter()
                            .map(|a| {
                                self.mark_consumed_expr(a);
                                self.compile_expr(a)
                            })
                            .collect();
                        self.emit_call_by_name(&call_target, &arg_regs, dst);
                    }
                    return dst;
                }

                // Enum namespace call: `Option.Some(42)`, `Result.Ok(v)`, `Shape.Circle(r)`, etc.
                // Object is an enum name used as a namespace, method is a variant constructor.
                if let ExprKind::Ident(type_name) = &object.node {
                    let is_enum_ns = self.enum_defs.contains_key(type_name.as_str())
                        && !self.regs.contains_key(type_name.as_str());
                    if is_enum_ns
                        && let Some(variants) = self.enum_defs.get(type_name.as_str())
                        && let Some(&tag) = variants.get(method.as_str())
                    {
                        let payload_regs: Vec<u8> = args
                            .iter()
                            .map(|a| {
                                self.mark_consumed_expr(a);
                                self.compile_expr(a)
                            })
                            .collect();
                        let ptr = self.alloc_reg();
                        self.chunk.emit(ri16(
                            Opcode::New,
                            ptr,
                            enum_variant_alloc_size(payload_regs.len()),
                        ));
                        let tag_reg = self.alloc_reg();
                        let encoded_tag = self.qzi_u16(tag, "enum variant tag");
                        self.chunk.emit(ri16(Opcode::MovI, tag_reg, encoded_tag));
                        self.chunk
                            .emit(rrr(Opcode::FieldStore, tag_reg, ptr, ENUM_DISCRIM_OFFSET));
                        for (i, &payload) in payload_regs.iter().enumerate() {
                            let off = ENUM_PAYLOAD_OFFSET + (i as u16 * 8);
                            self.chunk.emit(field_store(payload, ptr, off));
                        }
                        let dst = self.alloc_reg();
                        if dst != ptr {
                            self.chunk.emit(rrr(Opcode::Mov, dst, ptr, 0));
                        }
                        return dst;
                    }
                }

                // Static type-namespace call: `String.from(...)`, `Box.new(...)`, etc.
                // Object is a known struct name used as a namespace, not a value instance.
                // For generic structs, the typecheck pass annotates this call expression with
                // the concrete return type (e.g. Box[i32]), so we use that to pick the
                // monomorphized version (Box.new<i32>) when available.
                if let ExprKind::Ident(type_name) = &object.node {
                    let is_type_ns = self.struct_defs.contains_key(type_name.as_str())
                        && !self.regs.contains_key(type_name.as_str());
                    if is_type_ns {
                        let base_mangled = format!("{}.{}", type_name, method);
                        let expr_key = (expr.span.start, expr.span.end);
                        let type_kinds: Vec<TypeKind> = match self.type_map.get(&expr_key) {
                            Some(TypeKind::Named { type_args, .. }) => {
                                type_args.iter().map(|t| t.node.clone()).collect()
                            }
                            _ => vec![],
                        };
                        let call_target = if !type_kinds.is_empty() {
                            let mono = crate::semantic::typecheck::mangle_monomorphized(
                                &base_mangled,
                                &type_kinds,
                            );
                            if self.fn_index.contains_key(&mono) {
                                mono
                            } else {
                                base_mangled.clone()
                            }
                        } else {
                            base_mangled.clone()
                        };
                        if self.fn_index.contains_key(&call_target) {
                            let arg_regs: Vec<u8> = args
                                .iter()
                                .map(|a| {
                                    self.mark_consumed_expr(a);
                                    self.compile_expr(a)
                                })
                                .collect();
                            let dst = self.alloc_reg();
                            self.emit_call_by_name(&call_target, &arg_regs, dst);
                            return dst;
                        }
                    }
                }

                let obj = self.compile_expr(object);
                let key = (object.span.start, object.span.end);

                // Semantic analysis records the exact target for imported and
                // otherwise disambiguated inherent methods. Prefer that target
                // before reconstructing it from the receiver annotation: calls
                // chained through generic helpers such as `Result.unwrap()` can
                // lose enough surface type information for reconstruction.
                if let Some(call_target) = self.resolved_fn_for_span(expr.span)
                    && self.fn_index.contains_key(&call_target)
                {
                    let arg_regs: Vec<u8> = args
                        .iter()
                        .map(|arg| {
                            self.mark_consumed_expr(arg);
                            self.compile_expr(arg)
                        })
                        .collect();
                    let dst = self.alloc_reg();
                    let mut all_args = vec![obj];
                    all_args.extend_from_slice(&arg_regs);
                    self.emit_call_by_name(&call_target, &all_args, dst);
                    if method == "free"
                        && let ExprKind::Ident(name) = &object.node
                    {
                        self.deactivate_drop_local(name);
                    }
                    return dst;
                }

                // Resolve the receiver type through monomorphization substitution,
                // so generic param `T` resolves to the concrete type (e.g., Int32).
                let receiver_ty = self.type_of_expr(object);

                // Static dispatch: Named type with a known impl method takes priority
                // over built-in method dispatch so that user impls can override any name.
                if let Some(TypeKind::Named {
                    name: type_name,
                    type_args: receiver_type_args,
                }) = receiver_ty
                {
                    // @format instance methods: pre-format args, call method with single string.
                    {
                        let mangled_check = format!("{}.{}", type_name, method);
                        if self.str_variadic_fns.contains(&mangled_check) && !args.is_empty() {
                            if let Some(expanded) =
                                crate::parser::format::expand_format_call_args(args)
                            {
                                let idx = self
                                    .chunk
                                    .add_constant(ConstPoolEntry::Str(expanded.clean_template));
                                let template_reg = self.alloc_reg();
                                self.chunk.emit(ri16(Opcode::MovConst, template_reg, idx));

                                let fmt_reg = if !expanded.args.is_empty() {
                                    let mut coerced = vec![template_reg];
                                    for (i, arg) in expanded.args.iter().enumerate() {
                                        let r = self.compile_expr(arg);
                                        let spec =
                                            expanded.specs.get(i).map(|s| s.as_str()).unwrap_or("");
                                        let cr = if spec.is_empty() {
                                            self.coerce_to_display_str(r, arg.span)
                                        } else {
                                            self.coerce_with_spec(r, spec, arg.span)
                                        };
                                        coerced.push(cr);
                                    }
                                    let coerced_args = &coerced[1..];
                                    let first_slot = self.next_reg_slot();
                                    for &r in coerced_args {
                                        let slot = self.alloc_reg();
                                        if r != slot {
                                            self.chunk.emit(rrr(Opcode::Mov, slot, r, 0));
                                        }
                                    }
                                    let rp = self.alloc_reg();
                                    self.chunk.emit(mem_lea(first_slot, rp, 0));
                                    let rl = self.alloc_reg();
                                    self.chunk.emit(ri16(
                                        Opcode::MovI,
                                        rl,
                                        coerced_args.len() as u16,
                                    ));
                                    let fd = self.alloc_reg();
                                    self.emit_call_by_name(
                                        "fmt.format",
                                        &[template_reg, rp, rl],
                                        fd,
                                    );
                                    fd
                                } else {
                                    template_reg
                                };
                                let dst = self.alloc_reg();
                                self.emit_call_by_name(&mangled_check, &[obj, fmt_reg], dst);
                                return dst;
                            }
                        }
                    }

                    let base_mangled = format!("{}.{}", type_name, method);
                    let mangled = if receiver_type_args.is_empty() {
                        base_mangled.clone()
                    } else {
                        let type_kinds: Vec<TypeKind> =
                            receiver_type_args.iter().map(|t| t.node.clone()).collect();
                        crate::semantic::typecheck::mangle_monomorphized(&base_mangled, &type_kinds)
                    };
                    let lookup = if self.fn_index.contains_key(&mangled) {
                        Some(mangled.clone())
                    } else if self.fn_index.contains_key(&base_mangled) {
                        Some(base_mangled.clone())
                    } else {
                        None
                    };
                    if let Some(call_target) = lookup {
                        let arg_regs: Vec<u8> = args
                            .iter()
                            .map(|a| {
                                self.mark_consumed_expr(a);
                                self.compile_expr(a)
                            })
                            .collect();
                        let dst = self.alloc_reg();
                        let mut all_args = vec![obj];
                        all_args.extend_from_slice(&arg_regs);
                        self.emit_call_by_name(&call_target, &all_args, dst);
                        if method == "free"
                            && let ExprKind::Ident(name) = &object.node
                        {
                            self.deactivate_drop_local(name);
                        }
                        return dst;
                    }
                }

                // Dynamic dispatch: dyn Trait receiver → vtable lookup + CallReg.
                if let Some(TypeKind::Dyn { trait_name }) = self.type_of_span(key)
                    && let Some(slots) = self.trait_method_slots.get(&trait_name)
                {
                    let slot = slots.iter().position(|m: &String| m == method);
                    let slot_idx = match slot {
                        Some(slot) => self.qzi_u8(slot, "trait method slot"),
                        None => {
                            if self.codegen_error.is_none() {
                                self.codegen_error = Some(format!(
                                    "trait `{trait_name}` has no `{method}` slot"
                                ));
                            }
                            0
                        }
                    };
                    // Load vtable ptr from fat_ptr[8], then fn ptr from vtable[slot*8].
                    let vtbl_ptr = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::FieldLoad, vtbl_ptr, obj, 8));
                    let fn_ptr = self.alloc_reg();
                    self.chunk
                        .emit(rrr(Opcode::VtblLoad, fn_ptr, vtbl_ptr, slot_idx));
                    // Load concrete data ptr from fat_ptr[0].
                    let data_ptr = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::FieldLoad, data_ptr, obj, 0));
                    let dst = self.alloc_reg();
                    let mut all_args = vec![data_ptr];
                    for a in args {
                        self.mark_consumed_expr(a);
                        all_args.push(self.compile_expr(a));
                    }
                    for &r in &all_args {
                        self.chunk.emit(rrr(Opcode::CallArg, r, 0, 0));
                    }
                    self.chunk.emit(rrr(Opcode::CallReg, dst, fn_ptr, 0));
                    return dst;
                }

                // Built-in methods for primitive and known types.
                let resolved_receiver = self.type_of_expr(object);
                if let Some(pm) = resolve_primitive_method(method, args, resolved_receiver.as_ref())
                {
                    match pm {
                        PrimitiveMethod::Len => {
                            if matches!(self.type_map.get(&key), Some(TypeKind::Bytes)) {
                                let dst = self.alloc_reg();
                                self.chunk.emit(mem_load(obj, dst, 0));
                                return dst;
                            }
                            // Slice: .len() returns the hidden __len register.
                            if matches!(self.type_map.get(&key), Some(TypeKind::Slice { .. })) {
                                let len_reg = if let ExprKind::Ident(vname) = &object.node {
                                    self.regs
                                        .get(&format!("__len_{}", vname))
                                        .copied()
                                        .unwrap_or(obj + 1)
                                } else {
                                    obj + 1
                                };
                                let dst = self.alloc_reg();
                                self.chunk.emit(rrr(Opcode::Mov, dst, len_reg, 0));
                                return dst;
                            }
                            // str / &str
                            let dst = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::StrLen, dst, obj, 0));
                            return dst;
                        }
                        PrimitiveMethod::PrimToStr { tag } => {
                            let dst = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::PrimToStr, dst, obj, tag));
                            return dst;
                        }
                        PrimitiveMethod::StrToString => {
                            let dst = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::StrAsStr, dst, obj, 0));
                            return dst;
                        }
                        PrimitiveMethod::BoolToString => {
                            let dst = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::PrimToStr, dst, obj, 2));
                            return dst;
                        }
                        PrimitiveMethod::PrimToString { intrinsic_id } => {
                            let dst = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::Mov, dst, obj, 0));
                            let mut instr = ri16(Opcode::Intrinsic, dst, intrinsic_id);
                            instr.flags = 1;
                            self.chunk.emit(instr);
                            return dst;
                        }
                        PrimitiveMethod::AsStr => {
                            let dst = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::StrAsStr, dst, obj, 0));
                            return dst;
                        }
                        PrimitiveMethod::BytesAsPtr => {
                            let eight = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                            let dst = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::Add, dst, obj, eight));
                            return dst;
                        }
                        PrimitiveMethod::Parse { is_float } => {
                            let dst = self.alloc_reg();
                            let op = if is_float
                                || matches!(
                                    type_args.first().map(|t| &t.node),
                                    Some(TypeKind::Float32 | TypeKind::Float64)
                                ) {
                                Opcode::StrToFloat
                            } else {
                                Opcode::StrToInt
                            };
                            self.chunk.emit(rrr(op, dst, obj, 0));
                            return dst;
                        }
                    }
                }
                // General vtable dispatch (fallback for dynamic/polymorphic calls).
                // Determine method slot from trait_method_slots.
                let resolved_method_slot = self
                    .type_of_expr(object)
                    .and_then(|ty| {
                        let type_name = match &ty {
                            TypeKind::Named { name, .. } => Some(name.clone()),
                            _ => None,
                        }?;
                        // Find a trait implemented by this type that defines the method.
                        let traits = self.trait_impls.get(type_name.as_str())?;
                        for trait_name in traits {
                            if let Some(slots) = self.trait_method_slots.get(trait_name)
                                && let Some(idx) = slots.iter().position(|m| m == method)
                            {
                                return Some(idx);
                            }
                        }
                        None
                    });
                let method_slot = match resolved_method_slot {
                    Some(slot) => self.qzi_u8(slot, "trait method slot"),
                    None => {
                        if self.codegen_error.is_none() {
                            let receiver = self
                                .type_of_expr(object)
                                .map(|ty| format!("{ty:?}"))
                                .unwrap_or_else(|| "unknown receiver type".to_string());
                            self.codegen_error = Some(format!(
                                "no direct or trait method resolves `{method}` for {receiver}"
                            ));
                        }
                        0
                    }
                };
                let arg_regs: Vec<u8> = args
                    .iter()
                    .map(|a| {
                        self.mark_consumed_expr(a);
                        self.compile_expr(a)
                    })
                    .collect();
                for &r in &arg_regs {
                    self.chunk.emit(rrr(Opcode::CallArg, r, 0, 0));
                }
                let vtbl = self.alloc_reg();
                let dst = self.alloc_reg();
                self.chunk
                    .emit(rrr(Opcode::VtblLoad, vtbl, obj, method_slot));
                self.chunk.emit(rrr(Opcode::CallReg, dst, vtbl, 0));
                dst
            }

            ExprKind::Field { object, name } => {
                // Module namespace field used as a function value: `bar.foo`.
                // Sema already resolved the target name; emit a fn-pointer value.
                if let Some(resolved) = self.resolved_fn_for_span(expr.span) {
                    if self.is_c_abi_function_span(expr.span)
                        && let Some(address) = self.emit_c_callback_address(&resolved)
                    {
                        return address;
                    }
                    let key = (expr.span.start, expr.span.end);
                    if let Some(&fn_idx) = self.fn_index.get(resolved.as_str()) {
                        let user_param_count =
                            if let Some(TypeKind::Fn { params, .. }) = self.type_map.get(&key) {
                                params.len()
                            } else {
                                0
                            };
                        let fwd_name = format!("__quazi_fwd_{}", resolved);
                        let mut fwd_chunk = Chunk::with_params(&fwd_name, user_param_count + 1);
                        for i in 0..user_param_count {
                            fwd_chunk.emit(rrr(Opcode::CallArg, (i + 1) as u8, 0, 0));
                        }
                        fwd_chunk.emit(ri16(Opcode::CallIdx, 0, fn_idx));
                        fwd_chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
                        self.output_chunks.push(fwd_chunk);
                        let env_ptr = self.alloc_reg();
                        self.chunk.emit(ri16(Opcode::New, env_ptr, 16));
                        let fn_addr_reg = self.alloc_reg();
                        let cidx = self.chunk.add_constant(ConstPoolEntry::FnAddr(fwd_name));
                        self.chunk.emit(ri16(Opcode::MovConst, fn_addr_reg, cidx));
                        self.chunk.emit(rrr(
                            Opcode::FieldStore,
                            fn_addr_reg,
                            env_ptr,
                            ENUM_DISCRIM_OFFSET,
                        ));
                        self.closure_env_regs.insert(env_ptr);
                        return env_ptr;
                    }
                }

                // Enum namespace: `Option.None`, `Result.Err(...)` zero-arg variants.
                if let ExprKind::Ident(type_name) = &object.node {
                    let is_enum_ns = self.enum_defs.contains_key(type_name.as_str())
                        && !self.regs.contains_key(type_name.as_str());
                    if is_enum_ns
                        && let Some(variants) = self.enum_defs.get(type_name.as_str())
                        && let Some(&tag) = variants.get(name.as_str())
                    {
                        let ptr = self.alloc_reg();
                        self.chunk
                            .emit(ri16(Opcode::New, ptr, enum_variant_alloc_size(0)));
                        let tag_reg = self.alloc_reg();
                        let encoded_tag = self.qzi_u16(tag, "enum variant tag");
                        self.chunk.emit(ri16(Opcode::MovI, tag_reg, encoded_tag));
                        self.chunk
                            .emit(rrr(Opcode::FieldStore, tag_reg, ptr, ENUM_DISCRIM_OFFSET));
                        let dst = self.alloc_reg();
                        if dst != ptr {
                            self.chunk.emit(rrr(Opcode::Mov, dst, ptr, 0));
                        }
                        return dst;
                    }
                }
                if matches!(
                    self.type_of_span((expr.span.start, expr.span.end)),
                    Some(TypeKind::FlexibleArray { .. })
                ) {
                    let base = self.compile_expr(object);
                    let byte_offset = self.field_offset(object, name);
                    if byte_offset == 0 {
                        return base;
                    }
                    let offset = self.emit_u64_constant(byte_offset as u64);
                    let address = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Add, address, base, offset));
                    return address;
                }
                let obj = self.compile_expr(object);
                if let Some(layout) = self.bit_field_layout(object, name) {
                    return self.emit_bit_field_load(obj, layout);
                }
                let byte_offset = self.field_offset(object, name);
                let (width, signed, float32) = self.ffi_field_access(object, name);
                let dst = self.alloc_reg();
                self.chunk.emit(field_load_typed(
                    dst,
                    obj,
                    byte_offset,
                    width,
                    signed,
                    float32,
                ));
                dst
            }

            ExprKind::ArrayLit(elems) => {
                let base = self.reserve_reg_block(elems.len());
                for (i, elem) in elems.iter().enumerate() {
                    let val = self.compile_expr(elem);
                    let dst = u8::try_from(i).ok().and_then(|i| base.checked_add(i)).unwrap_or_else(|| {
                        self.codegen_error.get_or_insert_with(|| {
                            format!(
                                "array literal in `{}` exceeds the 256-register QZI limit",
                                self.chunk.name
                            )
                        });
                        0
                    });
                    if val != dst {
                        self.chunk.emit(rrr(Opcode::Mov, dst, val, 0));
                    }
                }
                base
            }

            ExprKind::Index { object, indices } => {
                // Named type that implements the Index trait → dispatch to Type.index.
                // Checks trait_impls registry — no accidental dispatch from any "index" method.
                let key = (object.span.start, object.span.end);
                if let Some(TypeKind::Named {
                    name: type_name, ..
                }) = self.type_of_span(key)
                {
                    let implements_index = self
                        .trait_impls
                        .get(type_name.as_str())
                        .map(|ts| ts.contains("Index"))
                        .unwrap_or(false);
                    if implements_index {
                        let mangled = format!("{}.index", type_name);
                        if self.fn_index.contains_key(&mangled) {
                            let obj = self.compile_expr(object);
                            let idx_regs: Vec<u8> =
                                indices.iter().map(|i| self.compile_expr(i)).collect();
                            let dst = self.alloc_reg();
                            let mut all_args = vec![obj];
                            all_args.extend_from_slice(&idx_regs);
                            self.emit_call_by_name(&mangled, &all_args, dst);
                            return dst;
                        }
                    }
                }
                let index = indices
                    .first()
                    .expect("index expr must have at least one index");
                let obj_key = (object.span.start, object.span.end);
                if matches!(self.type_of_span(obj_key), Some(TypeKind::Bytes)) {
                    let bytes = self.compile_expr(object);
                    let idx = self.compile_expr(index);
                    let eight = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                    let data = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Add, data, bytes, eight));
                    let addr = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Add, addr, data, idx));
                    let dst = self.alloc_reg();
                    self.chunk
                        .emit(mem_load_w(addr, dst, 0, MemWidth::Byte, false));
                    return dst;
                }
                if let Some(TypeKind::FlexibleArray { elem_ty }) = self.type_of_span(obj_key) {
                    let base = self.compile_expr(object);
                    let idx = self.compile_expr(index);
                    let (width, signed, elem_size, float32) = self.c_memory_access(&elem_ty.node);
                    let address = self.emit_indexed_c_address(base, idx, elem_size);
                    return self.emit_c_load(address, width, signed, float32);
                }
                // Slice (variadic param): ptr register holds caller's stack address.
                if matches!(self.type_of_span(obj_key), Some(TypeKind::Slice { .. })) {
                    let ptr = self.compile_expr(object);
                    let idx = self.compile_expr(index);
                    let eight = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                    let offset = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Mul, offset, idx, eight));
                    let addr = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Sub, addr, ptr, offset));
                    let dst = self.alloc_reg();
                    self.chunk.emit(mem_load(addr, dst, 0));
                    return dst;
                }
                // Fallback: raw static-array register arithmetic (single index only).
                let base = self.compile_expr(object);
                if let ExprKind::Literal(Literal::Int(n)) = &index.node
                    && *n >= 0
                {
                    return base + *n as u8;
                }
                // Dynamic index: Lea + scale + Sub + Load
                let idx = self.compile_expr(index);
                let ptr = self.alloc_reg();
                self.chunk.emit(mem_lea(base, ptr, 0));
                let eight = self.alloc_reg();
                self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                let offset = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::Mul, offset, idx, eight));
                self.chunk.emit(rrr(Opcode::Sub, ptr, ptr, offset));
                let dst = self.alloc_reg();
                self.chunk.emit(mem_load(ptr, dst, 0));
                dst
            }

            ExprKind::Match { scrutinee, arms } => {
                let scr = self.compile_expr(scrutinee);
                let dst = self.alloc_reg();
                let mut end_jumps: Vec<usize> = Vec::new();
                let mut guard_fail_jumps: Vec<usize> = Vec::new();
                #[allow(unused_assignments)]
                let mut _last_arm_is_full_default = false;

                for arm in arms {
                    // Patch guard-fail jumps from the previous arm to this arm's start.
                    for j in &guard_fail_jumps {
                        self.chunk.patch_jump(*j, self.chunk.len() as u16);
                    }
                    guard_fail_jumps.clear();

                    // Compile pattern: collect jumps that skip to the next arm on mismatch.
                    let mut skip_patches: Vec<usize> = Vec::new();
                    self.compile_pattern_match(&arm.pattern, scr, &mut skip_patches);

                    // Optional guard: failed guard → try next arm.
                    if let Some(guard) = &arm.guard {
                        let guard_val = self.compile_expr(guard);
                        self.chunk.emit(rrr(Opcode::Cmp, 0, guard_val, 0));
                        guard_fail_jumps.push(self.chunk.emit(ri16(Opcode::Je, 0, 0)));
                    }

                    // Arm body.
                    let val = self.compile_expr(&arm.expr);
                    if val != dst {
                        self.chunk.emit(rrr(Opcode::Mov, dst, val, 0));
                    }

                    if arm.guard.is_none() {
                        _last_arm_is_full_default = matches!(
                            arm.pattern.node,
                            PatternKind::Wildcard | PatternKind::Bind(_) | PatternKind::Literal(_)
                        );
                    }
                    end_jumps.push(self.chunk.emit(ri16(Opcode::Jmp, 0, 0)));

                    // Patch all skip_patches to the start of the next arm.
                    let next_arm_start = self.chunk.len() as u16;
                    for p in skip_patches {
                        self.chunk.patch_jump(p, next_arm_start);
                    }
                }

                let end = self.chunk.len() as u16;
                for j in guard_fail_jumps {
                    self.chunk.patch_jump(j, end);
                }
                for j in end_jumps {
                    self.chunk.patch_jump(j, end);
                }
                dst
            }

            ExprKind::Try { expr: inner } => {
                let scr = self.compile_expr(inner);
                let key = (inner.span.start, inner.span.end);
                let enum_name = match self.type_of_span(key) {
                    Some(TypeKind::Named { name, .. }) if name == "Option" => "Option",
                    _ => "Result",
                };
                let (success_variant, failure_variant) = match enum_name {
                    "Option" => ("Some", "None"),
                    _ => ("Ok", "Err"),
                };
                let success_tag = *self
                    .enum_defs
                    .get(enum_name)
                    .and_then(|v| v.get(success_variant))
                    .expect("? operator: success variant not found in enum_defs");
                let failure_tag = *self
                    .enum_defs
                    .get(enum_name)
                    .and_then(|v| v.get(failure_variant))
                    .expect("? operator: failure variant not found in enum_defs");

                // Load discriminant and compare against success tag.
                let disc = self.alloc_reg();
                self.chunk
                    .emit(rrr(Opcode::FieldLoad, disc, scr, ENUM_DISCRIM_OFFSET));
                let tag_ok = self.alloc_reg();
                let encoded_success = self.qzi_u16(success_tag, "result success tag");
                self.chunk
                    .emit(ri16(Opcode::MovI, tag_ok, encoded_success));
                self.chunk.emit(rrr(Opcode::Cmp, 0, disc, tag_ok));
                let jne = self.chunk.emit(ri16(Opcode::Jne, 0, 0));

                // Success path: extract first payload.
                let val = self.alloc_reg();
                self.chunk
                    .emit(field_load(val, scr, ENUM_PAYLOAD_OFFSET));
                let jmp_end = self.chunk.emit(ri16(Opcode::Jmp, 0, 0));

                // Failure path: build failure variant and early-return.
                self.chunk.patch_jump(jne, self.chunk.len() as u16);
                if enum_name == "Option" {
                    // Return None — zero-arg variant.
                    let none_ptr = self.alloc_reg();
                    self.chunk
                        .emit(ri16(Opcode::New, none_ptr, enum_variant_alloc_size(0)));
                    let tag_fail = self.alloc_reg();
                    let encoded_failure = self.qzi_u16(failure_tag, "result failure tag");
                    self.chunk
                        .emit(ri16(Opcode::MovI, tag_fail, encoded_failure));
                    self.chunk.emit(rrr(
                        Opcode::FieldStore,
                        tag_fail,
                        none_ptr,
                        ENUM_DISCRIM_OFFSET,
                    ));
                    if none_ptr != 0 {
                        self.chunk.emit(rrr(Opcode::Mov, 0, none_ptr, 0));
                    }
                } else {
                    // Return Err(payload) — copy scrutinee payload into new Err object.
                    let err_payload = self.alloc_reg();
                    self.chunk
                        .emit(field_load(err_payload, scr, ENUM_PAYLOAD_OFFSET));
                    let err_ptr = self.alloc_reg();
                    self.chunk
                        .emit(ri16(Opcode::New, err_ptr, enum_variant_alloc_size(1)));
                    let tag_fail = self.alloc_reg();
                    let encoded_failure = self.qzi_u16(failure_tag, "result failure tag");
                    self.chunk
                        .emit(ri16(Opcode::MovI, tag_fail, encoded_failure));
                    self.chunk.emit(rrr(
                        Opcode::FieldStore,
                        tag_fail,
                        err_ptr,
                        ENUM_DISCRIM_OFFSET,
                    ));
                    self.chunk
                        .emit(field_store(err_payload, err_ptr, ENUM_PAYLOAD_OFFSET));
                    if err_ptr != 0 {
                        self.chunk.emit(rrr(Opcode::Mov, 0, err_ptr, 0));
                    }
                }
                self.chunk.emit(rrr(Opcode::Ret, 0, 0, 0));

                self.chunk.patch_jump(jmp_end, self.chunk.len() as u16);
                val
            }

            ExprKind::Closure { params, body } => {
                let val = *self.next_closure_idx;
                *self.next_closure_idx = val.wrapping_add(1);
                let anon_name = format!("__quazi_closure_{}", val);

                let captures = self.capture_ident_names(body, params);

                // Use a temporary output buffer for the sub-compiler
                // so we avoid borrowing conflicts with self.output_chunks.
                let mut temp_chunks = Vec::new();
                let mut temp_idx = 0u16;
                // All closures take hidden env_ptr as r0 for uniform dispatch.
                let anon_param_count = params.len() + 1;
                let mut anon = FnCompiler::new(
                    &anon_name,
                    anon_param_count,
                    self.fn_index,
                    self.const_map,
                    self.type_map,
                    self.autoderef_map,
                    self.import_names,
                    self.struct_defs,
                    self.struct_sizes,
                    self.struct_field_offsets,
                    self.struct_alignments,
                    self.bit_field_layouts,
                    self.repr_c_structs,
                    self.type_aliases,
                    self.foreign_imports,
                    self.foreign_globals,
                    self.trait_impls,
                    self.variadic_fn_info,
                    self.enum_defs,
                    self.str_variadic_fns,
                    self.variadic_intrinsic_fns,
                    self.monomorphizations,
                    self.trait_method_slots,
                    &mut temp_chunks,
                    &mut temp_idx,
                    self.type_subst.clone(),
                    self.fn_param_names,
                    self.source_files,
                    self.annotated_exprs,
                );
                if captures.is_empty() {
                    // No-capture: r0 = env_ptr (ignored), user params at r1+.
                    let _ = anon.alloc_reg(); // consume r0 = env_ptr slot
                    for p in params {
                        anon.bind(p.clone()); // r1, r2, ...
                    }
                } else {
                    // With captures: r0 = env ptr, user params start at r1.
                    // Load each captured variable from the env struct.
                    for (i, cap_name) in captures.iter().enumerate() {
                        let cap_reg = anon.alloc_reg();
                        let off = ENUM_PAYLOAD_OFFSET + (i as u16 * 8);
                        anon.chunk.emit(field_load(cap_reg, 0, off));
                        anon.regs.insert(cap_name.clone(), cap_reg);
                    }
                    // Bind user params starting at r1.
                    for (i, p) in params.iter().enumerate() {
                        anon.regs.insert(p.clone(), (i + 1) as u8);
                    }
                }
                let body_reg = anon.compile_expr(body);
                if body_reg != 0 {
                    anon.chunk.emit(rrr(Opcode::Mov, 0, body_reg, 0));
                }
                anon.chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
                if let Some(error) = anon.codegen_error.take() {
                    self.codegen_error.get_or_insert(error);
                }
                anon.chunk.reg_count = anon.next_reg.min(u8::MAX as u16) as u8;

                // Push the closure chunk and any nested closures into
                // the parent's output queue.
                self.output_chunks.push(anon.chunk);
                self.output_chunks.extend(temp_chunks);

                if captures.is_empty() {
                    // No-capture: wrap in env struct {fn_ptr} for uniform dispatch.
                    let env_ptr = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::New, env_ptr, 16));
                    let fn_addr_reg = self.alloc_reg();
                    let cidx = self.chunk.add_constant(ConstPoolEntry::FnAddr(anon_name));
                    self.chunk.emit(ri16(Opcode::MovConst, fn_addr_reg, cidx));
                    self.chunk.emit(rrr(
                        Opcode::FieldStore,
                        fn_addr_reg,
                        env_ptr,
                        ENUM_DISCRIM_OFFSET,
                    ));
                    self.closure_env_regs.insert(env_ptr);
                    env_ptr
                } else {
                    // With captures: allocate an environment struct, store fn ptr + captures.
                    let env_ptr = self.alloc_reg();
                    let env_size = ((captures.len() + 1) * 8).max(16) as u16;
                    self.chunk.emit(ri16(Opcode::New, env_ptr, env_size));

                    let fn_addr_reg = self.alloc_reg();
                    let cidx = self.chunk.add_constant(ConstPoolEntry::FnAddr(anon_name));
                    self.chunk.emit(ri16(Opcode::MovConst, fn_addr_reg, cidx));
                    self.chunk.emit(rrr(
                        Opcode::FieldStore,
                        fn_addr_reg,
                        env_ptr,
                        ENUM_DISCRIM_OFFSET,
                    ));

                    for (i, cap_name) in captures.iter().enumerate() {
                        let cap_reg = self.reg_of(cap_name);
                        let off = ENUM_PAYLOAD_OFFSET + (i as u16 * 8);
                        self.chunk.emit(field_store(cap_reg, env_ptr, off));
                    }

                    self.closure_env_regs.insert(env_ptr);
                    env_ptr
                }
            }
        }
    }

    // ── Literal / const-value emitters ────────────────────────────────────────

    fn emit_literal(&mut self, lit: &Literal) -> u8 {
        let dst = self.alloc_reg();
        match lit {
            Literal::Int(n) if *n >= 0 && *n <= 0xFFFF => {
                self.chunk.emit(ri16(Opcode::MovI, dst, *n as u16));
            }
            Literal::Int(n) => {
                let idx = self.chunk.add_constant(ConstPoolEntry::Int(*n));
                self.chunk.emit(ri16(Opcode::MovConst, dst, idx));
            }
            Literal::Float(f) => {
                let idx = self.chunk.add_constant(ConstPoolEntry::Float(*f));
                self.chunk.emit(ri16(Opcode::MovConst, dst, idx));
            }
            Literal::String(s) => {
                let idx = self.chunk.add_constant(ConstPoolEntry::Str(s.clone()));
                self.chunk.emit(ri16(Opcode::MovConst, dst, idx));
            }
            Literal::Bytes(bytes) => {
                let idx = self
                    .chunk
                    .add_constant(ConstPoolEntry::Bytes(bytes.clone()));
                self.chunk.emit(ri16(Opcode::MovConst, dst, idx));
            }
            Literal::Bool(b) => {
                self.chunk.emit(ri16(Opcode::MovI, dst, *b as u16));
            }
        }
        dst
    }

    fn emit_const_value(&mut self, cv: ConstValue) -> u8 {
        let dst = self.alloc_reg();
        match cv {
            ConstValue::Int(n) if (0..=0xFFFF).contains(&n) => {
                self.chunk.emit(ri16(Opcode::MovI, dst, n as u16));
            }
            ConstValue::Int(n) => {
                let idx = self.chunk.add_constant(ConstPoolEntry::Int(n));
                self.chunk.emit(ri16(Opcode::MovConst, dst, idx));
            }
            ConstValue::Float(f) => {
                let idx = self.chunk.add_constant(ConstPoolEntry::Float(f));
                self.chunk.emit(ri16(Opcode::MovConst, dst, idx));
            }
            ConstValue::String(s) => {
                let idx = self.chunk.add_constant(ConstPoolEntry::Str(s));
                self.chunk.emit(ri16(Opcode::MovConst, dst, idx));
            }
            ConstValue::Bool(b) => {
                self.chunk.emit(ri16(Opcode::MovI, dst, b as u16));
            }
        }
        dst
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub(crate) fn remap_instr_regs(
    instr: &mut crate::bytecode::instruction::Instruction,
    remap: impl Fn(u8) -> u8,
) {
    use crate::bytecode::opcode::Opcode;
    let Some(op) = Opcode::from_u8(instr.opcode) else {
        return;
    };
    match op {
        // No-op / no regs
        Opcode::Nop
        | Opcode::MemFence
        | Opcode::Jmp
        | Opcode::Je
        | Opcode::Jne
        | Opcode::Jg
        | Opcode::Jge
        | Opcode::Jl
        | Opcode::Jle
        | Opcode::Ja
        | Opcode::Jb => {}

        // Ret reads its return-value register from ops[0].
        Opcode::Ret => {
            instr.ops[0] = remap(instr.ops[0]);
        }

        // RI16 — ops[0]=dst only; ops[1..2] are an immediate (id/index), not registers
        Opcode::MovI
        | Opcode::MovConst
        | Opcode::CallIdx
        | Opcode::CallExt
        | Opcode::Syscall
        | Opcode::Intrinsic
        | Opcode::New
        | Opcode::NewObj => {
            instr.ops[0] = remap(instr.ops[0]);
        }

        // Jz/Jnz: ops[0] may be a register (or 0 for flag-only)
        Opcode::Jz | Opcode::Jnz => {
            if instr.ops[0] != 0 {
                instr.ops[0] = remap(instr.ops[0]);
            }
        }

        // CallArg / Drop — single reg
        Opcode::CallArg | Opcode::Drop => {
            instr.ops[0] = remap(instr.ops[0]);
        }

        Opcode::CallCReg => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
        }

        // MEM — ops[0]=val/dst, ops[1]=base
        Opcode::Load | Opcode::Store | Opcode::Lea => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
        }

        // FieldLoad/FieldStore — ops[2] is a byte offset, NOT a register.
        Opcode::FieldLoad | Opcode::FieldStore => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
            // ops[2] = byte_off — leave unchanged
        }

        // VtblLoad — ops[2] is a method slot index, NOT a register.
        Opcode::VtblLoad => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
            // ops[2] = method_slot — leave unchanged
        }

        // PrimToStr — ops[2] is a type tag, NOT a register.
        Opcode::PrimToStr => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
            // ops[2] = type tag — leave unchanged
        }

        // StrLen, StrToInt, StrToFloat, StrAsStr, StrConcat — ops[2] unused or not a reg.
        Opcode::StrLen | Opcode::StrToInt | Opcode::StrToFloat | Opcode::StrAsStr => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
            // ops[2] unused — leave unchanged
        }

        // Pow — RRR
        Opcode::Pow => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
            instr.ops[2] = remap(instr.ops[2]);
        }

        // RRR and all others — remap ops[0], ops[1], ops[2]
        _ => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
            instr.ops[2] = remap(instr.ops[2]);
        }
    }
}

/// Describes a resolved primitive method operation for codegen emission.
enum PrimitiveMethod {
    /// .len() on &str or slice — emit StrLen or copy hidden __len register
    Len,
    /// .to_str() on primitives — emit PrimToStr with the given type tag
    PrimToStr { tag: u8 },
    /// .to_string() on &str — identity view via StrAsStr
    StrToString,
    /// .to_string() on bool — PrimToStr with tag=2
    BoolToString,
    /// .to_string() on int/float — Intrinsic with given id
    PrimToString { intrinsic_id: u16 },
    /// .as_str() / .as_string() — StrAsStr identity view
    AsStr,
    /// .as_ptr() on bytes â€” skip the read-only length prefix.
    BytesAsPtr,
    /// .parse[T]() — true=float, false=int
    Parse { is_float: bool },
}

/// Maps receiver types to their `PrimToStr` tag value.
fn prim_to_str_tag(receiver_type: Option<&TypeKind>) -> u8 {
    match receiver_type {
        Some(TypeKind::Float32 | TypeKind::Float64) => 1,
        Some(TypeKind::Bool) => 2,
        _ => 0,
    }
}

/// Maps receiver types to the `PrimToString` intrinsic ID.
fn prim_to_string_intrinsic_id(receiver_type: Option<&TypeKind>) -> u16 {
    match receiver_type {
        Some(TypeKind::Float32 | TypeKind::Float64) => 16,
        _ => 15,
    }
}

/// Resolve a method call on a primitive/built-in receiver to a `PrimitiveMethod` operation.
/// Returns `None` if the method is not a known built-in (falls through to vtable dispatch).
fn resolve_primitive_method(
    method: &str,
    args: &[Expr],
    receiver_ty: Option<&TypeKind>,
) -> Option<PrimitiveMethod> {
    match method {
        "len" if args.is_empty() => Some(PrimitiveMethod::Len),
        "to_str" if args.is_empty() => match receiver_ty {
            Some(TypeKind::Str) | Some(TypeKind::Ref { .. }) => Some(PrimitiveMethod::AsStr),
            _ => Some(PrimitiveMethod::PrimToStr {
                tag: prim_to_str_tag(receiver_ty),
            }),
        },
        "to_string" if args.is_empty() => match receiver_ty {
            Some(TypeKind::Str) | Some(TypeKind::Ref { .. }) | None => {
                Some(PrimitiveMethod::StrToString)
            }
            Some(TypeKind::Bool) => Some(PrimitiveMethod::BoolToString),
            _ => Some(PrimitiveMethod::PrimToString {
                intrinsic_id: prim_to_string_intrinsic_id(receiver_ty),
            }),
        },
        "as_string" | "as_str" if args.is_empty() => Some(PrimitiveMethod::AsStr),
        "as_ptr" if args.is_empty() && matches!(receiver_ty, Some(TypeKind::Bytes)) => {
            Some(PrimitiveMethod::BytesAsPtr)
        }
        "parse" => Some(PrimitiveMethod::Parse { is_float: false }),
        _ => None,
    }
}

fn extract_field_chain(expr: &Expr) -> Option<(String, Vec<String>)> {
    match &expr.node {
        ExprKind::Ident(name) => Some((name.clone(), vec![])),
        ExprKind::Field { object, name } => {
            let (base, mut path) = extract_field_chain(object)?;
            path.push(name.clone());
            Some((base, path))
        }
        _ => None,
    }
}

/// Maps `@intrinsic("quazi.X")` attribute strings to case IDs for the `Intrinsic` opcode.
fn intrinsic_id(attr: &crate::parser::ast::Attribute) -> Option<u16> {
    static INTRINSIC_MAP: LazyLock<HashMap<&'static str, u16>> = LazyLock::new(|| {
        let mut m = HashMap::new();
        m.insert("quazi.write", 0);
        m.insert("quazi.read", 1);
        m.insert("quazi.exit", 2);
        m.insert("quazi.malloc", 3);
        m.insert("quazi.free", 4);
        m.insert("quazi.realloc", 5);
        m.insert("quazi.memcpy", 6);
        m.insert("quazi.memset", 7);
        m.insert("quazi.memmove", 8);
        m.insert("quazi.memcmp", 9);
        m.insert("quazi.strlen", 10);
        m.insert("quazi.stderr_write", 11);
        m.insert("quazi.sleep_ms", 12);
        m.insert("quazi.getenv", 13);
        m.insert("quazi.str_concat", 14);
        m.insert("quazi.int_to_str", 15);
        m.insert("quazi.float_to_str", 16);
        // Threading: malloc+pthread_create/CreateThread; pthread_join+free/WaitForSingleObject
        m.insert("quazi.thread.spawn", 18);
        m.insert("quazi.thread.join", 19);
        // Net: only the ops that need sockaddr_in construction (accept uses 0 directly now)
        m.insert("quazi.net.bind_tcp", 20);
        m.insert("quazi.net.connect_tcp", 21);
        // String primitives needed to implement format in void
        m.insert("quazi.str.byte_at", 23);
        m.insert("quazi.str.from_byte", 24);
        m.insert("quazi.print_backtrace", 25);
        m.insert("quazi.os.memory_total", 26);
        m.insert("quazi.os.memory_available", 27);
        m.insert("quazi.os.hostname", 28);
        m.insert("quazi.str.as_ptr", 29);
        m
    });
    let name = attr
        .args
        .first()
        .and_then(|a| match a {
            crate::parser::ast::AttrArg::Positional(crate::parser::ast::AttrVal::Str(s)) => {
                Some(s.as_str())
            }
            _ => None,
        })
        .unwrap_or("");
    INTRINSIC_MAP.get(name).copied()
}

fn api_symbol(attr: &crate::parser::ast::Attribute) -> Option<String> {
    attr.args.first().and_then(|a| match a {
        crate::parser::ast::AttrArg::Positional(crate::parser::ast::AttrVal::Str(s)) => {
            Some(s.clone())
        }
        _ => None,
    })
}

fn cfg_condition_matches(attr: &crate::parser::ast::Attribute) -> bool {
    use crate::parser::ast::{AttrArg, AttrVal};
    for arg in &attr.args {
        match arg {
            AttrArg::KeyValue(key, AttrVal::Str(val)) if key == "target_os" => {
                return val.as_str() == std::env::consts::OS;
            }
            AttrArg::KeyValue(key, AttrVal::Str(val)) if key == "target_arch" => {
                return val.as_str() == std::env::consts::ARCH;
            }
            AttrArg::KeyValue(key, AttrVal::Str(val)) if key == "target_abi" => {
                #[cfg(target_os = "windows")]
                let host_abi = "win64";
                #[cfg(not(target_os = "windows"))]
                let host_abi = "sysv";
                return val.as_str() == host_abi;
            }
            _ => {}
        }
    }
    true // unknown condition — include unconditionally
}

/// Check whether an item's @cfg attributes (if any) evaluate to true on this host.
fn item_cfg_active(attributes: &[crate::parser::ast::Attribute]) -> bool {
    for attr in attributes {
        if attr.name == "cfg" && !cfg_condition_matches(attr) {
            return false;
        }
    }
    true
}

fn export_adapter_name(function_name: &str, function_index: u16) -> String {
    format!(
        "__quazi_export_adapter_{}_{}",
        function_name
            .chars()
            .map(|ch| if ch.is_alphanumeric() { ch } else { '_' })
            .collect::<String>(),
        function_index
    )
}

fn collect_destructor_roots(program: &Program) -> HashSet<String> {
    let mut roots = HashSet::new();
    for item in &program.items {
        match &item.node {
            ItemKind::Fn { name, .. } if name == "free" => {
                roots.insert(name.clone());
            }
            ItemKind::Impl {
                for_ty, methods, ..
            } => {
                let type_name = type_kind_base_name(&for_ty.node);
                for method in methods {
                    if let ItemKind::Fn { name, .. } = &method.node
                        && name == "free"
                    {
                        roots.insert(format!("{}.{}", type_name, name));
                    }
                }
            }
            _ => {}
        }
    }
    roots
}

/// Recursively collect all `ExprKind::Ident` names from an expression tree.
fn collect_idents(expr: &Expr, names: &mut Vec<String>) {
    match &expr.node {
        ExprKind::Ident(name) => names.push(name.clone()),
        ExprKind::Literal(_) => {}
        ExprKind::Group(inner) => collect_idents(inner, names),
        ExprKind::Unary { expr: inner, .. } => collect_idents(inner, names),
        ExprKind::Cast { expr: inner, .. } => collect_idents(inner, names),
        ExprKind::Binary { left, right, .. } => {
            collect_idents(left, names);
            collect_idents(right, names);
        }
        ExprKind::Assign { target, value, .. } => {
            collect_idents(target, names);
            collect_idents(value, names);
        }
        ExprKind::Call {
            callee,
            args,
            named_args,
            ..
        } => {
            collect_idents(callee, names);
            for a in args {
                collect_idents(a, names);
            }
            for (_, a) in named_args {
                collect_idents(a, names);
            }
        }
        ExprKind::Field { object, .. } => collect_idents(object, names),
        ExprKind::StructInit { fields, .. } => {
            for (_, f) in fields {
                collect_idents(f, names);
            }
        }
        ExprKind::MethodCall {
            object,
            args,
            named_args,
            ..
        } => {
            collect_idents(object, names);
            for a in args {
                collect_idents(a, names);
            }
            for (_, a) in named_args {
                collect_idents(a, names);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            collect_idents(scrutinee, names);
            for arm in arms {
                collect_idents(&arm.expr, names);
                if let Some(g) = &arm.guard {
                    collect_idents(g, names);
                }
            }
        }
        ExprKind::CompoundAssign { target, value, .. } => {
            collect_idents(target, names);
            collect_idents(value, names);
        }
        ExprKind::IncDec { expr: inner, .. } => collect_idents(inner, names),
        ExprKind::ArrayLit(elems) => {
            for e in elems {
                collect_idents(e, names);
            }
        }
        ExprKind::Index { object, indices } => {
            collect_idents(object, names);
            for i in indices {
                collect_idents(i, names);
            }
        }
        ExprKind::Try { expr: inner } => collect_idents(inner, names),
        ExprKind::Closure { body, .. } => collect_idents(body, names),
    }
}

fn is_comparison(op: &BinOpKind) -> bool {
    matches!(
        op,
        BinOpKind::Lt
            | BinOpKind::LtEq
            | BinOpKind::Gt
            | BinOpKind::GtEq
            | BinOpKind::EqEq
            | BinOpKind::NotEq
    )
}

/// Conditional jump opcode that fires when the comparison is TRUE.
fn direct_cmp(op: &BinOpKind) -> Opcode {
    match op {
        BinOpKind::Lt => Opcode::Jl,
        BinOpKind::LtEq => Opcode::Jle,
        BinOpKind::Gt => Opcode::Jg,
        BinOpKind::GtEq => Opcode::Jge,
        BinOpKind::EqEq => Opcode::Je,
        BinOpKind::NotEq => Opcode::Jne,
        _ => Opcode::Jnz,
    }
}

/// Conditional jump opcode that fires when the comparison is FALSE (negated).
fn negate_cmp(op: &BinOpKind) -> Opcode {
    match op {
        BinOpKind::Lt => Opcode::Jge,
        BinOpKind::LtEq => Opcode::Jg,
        BinOpKind::Gt => Opcode::Jle,
        BinOpKind::GtEq => Opcode::Jl,
        BinOpKind::EqEq => Opcode::Jne,
        BinOpKind::NotEq => Opcode::Je,
        _ => Opcode::Jz,
    }
}

fn type_kind_base_name(ty: &TypeKind) -> String {
    match ty {
        TypeKind::Named { name, .. } => name.clone(),
        TypeKind::Int8 => "i8".to_string(),
        TypeKind::Int16 => "i16".to_string(),
        TypeKind::Int32 => "i32".to_string(),
        TypeKind::Int64 => "i64".to_string(),
        TypeKind::Uint8 => "u8".to_string(),
        TypeKind::Uint16 => "u16".to_string(),
        TypeKind::Uint32 => "u32".to_string(),
        TypeKind::Uint64 => "u64".to_string(),
        TypeKind::Isize => "isize".to_string(),
        TypeKind::Usize => "usize".to_string(),
        TypeKind::Float16 => "f16".to_string(),
        TypeKind::Float32 => "f32".to_string(),
        TypeKind::Float64 => "f64".to_string(),
        TypeKind::Bool => "bool".to_string(),
        TypeKind::Str => "str".to_string(),
        TypeKind::Bytes => "bytes".to_string(),
        TypeKind::CFn { .. } => "C fn".to_string(),
        TypeKind::Ref { inner } => type_kind_base_name(&inner.node),
        TypeKind::RawPtr { inner } => type_kind_base_name(&inner.node),
        other => format!("{}", other),
    }
}

/// Structural equality comparison for TypeKind (no PartialEq derive available).
fn types_equal(a: &TypeKind, b: &TypeKind) -> bool {
    use TypeKind::*;
    match (a, b) {
        (Int8, Int8) | (Int16, Int16) | (Int32, Int32) | (Int64, Int64) => true,
        (Uint8, Uint8) | (Uint16, Uint16) | (Uint32, Uint32) | (Uint64, Uint64) => true,
        (Isize, Isize) | (Usize, Usize) => true,
        (Float16, Float16) | (Float32, Float32) | (Float64, Float64) => true,
        (Bool, Bool) | (Str, Str) | (Bytes, Bytes) | (Void, Void) | (Any, Any) | (Never, Never) => {
            true
        }
        (
            Named {
                name: n1,
                type_args: a1,
            },
            Named {
                name: n2,
                type_args: a2,
            },
        ) => {
            n1 == n2
                && a1.len() == a2.len()
                && a1
                    .iter()
                    .zip(a2)
                    .all(|(t1, t2)| types_equal(&t1.node, &t2.node))
        }
        (Ref { inner: i1 }, Ref { inner: i2 }) => types_equal(&i1.node, &i2.node),
        (RawPtr { inner: i1 }, RawPtr { inner: i2 }) => types_equal(&i1.node, &i2.node),
        (
            Array {
                elem_ty: e1,
                len: l1,
            },
            Array {
                elem_ty: e2,
                len: l2,
            },
        ) => l1 == l2 && types_equal(&e1.node, &e2.node),
        (Slice { elem_ty: e1 }, Slice { elem_ty: e2 }) => types_equal(&e1.node, &e2.node),
        (
            Fn {
                params: p1,
                return_ty: r1,
            },
            Fn {
                params: p2,
                return_ty: r2,
            },
        )
        | (
            CFn {
                params: p1,
                return_ty: r1,
            },
            CFn {
                params: p2,
                return_ty: r2,
            },
        ) => {
            p1.len() == p2.len()
                && p1
                    .iter()
                    .zip(p2)
                    .all(|(t1, t2)| types_equal(&t1.node, &t2.node))
                && types_equal(&r1.node, &r2.node)
        }
        _ => false,
    }
}

fn types_equal_slice(a: &[TypeKind], b: &[TypeKind]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(t1, t2)| types_equal(t1, t2))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::semantic::Analyzer;

    fn compile(src: &str) -> Vec<Chunk> {
        let tokens = Lexer::new(src).tokenize();
        let program = Parser::new(tokens).parse().expect("parse failed");
        let report = Analyzer::new().analyze_program(&program);
        assert!(
            report.errors.is_empty(),
            "semantic errors: {:?}",
            report.errors
        );
        Codegen::new(&report)
            .compile_program(&program, &[])
            .expect("code generation should succeed")
    }

    #[test]
    fn simple_add_function_emits_add_and_ret() {
        let chunks = compile("fn add(a: i32, b: i32) i32 { ret a + b; }");
        assert_eq!(chunks.len(), 1);
        let chunk = &chunks[0];
        assert_eq!(chunk.name, "add");
        assert!(
            chunk.code.iter().any(|i| i.opcode == Opcode::Add as u8),
            "expected Add instruction"
        );
        assert_eq!(chunk.code.last().unwrap().opcode, Opcode::Ret as u8);
    }

    #[test]
    fn owned_parameter_is_destroyed_on_fallthrough() {
        let chunks = compile(
            r#"struct Owned { value: i32 }
               impl Owned { fn free(self: Owned) void {} }
               fn take(value: Owned) void {}"#,
        );
        let take = chunks.iter().find(|chunk| chunk.name == "take").unwrap();
        assert_eq!(
            take.code
                .iter()
                .filter(|instruction| instruction.opcode == Opcode::CallIdx as u8)
                .count(),
            1,
            "a by-value parameter must be destroyed at function fallthrough"
        );
    }

    #[test]
    fn returning_owned_parameter_transfers_ownership() {
        let chunks = compile(
            r#"struct Owned { value: i32 }
               impl Owned { fn free(self: Owned) void {} }
               fn pass(value: Owned) Owned { ret value; }"#,
        );
        let pass = chunks.iter().find(|chunk| chunk.name == "pass").unwrap();
        assert!(
            pass.code
                .iter()
                .all(|instruction| instruction.opcode != Opcode::CallIdx as u8),
            "a returned parameter must not be destroyed by its former owner"
        );
    }

    #[test]
    fn cleanup_for_early_return_does_not_disable_later_path() {
        let chunks = compile(
            r#"struct Owned { value: i32 }
               impl Owned { fn free(self: Owned) void {} }
               fn branch(value: Owned, early: bool) void {
                   if (early) { ret; }
               }"#,
        );
        let branch = chunks
            .iter()
            .find(|chunk| chunk.name == "branch")
            .unwrap();
        assert_eq!(
            branch
                .code
                .iter()
                .filter(|instruction| instruction.opcode == Opcode::CallIdx as u8)
                .count(),
            2,
            "both the early-return and fallthrough paths need cleanup"
        );
    }

    #[test]
    fn const_fold_reduces_instruction_count() {
        // Without folding: MovI, MovI, Add, Mov, Ret = 5
        // With folding:    MovI(3), Mov, Ret = 3
        let chunks = compile("fn foo() i32 { const x: i32 = 1 + 2; ret x; }");
        assert_eq!(chunks.len(), 1);
        let count = chunks[0].code.len();
        assert!(
            count <= 3,
            "expected ≤3 instructions (const-folded), got {}",
            count
        );
        // Must not contain Add — the folded path emits only MovI
        assert!(
            !chunks[0].code.iter().any(|i| i.opcode == Opcode::Add as u8),
            "Add should be eliminated by const folding"
        );
    }

    #[test]
    fn while_loop_jump_points_back_to_condition() {
        let chunks = compile(
            r#"fn countdown(x: i32) void {
                for x > 0 { x = x + 1; }
            }"#,
        );
        assert_eq!(chunks.len(), 1);
        let code = &chunks[0].code;

        // Find the trailing Jmp (back-edge of loop).
        let back_jmp = code
            .iter()
            .rposition(|i| i.opcode == Opcode::Jmp as u8)
            .expect("expected Jmp back-edge");

        // Its target must be instruction 0 (loop_top = 0).
        let (_, target) = code[back_jmp].ri16();
        assert_eq!(
            target, 0,
            "back-edge Jmp must target instruction 0 (loop top)"
        );
    }

    #[test]
    fn if_else_jump_targets_are_patched() {
        let chunks = compile(
            r#"fn sign(x: i32) i32 {
                if (x > 0) { ret 1; } else { ret 0; }
            }"#,
        );
        let code = &chunks[0].code;
        let len = code.len() as u16;

        // All jump targets must be valid instruction indices.
        for instr in code {
            let op = instr.opcode;
            let is_jump = matches!(
                Opcode::from_u8(op),
                Some(
                    Opcode::Jmp
                        | Opcode::Je
                        | Opcode::Jne
                        | Opcode::Jl
                        | Opcode::Jle
                        | Opcode::Jg
                        | Opcode::Jge
                        | Opcode::Jz
                        | Opcode::Jnz
                )
            );
            if is_jump {
                let (_, target) = instr.ri16();
                assert!(
                    target <= len,
                    "jump target {} out of bounds (chunk has {} instructions)",
                    target,
                    len
                );
            }
        }
    }

    #[test]
    fn function_call_emits_call_idx() {
        // Use a function with more than 2 statements so it won't be inlined.
        let chunks = compile(
            r#"fn helper(x: i32) i32 { const a: i32 = 1; const b: i32 = 2; ret x; }
               fn main() void { helper(1); }"#,
        );
        let main_chunk = chunks
            .iter()
            .find(|c| c.name == "main")
            .expect("no main chunk");
        assert!(
            main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::CallIdx as u8),
            "expected CallIdx in main"
        );
    }

    #[test]
    fn ret_always_last_in_every_chunk() {
        let chunks = compile(
            r#"fn a() void {}
               fn b(x: i32) i32 { ret x; }"#,
        );
        for chunk in &chunks {
            assert_eq!(
                chunk.code.last().map(|i| i.opcode),
                Some(Opcode::Ret as u8),
                "chunk '{}' does not end with Ret",
                chunk.name
            );
        }
    }

    #[test]
    fn generic_function_produces_monomorphized_chunks() {
        let chunks = compile(
            r#"fn id[T](x: T) T { ret x; }
               fn main() void { id[i32](5); }"#,
        );
        // Should have chunks: id<i32> and main
        let id_i32 = "id<i32>";
        assert!(
            chunks.iter().any(|c| c.name == id_i32),
            "expected monomorphized chunk {}",
            id_i32
        );
        let main_chunk = chunks.iter().find(|c| c.name == "main");
        assert!(main_chunk.is_some(), "expected main chunk");
        // main should call the monomorphized function
        assert!(
            main_chunk
                .unwrap()
                .code
                .iter()
                .any(|i| i.opcode == Opcode::CallIdx as u8),
            "expected CallIdx in main"
        );
    }

    #[test]
    fn monomorphized_primitive_method_uses_concrete_type() {
        let chunks = compile(
            r#"fn show[T](x: T) void {
                var s = x.to_string();
            }
            fn main() void {
                show[i32](1);
                show[f64](2);
            }"#,
        );
        // show<i32> should use Intrinsic(15) for int to_string
        let show_i32 = chunks.iter().find(|c| c.name == "show<i32>").unwrap();
        let has_int_intrinsic = show_i32
            .code
            .iter()
            .any(|i| i.opcode == Opcode::Intrinsic as u8 && i.ri16().1 == 15);
        assert!(
            has_int_intrinsic,
            "show<i32> should use Intrinsic(15) for int to_string"
        );
        // show<f64> should use Intrinsic(16) for float to_string
        let show_f64 = chunks.iter().find(|c| c.name == "show<f64>").unwrap();
        let has_float_intrinsic = show_f64
            .code
            .iter()
            .any(|i| i.opcode == Opcode::Intrinsic as u8 && i.ri16().1 == 16);
        assert!(
            has_float_intrinsic,
            "show<f64> should use Intrinsic(16) for float to_string"
        );
    }

    #[test]
    fn compound_assign_emits_arithmetic_op_in_place() {
        let chunks = compile(
            r#"fn inc(x: i32) i32 {
                x += 1;
                ret x;
            }"#,
        );
        assert!(
            chunks[0].code.iter().any(|i| i.opcode == Opcode::Add as u8),
            "x += 1 should emit Add"
        );
    }

    #[test]
    fn inc_dec_emits_inc_dec_opcode() {
        let chunks = compile(
            r#"fn bump(x: i32) i32 {
                x++;
                ret x;
            }"#,
        );
        assert!(
            chunks[0].code.iter().any(|i| i.opcode == Opcode::Inc as u8),
            "x++ should emit Inc"
        );
    }

    #[test]
    fn compound_assign_on_index_loads_modifies_stores() {
        // `n` is a parameter — constprop cannot fold arr[0] + n to a constant.
        // This exercises the load-modify-store path for index compound assignment.
        let chunks = compile(
            r#"fn test(n: i32) i32 {
                var arr = [1, 2, 3];
                arr[0] += n;
                ret arr[0];
            }"#,
        );
        assert!(
            chunks[0].code.iter().any(|i| i.opcode == Opcode::Add as u8),
            "arr[0] += n should emit Add when n is not constant"
        );
    }

    #[test]
    fn raw_pointer_dereferences_use_pointee_width() {
        let chunks = compile(
            r#"unsafe fn read(p: *i8) i8 { ret *p; }
               unsafe fn write(p: *u16, value: u16) void { *p = value; }
               unsafe fn add(p: *i32, value: i32) void { *p += value; }"#,
        );

        let read = chunks.iter().find(|chunk| chunk.name == "read").unwrap();
        let load = read
            .code
            .iter()
            .find(|instr| instr.opcode == Opcode::Load as u8)
            .expect("dereference read should emit Load");
        assert_eq!(load.mem_width(), MemWidth::Byte);
        assert!(load.mem_signed());

        let write = chunks.iter().find(|chunk| chunk.name == "write").unwrap();
        let store = write
            .code
            .iter()
            .find(|instr| instr.opcode == Opcode::Store as u8)
            .expect("dereference write should emit Store");
        assert_eq!(store.mem_width(), MemWidth::Word);

        let add = chunks.iter().find(|chunk| chunk.name == "add").unwrap();
        let load = add
            .code
            .iter()
            .find(|instr| instr.opcode == Opcode::Load as u8)
            .expect("compound dereference should load");
        let store = add
            .code
            .iter()
            .find(|instr| instr.opcode == Opcode::Store as u8)
            .expect("compound dereference should store");
        assert_eq!(load.mem_width(), MemWidth::Dword);
        assert!(load.mem_signed());
        assert_eq!(store.mem_width(), MemWidth::Dword);
    }

    #[test]
    fn large_int_goes_to_constant_pool() {
        let chunks = compile("fn big() i32 { ret 100000; }");
        assert!(
            !chunks[0].constants.is_empty(),
            "100000 should be in constant pool"
        );
        assert!(
            chunks[0]
                .code
                .iter()
                .any(|i| i.opcode == Opcode::MovConst as u8),
            "expected MovConst for large literal"
        );
    }

    #[test]
    fn string_literal_goes_to_constant_pool() {
        let chunks = compile(r#"fn greeting() str { ret "hello"; }"#);
        assert!(
            matches!(chunks[0].constants.first(), Some(ConstPoolEntry::Str(s)) if s == "hello"),
            "string literal should be in constant pool"
        );
    }

    #[test]
    fn to_bytes_produces_six_bytes_per_instruction() {
        let chunks = compile("fn f(a: i32, b: i32) i32 { ret a + b; }");
        let bytes = chunks[0].to_bytes();
        assert_eq!(bytes.len(), chunks[0].code.len() * 6);
    }

    #[test]
    fn negative_const_value_goes_to_constant_pool() {
        // const-folded 0 - 1 produces ConstValue::Int(-1) which must use MovConst not MovI
        let chunks = compile("fn neg() i32 { const x: i32 = 0 - 1; ret x; }");
        assert!(
            chunks[0]
                .code
                .iter()
                .any(|i| i.opcode == Opcode::MovConst as u8),
            "negative constant should be in constant pool"
        );
    }

    #[test]
    fn syscall_attribute_emits_syscall_opcode() {
        let chunks =
            compile(r#"@syscall("write") fn write(fd: i32, buf: str, len: usize) isize { }"#);
        assert_eq!(chunks[0].name, "write");
        assert!(
            chunks[0]
                .code
                .iter()
                .any(|i| i.opcode == Opcode::Syscall as u8),
            "expected Syscall instruction for @syscall fn"
        );
        assert_eq!(chunks[0].code.last().unwrap().opcode, Opcode::Ret as u8);
    }

    #[test]
    fn syscall_attribute_accepts_numeric_id() {
        let chunks = compile(r#"@syscall(60) fn exit(code: i32) isize { }"#);
        let instr = chunks[0]
            .code
            .iter()
            .find(|i| i.opcode == Opcode::Syscall as u8)
            .expect("expected Syscall instruction");
        // Numeric syscall id is stored in the const pool, ri16 gives the pool index.
        let (_, idx) = instr.ri16();
        assert!(
            matches!(
                chunks[0].constants.get(idx as usize),
                Some(ConstPoolEntry::Int(60))
            ),
            "expected Int(60) in const pool at index {idx}"
        );
        assert_eq!(instr.flags, 1);
    }

    #[test]
    fn api_attribute_emits_call_ext_opcode() {
        let chunks = compile(
            r#"@api("WriteFile") unsafe fn win_write(h: usize, buf: *u8, len: usize, out: *u8, ovl: *u8) usize;"#,
        );
        let chunk = &chunks[0];
        assert!(
            chunk.code.iter().any(|i| i.opcode == Opcode::CallExt as u8),
            "expected CallExt instruction for @api fn"
        );
        assert!(
            chunk
                .constants
                .iter()
                .any(|c| matches!(c, ConstPoolEntry::ForeignSymbol(s) if s.symbol == "WriteFile")),
            "expected WriteFile in constant pool"
        );
    }

    #[test]
    fn c_variadic_call_records_promoted_actual_signature() {
        let chunks = compile(
            r#"
@api("printf") unsafe fn printf(format: *u8, ...) i32;
fn main() void { unsafe { printf(0, 1.5, 7 as i8); } }
"#,
        );
        let main = chunks.iter().find(|chunk| chunk.name == "main").unwrap();
        let foreign = main
            .constants
            .iter()
            .find_map(|constant| match constant {
                ConstPoolEntry::ForeignSymbol(symbol) if symbol.symbol == "printf" => Some(symbol),
                _ => None,
            })
            .expect("direct C-variadic call should carry ABI metadata");
        assert!(foreign.signature.variadic);
        assert_eq!(foreign.signature.params.len(), 3);
        assert_eq!(foreign.signature.params[0], AbiType::Pointer);
        assert_eq!(foreign.signature.params[1], AbiType::Float64);
        assert_eq!(
            foreign.signature.params[2],
            AbiType::Integer {
                bytes: 4,
                signed: true
            }
        );
    }

    #[test]
    fn export_uses_c_adapter_and_keeps_internal_function_abi() {
        let chunks = compile(
            r#"
@repr(C) struct Pair { x: f64, y: f64, }
@export("quazi_identity_pair") pub fn identity_pair(pair: Pair) Pair { ret pair; }
"#,
        );
        let original = chunks
            .iter()
            .find(|chunk| chunk.name == "identity_pair")
            .expect("exported Quazi function should remain present");
        assert!(original.export.is_none());

        let adapter = chunks
            .iter()
            .find(|chunk| chunk.export.is_some())
            .expect("export should have a C ABI adapter");
        let export = adapter.export.as_ref().unwrap();
        assert_eq!(export.symbol, "quazi_identity_pair");
        assert!(matches!(
            export.signature.params.as_slice(),
            [AbiType::Aggregate { size: 16, .. }]
        ));
        assert!(
            adapter
                .code
                .iter()
                .any(|instruction| instruction.opcode == Opcode::CallIdx as u8)
        );
    }

    #[test]
    fn export_roots_keep_their_quazi_dependencies() {
        let chunks = compile(
            r#"
fn helper(value: i32) i32 {
    var one = 1;
    var two = 2;
    ret value + one + two;
}
@export("quazi_entry") pub fn entry(value: i32) i32 { ret helper(value); }
fn main() void { }
"#,
        );
        assert!(
            chunks.iter().any(|chunk| chunk.name == "helper"),
            "dependencies reachable only from a C export must not be tree-shaken"
        );
    }

    #[test]
    fn repr_c_fields_use_c_offsets_and_widths() {
        let chunks = compile(
            r#"
@repr(C) struct Record { tag: i8, value: i32, next: *Record, }
fn read(record: Record) i32 { ret record.value; }
"#,
        );
        let load = chunks
            .iter()
            .flat_map(|chunk| chunk.code.iter())
            .find(|instr| instr.opcode == Opcode::FieldLoad as u8)
            .expect("C field access should emit FieldLoad");
        let (_, _, offset) = load.rrr();
        assert_eq!(offset, 4);
        assert_eq!(load.mem_width(), MemWidth::Dword);
        assert!(load.mem_signed());
    }

    #[test]
    fn repr_c_f32_fields_convert_at_the_memory_boundary() {
        let chunks = compile(
            r#"
@repr(C) struct Sample { value: f32, }
fn update(sample: Sample, value: f32) f32 {
    sample.value = value;
    ret sample.value;
}
"#,
        );
        let fields: Vec<_> = chunks
            .iter()
            .flat_map(|chunk| chunk.code.iter())
            .filter(|instruction| {
                matches!(
                    Opcode::from_u8(instruction.opcode),
                    Some(Opcode::FieldLoad | Opcode::FieldStore)
                )
            })
            .collect();
        assert!(!fields.is_empty());
        assert!(fields.iter().all(|instruction| {
            instruction.mem_width() == MemWidth::Dword
                && instruction.flags & crate::bytecode::instruction::FLOAT_FLAG != 0
        }));
    }

    #[test]
    fn repr_c_bitfields_emit_masked_storage_loads_and_stores() {
        let chunks = compile(
            r#"
@repr(C) struct Flags { low: u32:3, high: u32:5 }
fn main() u32 {
    var flags = Flags { low: 1, high: 2 };
    flags.high += 3;
    ret flags.low + flags.high;
}
"#,
        );
        let main = chunks.iter().find(|chunk| chunk.name == "main").unwrap();
        assert!(main.code.iter().any(|instruction| {
            instruction.opcode == Opcode::FieldLoad as u8
                && instruction.mem_width() == MemWidth::Dword
        }));
        assert!(
            main.code
                .iter()
                .any(|instruction| instruction.opcode == Opcode::And as u8)
        );
        assert!(
            main.code
                .iter()
                .any(|instruction| instruction.opcode == Opcode::Shl as u8)
        );
    }

    #[test]
    fn repr_c_flexible_array_index_uses_c_element_width() {
        let chunks = compile(
            r#"
@repr(C) struct Packet { length: u32, data: [u8; ..] }
unsafe fn first(packet: *Packet) u8 { ret (*packet).data[0]; }
"#,
        );
        let first = chunks.iter().find(|chunk| chunk.name == "first").unwrap();
        assert!(first.code.iter().any(|instruction| {
            instruction.opcode == Opcode::Load as u8 && instruction.mem_width() == MemWidth::Byte
        }));
    }

    #[test]
    fn c_function_pointers_use_export_adapters_and_c_indirect_calls() {
        let chunks = compile(
            r#"
@repr(C) type Callback = fn(i32) i32;
@export("increment") pub fn increment(value: i32) i32 { ret value + 1; }
@api("get_callback") unsafe fn get_callback() Callback;

fn main() i32 {
    var result: i32 = 0;
    unsafe {
        var local: Callback = increment;
        var foreign: Callback = get_callback();
        result = local(1) + foreign(2);
    }

    ret result;
}
"#,
        );
        let main = chunks.iter().find(|chunk| chunk.name == "main").unwrap();
        assert!(main
            .constants
            .iter()
            .any(|constant| matches!(constant, ConstPoolEntry::FnAddr(name) if name.starts_with("__quazi_export_adapter_increment_"))));
        assert_eq!(
            main.code
                .iter()
                .filter(|instruction| instruction.opcode == Opcode::CallCReg as u8)
                .count(),
            2
        );
        assert!(main.constants.iter().any(|constant| {
            matches!(
                constant,
                ConstPoolEntry::ForeignSymbol(symbol)
                    if symbol.symbol == "<function-pointer>"
                        && symbol.signature.params.len() == 1
            )
        }));
    }

    #[test]
    fn foreign_globals_use_typed_external_data_loads_and_stores() {
        let chunks = compile(
            r#"
@api("native_counter") var counter: i32;
@api("native_ratio") var ratio: f32;
fn main() i32 {
    var result: i32 = 0;
    unsafe {
        counter += 1;
        ratio = 2.5 as f32;
        result = counter;
    }
    ret result;
}
"#,
        );
        let main = chunks.iter().find(|chunk| chunk.name == "main").unwrap();
        assert!(main.constants.iter().any(|constant| {
            matches!(constant, ConstPoolEntry::ForeignGlobal(global) if global.symbol == "native_counter")
        }));
        assert!(main.code.iter().any(|instruction| {
            instruction.opcode == Opcode::Load as u8
                && instruction.mem_width() == MemWidth::Dword
                && instruction.mem_signed()
        }));
        assert!(main.code.iter().any(|instruction| {
            instruction.opcode == Opcode::Store as u8
                && instruction.mem_width() == MemWidth::Dword
                && instruction.flags & FLOAT_FLAG != 0
        }));
    }

    #[test]
    fn str_len_emits_strlen_opcode() {
        let chunks = compile(r#"fn f(s: str) any { ret s.len(); }"#);
        assert!(
            chunks[0]
                .code
                .iter()
                .any(|i| i.opcode == Opcode::StrLen as u8),
            "s.len() should emit StrLen"
        );
    }

    #[test]
    fn method_call_emits_call_arg_and_vtbl_load() {
        let chunks = compile(
            r#"
            struct Worker { id: i32, }
            trait Runnable { fn run(self: Worker, value: i32) void; }
            fn invoke(worker: dyn Runnable) void { worker.run(1); }
            "#,
        );
        let main_chunk = chunks.iter().find(|c| c.name == "invoke").unwrap();
        assert!(
            main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::CallArg as u8),
            "method call with args should emit CallArg"
        );
        assert!(
            main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::VtblLoad as u8),
            "method call should emit VtblLoad"
        );
    }

    #[test]
    fn tree_shaking_omits_unreachable_function() {
        // dead_fn is called only by zombie_fn; zombie_fn is never called by main.
        // Tree-shaking should exclude both from the output chunks.
        let chunks = compile(
            r#"fn dead_fn(x: i32) i32 { const a: i32 = 1; const b: i32 = 2; ret x; }
               fn zombie_fn() void { dead_fn(1); }
               fn main() void { ret; }"#,
        );
        assert!(
            !chunks.iter().any(|c| c.name == "dead_fn"),
            "dead_fn should be tree-shaken"
        );
        assert!(
            !chunks.iter().any(|c| c.name == "zombie_fn"),
            "zombie_fn should be tree-shaken"
        );
        assert!(
            chunks.iter().any(|c| c.name == "main"),
            "main must be present"
        );
    }

    #[test]
    fn const_true_if_skips_else_branch() {
        // `1 == 1` const-folds to Bool(true) — else branch must not be compiled.
        let chunks = compile(
            r#"fn f() i32 {
                if (1 == 1) { ret 1; } else { ret 2; }
            }"#,
        );
        let code = &chunks[0].code;
        // With const-condition elimination, no conditional jump instruction.
        let has_conditional_jump = code.iter().any(|i| {
            matches!(
                Opcode::from_u8(i.opcode),
                Some(
                    Opcode::Je
                        | Opcode::Jne
                        | Opcode::Jl
                        | Opcode::Jle
                        | Opcode::Jg
                        | Opcode::Jge
                        | Opcode::Jz
                        | Opcode::Jnz
                )
            )
        });
        assert!(
            !has_conditional_jump,
            "const-true if should not emit a conditional jump"
        );
        // Dead else branch must not emit MovI(2).
        let has_movi_2 = code.iter().any(|i| {
            let (_, imm) = i.ri16();
            i.opcode == Opcode::MovI as u8 && imm == 2
        });
        assert!(
            !has_movi_2,
            "const-true if should not emit MovI(2) from dead else branch"
        );
    }

    #[test]
    fn const_false_if_skips_then_branch() {
        // `0 == 1` const-folds to Bool(false) — then branch must not be compiled.
        let chunks = compile(
            r#"fn f() i32 {
                if (0 == 1) { ret 99; } else { ret 7; }
            }"#,
        );
        let code = &chunks[0].code;
        let has_movi_99 = code.iter().any(|i| {
            let (_, imm) = i.ri16();
            i.opcode == Opcode::MovI as u8 && imm == 99
        });
        assert!(
            !has_movi_99,
            "const-false if should not emit MovI(99) from dead then branch"
        );
    }

    #[test]
    fn const_false_while_emits_no_loop_instructions() {
        // `0 == 1` const-folds to Bool(false) — while body must be skipped entirely.
        let chunks = compile(
            r#"fn f() void {
                for 0 == 1 { var x: i32 = 1; }
            }"#,
        );
        let code = &chunks[0].code;
        // No loop-back Jmp should exist.
        assert!(
            !code.iter().any(|i| i.opcode == Opcode::Jmp as u8),
            "for(0==1) should emit no Jmp"
        );
    }

    #[test]
    fn impl_method_is_compiled_with_mangled_name() {
        let chunks = compile(
            r#"struct Point { x: i32, y: i32, }
               impl Point {
                   fn get_x(self: Point) i32 { ret self.x; }
               }
               fn main() void {
                   var p: Point = Point { x: 1, y: 0 };
                   p.get_x();
                   ret;
               }"#,
        );
        assert!(
            chunks.iter().any(|c| c.name == "Point.get_x"),
            "impl method should be compiled as 'Point.get_x', got: {:?}",
            chunks.iter().map(|c| &c.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn impl_method_call_emits_call_idx_not_vtbl() {
        // Static dispatch: impl method call must NOT use VtblLoad/CallReg (dynamic dispatch).
        // After inlining the method body is expanded in-place — CallIdx may be absent — but
        // dynamic dispatch instructions must never appear for a Known Named type.
        let chunks = compile(
            r#"struct Counter { val: i32, }
               impl Counter {
                   fn get(self: Counter) i32 { ret self.val; }
               }
               fn main() void {
                   var c: Counter = Counter { val: 0 };
                   var n: i32 = c.get();
               }"#,
        );
        let main_chunk = chunks.iter().find(|c| c.name == "main").unwrap();
        assert!(
            !main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::VtblLoad as u8),
            "impl method call on Named type should NOT emit VtblLoad"
        );
        assert!(
            !main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::CallReg as u8),
            "impl method call on Named type should NOT emit CallReg (dynamic dispatch)"
        );
        // Verify Counter.get was compiled as a named chunk (static identity exists)
        assert!(
            chunks.iter().any(|c| c.name == "Counter.get"),
            "Counter.get must be compiled as its own chunk"
        );
    }

    #[test]
    fn trait_impl_method_is_compiled_with_mangled_name() {
        let chunks = compile(
            r#"trait Display { fn to_str(self: Num) str; }
               struct Num { val: i32, }
               impl Display for Num {
                   fn to_str(self: Num) str { ret "num"; }
               }
               fn main() void {
                   var n: Num = Num { val: 42 };
                   n.to_str();
                   ret;
               }"#,
        );
        assert!(
            chunks.iter().any(|c| c.name == "Num.to_str"),
            "trait impl method should be compiled as 'Num.to_str'"
        );
    }

    #[test]
    fn impl_method_with_args_passes_receiver_first() {
        // Verify that an impl method with (self + explicit args) is compiled correctly.
        // The method may be inlined, but dynamic dispatch must never be used for Named types.
        // We verify: (1) no VtblLoad, (2) Acc.add has param_count == 2 (receiver + n).
        let chunks = compile(
            r#"struct Acc { sum: i32, }
               impl Acc {
                   fn add(self: Acc, n: i32) i32 { ret self.sum + n; }
               }
               fn main() void {
                   var a: Acc = Acc { sum: 0 };
                   var r: i32 = a.add(5);
               }"#,
        );
        let main_chunk = chunks.iter().find(|c| c.name == "main").unwrap();
        assert!(
            !main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::VtblLoad as u8),
            "a.add(5) on Named type should NOT use VtblLoad"
        );
        let add_chunk = chunks.iter().find(|c| c.name == "Acc.add").unwrap();
        assert_eq!(
            add_chunk.param_count, 2,
            "Acc.add must have param_count == 2 (self + n)"
        );
    }

    #[test]
    fn inherent_impl_parses_and_compiles() {
        // impl Type {} without 'for' keyword (inherent impl)
        let chunks = compile(
            r#"struct Box { val: i32, }
               impl Box {
                   fn get_val(self: Box) i32 { ret self.val; }
               }
               fn main() void {
                   var b: Box = Box { val: 1 };
                   b.get_val();
                   ret;
               }"#,
        );
        assert!(chunks.iter().any(|c| c.name == "Box.get_val"));
    }

    #[test]
    fn closure_produces_anonymous_chunk() {
        let chunks = compile(
            r#"fn main() void {
                var f = || 42;
                var x: i32 = f();
            }"#,
        );
        assert!(
            chunks.iter().any(|c| c.name == "__quazi_closure_0"),
            "expected anonymous closure chunk"
        );
        let closure_chunk = chunks
            .iter()
            .find(|c| c.name == "__quazi_closure_0")
            .unwrap();
        assert_eq!(
            closure_chunk.code.last().unwrap().opcode,
            Opcode::Ret as u8,
            "closure chunk must end with Ret"
        );
        assert_eq!(
            closure_chunk.param_count, 1,
            "no-capture closure has 1 param: hidden env ptr as r0"
        );
        let main_chunk = chunks.iter().find(|c| c.name == "main").unwrap();
        assert!(
            main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::MovConst as u8),
            "expected MovConst for FnAddr in main"
        );
        assert!(
            main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::CallReg as u8),
            "expected CallReg for indirect call in main"
        );
    }

    #[test]
    fn closure_capture_allocates_env_struct() {
        let chunks = compile(
            r#"fn main() void {
                var a: i32 = 1;
                var f = || a;
                var r: i32 = f();
            }"#,
        );
        let closure_chunk = chunks
            .iter()
            .find(|c| c.name == "__quazi_closure_0")
            .unwrap();
        // With one capture, param_count = 0 user params + 1 hidden env ptr = 1
        assert_eq!(
            closure_chunk.param_count, 1,
            "closure should have hidden env ptr param"
        );
        // The closure should load from the env struct (FieldLoad)
        assert!(
            closure_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::FieldLoad as u8),
            "expected FieldLoad for capture access in closure"
        );
        let main_chunk = chunks.iter().find(|c| c.name == "main").unwrap();
        // Main should allocate the env struct
        assert!(
            main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::New as u8),
            "expected New for env struct allocation"
        );
        // Main should pass env ptr as hidden first arg (CallArg before fn_ptr load)
        assert!(
            main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::CallArg as u8),
            "expected CallArg for hidden env ptr"
        );
    }

    #[test]
    fn byte_string_uses_exact_bytes_and_byte_loads() {
        let chunks = compile(
            r#"fn main() i32 {
                var value: bytes = b"A\xFF\0";
                ret value[1] as i32;
            }"#,
        );
        let main = chunks.iter().find(|chunk| chunk.name == "main").unwrap();
        assert!(main.constants.iter().any(
            |constant| matches!(constant, ConstPoolEntry::Bytes(bytes) if bytes == &[b'A', 0xff, 0])
        ));
        assert!(main.code.iter().any(|instruction| {
            instruction.opcode == Opcode::Load as u8 && instruction.mem_width() == MemWidth::Byte
        }));
    }

    #[test]
    fn try_preserves_payload_type_for_chained_inherent_method() {
        let chunks = compile(
            r#"
            struct Value { n: i32, }
            impl Value {
                fn increment(self: Value) i32 { ret self.n + 1; }
            }
            fn make() Result[Value, i32] { ret Ok(Value { n: 41 }); }
            fn answer() Result[i32, i32] { ret Ok(make()?.increment()); }
            "#,
        );
        let answer = chunks.iter().find(|chunk| chunk.name == "answer").unwrap();
        assert!(answer.code.iter().all(|instruction| {
            instruction.opcode != Opcode::VtblLoad as u8
        }));
    }
}
