// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! One program for every QZI target.
//!
//! A QZI is analyzed and lowered once. This pass turns the loaded
//! program, whose items and statements still carry `@cfg`, into one
//! analyzable program in which every declaration is valid for a known set of
//! targets (a bit mask over the target list):
//!
//! - Declarations without target-specific syntax stay single and shared.
//! - Identical conditional declarations (`@cfg(linux)` and `@cfg(netbsd)`
//!   with the same text) become one declaration available on both.
//! - A function whose body differs between targets (`@cfg` statements,
//!   conditional variants with the same signature) gets one copy per
//!   distinct target class. The first copy keeps the source name; the others
//!   are named `name@t<mask>`. Callers keep calling `name`, and the QZI
//!   selects the body for the target by name (format 17 body masks).
//! - A target-dependent type alias (`c_long`) is substituted by its
//!   per-target definition inside each copy that uses it, and declarations
//!   whose signatures change through it are copied the same way; their
//!   callers are rewritten to the copy of their class.
//!
//! Copies of one source item must not share source coordinates, because
//! analysis and lowering key facts by span. Every file therefore gets
//! `replicas` consecutive images of its text range: copy `r` of an item lives
//! in image `r`, so it still belongs to its file, imports and module, and
//! [`SpanLayout::original`] maps diagnostics back to the real source.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;

use crate::loader::CfgTarget;
use crate::parser::ast::*;
use crate::parser::visit::{self, MutVisitor};

use super::item_should_include_for;
use super::types::SourceFile;

/// Internal attribute carrying the target mask of a declaration copy.
pub const TARGETS_ATTRIBUTE: &str = "__targets";
/// Separator between a source name and the target mask of a non-primary copy.
pub const VARIANT_MARKER: &str = "@t";

/// The source name of a declaration, function or chunk symbol: every
/// `@t<mask>` copy marker removed.
pub fn strip_variant_markers(name: &str) -> String {
    if !name.contains(VARIANT_MARKER) {
        return name.to_string();
    }
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(index) = rest.find(VARIANT_MARKER) {
        out.push_str(&rest[..index]);
        let after = &rest[index + VARIANT_MARKER.len()..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            out.push_str(VARIANT_MARKER);
        }
        rest = &after[digits..];
    }
    out.push_str(rest);
    out
}

/// Target mask encoded in the innermost copy marker of a symbol, if any.
pub fn variant_marker_mask(name: &str) -> Option<u8> {
    let index = name.rfind(VARIANT_MARKER)?;
    let digits: String = name[index + VARIANT_MARKER.len()..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// Target mask of a declaration copy produced by this pass.
pub fn declared_targets(attributes: &[Attribute]) -> Option<u8> {
    attributes
        .iter()
        .find(|attribute| attribute.name == TARGETS_ATTRIBUTE)
        .and_then(|attribute| match attribute.args.first() {
            Some(AttrArg::Positional(AttrVal::Int(mask))) => u8::try_from(*mask).ok(),
            _ => None,
        })
}

pub struct TargetViewInput<'a> {
    pub program: &'a Program,
    pub source_files: &'a [SourceFile],
    pub library_char_ranges: &'a [Range<usize>],
    /// Targets that load each file of `source_files` (same order).
    pub file_masks: &'a [u8],
    pub targets: &'a [CfgTarget<'a>],
}

pub struct TargetViews {
    pub program: Program,
    pub source_files: Vec<SourceFile>,
    pub library_char_ranges: Vec<Range<usize>>,
    pub layout: SpanLayout,
    pub full_mask: u8,
}

#[derive(Debug, Clone)]
pub struct TargetViewError {
    pub span: Span,
    pub message: String,
}

/// Placement of the source images of every file.
#[derive(Debug, Clone, Default)]
pub struct SpanLayout {
    /// (original start, original length, relocated start) of each segment.
    segments: Vec<(usize, usize, usize)>,
    replicas: usize,
}

impl SpanLayout {
    fn new(source_files: &[SourceFile], replicas: usize) -> Self {
        let mut boundaries: Vec<usize> = source_files.iter().map(|file| file.start).collect();
        boundaries.sort_unstable();
        boundaries.dedup();
        let end = source_files.iter().map(|file| file.end).max().unwrap_or(0);
        let mut segments = Vec::new();
        let mut cursor = 0usize;
        let mut relocated = 0usize;
        let mut push = |start: usize, stop: usize, relocated: &mut usize| {
            let length = stop.saturating_sub(start);
            segments.push((start, length, *relocated));
            *relocated += length * replicas;
        };
        for boundary in boundaries {
            if boundary > cursor {
                push(cursor, boundary, &mut relocated);
            }
            cursor = boundary;
        }
        // The final segment also absorbs synthetic positions past the text.
        push(cursor, end.max(cursor) + 1, &mut relocated);
        Self { segments, replicas }
    }

    pub fn replicas(&self) -> usize {
        self.replicas
    }

    fn segment_of(&self, position: usize) -> usize {
        self.segments
            .partition_point(|(start, _, _)| *start <= position)
            .saturating_sub(1)
    }

    fn relocate(&self, position: usize, replica: usize) -> usize {
        let Some(&(start, length, relocated)) = self.segments.get(self.segment_of(position)) else {
            return position;
        };
        relocated + replica * length + position.saturating_sub(start)
    }

    fn relocate_span(&self, span: Span, replica: usize) -> Span {
        let start = self.relocate(span.start, replica);
        Span {
            start,
            end: start + span.end.saturating_sub(span.start),
            ..span
        }
    }

    /// The source span a relocated span was produced from. Spans past every
    /// image (compiler-generated declarations) are returned unchanged.
    pub fn original(&self, span: Span) -> Span {
        let end = self.segments.last().map_or(0, |(_, length, relocated)| {
            relocated + length * self.replicas
        });
        if span.start >= end {
            return span;
        }
        let index = self
            .segments
            .partition_point(|(_, _, relocated)| *relocated <= span.start)
            .saturating_sub(1);
        let Some(&(start, length, relocated)) = self.segments.get(index) else {
            return span;
        };
        let offset = span.start - relocated;
        let original_start = start + if length == 0 { 0 } else { offset % length };
        Span {
            start: original_start,
            end: original_start + span.end.saturating_sub(span.start),
            ..span
        }
    }
}

struct Relocate<'a> {
    layout: &'a SpanLayout,
    replica: usize,
}

impl MutVisitor for Relocate<'_> {
    fn visit_span(&mut self, span: &mut Span) {
        *span = self.layout.relocate_span(*span, self.replica);
    }
}

// ── Declarations ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Slot {
    Top(usize),
    Method(usize, usize),
}

#[derive(Debug)]
struct Decl {
    slot: Slot,
    avail: u8,
    key: Option<String>,
    module: String,
    name: String,
    kind: DeclKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DeclKind {
    Fn,
    Method,
    Const,
    Alias,
    Type,
    Other,
}

struct Builder<'a> {
    input: &'a TargetViewInput<'a>,
    full: u8,
    errors: Vec<TargetViewError>,
}

fn zero_spans(item: &mut Item) {
    struct Zero;
    impl MutVisitor for Zero {
        fn visit_span(&mut self, span: &mut Span) {
            *span = Span::new(0, 0, 0, 0);
        }
    }
    Zero.visit_item(item);
}

fn without_cfg(attributes: &[Attribute]) -> Vec<Attribute> {
    attributes
        .iter()
        .filter(|attribute| attribute.name != "cfg")
        .cloned()
        .collect()
}

fn strip_item_cfg(node: &mut ItemKind) {
    let attributes = match node {
        ItemKind::Fn { attributes, .. }
        | ItemKind::Struct { attributes, .. }
        | ItemKind::Trait { attributes, .. }
        | ItemKind::Enum { attributes, .. }
        | ItemKind::TypeAlias { attributes, .. }
        | ItemKind::ForeignGlobal { attributes, .. }
        | ItemKind::Const { attributes, .. } => attributes,
        ItemKind::Import(import) => &mut import.attributes,
        ItemKind::Impl { .. } => return,
    };
    *attributes = without_cfg(attributes);
}

/// Span-insensitive text of a declaration; `signature_only` drops function bodies.
fn fingerprint(item: &Item, signature_only: bool) -> String {
    let mut item = item.clone();
    strip_item_cfg(&mut item.node);
    if signature_only && let ItemKind::Fn { body, .. } = &mut item.node {
        *body = None;
    }
    zero_spans(&mut item);
    format!("{:?}", item.node)
}

fn item_attributes(node: &ItemKind) -> &[Attribute] {
    match node {
        ItemKind::Fn { attributes, .. }
        | ItemKind::Struct { attributes, .. }
        | ItemKind::Trait { attributes, .. }
        | ItemKind::Enum { attributes, .. }
        | ItemKind::TypeAlias { attributes, .. }
        | ItemKind::ForeignGlobal { attributes, .. }
        | ItemKind::Const { attributes, .. } => attributes,
        ItemKind::Import(import) => &import.attributes,
        ItemKind::Impl { .. } => &[],
    }
}

/// Every identifier-like spelling in a syntax tree. It over-approximates the
/// declarations an item can depend on; that only makes sharing coarser.
#[derive(Default)]
struct Names {
    all: BTreeSet<String>,
}

impl Names {
    fn add_path(&mut self, path: &str) {
        for segment in path.split('.') {
            self.all.insert(segment.to_string());
        }
    }
}

impl MutVisitor for Names {
    fn visit_expr(&mut self, expr: &mut Expr) {
        match &expr.node {
            ExprKind::Ident(name) => self.add_path(name),
            ExprKind::Field { name, .. } => self.add_path(name),
            ExprKind::MethodCall { method, .. } => self.add_path(method),
            ExprKind::StructInit { name, .. } => self.add_path(name),
            _ => {}
        }
        visit::walk_expr(self, expr);
    }

    fn visit_type(&mut self, ty: &mut Type) {
        match &ty.node {
            TypeKind::Named { name, .. } => self.add_path(name),
            TypeKind::Dyn { trait_name } => self.add_path(trait_name),
            _ => {}
        }
        visit::walk_type(self, ty);
    }

    fn visit_pattern(&mut self, pattern: &mut Pattern) {
        match &pattern.node {
            PatternKind::Variant { enum_name, .. } => {
                if let Some(name) = enum_name {
                    self.add_path(name);
                }
            }
            PatternKind::Struct { name, .. } => self.add_path(name),
            _ => {}
        }
        visit::walk_pattern(self, pattern);
    }
}

fn collect_names(item: &Item) -> Names {
    let mut names = Names::default();
    names.visit_item(&mut item.clone());
    names
}

/// The names a declaration's interface (not its body) mentions.
fn signature_names(item: &Item) -> BTreeSet<String> {
    let mut item = item.clone();
    if let ItemKind::Fn { body, .. } = &mut item.node {
        *body = None;
    }
    collect_names(&item).all
}

/// Masks of the `@cfg` statements inside an item.
struct CfgMasks<'a, 'b> {
    builder: &'a Builder<'b>,
    masks: Vec<u8>,
}

impl MutVisitor for CfgMasks<'_, '_> {
    fn visit_stmt(&mut self, stmt: &mut Stmt) {
        if let StmtKind::CfgBlock { condition, .. } = &stmt.node {
            self.masks
                .push(self.builder.cfg_mask(std::slice::from_ref(condition)));
        }
        visit::walk_stmt(self, stmt);
    }
}

/// Whether a module identity (logical name or file path) is named by `qualifier`.
fn module_matches(module: &str, qualifier: &str) -> bool {
    let stem = std::path::Path::new(module)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    stem == qualifier || module.rsplit('.').next() == Some(qualifier)
}

/// Value names a file imports by selector, with the module they come from.
fn imported_values(
    program: &Program,
    builder: &Builder<'_>,
) -> HashMap<usize, HashMap<String, String>> {
    let mut files: HashMap<usize, HashMap<String, String>> = HashMap::new();
    for item in &program.items {
        let ItemKind::Import(import) = &item.node else {
            continue;
        };
        let (Some(file), Some(module)) = (builder.file_of(item.span), import.path.last()) else {
            continue;
        };
        let names: Vec<(String, String)> = match &import.items {
            ImportItems::Single(name) => vec![(name.clone(), name.clone())],
            ImportItems::Multiple(names) => names
                .iter()
                .map(|name| (name.clone(), name.clone()))
                .collect(),
            ImportItems::Aliased(name, alias) => vec![(alias.clone(), name.clone())],
            ImportItems::All => Vec::new(),
        };
        let entry = files.entry(file).or_default();
        for (local, _) in names {
            entry.insert(local, module.clone());
        }
    }
    files
}

fn refine(avail: u8, cuts: impl IntoIterator<Item = u8>) -> Vec<u8> {
    let mut classes = vec![avail];
    for cut in cuts {
        classes = classes
            .into_iter()
            .flat_map(|class| [class & cut, class & !cut])
            .filter(|class| *class != 0)
            .collect();
    }
    classes.sort_unstable_by_key(|class| class.trailing_zeros());
    classes
}

/// A target-dependent alias: definitions per target class.
#[derive(Clone)]
struct AliasVariants {
    module: String,
    variants: Vec<(u8, TypeKind)>,
}

/// Copies of a value declaration (function or constant) per target class.
#[derive(Clone)]
struct ValueCopies {
    module: String,
    copies: Vec<(u8, String)>,
    /// Copies have different signatures: callers must name their own copy.
    target_dependent: bool,
}

impl<'a> Builder<'a> {
    fn cfg_mask(&self, attributes: &[Attribute]) -> u8 {
        if !attributes.iter().any(|attribute| attribute.name == "cfg") {
            return self.full;
        }
        self.input
            .targets
            .iter()
            .enumerate()
            .fold(0, |mask, (bit, target)| {
                if item_should_include_for(attributes, target.os, target.arch, target.abi) {
                    mask | (1 << bit)
                } else {
                    mask
                }
            })
    }

    fn file_of(&self, span: Span) -> Option<usize> {
        let files = self.input.source_files;
        let index = files
            .partition_point(|file| file.start <= span.start)
            .checked_sub(1)?;
        files[index].contains(span).then_some(index)
    }

    fn module_of(&self, file: Option<usize>) -> String {
        file.map(|index| {
            let source = &self.input.source_files[index];
            source
                .module_name
                .clone()
                .unwrap_or_else(|| source.path.clone())
        })
        .unwrap_or_default()
    }

    fn file_mask(&self, file: Option<usize>) -> u8 {
        file.and_then(|index| self.input.file_masks.get(index).copied())
            .unwrap_or(self.full)
    }

    fn item(&self, slot: Slot) -> &'a Item {
        match slot {
            Slot::Top(index) => &self.input.program.items[index],
            Slot::Method(index, method) => match &self.input.program.items[index].node {
                ItemKind::Impl { methods, .. } => &methods[method],
                _ => unreachable!("method slot outside an impl"),
            },
        }
    }

    fn error(&mut self, span: Span, message: String) {
        self.errors.push(TargetViewError { span, message });
    }

    fn collect_decls(&mut self) -> Vec<Decl> {
        let mut decls = Vec::new();
        for (index, item) in self.input.program.items.iter().enumerate() {
            let file = self.file_of(item.span);
            let module = self.module_of(file);
            let avail = self.file_mask(file) & self.cfg_mask(item_attributes(&item.node));
            if avail == 0 {
                continue;
            }
            let (name, kind) = match &item.node {
                ItemKind::Fn { name, .. } => (Some(name.clone()), DeclKind::Fn),
                ItemKind::Const { name, .. } | ItemKind::ForeignGlobal { name, .. } => {
                    (Some(name.clone()), DeclKind::Const)
                }
                ItemKind::TypeAlias { name, .. } => (Some(name.clone()), DeclKind::Alias),
                ItemKind::Struct { name, .. }
                | ItemKind::Enum { name, .. }
                | ItemKind::Trait { name, .. } => (Some(name.clone()), DeclKind::Type),
                ItemKind::Impl {
                    trait_ty,
                    for_ty,
                    methods,
                } => {
                    decls.push(Decl {
                        slot: Slot::Top(index),
                        avail,
                        key: None,
                        module: module.clone(),
                        name: String::new(),
                        kind: DeclKind::Other,
                    });
                    let owner = format!(
                        "impl {} for {}",
                        trait_ty
                            .as_ref()
                            .map(|ty| ty.node.to_string())
                            .unwrap_or_default(),
                        for_ty.node
                    );
                    for (method_index, method) in methods.iter().enumerate() {
                        let ItemKind::Fn {
                            name, attributes, ..
                        } = &method.node
                        else {
                            continue;
                        };
                        let method_avail = avail & self.cfg_mask(attributes);
                        if method_avail == 0 {
                            continue;
                        }
                        decls.push(Decl {
                            slot: Slot::Method(index, method_index),
                            avail: method_avail,
                            key: Some(format!("{module}\0{owner}\0{name}")),
                            module: module.clone(),
                            name: name.clone(),
                            kind: DeclKind::Method,
                        });
                    }
                    continue;
                }
                ItemKind::Import(_) => (None, DeclKind::Other),
            };
            decls.push(Decl {
                slot: Slot::Top(index),
                avail,
                key: name.as_ref().map(|name| format!("{module}\0{name}")),
                module,
                name: name.unwrap_or_default(),
                kind,
            });
        }
        decls
    }
}

/// Merge identical conditional declarations and group the rest by name.
fn group_decls(decls: Vec<Decl>) -> (Vec<Decl>, HashMap<String, Vec<usize>>, Vec<String>) {
    let mut kept: Vec<Decl> = Vec::new();
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    let mut order = Vec::new();
    for decl in decls {
        let Some(key) = decl.key.clone() else {
            kept.push(decl);
            continue;
        };
        let members = groups.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            Vec::new()
        });
        members.push(kept.len());
        kept.push(decl);
    }
    (kept, groups, order)
}

pub fn build_target_views(
    input: &TargetViewInput<'_>,
) -> Result<TargetViews, Vec<TargetViewError>> {
    let count = input.targets.len();
    assert!(
        (1..=8).contains(&count),
        "between one and eight QZI targets"
    );
    let full = (u16::from(u8::MAX) >> (8 - count)) as u8;
    let mut builder = Builder {
        input,
        full,
        errors: Vec::new(),
    };
    let (mut decls, mut groups, order) = group_decls(builder.collect_decls());

    // Identical conditional declarations are one declaration on their union.
    let mut removed = HashSet::new();
    for key in &order {
        let members = groups[key].clone();
        let mut distinct: Vec<(usize, String)> = Vec::new();
        for member in members {
            let text = fingerprint(builder.item(decls[member].slot), false);
            if let Some((first, _)) = distinct.iter().find(|(_, known)| *known == text) {
                let avail = decls[member].avail;
                decls[*first].avail |= avail;
                removed.insert(member);
            } else {
                distinct.push((member, text));
            }
        }
        groups.insert(
            key.clone(),
            distinct.into_iter().map(|(member, _)| member).collect(),
        );
    }

    // Overlapping distinct declarations are ambiguous on the shared targets.
    for key in &order {
        let members = &groups[key];
        let mut seen = 0u8;
        for &member in members {
            if seen & decls[member].avail != 0 {
                let span = builder.item(decls[member].slot).span;
                let name = decls[member].name.clone();
                builder.error(
                    span,
                    format!("`{name}` is declared more than once for the same target"),
                );
            }
            seen |= decls[member].avail;
        }
    }
    if !builder.errors.is_empty() {
        return Err(builder.errors);
    }

    // Target-dependent aliases are substituted per target class.
    let mut aliases: HashMap<String, Vec<AliasVariants>> = HashMap::new();
    for key in &order {
        let members = &groups[key];
        if members.len() < 2 || decls[members[0]].kind != DeclKind::Alias {
            continue;
        }
        let mut variants = Vec::new();
        for &member in members {
            let item = builder.item(decls[member].slot);
            let ItemKind::TypeAlias {
                generic_params,
                aliased_type,
                attributes,
                ..
            } = &item.node
            else {
                unreachable!();
            };
            if !generic_params.is_empty() || !self_contained_type(&aliased_type.node) {
                builder.error(
                    item.span,
                    format!(
                        "type alias `{}` differs between targets; a QZI supports this only for non-generic aliases of built-in types",
                        decls[member].name
                    ),
                );
                continue;
            }
            let repr_c = attributes.iter().any(|attribute| {
                attribute.name == "repr"
                    && matches!(attribute.args.first(), Some(AttrArg::Positional(AttrVal::Ident(value))) if value == "C")
            });
            let body = match (&aliased_type.node, repr_c) {
                (TypeKind::Fn { params, return_ty }, true) => TypeKind::CFn {
                    params: params.clone(),
                    return_ty: return_ty.clone(),
                },
                (other, _) => other.clone(),
            };
            variants.push((decls[member].avail, body));
        }
        let name = decls[members[0]].name.clone();
        aliases.entry(name).or_default().push(AliasVariants {
            module: decls[members[0]].module.clone(),
            variants,
        });
        // The first variant stays declared so imports of the name resolve.
        for &member in &members[1..] {
            removed.insert(member);
        }
    }
    if !builder.errors.is_empty() {
        return Err(builder.errors);
    }

    // Per declaration: names, cfg statement masks and target classes.
    let names: Vec<Names> = decls
        .iter()
        .map(|decl| collect_names(builder.item(decl.slot)))
        .collect();
    let sig_names: Vec<BTreeSet<String>> = decls
        .iter()
        .map(|decl| signature_names(builder.item(decl.slot)))
        .collect();
    let cfg_masks: Vec<Vec<u8>> = decls
        .iter()
        .map(|decl| {
            let mut masks = CfgMasks {
                builder: &builder,
                masks: Vec::new(),
            };
            masks.visit_item(&mut builder.item(decl.slot).clone());
            masks.masks
        })
        .collect();
    let alias_cuts = |names: &BTreeSet<String>| -> Vec<u8> {
        names
            .iter()
            .filter_map(|name| aliases.get(name))
            .flatten()
            .flat_map(|alias| alias.variants.iter().map(|(mask, _)| *mask))
            .collect()
    };

    let is_value =
        |kind: DeclKind| matches!(kind, DeclKind::Fn | DeclKind::Const | DeclKind::Method);
    let live_members = |key: &String| -> Vec<usize> {
        groups[key]
            .iter()
            .copied()
            .filter(|member| !removed.contains(member))
            .collect()
    };
    let mut classes: Vec<Vec<u8>> = decls.iter().map(|decl| vec![decl.avail]).collect();
    // Copy names per declaration group, and an index of top-level values by
    // their source name for rewriting references.
    let mut copies_by_key: HashMap<String, (Vec<(u8, String)>, bool)> = HashMap::new();
    let mut values: HashMap<String, Vec<ValueCopies>> = HashMap::new();
    for _round in 0..64 {
        let mut changed = false;
        for (index, decl) in decls.iter().enumerate() {
            if removed.contains(&index) || decl.kind == DeclKind::Other {
                continue;
            }
            let mut cuts = cfg_masks[index].clone();
            cuts.extend(alias_cuts(&names[index].all));
            for name in &names[index].all {
                for group in values.get(name).into_iter().flatten() {
                    if group.target_dependent {
                        cuts.extend(group.copies.iter().map(|(mask, _)| *mask));
                    }
                }
            }
            let refined = refine(decl.avail, cuts);
            if refined != classes[index] {
                classes[index] = refined;
                changed = true;
            }
        }
        let mut next_copies: HashMap<String, (Vec<(u8, String)>, bool)> = HashMap::new();
        let mut next_values: HashMap<String, Vec<ValueCopies>> = HashMap::new();
        for key in &order {
            let members = live_members(key);
            let Some(&first) = members.first() else {
                continue;
            };
            if !is_value(decls[first].kind) {
                continue;
            }
            // Copies whose signatures differ need callers of their own class.
            let distinct_signatures = members
                .iter()
                .map(|member| fingerprint(builder.item(decls[*member].slot), true))
                .collect::<HashSet<_>>()
                .len();
            let split_signature = members.iter().any(|member| {
                refine(decls[*member].avail, alias_cuts(&sig_names[*member])).len() > 1
                    || sig_names[*member].iter().any(|name| {
                        values
                            .get(name)
                            .is_some_and(|groups| groups.iter().any(|group| group.target_dependent))
                    })
            });
            let target_dependent = distinct_signatures > 1 || split_signature;
            let mut copies = Vec::new();
            for &member in &members {
                for &class in &classes[member] {
                    let name = if copies.is_empty() {
                        decls[first].name.clone()
                    } else {
                        format!("{}{VARIANT_MARKER}{class}", decls[first].name)
                    };
                    copies.push((class, name));
                }
            }
            if decls[first].kind != DeclKind::Method {
                next_values
                    .entry(decls[first].name.clone())
                    .or_default()
                    .push(ValueCopies {
                        module: decls[first].module.clone(),
                        copies: copies.clone(),
                        target_dependent,
                    });
            }
            next_copies.insert(key.clone(), (copies, target_dependent));
        }
        let stable = !changed && next_copies == copies_by_key;
        copies_by_key = next_copies;
        values = next_values;
        if stable {
            break;
        }
    }

    // Types whose meaning differs between targets are not supported yet, nor
    // are methods whose signature differs (their calls are resolved by type).
    for (index, decl) in decls.iter().enumerate() {
        if removed.contains(&index) {
            continue;
        }
        let is_type = decl.kind == DeclKind::Type
            || (decl.kind == DeclKind::Alias && !aliases.contains_key(&decl.name));
        let multiple = decl
            .key
            .as_ref()
            .is_some_and(|key| live_members(key).len() > 1);
        let method_signature = decl.kind == DeclKind::Method
            && decl
                .key
                .as_ref()
                .and_then(|key| copies_by_key.get(key))
                .is_some_and(|(_, target_dependent)| *target_dependent);
        if (is_type && (classes[index].len() > 1 || multiple)) || method_signature {
            let span = builder.item(decl.slot).span;
            builder.error(
                span,
                format!(
                    "`{}` has a different signature or definition on some targets; a QZI supports target-specific functions, constants and built-in type aliases, but not target-specific types or method signatures",
                    decl.name
                ),
            );
        }
    }
    if !builder.errors.is_empty() {
        return Err(builder.errors);
    }

    // Emit copies.
    let value_name = |decl: &Decl, class: u8| -> String {
        decl.key
            .as_ref()
            .and_then(|key| copies_by_key.get(key))
            .and_then(|(copies, _)| {
                copies
                    .iter()
                    .find(|(mask, _)| *mask == class)
                    .map(|(_, name)| name.clone())
            })
            .unwrap_or_else(|| decl.name.clone())
    };
    let imports = imported_values(input.program, &builder);
    let mut emitted: Vec<(Item, usize, Vec<(Item, usize)>)> = Vec::new();
    let mut impl_slots: HashMap<usize, usize> = HashMap::new();
    let mut variant_impls: HashMap<usize, usize> = HashMap::new();
    for (index, decl) in decls.iter().enumerate() {
        if removed.contains(&index) {
            continue;
        }
        let source = builder.item(decl.slot);
        let file_imports = builder
            .file_of(source.span)
            .and_then(|file| imports.get(&file));
        for (replica, &class) in classes[index].iter().enumerate() {
            let mut copy = source.clone();
            let mut rewrite = Rewrite {
                builder: &builder,
                class,
                module: &decl.module,
                aliases: &aliases,
                values: &values,
                imports: file_imports,
                scopes: Vec::new(),
            };
            rewrite.visit_item(&mut copy);
            strip_item_cfg(&mut copy.node);
            if is_value(decl.kind) {
                let name = value_name(decl, class);
                rename(&mut copy.node, name);
            }
            if class != full {
                mark_targets(&mut copy.node, class);
            }
            if let ItemKind::Import(import) = &mut copy.node {
                expand_import(import, &values);
            }
            match decl.slot {
                Slot::Top(item_index) => {
                    if let ItemKind::Impl { methods, .. } = &mut copy.node {
                        methods.clear();
                        impl_slots.insert(item_index, emitted.len());
                    }
                    emitted.push((copy, replica, Vec::new()));
                }
                Slot::Method(item_index, _) => {
                    let ItemKind::Impl {
                        trait_ty, for_ty, ..
                    } = &builder.input.program.items[item_index].node
                    else {
                        unreachable!();
                    };
                    let renamed = matches!(&copy.node, ItemKind::Fn { name, .. } if name.contains(VARIANT_MARKER));
                    let container = if trait_ty.is_some() && renamed {
                        // Extra copies of trait methods are not trait members.
                        *variant_impls.entry(item_index).or_insert_with(|| {
                            let original = &builder.input.program.items[item_index];
                            let mut header = Spanned::new(
                                ItemKind::Impl {
                                    trait_ty: None,
                                    for_ty: for_ty.clone(),
                                    methods: Vec::new(),
                                },
                                original.span,
                            );
                            let mut rewrite = Rewrite {
                                builder: &builder,
                                class: full,
                                module: &decl.module,
                                aliases: &aliases,
                                values: &values,
                                imports: None,
                                scopes: Vec::new(),
                            };
                            rewrite.visit_item(&mut header);
                            emitted.push((header, 1, Vec::new()));
                            emitted.len() - 1
                        })
                    } else {
                        impl_slots[&item_index]
                    };
                    emitted[container].2.push((copy, replica));
                }
            }
        }
    }

    let replicas = emitted
        .iter()
        .flat_map(|(_, replica, methods)| {
            std::iter::once(*replica).chain(methods.iter().map(|(_, replica)| *replica))
        })
        .max()
        .unwrap_or(0)
        + 1;
    let layout = SpanLayout::new(input.source_files, replicas);
    let mut items = Vec::with_capacity(emitted.len());
    for (mut item, replica, methods) in emitted {
        Relocate {
            layout: &layout,
            replica,
        }
        .visit_item(&mut item);
        if let ItemKind::Impl { methods: slot, .. } = &mut item.node {
            for (mut method, method_replica) in methods {
                Relocate {
                    layout: &layout,
                    replica: method_replica,
                }
                .visit_item(&mut method);
                slot.push(method);
            }
        }
        items.push(item);
    }

    let source_files = input
        .source_files
        .iter()
        .map(|file| {
            let mut relocated = file.clone();
            relocated.start = layout.relocate(file.start, 0);
            relocated.end = layout.relocate(file.end, replicas - 1);
            relocated.type_imports = Vec::new();
            for replica in 0..replicas {
                for import in &file.type_imports {
                    let mut import = import.clone();
                    import.selector_span = layout.relocate_span(import.selector_span, replica);
                    relocated.type_imports.push(import);
                }
            }
            relocated
        })
        .collect();
    let library_char_ranges = input
        .library_char_ranges
        .iter()
        .map(|range| layout.relocate(range.start, 0)..layout.relocate(range.end, replicas - 1))
        .collect();
    Ok(TargetViews {
        program: Program {
            items,
            span: input.program.span,
        },
        source_files,
        library_char_ranges,
        layout,
        full_mask: full,
    })
}

/// Built-in types only: target alias bodies are substituted into other modules.
fn self_contained_type(ty: &TypeKind) -> bool {
    match ty {
        TypeKind::Named { .. } | TypeKind::Dyn { .. } | TypeKind::Any | TypeKind::Error => false,
        TypeKind::Array { elem_ty, .. }
        | TypeKind::FlexibleArray { elem_ty }
        | TypeKind::Slice { elem_ty } => self_contained_type(&elem_ty.node),
        TypeKind::Ref { inner } | TypeKind::MutRef { inner } | TypeKind::RawPtr { inner, .. } => {
            self_contained_type(&inner.node)
        }
        TypeKind::Fn { params, return_ty } | TypeKind::CFn { params, return_ty } => {
            params.iter().all(|param| self_contained_type(&param.node))
                && self_contained_type(&return_ty.node)
        }
        _ => true,
    }
}

fn rename(node: &mut ItemKind, new_name: String) {
    match node {
        ItemKind::Fn { name, .. }
        | ItemKind::Const { name, .. }
        | ItemKind::ForeignGlobal { name, .. } => *name = new_name,
        _ => {}
    }
}

fn mark_targets(node: &mut ItemKind, mask: u8) {
    let attributes = match node {
        ItemKind::Fn { attributes, .. }
        | ItemKind::Const { attributes, .. }
        | ItemKind::ForeignGlobal { attributes, .. }
        | ItemKind::Struct { attributes, .. }
        | ItemKind::Enum { attributes, .. }
        | ItemKind::Trait { attributes, .. }
        | ItemKind::TypeAlias { attributes, .. } => attributes,
        ItemKind::Import(_) | ItemKind::Impl { .. } => return,
    };
    attributes.push(Attribute {
        name: TARGETS_ATTRIBUTE.to_string(),
        args: vec![AttrArg::Positional(AttrVal::Int(u64::from(mask)))],
        span: Span::new(0, 0, 0, 0),
    });
}

/// Import every copy of a target-dependent value together with its name.
fn expand_import(import: &mut ImportPath, values: &HashMap<String, Vec<ValueCopies>>) {
    let Some(module) = import.path.last().cloned() else {
        return;
    };
    let extra = |name: &str| -> Vec<String> {
        values
            .get(name)
            .into_iter()
            .flatten()
            .filter(|group| group.target_dependent && module_matches(&group.module, &module))
            .flat_map(|group| group.copies.iter().skip(1).map(|(_, copy)| copy.clone()))
            .collect()
    };
    let span = import.selector_spans.first().copied();
    let names = match &import.items {
        ImportItems::Single(name) => vec![name.clone()],
        ImportItems::Multiple(names) => names.clone(),
        _ => return,
    };
    let mut all = names.clone();
    for name in &names {
        all.extend(extra(name));
    }
    if all.len() == names.len() {
        return;
    }
    while import.selector_spans.len() < all.len() {
        import.selector_spans.push(span.unwrap_or(import.span));
    }
    import.items = ImportItems::Multiple(all);
}

/// Specializes one copy of an item for a target class.
struct Rewrite<'a, 'b> {
    builder: &'a Builder<'b>,
    class: u8,
    module: &'a str,
    aliases: &'a HashMap<String, Vec<AliasVariants>>,
    values: &'a HashMap<String, Vec<ValueCopies>>,
    /// Values the item's file imports by selector: name -> module.
    imports: Option<&'a HashMap<String, String>>,
    /// Lexical scopes of local bindings, innermost last.
    scopes: Vec<HashSet<String>>,
}

impl Rewrite<'_, '_> {
    fn is_local(&self, name: &str) -> bool {
        self.scopes.iter().any(|scope| scope.contains(name))
    }

    fn bind(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string());
        }
    }

    /// The declaration group a reference names: a qualified reference must
    /// name the group's module; an unqualified one is imported by selector
    /// or declared in the referencing module.
    fn pick<'g, T>(
        &self,
        groups: &'g [T],
        module_of: impl Fn(&T) -> &str,
        leaf: &str,
        qualifier: Option<&str>,
    ) -> Option<&'g T> {
        match qualifier {
            Some(qualifier) => groups
                .iter()
                .find(|group| module_matches(module_of(group), qualifier)),
            None => match self.imports.and_then(|imports| imports.get(leaf)) {
                Some(module) => groups
                    .iter()
                    .find(|group| module_matches(module_of(group), module)),
                None => groups.iter().find(|group| module_of(group) == self.module),
            },
        }
    }

    fn alias_type(&self, path: &str) -> Option<TypeKind> {
        let (qualifier, leaf) = match path.rsplit_once('.') {
            Some((qualifier, leaf)) => (
                Some(qualifier.rsplit('.').next().unwrap_or(qualifier)),
                leaf,
            ),
            None => (None, path),
        };
        let groups = self.aliases.get(leaf)?;
        let group = self.pick(groups, |group| group.module.as_str(), leaf, qualifier)?;
        group
            .variants
            .iter()
            .find(|(mask, _)| mask & self.class == self.class)
            .map(|(_, ty)| ty.clone())
    }

    fn value_name(&self, leaf: &str, qualifier: Option<&str>) -> Option<String> {
        let groups = self.values.get(leaf)?;
        let group = self.pick(groups, |group| group.module.as_str(), leaf, qualifier)?;
        if !group.target_dependent {
            return None;
        }
        group
            .copies
            .iter()
            .find(|(mask, _)| mask & self.class == self.class)
            .map(|(_, name)| name.clone())
            .filter(|name| name != leaf)
    }

    /// Resolve `@cfg` statements for the class; their bodies share the scope.
    fn rewrite_stmts(&mut self, stmts: Vec<Stmt>, out: &mut Vec<Stmt>) {
        for stmt in stmts {
            match stmt.node {
                StmtKind::CfgBlock { condition, body } => {
                    let mask = self.builder.cfg_mask(std::slice::from_ref(&condition));
                    if mask & self.class == self.class {
                        self.rewrite_stmts(body.stmts, out);
                    }
                }
                node => {
                    let mut stmt = Spanned::new(node, stmt.span);
                    self.visit_stmt(&mut stmt);
                    out.push(stmt);
                }
            }
        }
    }
}

impl MutVisitor for Rewrite<'_, '_> {
    fn visit_item(&mut self, item: &mut Item) {
        if let ItemKind::Fn { params, .. } = &item.node {
            self.scopes
                .push(params.iter().map(|param| param.name.clone()).collect());
            visit::walk_item(self, item);
            self.scopes.pop();
        } else {
            visit::walk_item(self, item);
        }
    }

    fn visit_block(&mut self, block: &mut Block) {
        self.scopes.push(HashSet::new());
        let mut stmts = Vec::with_capacity(block.stmts.len());
        self.rewrite_stmts(std::mem::take(&mut block.stmts), &mut stmts);
        block.stmts = stmts;
        self.scopes.pop();
    }

    fn visit_stmt(&mut self, stmt: &mut Stmt) {
        match &mut stmt.node {
            StmtKind::Var { name, .. } | StmtKind::Const { name, .. } => {
                let name = name.clone();
                visit::walk_stmt(self, stmt);
                self.bind(&name);
            }
            StmtKind::For {
                kind: ForLoop::Each { vars, iter, .. },
                body,
            } => {
                match iter {
                    ForIter::Range { start, end, .. } => {
                        self.visit_expr(start);
                        self.visit_expr(end);
                    }
                    ForIter::Iter(expr) => self.visit_expr(expr),
                }
                self.scopes.push(vars.iter().cloned().collect());
                self.visit_block(body);
                self.scopes.pop();
            }
            StmtKind::For {
                kind: ForLoop::CStyle { .. },
                ..
            } => {
                // The initializer's binding is visible in the whole loop.
                self.scopes.push(HashSet::new());
                visit::walk_stmt(self, stmt);
                self.scopes.pop();
            }
            _ => visit::walk_stmt(self, stmt),
        }
    }

    fn visit_type(&mut self, ty: &mut Type) {
        if let TypeKind::Named { name, type_args } = &ty.node
            && type_args.is_empty()
            && let Some(replacement) = self.alias_type(name)
        {
            ty.node = replacement;
        }
        visit::walk_type(self, ty);
    }

    fn visit_expr(&mut self, expr: &mut Expr) {
        match &mut expr.node {
            ExprKind::Ident(name) if !self.is_local(name) => {
                if let Some(renamed) = self.value_name(name, None) {
                    *name = renamed;
                }
            }
            // `module.value` and `module.function(...)`.
            ExprKind::Field { object, name }
            | ExprKind::MethodCall {
                object,
                method: name,
                ..
            } => {
                if let ExprKind::Ident(qualifier) = &object.node
                    && !self.is_local(qualifier)
                    && let Some(renamed) = self.value_name(name, Some(qualifier))
                {
                    *name = renamed;
                }
            }
            ExprKind::Closure { params, body, .. } => {
                self.scopes.push(params.iter().cloned().collect());
                self.visit_expr(body);
                self.scopes.pop();
                self.visit_span(&mut expr.span);
                return;
            }
            ExprKind::Match { scrutinee, arms } => {
                self.visit_expr(scrutinee);
                for arm in arms {
                    self.visit_pattern(&mut arm.pattern);
                    self.scopes
                        .push(pattern_all_bindings(&arm.pattern).into_iter().collect());
                    if let Some(guard) = &mut arm.guard {
                        self.visit_expr(guard);
                    }
                    self.visit_expr(&mut arm.expr);
                    self.scopes.pop();
                }
                return;
            }
            _ => {}
        }
        visit::walk_expr(self, expr);
    }
}

/// The declarations target `bit` sees, under their source names. Used to
/// render a library's public interface for that target.
pub fn program_for_target(program: &Program, layout: &SpanLayout, bit: u8) -> Program {
    let target = 1u8 << bit;
    let visible = |attributes: &[Attribute]| {
        declared_targets(attributes).is_none_or(|mask| mask & target != 0)
    };
    let restore = |item: &Item| -> Item {
        let mut item = item.clone();
        match &mut item.node {
            ItemKind::Fn {
                name, attributes, ..
            }
            | ItemKind::Const {
                name, attributes, ..
            }
            | ItemKind::ForeignGlobal {
                name, attributes, ..
            } => {
                *name = strip_variant_markers(name);
                attributes.retain(|attribute| attribute.name != TARGETS_ATTRIBUTE);
            }
            _ => {}
        }
        item
    };
    // Extra copies of trait methods live in an inherent impl placed at the
    // trait impl's source position; return them to their trait impl.
    let mut trait_impls: HashMap<usize, usize> = HashMap::new();
    let mut items: Vec<Item> = Vec::new();
    for item in &program.items {
        if !visible(item_attributes(&item.node)) {
            continue;
        }
        let ItemKind::Impl {
            trait_ty,
            for_ty,
            methods,
        } = &item.node
        else {
            items.push(restore(item));
            continue;
        };
        let methods: Vec<Item> = methods
            .iter()
            .filter(|method| visible(item_attributes(&method.node)))
            .map(restore)
            .collect();
        let origin = layout.original(item.span).start;
        if trait_ty.is_none()
            && let Some(&index) = trait_impls.get(&origin)
        {
            if let ItemKind::Impl {
                methods: target, ..
            } = &mut items[index].node
            {
                target.extend(methods);
            }
            continue;
        }
        if trait_ty.is_some() {
            trait_impls.insert(origin, items.len());
        }
        items.push(Spanned::new(
            ItemKind::Impl {
                trait_ty: trait_ty.clone(),
                for_ty: for_ty.clone(),
                methods,
            },
            item.span,
        ));
    }
    Program {
        items,
        span: program.span,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    const TARGETS: [CfgTarget<'static>; 3] = [
        CfgTarget {
            os: "linux",
            arch: "x86_64",
            abi: "sysv",
        },
        CfgTarget {
            os: "windows",
            arch: "x86_64",
            abi: "win64",
        },
        CfgTarget {
            os: "netbsd",
            arch: "x86_64",
            abi: "sysv",
        },
    ];

    fn views(source: &str) -> Result<TargetViews, Vec<TargetViewError>> {
        let program = Parser::new(Lexer::new(source).tokenize())
            .parse()
            .expect("source must parse");
        let files = [SourceFile {
            path: "main.qz".into(),
            module_name: None,
            implicit_prelude: false,
            runtime_string: None,
            runtime_panic: None,
            type_imports: Vec::new(),
            start: 0,
            end: source.chars().count(),
            line_start: 1,
        }];
        build_target_views(&TargetViewInput {
            program: &program,
            source_files: &files,
            library_char_ranges: &[],
            file_masks: &[0b111],
            targets: &TARGETS,
        })
    }

    /// One program from several files: (path, source) in load order.
    fn views_of_files(files: &[(&str, &str)]) -> TargetViews {
        let mut merged = String::new();
        let mut sources = Vec::new();
        for (path, source) in files {
            let start = merged.chars().count();
            let line_start = merged.matches('\n').count() + 1;
            merged.push_str(source);
            sources.push(SourceFile {
                path: path.to_string(),
                module_name: None,
                implicit_prelude: false,
                runtime_string: None,
                runtime_panic: None,
                type_imports: Vec::new(),
                start,
                end: merged.chars().count(),
                line_start,
            });
            merged.push('\n');
        }
        let program = Parser::new(Lexer::new(&merged).tokenize())
            .parse()
            .expect("source must parse");
        build_target_views(&TargetViewInput {
            program: &program,
            source_files: &sources,
            library_char_ranges: &[],
            file_masks: &vec![0b111; sources.len()],
            targets: &TARGETS,
        })
        .unwrap_or_else(|errors| panic!("{}", errors[0].message))
    }

    /// (name, target mask, debug text) of every function, methods included.
    fn functions(views: &TargetViews) -> Vec<(String, Option<u8>, String)> {
        let mut out = Vec::new();
        let mut push = |item: &Item| {
            if let ItemKind::Fn {
                name, attributes, ..
            } = &item.node
            {
                out.push((
                    name.clone(),
                    declared_targets(attributes),
                    format!("{:?}", item.node),
                ));
            }
        };
        for item in &views.program.items {
            push(item);
            if let ItemKind::Impl { methods, .. } = &item.node {
                methods.iter().for_each(&mut push);
            }
        }
        out
    }

    #[test]
    fn conditional_statements_split_only_their_function() {
        let views = views(
            r#"
fn os() i32 {
    @cfg(target_os = "linux") { ret 1; }
    @cfg(target_os = "windows") { ret 2; }
    ret 3;
}
fn shared() i32 { ret os(); }
"#,
        )
        .unwrap();
        let functions = functions(&views);
        let summary: Vec<_> = functions
            .iter()
            .map(|(name, mask, _)| (name.as_str(), *mask))
            .collect();
        assert_eq!(
            summary,
            [
                ("os", Some(0b001)),
                ("os@t2", Some(0b010)),
                ("os@t4", Some(0b100)),
                ("shared", None)
            ]
        );
        assert!(
            functions
                .iter()
                .all(|(_, _, text)| !text.contains("CfgBlock"))
        );
        // Each copy lives in its own image of the file and maps back to it.
        let spans: Vec<Span> = views
            .program
            .items
            .iter()
            .filter(
                |item| matches!(&item.node, ItemKind::Fn { name, .. } if name.starts_with("os")),
            )
            .map(|item| item.span)
            .collect();
        assert_eq!(views.layout.replicas(), 3);
        assert_ne!(spans[0].start, spans[1].start);
        assert_eq!(
            views.layout.original(spans[1]),
            views.layout.original(spans[0])
        );
        assert!(views.source_files[0].contains(spans[2]));
    }

    #[test]
    fn identical_conditional_declarations_become_one() {
        let views = views(
            r#"
@cfg(target_os = "linux") fn word() i32 { ret 8; }
@cfg(target_os = "netbsd") fn word() i32 { ret 8; }
@cfg(target_os = "windows") fn word() i32 { ret 4; }
"#,
        )
        .unwrap();
        let summary: Vec<_> = functions(&views)
            .into_iter()
            .map(|(name, mask, _)| (name, mask))
            .collect();
        assert_eq!(
            summary,
            [
                ("word".to_string(), Some(0b101)),
                ("word@t2".to_string(), Some(0b010))
            ]
        );
    }

    #[test]
    fn target_dependent_aliases_specialize_signatures_and_callers() {
        let views = views(
            r#"
@cfg(target_os = "windows") type Long = i32;
@cfg(target_os = "linux") type Long = i64;
@cfg(target_os = "netbsd") type Long = i64;
fn widen(value: Long) Long { ret value; }
fn main() i32 { widen(1); ret 0; }
"#,
        )
        .unwrap();
        let functions = functions(&views);
        let find = |name: &str| {
            functions
                .iter()
                .find(|(known, _, _)| known == name)
                .unwrap()
        };
        assert_eq!(find("widen").1, Some(0b101));
        assert!(find("widen").2.contains("Int64") && !find("widen").2.contains("Long"));
        assert_eq!(find("widen@t2").1, Some(0b010));
        assert!(find("widen@t2").2.contains("Int32"));
        // Each copy of the caller names the copy of its own target.
        assert!(find("main").2.contains("Ident(\"widen\")"));
        assert!(find("main@t2").2.contains("Ident(\"widen@t2\")"));
    }

    #[test]
    fn conditional_trait_methods_keep_extra_copies_outside_the_trait_impl() {
        let views = views(
            r#"
trait Named { fn name(self: &Self) i32; }
struct Box1 {}
impl Named for Box1 {
    fn name(self: &Box1) i32 {
        @cfg(target_os = "windows") { ret 2; }
        ret 1;
    }
}
"#,
        )
        .unwrap();
        let impls: Vec<_> = views
            .program
            .items
            .iter()
            .filter_map(|item| match &item.node {
                ItemKind::Impl {
                    trait_ty, methods, ..
                } => Some((
                    trait_ty.is_some(),
                    methods
                        .iter()
                        .map(|method| match &method.node {
                            ItemKind::Fn { name, .. } => name.clone(),
                            _ => String::new(),
                        })
                        .collect::<Vec<_>>(),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            impls,
            [
                (true, vec!["name".to_string()]),
                (false, vec!["name@t2".to_string()])
            ]
        );
        // The interface view of Windows sees the trait method under its name.
        let windows = program_for_target(&views.program, &views.layout, 1);
        let trait_methods = windows.items.iter().find_map(|item| match &item.node {
            ItemKind::Impl {
                trait_ty: Some(_),
                methods,
                ..
            } => Some(methods.len()),
            _ => None,
        });
        assert_eq!(trait_methods, Some(1));
    }

    #[test]
    fn target_specific_types_are_reported() {
        let errors = views(
            r#"
@cfg(target_os = "windows") struct Handle { raw: u32 }
@cfg(target_os = "linux") struct Handle { raw: i32 }
@cfg(target_os = "netbsd") struct Handle { raw: i32 }
"#,
        )
        .err()
        .expect("target-specific struct layouts are rejected");
        assert!(
            errors[0].message.contains("`Handle` has a different"),
            "{}",
            errors[0].message
        );
    }

    const LONG: &str = r#"
@cfg(target_os = "windows") type Long = i32;
@cfg(target_os = "linux") type Long = i64;
@cfg(target_os = "netbsd") type Long = i64;
"#;

    #[test]
    fn rewrites_follow_scopes_imports_and_qualifiers() {
        let a = format!("{LONG}pub fn widen(v: Long) Long {{ ret v; }}");
        let views = views_of_files(&[
            ("a.qz", a.as_str()),
            ("b.qz", "pub fn widen(v: i32) i32 { ret v; }"),
            (
                "main.qz",
                r#"import a.widen;
struct S {}
impl S { fn widen(x: i32) i32 { ret x; } }
fn main() i32 {
    var f: fn(i32) i32 = |widen| widen + 1;
    const local = widen(1);
    const own = S.widen(3);
    const other = b.widen(4);
    ret f(1);
}"#,
            ),
        ]);
        let functions = functions(&views);
        let windows_main = &functions
            .iter()
            .find(|(name, _, _)| name == "main@t2")
            .expect("main has a Windows copy")
            .2;
        // The imported target-dependent function gets the Windows copy even
        // though a closure elsewhere binds the same name; the type's static
        // method and the other module's function keep their names.
        assert!(
            windows_main.contains("Ident(\"widen@t2\")"),
            "{windows_main}"
        );
        assert!(
            windows_main.contains("Ident(\"widen\")"),
            "closure parameter kept"
        );
        assert!(windows_main.contains("method: \"widen\""), "{windows_main}");
        assert!(
            !windows_main.contains("method: \"widen@t2\""),
            "{windows_main}"
        );
        // The import brings every copy of `a.widen` and nothing of `b`.
        let import = views
            .program
            .items
            .iter()
            .find_map(|item| match &item.node {
                ItemKind::Import(import) => Some(format!("{:?}", import.items)),
                _ => None,
            })
            .unwrap();
        assert_eq!(import, "Multiple([\"widen\", \"widen@t2\"])");
    }

    #[test]
    fn variant_markers_round_trip() {
        assert_eq!(strip_variant_markers("io.write@t2"), "io.write");
        assert_eq!(
            strip_variant_markers("Type.m@t5.closure0"),
            "Type.m.closure0"
        );
        assert_eq!(strip_variant_markers("f@t3<i32>"), "f<i32>");
        assert_eq!(strip_variant_markers("email@tx"), "email@tx");
        assert_eq!(variant_marker_mask("Type.m@t5.closure0"), Some(5));
        assert_eq!(variant_marker_mask("plain"), None);
    }
}
