// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::lexer::Lexer;
use crate::lexer::token::TokenKind;
use crate::parser::Parser;
use crate::parser::ast::{ItemKind, Program};
use crate::semantic::SourceFile;

pub struct LoadResult {
    pub merged_source: String,
    pub program: Program,
    pub loaded_files: Vec<PathBuf>,
    /// Function names declared in dependency (library) files.
    pub library_fn_names: HashSet<String>,
    /// Paths of files that were loaded from external modules (not user source).
    pub library_file_paths: Vec<PathBuf>,
    /// Character-index ranges within `merged_source` that belong to library files.
    pub library_char_ranges: Vec<std::ops::Range<usize>>,
    /// Per-file ranges and starting lines within `merged_source`.
    pub source_files: Vec<SourceFile>,
    /// (importer, importee) pairs from the final resolved pass — used for dep tree rendering.
    pub dep_edges: Vec<(PathBuf, PathBuf)>,
    /// Stable logical names used by dependency-tree output.
    pub display_names: HashMap<PathBuf, String>,
    /// Token count across user (non-library) source files.
    pub token_count: usize,
    /// Parse error message, if parsing failed. IO errors are returned as `Err` from the loader.
    pub parse_error: Option<String>,
    /// Paths of files that are treated as separate modules and whose top-level
    /// definitions are namespaced/mangled. This is every loaded file except the
    /// original entry files passed to the loader.
    pub namespaced_paths: HashSet<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ModuleSpec {
    pub name: String,
    pub root: PathBuf,
    pub src_dir: PathBuf,
    pub entry: PathBuf,
    /// Whether declarations in `entry` belong directly to the package namespace.
    pub entry_is_package_root: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ModuleResolver {
    pub modules: HashMap<String, ModuleSpec>,
}

impl ModuleResolver {
    pub fn insert(&mut self, spec: ModuleSpec) -> Result<(), String> {
        if let Some(existing) = self.modules.get(&spec.name) {
            if existing.root != spec.root {
                return Err(format!(
                    "module name conflict for '{}': {} vs {}",
                    spec.name,
                    existing.root.to_string_lossy(),
                    spec.root.to_string_lossy()
                ));
            }
            return Ok(());
        }
        self.modules.insert(spec.name.clone(), spec);
        Ok(())
    }
}

/// Load one or more entry files, recursively resolving local imports.
/// Local import: `import foo.bar` where `foo.qz` exists next to the importing file.
/// Sources are merged in dependency-first order so definitions precede their uses.
pub fn load_programs(entries: &[PathBuf]) -> Result<LoadResult, String> {
    load_programs_with_resolver(entries, None)
}

/// Load one or more entry files using an optional module resolver.
pub fn load_programs_with_resolver(
    entries: &[PathBuf],
    resolver: Option<&ModuleResolver>,
) -> Result<LoadResult, String> {
    // First pass: lenient — only checks for @no_std, skips import errors.
    let _initial = collect_sources(entries, resolver, false)?;

    // Build resolver that always includes both prelude and std.
    // @no_std does not disable prelude or std; both are always available.
    let effective_resolver = resolver_with_builtin_modules(resolver, true);

    // Auto-inject prelude before user entries (as a library file).
    let prelude_path: Option<PathBuf> = effective_resolver
        .as_ref()
        .and_then(|r| r.modules.get("prelude"))
        .and_then(|spec| {
            let mod_entry = spec.src_dir.join("mod.qz");
            if mod_entry.exists() {
                return Some(mod_entry);
            }
            let flat = spec.src_dir.join("prelude.qz");
            if flat.exists() { Some(flat) } else { None }
        });

    collect_sources_with_prelude(
        entries,
        effective_resolver.as_ref(),
        prelude_path.as_deref(),
        true,
    )
    .and_then(finalize_sources)
}

struct SourceCollection {
    sources: Vec<(PathBuf, String)>,
    library_paths: HashSet<PathBuf>,
    dep_edges: Vec<(PathBuf, PathBuf)>,
    /// Configured package entries use the package name, not `lib`/`src`, as namespace.
    module_name_overrides: HashMap<PathBuf, String>,
    module_specs: Vec<ModuleSpec>,
    /// Canonicalized paths of the original entry files; everything else is a
    /// dependency and gets namespaced/mangled.
    entry_paths: HashSet<PathBuf>,
}

fn collect_sources_with_prelude(
    entries: &[PathBuf],
    resolver: Option<&ModuleResolver>,
    prelude: Option<&Path>,
    strict: bool,
) -> Result<SourceCollection, String> {
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut sources: Vec<(PathBuf, String)> = Vec::new();
    let mut library_paths: HashSet<PathBuf> = HashSet::new();
    let mut dep_edges: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut entry_paths: HashSet<PathBuf> = HashSet::new();
    let module_name_overrides = resolver
        .into_iter()
        .flat_map(|resolver| resolver.modules.values())
        .filter(|spec| spec.entry_is_package_root)
        .filter_map(|spec| {
            spec.entry
                .canonicalize()
                .ok()
                .map(|entry| (entry, spec.name.clone()))
        })
        .collect();
    let module_specs = resolver
        .into_iter()
        .flat_map(|resolver| resolver.modules.values().cloned())
        .collect();

    if let Some(p) = prelude {
        collect(
            p,
            true,
            &mut visited,
            &mut sources,
            &mut library_paths,
            resolver,
            strict,
            &mut dep_edges,
        )?;
    }

    for entry in entries {
        let canonical = entry
            .canonicalize()
            .map_err(|e| format!("cannot resolve '{}': {}", entry.display(), e))?;
        entry_paths.insert(canonical);
        collect(
            entry,
            false,
            &mut visited,
            &mut sources,
            &mut library_paths,
            resolver,
            strict,
            &mut dep_edges,
        )?;
    }

    Ok(SourceCollection {
        sources,
        library_paths,
        dep_edges,
        module_name_overrides,
        module_specs,
        entry_paths,
    })
}

fn collect_sources(
    entries: &[PathBuf],
    resolver: Option<&ModuleResolver>,
    strict: bool,
) -> Result<SourceCollection, String> {
    collect_sources_with_prelude(entries, resolver, None, strict)
}

fn finalize_sources(collection: SourceCollection) -> Result<LoadResult, String> {
    let SourceCollection {
        mut sources,
        library_paths,
        dep_edges,
        module_name_overrides,
        module_specs,
        entry_paths,
    } = collection;

    let namespaced_paths: HashSet<PathBuf> = sources
        .iter()
        .map(|(p, _)| p)
        .filter(|p| !entry_paths.contains(*p))
        .cloned()
        .collect();
    let display_names = sources
        .iter()
        .filter(|(path, _)| !entry_paths.contains(path))
        .filter_map(|(path, _)| {
            logical_module_name(path, &module_specs).map(|name| (path.clone(), name))
        })
        .collect();

    let user_fn_names = collect_user_function_names(&sources, &library_paths, &entry_paths);
    let explicitly_imported_names = collect_explicit_library_import_names(&sources, &library_paths);
    let shadowed_library_fn_names: HashSet<String> = user_fn_names
        .difference(&explicitly_imported_names)
        .cloned()
        .collect();

    if !shadowed_library_fn_names.is_empty() {
        for (path, src) in &mut sources {
            // Namespaced (library) files use module-qualified function names,
            // so bare-name shadowing is no longer a concern there.
            if library_paths.contains(path) && !namespaced_paths.contains(path) {
                *src = remove_shadowed_library_functions(src, &shadowed_library_fn_names);
            }
        }
    }

    let loaded_files: Vec<PathBuf> = sources.iter().map(|(p, _)| p.clone()).collect();
    let library_file_paths: Vec<PathBuf> = sources
        .iter()
        .filter(|(p, _)| library_paths.contains(p))
        .map(|(p, _)| p.clone())
        .collect();

    let mut merged = String::new();
    let mut library_char_ranges: Vec<std::ops::Range<usize>> = Vec::new();
    let mut source_files: Vec<SourceFile> = Vec::new();
    let mut char_pos: usize = 0;
    let mut line_pos: usize = 1;

    for (path, src) in &sources {
        if !merged.is_empty() {
            merged.push('\n');
            char_pos += 1;
            line_pos += 1;
        }
        let src_char_len = src.chars().count();
        source_files.push(SourceFile {
            path: path.to_string_lossy().into_owned(),
            module_name: module_name_overrides.get(path).cloned(),
            start: char_pos,
            end: char_pos + src_char_len,
            line_start: line_pos,
        });
        if library_paths.contains(path) {
            library_char_ranges.push(char_pos..char_pos + src_char_len);
        }
        merged.push_str(src);
        char_pos += src_char_len;
        line_pos += src.bytes().filter(|&b| b == b'\n').count();
    }

    // Count tokens before parsing so the count is available even on parse failure.
    let token_count: usize = sources
        .iter()
        .filter(|(p, _)| !library_paths.contains(p))
        .map(|(_, src)| {
            let mut lx = Lexer::new(src);
            lx.tokenize()
                .into_iter()
                .filter(|t| !matches!(t.kind, TokenKind::Eof))
                .count()
        })
        .sum();

    let mut lexer = Lexer::new(&merged);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new_with_source_files(tokens, &merged, source_files.clone());
    let (program, parse_error) = match parser.parse() {
        Ok(p) => (p, None),
        Err(e) => (
            crate::parser::ast::Program {
                items: vec![],
                span: None,
            },
            Some(e),
        ),
    };

    // Reuse the merged parse to collect public dependency functions. Parsing
    // every dependency again here made warm builds pay for each module three
    // times: import discovery, name collection, and the merged program.
    let mut library_fn_names: HashSet<String> = HashSet::new();
    for item in &program.items {
        let Some(source_file) = source_files.iter().find(|file| file.contains(item.span)) else {
            continue;
        };
        let source_path = Path::new(&source_file.path);
        if !namespaced_paths.contains(source_path) {
            continue;
        }
        let ItemKind::Fn {
            name,
            pub_fn,
            attributes,
            ..
        } = &item.node
        else {
            continue;
        };
        if !pub_fn {
            continue;
        }
        let exported = attributes
            .iter()
            .find(|attribute| attribute.name == "export")
            .and_then(|attribute| {
                attribute.args.first().and_then(|argument| match argument {
                    crate::parser::ast::AttrArg::Positional(crate::parser::ast::AttrVal::Str(
                        symbol,
                    )) => Some(symbol.clone()),
                    _ => None,
                })
            });
        let entry_name = if let Some(symbol) = exported {
            symbol
        } else if attributes
            .iter()
            .any(|attribute| attribute.name == "no_mangle")
        {
            name.clone()
        } else {
            let module_name = source_file
                .module_name
                .as_deref()
                .map(str::to_owned)
                .unwrap_or_else(|| path_module_name(source_path));
            format!("{}.{}", module_name, name)
        };
        library_fn_names.insert(entry_name);
    }

    Ok(LoadResult {
        merged_source: merged,
        program,
        loaded_files,
        library_fn_names,
        library_file_paths,
        library_char_ranges,
        source_files,
        dep_edges,
        display_names,
        token_count,
        parse_error,
        namespaced_paths,
    })
}

fn logical_module_name(path: &Path, specs: &[ModuleSpec]) -> Option<String> {
    for spec in specs {
        let Ok(src_dir) = spec.src_dir.canonicalize() else {
            continue;
        };
        let Ok(relative) = path.strip_prefix(&src_dir) else {
            continue;
        };
        if spec.entry_is_package_root && spec.entry.canonicalize().is_ok_and(|entry| path == entry)
        {
            return Some(spec.name.clone());
        }
        let mut parts: Vec<String> = relative
            .components()
            .filter_map(|component| component.as_os_str().to_str().map(str::to_owned))
            .collect();
        let file = parts.last_mut()?;
        *file = Path::new(file).file_stem()?.to_str()?.to_string();
        if file == "mod" {
            parts.pop();
        }
        let mut name = spec.name.clone();
        if !parts.is_empty() {
            name.push('.');
            name.push_str(&parts.join("."));
        }
        return Some(name);
    }
    None
}

fn collect_user_function_names(
    sources: &[(PathBuf, String)],
    library_paths: &HashSet<PathBuf>,
    entry_paths: &HashSet<PathBuf>,
) -> HashSet<String> {
    let mut names = HashSet::new();
    for (path, src) in sources {
        if library_paths.contains(path) {
            continue;
        }
        // Dependency files are namespaced, so their bare function names cannot
        // collide with library functions.
        if !entry_paths.contains(path) {
            continue;
        }
        for name in function_names_in_source(src) {
            names.insert(name);
        }
    }
    names
}

fn collect_explicit_library_import_names(
    sources: &[(PathBuf, String)],
    library_paths: &HashSet<PathBuf>,
) -> HashSet<String> {
    let mut names = HashSet::new();
    for (path, src) in sources {
        if library_paths.contains(path) {
            continue;
        }
        let Ok(program) = parse_source(src) else {
            continue;
        };
        for item in &program.items {
            let ItemKind::Import(ip) = &item.node else {
                continue;
            };
            let Some((base, _)) = import_base_and_remainder(ip) else {
                continue;
            };
            if base != "std" {
                continue;
            }
            match &ip.items {
                crate::parser::ast::ImportItems::Single(name)
                | crate::parser::ast::ImportItems::Aliased(name, _) => {
                    names.insert(name.clone());
                }
                crate::parser::ast::ImportItems::Multiple(items) => {
                    names.extend(items.iter().cloned());
                }
                crate::parser::ast::ImportItems::All => {}
            }
        }
    }
    names
}

fn function_names_in_source(src: &str) -> Vec<String> {
    let Ok(program) = parse_source(src) else {
        return Vec::new();
    };
    program
        .items
        .into_iter()
        .filter_map(|item| match item.node {
            ItemKind::Fn { name, .. } => Some(name),
            _ => None,
        })
        .collect()
}

fn parse_source(src: &str) -> Result<Program, String> {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new_with_source(tokens, src);
    parser.parse()
}

fn remove_shadowed_library_functions(src: &str, shadowed_names: &HashSet<String>) -> String {
    let Ok(program) = parse_source(src) else {
        return src.to_string();
    };
    let mut ranges = Vec::new();
    for item in &program.items {
        let ItemKind::Fn { name, .. } = &item.node else {
            continue;
        };
        if shadowed_names.contains(name) {
            let start = char_offset_to_byte(src, item.span.start);
            let end = char_offset_to_byte(src, item.span.end);
            ranges.push(expand_removal_start_to_attributes(src, start)..end);
        }
    }

    if ranges.is_empty() {
        return src.to_string();
    }

    ranges.sort_by_key(|range| range.start);
    let mut out = String::with_capacity(src.len());
    let mut cursor = 0usize;
    for range in ranges {
        if cursor < range.start {
            out.push_str(&src[cursor..range.start]);
        }
        cursor = range.end.min(src.len());
    }
    if cursor < src.len() {
        out.push_str(&src[cursor..]);
    }
    out
}

fn expand_removal_start_to_attributes(src: &str, item_start: usize) -> usize {
    let mut start = line_start(src, item_start);
    loop {
        let Some(prev_end) = start.checked_sub(1) else {
            return start;
        };
        let prev_start = line_start(src, prev_end);
        let prev_line = &src[prev_start..start];
        let trimmed = prev_line.trim();
        if trimmed.starts_with('@') || trimmed.is_empty() {
            start = prev_start;
        } else {
            return start;
        }
    }
}

fn line_start(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())]
        .rfind('\n')
        .map(|idx| idx + 1)
        .unwrap_or(0)
}

/// Derive the module name for a source file path.
/// `src/bar.qz` → `bar`; `src/foo/mod.qz` → `foo`.
fn path_module_name(path: &Path) -> String {
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        if stem == "mod"
            && let Some(parent) = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
        {
            return parent.to_string();
        }
        return stem.to_string();
    }
    path.to_string_lossy().into_owned()
}

fn char_offset_to_byte(src: &str, char_offset: usize) -> usize {
    src.char_indices()
        .nth(char_offset)
        .map(|(idx, _)| idx)
        .unwrap_or(src.len())
}

fn resolver_with_builtin_modules(
    resolver: Option<&ModuleResolver>,
    include_std: bool,
) -> Option<ModuleResolver> {
    let mut combined = resolver.cloned().unwrap_or_default();

    if let Some(prelude_spec) = builtin_prelude_module_spec() {
        combined
            .modules
            .entry("prelude".to_string())
            .or_insert(prelude_spec);
    }

    if include_std && let Some(std_spec) = builtin_std_module_spec() {
        combined
            .modules
            .entry("std".to_string())
            .or_insert(std_spec);
    }

    if combined.modules.is_empty() {
        None
    } else {
        Some(combined)
    }
}

fn builtin_std_module_spec() -> Option<ModuleSpec> {
    let root = find_builtin_std_root()?;
    let src_dir = root.join("src");
    let entry = src_dir.join("core.qz");
    if !entry.exists() {
        return None;
    }
    Some(ModuleSpec {
        name: "std".to_string(),
        root,
        src_dir,
        entry,
        entry_is_package_root: false,
        version: None,
    })
}

fn builtin_prelude_module_spec() -> Option<ModuleSpec> {
    let root = find_builtin_prelude_root()?;
    let src_dir = root.join("src");
    let entry = src_dir.join("mod.qz");
    if !entry.exists() {
        return None;
    }
    Some(ModuleSpec {
        name: "prelude".to_string(),
        root,
        src_dir,
        entry,
        entry_is_package_root: true,
        version: None,
    })
}

pub fn find_builtin_std_root() -> Option<PathBuf> {
    if let Ok(root) = std::env::var("QUAZI_STD_ROOT") {
        let path = PathBuf::from(root);
        if path.join("src").join("core.qz").exists() {
            return Some(path);
        }
    }

    let compiler_std = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("std");
    if compiler_std.join("src").join("core.qz").exists() {
        return Some(compiler_std);
    }

    // Development workspace layout keeps compiler and std as sibling repos.
    let workspace_std = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("std");
    if workspace_std.join("src").join("core.qz").exists() {
        return workspace_std.canonicalize().ok().or(Some(workspace_std));
    }

    // Check ~/.quazi/std (Unix) or %USERPROFILE%/.quazi/std (Windows)
    for home_var in &["HOME", "USERPROFILE"] {
        if let Ok(home) = std::env::var(home_var) {
            let home_path = PathBuf::from(home).join(".quazi").join("std");
            if home_path.join("src").join("core.qz").exists() {
                return Some(home_path);
            }
        }
    }

    None
}

fn find_builtin_prelude_root() -> Option<PathBuf> {
    if let Ok(root) = std::env::var("QUAZI_PRELUDE_ROOT") {
        let path = PathBuf::from(root);
        if path.join("src").join("mod.qz").exists() {
            return Some(path);
        }
    }

    for home_var in &["HOME", "USERPROFILE"] {
        if let Ok(home) = std::env::var(home_var) {
            let home_path = PathBuf::from(home).join(".quazi").join("prelude");
            if home_path.join("src").join("mod.qz").exists() {
                return Some(home_path);
            }
        }
    }

    let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("prelude");
    if manifest_path.join("src").join("mod.qz").exists() {
        return Some(manifest_path);
    }

    let cwd_path = std::env::current_dir().ok()?.join("prelude");
    if cwd_path.join("src").join("mod.qz").exists() {
        return Some(cwd_path);
    }

    None
}

fn sources_contain_no_std(sources: &[(PathBuf, String)]) -> bool {
    sources.iter().any(|(_, src)| source_contains_no_std(src))
}

fn source_contains_no_std(src: &str) -> bool {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();
    tokens.windows(2).any(|pair| {
        matches!(pair[0].kind, TokenKind::At)
            && matches!(&pair[1].kind, TokenKind::Ident(name) if name == "no_std")
    })
}

fn used_std_modules(src: &str) -> HashSet<String> {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut modules = HashSet::new();

    for window in tokens.windows(4) {
        if matches!(&window[0].kind, TokenKind::Ident(name) if name == "std")
            && matches!(window[1].kind, TokenKind::Dot)
            && matches!(&window[2].kind, TokenKind::Ident(_))
            && matches!(window[3].kind, TokenKind::Dot)
            && let TokenKind::Ident(module) = &window[2].kind
        {
            modules.insert(module.clone());
        }
    }

    modules
}

fn collect(
    path: &Path,
    is_library: bool,
    visited: &mut HashSet<PathBuf>,
    sources: &mut Vec<(PathBuf, String)>,
    library_paths: &mut HashSet<PathBuf>,
    resolver: Option<&ModuleResolver>,
    strict: bool,
    dep_edges: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), String> {
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("cannot resolve '{}': {}", path.display(), e))?;

    if !visited.insert(canonical.clone()) {
        return Ok(());
    }

    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read '{}': {}", path.display(), e))?;

    for (dep, dep_is_lib) in local_import_paths(&canonical, &src, resolver, strict)? {
        dep_edges.push((canonical.clone(), dep.clone()));
        collect(
            &dep,
            is_library || dep_is_lib,
            visited,
            sources,
            library_paths,
            resolver,
            strict,
            dep_edges,
        )?;
    }

    if is_library {
        library_paths.insert(canonical.clone());
    }
    sources.push((canonical, src));
    Ok(())
}

/// Parse `src` just enough to find imports that resolve to local `.qz` files.
/// Returns `(path, is_library)` pairs — library=true for module-resolver imports.
fn local_import_paths(
    file: &Path,
    src: &str,
    resolver: Option<&ModuleResolver>,
    strict: bool,
) -> Result<Vec<(PathBuf, bool)>, String> {
    let dir = file.parent().unwrap_or(Path::new("."));

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens);
    let Ok(program) = parser.parse() else {
        return Ok(vec![]);
    };

    let mut seen = HashSet::new();
    let mut paths = Vec::new();

    for item in &program.items {
        let ItemKind::Import(ip) = &item.node else {
            continue;
        };

        let Some((base, remainder)) = import_base_and_remainder(ip) else {
            continue;
        };

        if !ip.relative
            && let Some(mods) = resolver
            && let Some(spec) = mods.modules.get(&base)
        {
            if remainder.is_empty() && base == "std" {
                // import std; — only load sub-modules actually used in source
                for module in used_std_modules(src) {
                    if let Some(target) = resolve_module_file(spec, &module)
                        && seen.insert(target.clone())
                    {
                        paths.push((target, true));
                    }
                }
                continue;
            }

            let target = if remainder.is_empty() {
                spec.entry.clone()
            } else if spec.entry_is_package_root
                && remainder.len() == 1
                && is_public_item_in_entry(&spec.entry, &remainder[0])?
            {
                // A library entry is its package root. Public declarations in
                // that file are exported directly; mod.qz is not required to
                // re-import declarations that already live in the entry.
                spec.entry.clone()
            } else {
                let root_mod_entry = spec.src_dir.join("mod.qz");
                if root_mod_entry.exists() {
                    let exported = &remainder[0];
                    if strict && !is_pub_exported_from_mod(&root_mod_entry, exported)? {
                        return Err(format!(
                            "cannot access '{}' from '{}': '{}' is not pub-imported in mod.qz",
                            exported, base, exported
                        ));
                    }
                    if let Some(specific) = find_pub_exported_file(&root_mod_entry, exported) {
                        if seen.insert(specific.clone()) {
                            paths.push((specific, true));
                        }
                        continue;
                    }
                    root_mod_entry
                } else {
                    // mod.qz opaque directory check: if first remainder segment is a
                    // directory with mod.qz, enforce opaqueness.
                    let first = &remainder[0];
                    let mod_entry_path = spec.src_dir.join(first).join("mod.qz");
                    if mod_entry_path.exists() {
                        if remainder.len() > 1 {
                            let sub = &remainder[1];
                            if strict && !is_pub_exported_from_mod(&mod_entry_path, sub)? {
                                return Err(format!(
                                    "cannot access '{}' from '{}': '{}' is not pub-imported in '{}/mod.qz'",
                                    sub, first, sub, first
                                ));
                            }
                            // Targeted: load only the specific file, skip mod.qz
                            if let Some(specific) = find_pub_exported_file(&mod_entry_path, sub) {
                                if seen.insert(specific.clone()) {
                                    paths.push((specific, true));
                                }
                                continue;
                            }
                        }
                        mod_entry_path
                    } else {
                        // Try progressively shorter paths: the last segment(s) may be
                        // function/symbol names rather than file path components.
                        // e.g. `import std.core.write` → try core/write.qz first,
                        // then core.qz (where `write` is the imported symbol name).
                        let mut found: Option<PathBuf> = None;
                        for len in (1..=remainder.len()).rev() {
                            let mut candidate = spec.src_dir.clone();
                            for seg in &remainder[..len] {
                                candidate.push(seg);
                            }
                            candidate.set_extension("qz");
                            if candidate.exists() {
                                found = Some(candidate);
                                break;
                            }
                        }
                        if found.is_none() {
                            found = resolve_module_file(spec, &remainder[0]);
                        }
                        if found.is_none() && spec.entry.exists() {
                            // A package may expose its API directly from its entry
                            // file, including singular `type = "source"` dependencies.
                            found = Some(spec.entry.clone());
                        }
                        // If nothing found, use the full path so the error message is useful.
                        found.unwrap_or_else(|| {
                            let mut full = spec.src_dir.clone();
                            for seg in &remainder {
                                full.push(seg);
                            }
                            full.set_extension("qz");
                            full
                        })
                    }
                }
            };

            if !target.exists() {
                return Err(format!(
                    "cannot resolve module import '{}' (expected {})",
                    base,
                    target.to_string_lossy()
                ));
            }
            if seen.insert(target.clone()) {
                paths.push((target, true)); // from module resolver → library
            }
            continue;
        } // end !ip.relative

        let candidate = dir.join(format!("{}.qz", base));
        if candidate.exists() {
            if seen.insert(candidate.clone()) {
                paths.push((candidate, false)); // local file → not library
            }
        } else if !remainder.is_empty() {
            // mod.qz opaque directory check for local imports
            let base_dir = dir.join(&base);
            let mod_entry = base_dir.join("mod.qz");
            if mod_entry.exists() {
                let sub = &remainder[0];
                if strict && !is_pub_exported_from_mod(&mod_entry, sub)? {
                    return Err(format!(
                        "cannot access '{}' from '{}': '{}' is not pub-imported in '{}/mod.qz'",
                        sub, base, sub, base
                    ));
                }
                // Targeted: load only the specific file, skip mod.qz
                if let Some(specific) = find_pub_exported_file(&mod_entry, sub) {
                    if seen.insert(specific.clone()) {
                        paths.push((specific, true));
                    }
                } else if seen.insert(mod_entry.clone()) {
                    paths.push((mod_entry, true));
                }
            } else {
                // Progressive subfile resolution under a directory namespace.
                // e.g. `import a.y` → try `a/y.qz`, `import a.y.method` → `a/y/method.qz` then `a/y.qz`
                let mut found = false;
                for len in (1..=remainder.len()).rev() {
                    let mut sub = dir.join(&base);
                    for seg in &remainder[..len] {
                        sub.push(seg);
                    }
                    sub.set_extension("qz");
                    if sub.exists() {
                        if seen.insert(sub.clone()) {
                            paths.push((sub, true)); // library = true
                        }
                        found = true;
                        break;
                    }
                }
                if !found && strict {
                    return Err(format!(
                        "cannot resolve import '{}.{}': no such file",
                        base,
                        remainder.join(".")
                    ));
                }
            } // close else { progressive subfile }
        }
        // close else if !remainder.is_empty()
        else {
            // Directory namespace: import a; where a/ is a directory
            let ns_dir = dir.join(&base);
            if ns_dir.is_dir() {
                let mod_entry = ns_dir.join("mod.qz");
                if mod_entry.exists() {
                    // Opaque module: load only mod.qz (it pub-imports what's needed)
                    if seen.insert(mod_entry.clone()) {
                        paths.push((mod_entry, true));
                    }
                } else {
                    let mut entries: Vec<_> = std::fs::read_dir(&ns_dir)
                        .map_err(|e| {
                            format!("cannot read directory '{}': {}", ns_dir.display(), e)
                        })?
                        .filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .filter(|p| p.extension().is_some_and(|ext| ext == "qz"))
                        .collect();
                    entries.sort();
                    for f in entries {
                        if seen.insert(f.clone()) {
                            paths.push((f, true)); // library = true
                        }
                    }
                }
            } else if strict {
                return Err(format!(
                    "cannot resolve import '{}': no such file or directory",
                    base
                ));
            }
        }
    }

    Ok(paths)
}

fn resolve_module_file(spec: &ModuleSpec, module: &str) -> Option<PathBuf> {
    let root_mod_entry = spec.src_dir.join("mod.qz");
    if root_mod_entry.exists() {
        return find_pub_exported_file(&root_mod_entry, module);
    }

    let target = spec.src_dir.join(module);
    let mod_entry = target.join("mod.qz");
    if mod_entry.exists() {
        return Some(mod_entry);
    }

    let file = spec.src_dir.join(format!("{module}.qz"));
    if file.exists() {
        return Some(file);
    }

    None
}

fn is_public_item_in_entry(entry: &Path, name: &str) -> Result<bool, String> {
    let source = std::fs::read_to_string(entry)
        .map_err(|error| format!("cannot read '{}': {error}", entry.display()))?;
    let program = parse_source(&source)
        .map_err(|error| format!("cannot parse library entry '{}': {error}", entry.display()))?;
    Ok(program.items.iter().any(|item| match &item.node {
        ItemKind::Fn {
            name: item_name,
            pub_fn,
            ..
        } => item_name == name && *pub_fn,
        ItemKind::Struct {
            name: item_name,
            public,
            ..
        }
        | ItemKind::Trait {
            name: item_name,
            public,
            ..
        }
        | ItemKind::Enum {
            name: item_name,
            public,
            ..
        }
        | ItemKind::TypeAlias {
            name: item_name,
            public,
            ..
        }
        | ItemKind::ForeignGlobal {
            name: item_name,
            public,
            ..
        } => item_name == name && *public,
        _ => false,
    }))
}

/// Find the file that a `mod.qz` pub-exports `name` from.
/// e.g. `pub import map.Map` → returns `mod_entry_dir/map.qz`
fn find_pub_exported_file(mod_entry: &Path, name: &str) -> Option<PathBuf> {
    let src = std::fs::read_to_string(mod_entry).ok()?;
    let prog = parse_source(&src).ok()?;
    let mod_dir = mod_entry.parent()?;

    for item in &prog.items {
        let ItemKind::Import(ip) = &item.node else {
            continue;
        };
        if !ip.pub_import {
            continue;
        }

        let exports_name = match &ip.items {
            crate::parser::ast::ImportItems::Single(n) => n == name,
            crate::parser::ast::ImportItems::Aliased(_, alias) => alias == name,
            crate::parser::ast::ImportItems::Multiple(names) => names.iter().any(|n| n == name),
            crate::parser::ast::ImportItems::All => true,
        };
        if !exports_name {
            continue;
        }

        let mut candidates = Vec::new();
        match &ip.items {
            crate::parser::ast::ImportItems::Single(n)
            | crate::parser::ast::ImportItems::Aliased(n, _) => {
                let mut path_with_item = mod_dir.to_path_buf();
                for seg in &ip.path {
                    path_with_item.push(seg);
                }
                path_with_item.push(n);
                path_with_item.set_extension("qz");
                candidates.push(path_with_item);
            }
            crate::parser::ast::ImportItems::Multiple(names) => {
                if names.iter().any(|n| n == name) {
                    let mut path_with_item = mod_dir.to_path_buf();
                    for seg in &ip.path {
                        path_with_item.push(seg);
                    }
                    path_with_item.push(name);
                    path_with_item.set_extension("qz");
                    candidates.push(path_with_item);
                }
            }
            crate::parser::ast::ImportItems::All => {
                // Wildcard: recurse into the sub-module's mod.qz to find `name`.
                if !ip.path.is_empty() {
                    let mut sub_mod = mod_dir.to_path_buf();
                    for seg in &ip.path {
                        sub_mod.push(seg);
                    }
                    let sub_mod_entry = sub_mod.join("mod.qz");
                    if sub_mod_entry.exists()
                        && let Some(found) = find_pub_exported_file(&sub_mod_entry, name)
                    {
                        return Some(found);
                    }
                    // Also try direct file: path/name.qz
                    sub_mod.push(name);
                    sub_mod.set_extension("qz");
                    if sub_mod.exists() {
                        return Some(sub_mod);
                    }
                }
            }
        }

        let mut path_only = mod_dir.to_path_buf();
        for seg in &ip.path {
            path_only.push(seg);
        }
        if !ip.path.is_empty() {
            path_only.set_extension("qz");
            candidates.push(path_only);
        }

        for file_path in candidates {
            if file_path.exists() {
                return Some(file_path);
            }
            // A public import can name a module directory rather than a flat
            // source file. Resolve its gateway just as `resolve_module_file` does.
            let directory_entry = file_path.with_extension("").join("mod.qz");
            if directory_entry.exists() {
                return Some(directory_entry);
            }
        }
    }
    None
}

fn is_pub_exported_from_mod(mod_entry: &Path, name: &str) -> Result<bool, String> {
    let src = std::fs::read_to_string(mod_entry)
        .map_err(|e| format!("cannot read '{}': {}", mod_entry.display(), e))?;
    let Ok(prog) = parse_source(&src) else {
        return Ok(false);
    };
    for item in &prog.items {
        let ItemKind::Import(ip) = &item.node else {
            continue;
        };
        if !ip.pub_import {
            continue;
        }
        match &ip.items {
            crate::parser::ast::ImportItems::Single(n) => {
                if n == name {
                    return Ok(true);
                }
            }
            crate::parser::ast::ImportItems::Aliased(_, alias) => {
                if alias == name {
                    return Ok(true);
                }
            }
            crate::parser::ast::ImportItems::Multiple(names) => {
                if names.iter().any(|n| n == name) {
                    return Ok(true);
                }
            }
            crate::parser::ast::ImportItems::All => return Ok(true),
        }
    }
    Ok(false)
}

fn import_base_and_remainder(ip: &crate::parser::ast::ImportPath) -> Option<(String, Vec<String>)> {
    match &ip.items {
        crate::parser::ast::ImportItems::Single(name)
        | crate::parser::ast::ImportItems::Aliased(name, _) => {
            let mut segments = ip.path.clone();
            segments.push(name.clone());
            let base = segments.first()?.clone();
            let remainder = segments.into_iter().skip(1).collect();
            Some((base, remainder))
        }
        crate::parser::ast::ImportItems::Multiple(_) | crate::parser::ast::ImportItems::All => {
            let base = ip.path.first()?.clone();
            let remainder = ip.path.iter().skip(1).cloned().collect();
            Some((base, remainder))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        dir.push(format!("{}_{}_{}", prefix, std::process::id(), nanos));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn resolves_module_import_with_resolver() {
        let root = temp_dir("quazi_loader");
        let app_src = root.join("src");
        fs::create_dir_all(&app_src).expect("create app src");
        let main_path = app_src.join("main.qz");
        fs::write(&main_path, "import dep.util; fn main() void { ret; }").expect("write main.qz");

        let dep_root = root.join("dep");
        let dep_src = dep_root.join("src");
        fs::create_dir_all(&dep_src).expect("create dep src");
        fs::write(dep_src.join("util.qz"), "fn util() void { ret; }").expect("write util.qz");
        fs::write(dep_src.join("main.qz"), "fn dep_main() void { ret; }").expect("write dep main");

        let mut resolver = ModuleResolver::default();
        resolver
            .insert(ModuleSpec {
                name: "dep".to_string(),
                root: dep_root.clone(),
                src_dir: dep_src.clone(),
                entry: dep_src.join("main.qz"),
                entry_is_package_root: true,
                version: Some("0.1.0".to_string()),
            })
            .expect("insert module");

        let result =
            load_programs_with_resolver(&[main_path], Some(&resolver)).expect("load programs");
        assert!(
            result.loaded_files.iter().any(|p| p.ends_with("util.qz")),
            "expected util.qz to be loaded"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn public_entry_declaration_is_a_direct_package_export() {
        let root = temp_dir("quazi_loader_entry_export");
        let main_path = root.join("main.qz");
        fs::write(
            &main_path,
            "import dep.factorial; fn main() i32 { factorial(5); ret 0; }",
        )
        .expect("write app entry");

        let dep_root = root.join("dep");
        let dep_src = dep_root.join("src");
        fs::create_dir_all(&dep_src).expect("create dependency src");
        let dep_entry = dep_src.join("mod.qz");
        fs::write(
            &dep_entry,
            "pub fn factorial(n: u64) u64 { if (n <= 1) { ret 1; } ret n * factorial(n - 1); }",
        )
        .expect("write dependency entry");

        let mut resolver = ModuleResolver::default();
        resolver
            .insert(ModuleSpec {
                name: "dep".to_string(),
                root: dep_root,
                src_dir: dep_src,
                entry: dep_entry.clone(),
                entry_is_package_root: true,
                version: Some("0.1.0".to_string()),
            })
            .expect("insert dependency");

        let result = load_programs_with_resolver(&[main_path], Some(&resolver))
            .expect("load public entry declaration");
        assert!(
            result
                .loaded_files
                .iter()
                .any(|path| path.ends_with(Path::new("dep").join("src").join("mod.qz")))
        );
        assert!(result.parse_error.is_none());
        assert!(result.library_fn_names.contains("dep.factorial"));
        assert_eq!(
            result.display_names.get(
                &dep_entry
                    .canonicalize()
                    .expect("canonical dependency entry")
            ),
            Some(&"dep".to_string())
        );

        let namespaced_paths = result
            .namespaced_paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        let report = crate::analysis::analyze_program_with_source_files(
            &result.merged_source,
            &result.program,
            result.library_fn_names,
            result.library_char_ranges,
            result.source_files,
            namespaced_paths,
        );
        assert!(
            report.errors.is_empty(),
            "expected direct package export to analyze, got {:?}",
            report.errors
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn std_is_available_as_namespace_with_explicit_import() {
        let root = temp_dir("quazi_loader_std");
        let main_path = root.join("main.qz");
        fs::write(
            &main_path,
            "import std.core.write;\nfn main() void { ret; }",
        )
        .expect("write main.qz");

        let result = load_programs(&[main_path]).expect("load programs");
        assert!(
            result
                .library_file_paths
                .iter()
                .any(|p| p.ends_with(Path::new("src").join("core.qz"))),
            "expected std/src/core.qz to be loaded via explicit import, got {:?}",
            result.library_file_paths
        );
        assert!(result.library_fn_names.contains("core.write"));
        assert!(result.display_names.values().any(|name| name == "std.core"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prelude_directory_exports_resolve_as_modules() {
        let root = temp_dir("quazi_loader_prelude_exports");
        let main_path = root.join("main.qz");
        // Prelude contents are auto-imported; explicit module path uses prelude.* prefix.
        fs::write(
            &main_path,
            "import prelude.fmt.format;\nfn main() void { ret; }",
        )
        .expect("write main.qz");

        let result = load_programs(&[main_path]).expect("load programs");
        assert!(
            result
                .library_file_paths
                .iter()
                .any(|p| p.ends_with(Path::new("prelude").join("src").join("fmt.qz"))),
            "expected prelude/src/fmt.qz to be loaded, got {:?}",
            result.library_file_paths
        );
        assert!(
            result
                .library_file_paths
                .iter()
                .any(|p| p.ends_with(Path::new("prelude").join("src").join("string.qz"))),
            "expected prelude/src/string.qz to be loaded as fmt dependency, got {:?}",
            result.library_file_paths
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mod_entry_exports_flatten_child_imports() {
        let root = temp_dir("quazi_loader_mod_exports");
        let foo_dir = root.join("foo");
        fs::create_dir_all(&foo_dir).expect("create foo dir");
        fs::write(foo_dir.join("mod.qz"), "pub import a.METHOD;").expect("write mod.qz");
        fs::write(foo_dir.join("a.qz"), "pub fn METHOD() void { ret; }").expect("write a.qz");

        let ok_path = root.join("ok.qz");
        fs::write(&ok_path, "import foo.METHOD;\nfn main() void { ret; }").expect("write ok.qz");
        let result = load_programs(&[ok_path]).expect("load flattened import");
        assert!(
            result
                .loaded_files
                .iter()
                .any(|p| p.ends_with(Path::new("foo").join("a.qz"))),
            "expected foo/a.qz through flattened export, got {:?}",
            result.loaded_files
        );

        let bad_path = root.join("bad.qz");
        fs::write(&bad_path, "import foo.a.METHOD;\nfn main() void { ret; }")
            .expect("write bad.qz");
        let err = match load_programs(&[bad_path]) {
            Ok(_) => panic!("nested access through opaque mod should fail"),
            Err(err) => err,
        };
        assert!(
            err.contains("cannot access 'a' from 'foo'"),
            "expected opaque module access error, got {err}"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn public_import_resolves_directory_module_entry() {
        let root = temp_dir("quazi_loader_directory_public_import");
        let foo_dir = root.join("foo");
        let nested_dir = foo_dir.join("nested");
        fs::create_dir_all(&nested_dir).expect("create nested module");
        fs::write(foo_dir.join("mod.qz"), "pub import nested;").expect("write foo gateway");
        fs::write(nested_dir.join("mod.qz"), "pub import item.make;")
            .expect("write nested gateway");
        fs::write(nested_dir.join("item.qz"), "pub fn make() i32 { ret 7; }")
            .expect("write nested item");

        let main_path = root.join("main.qz");
        fs::write(
            &main_path,
            "import foo.nested.make;\nfn main() i32 { ret make(); }",
        )
        .expect("write entry");

        let result = load_programs(&[main_path]).expect("load directory re-export");
        assert!(
            result
                .loaded_files
                .iter()
                .any(|path| path.ends_with(Path::new("nested").join("mod.qz"))),
            "expected nested/mod.qz, got {:?}",
            result.loaded_files
        );
        assert!(
            result
                .loaded_files
                .iter()
                .any(|path| path.ends_with(Path::new("nested").join("item.qz"))),
            "expected nested/item.qz, got {:?}",
            result.loaded_files
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn no_std_keeps_prelude_and_std_resolver() {
        let root = temp_dir("quazi_loader_no_std");
        let main_path = root.join("main.qz");
        fs::write(&main_path, "@no_std\nfn main() void { ret; }").expect("write main.qz");

        let result = load_programs(&[main_path]).expect("load programs");
        assert!(
            !result.library_file_paths.is_empty(),
            "expected prelude library files, got {:?}",
            result.library_file_paths
        );
        assert!(
            result
                .library_file_paths
                .iter()
                .any(|p| p.ends_with(Path::new("prelude").join("src").join("mod.qz"))),
            "expected prelude/src/mod.qz to be loaded, got {:?}",
            result.library_file_paths
        );
        // The prelude is now fully self-contained (uses @intrinsic, no import std),
        // so std/src/core.qz is not a transitive dependency of the prelude.
        // std is still available for explicit imports even with @no_std
        // (the resolver registers it), but it won't appear in library_file_paths
        // unless the user program imports it.

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolves_builtin_std_module_import_without_project() {
        let root = temp_dir("quazi_loader_builtin_std");
        let main_path = root.join("main.qz");
        fs::write(&main_path, "import std.unix.open; fn main() void { ret; }")
            .expect("write main.qz");

        let result = load_programs(&[main_path]).expect("load programs");
        assert!(
            result
                .library_file_paths
                .iter()
                .any(|p| p.ends_with(Path::new("src").join("unix.qz"))),
            "expected std/src/unix.qz to be loaded, got {:?}",
            result.library_file_paths
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn user_function_does_not_collide_with_namespaced_std_function() {
        let root = temp_dir("quazi_loader_namespace_std");
        let main_path = root.join("main.qz");
        fs::write(
            &main_path,
            r#"
import std.core.write;

fn sleep_ms() void { }

fn main() i32 {
    const msg: str = "hey!\n";
    unsafe { write(1, msg, msg.len()); }
    sleep_ms();
    ret 0;
}
"#,
        )
        .expect("write main.qz");

        let result = load_programs(&[main_path]).expect("load programs");
        assert!(
            result.library_fn_names.contains("core.write"),
            "explicitly imported std.core.write should remain available"
        );
        assert!(
            result.library_fn_names.contains("core.sleep_ms"),
            "std.core.sleep_ms should remain available under its mangled name"
        );
        let namespaced_paths: HashSet<String> = result
            .namespaced_paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let report = crate::analysis::analyze_program_with_source_files(
            &result.merged_source,
            &result.program,
            result.library_fn_names,
            result.library_char_ranges,
            result.source_files,
            namespaced_paths,
        );
        assert!(
            report.errors.is_empty(),
            "expected namespaced std program to analyze cleanly, got {:?}",
            report.errors
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn importing_private_type_emits_s04() {
        let root = temp_dir("quazi_pub_type_private");
        let src = root.join("src");
        fs::create_dir_all(&src).expect("create src");

        fs::write(
            src.join("helper.qz"),
            "struct PrivateStruct { value: i32, }\npub fn make() PrivateStruct { ret PrivateStruct { value: 1 }; }",
        )
        .expect("write helper.qz");

        let main_path = src.join("main.qz");
        fs::write(
            &main_path,
            "import helper.PrivateStruct;\nfn main() void { ret; }",
        )
        .expect("write main.qz");

        let result = load_programs(&[main_path]).expect("load programs");
        let namespaced_paths: HashSet<String> = result
            .namespaced_paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let report = crate::analysis::analyze_program_with_source_files(
            &result.merged_source,
            &result.program,
            result.library_fn_names,
            result.library_char_ranges,
            result.source_files,
            namespaced_paths,
        );
        assert!(
            report.errors.iter().any(|e| e.code == "S04"),
            "expected S04 error when importing private struct, got {:?}",
            report.errors
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn importing_public_type_succeeds() {
        let root = temp_dir("quazi_pub_type_public");
        let src = root.join("src");
        fs::create_dir_all(&src).expect("create src");

        fs::write(
            src.join("helper.qz"),
            "pub struct PublicStruct { value: i32, }\nimpl PublicStruct { pub fn new() PublicStruct { ret PublicStruct { value: 1 }; } }",
        )
        .expect("write helper.qz");

        let main_path = src.join("main.qz");
        fs::write(
            &main_path,
            "import helper.PublicStruct;\nfn main() void { var _s: PublicStruct = PublicStruct.new(); ret; }",
        )
        .expect("write main.qz");

        let result = load_programs(&[main_path]).expect("load programs");
        let namespaced_paths: HashSet<String> = result
            .namespaced_paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let report = crate::analysis::analyze_program_with_source_files(
            &result.merged_source,
            &result.program,
            result.library_fn_names,
            result.library_char_ranges,
            result.source_files,
            namespaced_paths,
        );
        assert!(
            report.errors.iter().all(|e| e.code != "S04"),
            "expected no S04 errors when importing public struct, got {:?}",
            report.errors
        );

        let _ = fs::remove_dir_all(root);
    }
}
