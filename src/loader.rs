// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::lexer::Lexer;
use crate::lexer::token::TokenKind;
use crate::parser::Parser;
use crate::parser::ast::{ItemKind, Program};

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
    /// (importer, importee) pairs from the final resolved pass — used for dep tree rendering.
    pub dep_edges: Vec<(PathBuf, PathBuf)>,
    /// Token count across user (non-library) source files.
    pub token_count: usize,
}

#[derive(Debug, Clone)]
pub struct ModuleSpec {
    pub name: String,
    pub root: PathBuf,
    pub src_dir: PathBuf,
    pub entry: PathBuf,
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
/// Local import: `import foo.bar` where `foo.void` exists next to the importing file.
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
    let initial = collect_sources(entries, resolver, false)?;
    if sources_contain_no_std(&initial.sources) {
        return finalize_sources(initial);
    }

    // Second pass: strict — std resolver available, errors on unresolved imports.
    let resolver_with_std = resolver_with_builtin_std(resolver);
    let effective_resolver = resolver_with_std.as_ref().or(resolver);

    // Auto-inject std prelude before user entries (as a library file).
    let prelude_path: Option<PathBuf> = effective_resolver
        .and_then(|r| r.modules.get("std"))
        .map(|spec| spec.src_dir.join("prelude.void"))
        .filter(|p| p.exists());

    collect_sources_with_prelude(entries, effective_resolver, prelude_path.as_deref(), true)
        .and_then(finalize_sources)
}

struct SourceCollection {
    sources: Vec<(PathBuf, String)>,
    library_paths: HashSet<PathBuf>,
    dep_edges: Vec<(PathBuf, PathBuf)>,
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

    if let Some(p) = prelude {
        collect(p, true, &mut visited, &mut sources, &mut library_paths, resolver, strict, &mut dep_edges)?;
    }

    for entry in entries {
        collect(entry, false, &mut visited, &mut sources, &mut library_paths, resolver, strict, &mut dep_edges)?;
    }

    Ok(SourceCollection {
        sources,
        library_paths,
        dep_edges,
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
    } = collection;

    let user_fn_names = collect_user_function_names(&sources, &library_paths);
    let explicitly_imported_names = collect_explicit_library_import_names(&sources, &library_paths);
    let shadowed_library_fn_names: HashSet<String> = user_fn_names
        .difference(&explicitly_imported_names)
        .cloned()
        .collect();

    if !shadowed_library_fn_names.is_empty() {
        for (path, src) in &mut sources {
            if library_paths.contains(path) {
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

    // Collect function names declared in library (dependency) files.
    let mut library_fn_names: HashSet<String> = HashSet::new();
    for (path, src) in &sources {
        if !library_paths.contains(path) {
            continue;
        }
        let mut lx = Lexer::new(src);
        let toks = lx.tokenize();
        let mut pr = Parser::new(toks);
        if let Ok(prog) = pr.parse() {
            for item in &prog.items {
                if let ItemKind::Fn { name, pub_fn, .. } = &item.node {
                    if *pub_fn {
                        library_fn_names.insert(name.clone());
                    }
                }
            }
        }
    }

    let mut merged = String::new();
    let mut library_char_ranges: Vec<std::ops::Range<usize>> = Vec::new();
    let mut char_pos: usize = 0;

    for (path, src) in &sources {
        if !merged.is_empty() {
            merged.push('\n');
            char_pos += 1;
        }
        let src_char_len = src.chars().count();
        if library_paths.contains(path) {
            library_char_ranges.push(char_pos..char_pos + src_char_len);
        }
        merged.push_str(src);
        char_pos += src_char_len;
    }

    let mut lexer = Lexer::new(&merged);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new_with_source(tokens, &merged);
    let program = parser.parse()?;

    // Count tokens in user (non-library) source for progress display.
    let token_count = sources
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

    Ok(LoadResult {
        merged_source: merged,
        program,
        loaded_files,
        library_fn_names,
        library_file_paths,
        library_char_ranges,
        dep_edges,
        token_count,
    })
}

fn collect_user_function_names(
    sources: &[(PathBuf, String)],
    library_paths: &HashSet<PathBuf>,
) -> HashSet<String> {
    let mut names = HashSet::new();
    for (path, src) in sources {
        if library_paths.contains(path) {
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

fn char_offset_to_byte(src: &str, char_offset: usize) -> usize {
    src.char_indices()
        .nth(char_offset)
        .map(|(idx, _)| idx)
        .unwrap_or(src.len())
}

fn resolver_with_builtin_std(resolver: Option<&ModuleResolver>) -> Option<ModuleResolver> {
    let std_spec = builtin_std_module_spec()?;
    let mut combined = resolver.cloned().unwrap_or_default();
    if !combined.modules.contains_key("std") {
        combined.modules.insert("std".to_string(), std_spec);
    }
    Some(combined)
}

fn builtin_std_module_spec() -> Option<ModuleSpec> {
    let root = find_builtin_std_root()?;
    let src_dir = root.join("src");
    let entry = src_dir.join("core.void");
    if !entry.exists() {
        return None;
    }
    Some(ModuleSpec {
        name: "std".to_string(),
        root,
        src_dir,
        entry,
        version: None,
    })
}

fn find_builtin_std_root() -> Option<PathBuf> {
    if let Ok(root) = std::env::var("VOID_STD_ROOT") {
        let path = PathBuf::from(root);
        if path.join("src").join("core.void").exists() {
            return Some(path);
        }
    }

    // Check ~/.void/std (Unix) or %USERPROFILE%/.void/std (Windows)
    for home_var in &["HOME", "USERPROFILE"] {
        if let Ok(home) = std::env::var(home_var) {
            let home_path = PathBuf::from(home).join(".void").join("std");
            if home_path.join("src").join("core.void").exists() {
                return Some(home_path);
            }
        }
    }

    let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("std");
    if manifest_path.join("src").join("core.void").exists() {
        return Some(manifest_path);
    }

    let cwd_path = std::env::current_dir().ok()?.join("std");
    if cwd_path.join("src").join("core.void").exists() {
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
        {
            if let TokenKind::Ident(module) = &window[2].kind {
                modules.insert(module.clone());
            }
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

    if !visited.insert(canonical) {
        return Ok(());
    }

    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read '{}': {}", path.display(), e))?;

    for (dep, dep_is_lib) in local_import_paths(path, &src, resolver, strict)? {
        dep_edges.push((path.to_path_buf(), dep.clone()));
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
        library_paths.insert(path.to_path_buf());
    }
    sources.push((path.to_path_buf(), src));
    Ok(())
}

/// Parse `src` just enough to find imports that resolve to local `.void` files.
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

        if !ip.relative {
        if let Some(mods) = resolver {
            if let Some(spec) = mods.modules.get(&base) {
                if remainder.is_empty() && base == "std" {
                    // import std; — only load sub-modules actually used in source
                    for module in used_std_modules(src) {
                        let mut target = spec.src_dir.clone();
                        target.push(&module);
                        // Check for mod.void first (opaque module directory)
                        let mod_void = target.join("mod.void");
                        if mod_void.exists() {
                            if seen.insert(mod_void.clone()) {
                                paths.push((mod_void, true));
                            }
                        } else {
                            target.set_extension("void");
                            if target.exists() && seen.insert(target.clone()) {
                                paths.push((target, true));
                            }
                        }
                    }
                    continue;
                }

                let target = if remainder.is_empty() {
                    spec.entry.clone()
                } else {
                    // mod.void opaque directory check: if first remainder segment is a
                    // directory with mod.void, enforce opaqueness.
                    let first = &remainder[0];
                    let mod_void_path = spec.src_dir.join(first).join("mod.void");
                    if mod_void_path.exists() {
                        if remainder.len() > 1 {
                            let sub = &remainder[1];
                            if strict && !is_pub_exported_from_mod(&mod_void_path, sub)? {
                                return Err(format!(
                                    "cannot access '{}' from '{}': '{}' is not pub-imported in '{}/mod.void'",
                                    sub, first, sub, first
                                ));
                            }
                            // Targeted: load only the specific file, skip mod.void
                            if let Some(specific) = find_pub_exported_file(&mod_void_path, sub) {
                                if seen.insert(specific.clone()) {
                                    paths.push((specific, true));
                                }
                                continue;
                            }
                        }
                        mod_void_path
                    } else {
                        // Try progressively shorter paths: the last segment(s) may be
                        // function/symbol names rather than file path components.
                        // e.g. `import std.core.write` → try core/write.void first,
                        // then core.void (where `write` is the imported symbol name).
                        let mut found: Option<PathBuf> = None;
                        for len in (1..=remainder.len()).rev() {
                            let mut candidate = spec.src_dir.clone();
                            for seg in &remainder[..len] {
                                candidate.push(seg);
                            }
                            candidate.set_extension("void");
                            if candidate.exists() {
                                found = Some(candidate);
                                break;
                            }
                        }
                        // If nothing found, use the full path so the error message is useful.
                        found.unwrap_or_else(|| {
                            let mut full = spec.src_dir.clone();
                            for seg in &remainder {
                                full.push(seg);
                            }
                            full.set_extension("void");
                            full
                        })
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
            }
        }
        } // end !ip.relative

        let candidate = dir.join(format!("{}.void", base));
        if candidate.exists() && seen.insert(candidate.clone()) {
            paths.push((candidate, false)); // local file → not library
        } else if !remainder.is_empty() {
            // mod.void opaque directory check for local imports
            let base_dir = dir.join(&base);
            let mod_void = base_dir.join("mod.void");
            if mod_void.exists() {
                let sub = &remainder[0];
                if strict && !is_pub_exported_from_mod(&mod_void, sub)? {
                    return Err(format!(
                        "cannot access '{}' from '{}': '{}' is not pub-imported in '{}/mod.void'",
                        sub, base, sub, base
                    ));
                }
                // Targeted: load only the specific file, skip mod.void
                if let Some(specific) = find_pub_exported_file(&mod_void, sub) {
                    if seen.insert(specific.clone()) {
                        paths.push((specific, true));
                    }
                } else if seen.insert(mod_void.clone()) {
                    paths.push((mod_void, true));
                }
            } else {
                // Progressive subfile resolution under a directory namespace.
                // e.g. `import a.y` → try `a/y.void`, `import a.y.method` → `a/y/method.void` then `a/y.void`
                let mut found = false;
                for len in (1..=remainder.len()).rev() {
                    let mut sub = dir.join(&base);
                    for seg in &remainder[..len] {
                        sub.push(seg);
                    }
                    sub.set_extension("void");
                    if sub.exists() && seen.insert(sub.clone()) {
                        paths.push((sub, true)); // library = true
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
            }
        } else {
            // Directory namespace: import a; where a/ is a directory
            let ns_dir = dir.join(&base);
            if ns_dir.is_dir() {
                let mod_void = ns_dir.join("mod.void");
                if mod_void.exists() {
                    // Opaque module: load only mod.void (it pub-imports what's needed)
                    if seen.insert(mod_void.clone()) {
                        paths.push((mod_void, true));
                    }
                } else {
                    let mut entries: Vec<_> = std::fs::read_dir(&ns_dir)
                        .map_err(|e| format!("cannot read directory '{}': {}", ns_dir.display(), e))?
                        .filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .filter(|p| p.extension().is_some_and(|ext| ext == "void"))
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

/// Find the file that a `mod.void` pub-exports `name` from.
/// e.g. `pub import map.Map` → returns `mod_void_dir/map.void`
fn find_pub_exported_file(mod_void: &Path, name: &str) -> Option<PathBuf> {
    let src = std::fs::read_to_string(mod_void).ok()?;
    let prog = parse_source(&src).ok()?;
    let mod_dir = mod_void.parent()?;

    for item in &prog.items {
        let ItemKind::Import(ip) = &item.node else { continue };
        if !ip.pub_import { continue }

        let exports_name = match &ip.items {
            crate::parser::ast::ImportItems::Single(n)
            | crate::parser::ast::ImportItems::Aliased(n, _) => n == name,
            crate::parser::ast::ImportItems::Multiple(names) => names.iter().any(|n| n == name),
            crate::parser::ast::ImportItems::All => true,
        };
        if !exports_name { continue }

        // Resolve file: path segments + optional last segment from items
        let mut file_path = mod_dir.to_path_buf();
        for seg in &ip.path {
            file_path.push(seg);
        }
        file_path.set_extension("void");
        if file_path.exists() {
            return Some(file_path);
        }
    }
    None
}

fn is_pub_exported_from_mod(mod_void: &Path, name: &str) -> Result<bool, String> {
    let src = std::fs::read_to_string(mod_void)
        .map_err(|e| format!("cannot read '{}': {}", mod_void.display(), e))?;
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
            crate::parser::ast::ImportItems::Single(n)
            | crate::parser::ast::ImportItems::Aliased(n, _) => {
                if n == name {
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
        let root = temp_dir("void_loader");
        let app_src = root.join("src");
        fs::create_dir_all(&app_src).expect("create app src");
        let main_path = app_src.join("main.void");
        fs::write(&main_path, "import dep.util; fn main() void { ret; }").expect("write main.void");

        let dep_root = root.join("dep");
        let dep_src = dep_root.join("src");
        fs::create_dir_all(&dep_src).expect("create dep src");
        fs::write(dep_src.join("util.void"), "fn util() void { ret; }").expect("write util.void");
        fs::write(dep_src.join("main.void"), "fn dep_main() void { ret; }")
            .expect("write dep main");

        let mut resolver = ModuleResolver::default();
        resolver
            .insert(ModuleSpec {
                name: "dep".to_string(),
                root: dep_root.clone(),
                src_dir: dep_src.clone(),
                entry: dep_src.join("main.void"),
                version: Some("0.1.0".to_string()),
            })
            .expect("insert module");

        let result =
            load_programs_with_resolver(&[main_path], Some(&resolver)).expect("load programs");
        assert!(
            result.loaded_files.iter().any(|p| p.ends_with("util.void")),
            "expected util.void to be loaded"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn std_is_available_as_namespace_with_explicit_import() {
        let root = temp_dir("void_loader_std");
        let main_path = root.join("main.void");
        fs::write(
            &main_path,
            "import std.core.write;\nfn main() void { ret; }",
        )
        .expect("write main.void");

        let result = load_programs(&[main_path]).expect("load programs");
        assert!(
            result
                .library_file_paths
                .iter()
                .any(|p| p.ends_with(Path::new("src").join("core.void"))),
            "expected std/src/core.void to be loaded via explicit import, got {:?}",
            result.library_file_paths
        );
        assert!(result.library_fn_names.contains("write"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn no_std_disables_std_injection() {
        let root = temp_dir("void_loader_no_std");
        let main_path = root.join("main.void");
        fs::write(&main_path, "@no_std\nfn main() void { ret; }").expect("write main.void");

        let result = load_programs(&[main_path]).expect("load programs");
        assert!(
            result.library_file_paths.is_empty(),
            "expected no std library files, got {:?}",
            result.library_file_paths
        );
        assert!(result.library_fn_names.is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolves_builtin_std_module_import_without_project() {
        let root = temp_dir("void_loader_builtin_std");
        let main_path = root.join("main.void");
        fs::write(&main_path, "import std.unix.open; fn main() void { ret; }")
            .expect("write main.void");

        let result = load_programs(&[main_path]).expect("load programs");
        assert!(
            result
                .library_file_paths
                .iter()
                .any(|p| p.ends_with(Path::new("src").join("unix.void"))),
            "expected std/src/unix.void to be loaded, got {:?}",
            result.library_file_paths
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn user_function_can_shadow_unimported_std_function() {
        let root = temp_dir("void_loader_shadow_std");
        let main_path = root.join("main.void");
        fs::write(
            &main_path,
            // Shadow sleep_ms — unlike exit, panic.void does not call sleep_ms,
            // so there is no arity conflict from the prelude's __void_panic_handler.
            r#"
import std.core.write;
import std;

fn sleep_ms() void { }

fn main() i32 {
    const msg: str = "hey!\n";
    write(1, msg, msg.len());
    sleep_ms();
    ret 0;
}
"#,
        )
        .expect("write main.void");

        let result = load_programs(&[main_path]).expect("load programs");
        assert!(
            result.library_fn_names.contains("write"),
            "explicitly imported std.core.write should remain available"
        );
        assert!(
            !result.library_fn_names.contains("sleep_ms"),
            "unimported std.core.sleep_ms should be filtered when user defines sleep_ms"
        );
        let report = crate::analysis::analyze_program(
            &result.merged_source,
            &result.program,
            result.library_fn_names,
            result.library_char_ranges,
        );
        assert!(
            report.errors.is_empty(),
            "expected shadowed std exit program to analyze cleanly, got {:?}",
            report.errors
        );

        let _ = fs::remove_dir_all(root);
    }
}
