// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::lexer::Lexer;
use crate::parser::ast::{ItemKind, Program};
use crate::parser::Parser;

pub struct LoadResult {
    pub merged_source: String,
    pub program: Program,
    pub loaded_files: Vec<PathBuf>,
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
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut sources: Vec<(PathBuf, String)> = Vec::new();

    for entry in entries {
        collect(entry, &mut visited, &mut sources, resolver)?;
    }

    let loaded_files: Vec<PathBuf> = sources.iter().map(|(p, _)| p.clone()).collect();

    let mut merged = String::new();
    for (_, src) in &sources {
        if !merged.is_empty() {
            merged.push('\n');
        }
        merged.push_str(src);
    }

    let mut lexer = Lexer::new(&merged);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new_with_source(tokens, &merged);
    let program = parser.parse()?;

    Ok(LoadResult { merged_source: merged, program, loaded_files })
}

fn collect(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    sources: &mut Vec<(PathBuf, String)>,
    resolver: Option<&ModuleResolver>,
) -> Result<(), String> {
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("cannot resolve '{}': {}", path.display(), e))?;

    if !visited.insert(canonical) {
        return Ok(());
    }

    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read '{}': {}", path.display(), e))?;

    for dep in local_import_paths(path, &src, resolver)? {
        collect(&dep, visited, sources, resolver)?;
    }

    sources.push((path.to_path_buf(), src));
    Ok(())
}

/// Parse `src` just enough to find imports that resolve to local `.void` files.
fn local_import_paths(
    file: &Path,
    src: &str,
    resolver: Option<&ModuleResolver>,
) -> Result<Vec<PathBuf>, String> {
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

        if let Some(mods) = resolver {
            if let Some(spec) = mods.modules.get(&base) {
                let target = if remainder.is_empty() {
                    spec.entry.clone()
                } else {
                    let mut target = spec.src_dir.clone();
                    for seg in remainder {
                        target.push(seg);
                    }
                    target.set_extension("void");
                    target
                };

                if !target.exists() {
                    return Err(format!(
                        "cannot resolve module import '{}' (expected {})",
                        base,
                        target.to_string_lossy()
                    ));
                }
                if seen.insert(target.clone()) {
                    paths.push(target);
                }
                continue;
            }
        }

        let candidate = dir.join(format!("{}.void", base));
        if candidate.exists() && seen.insert(candidate.clone()) {
            paths.push(candidate);
        }
    }

    Ok(paths)
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
        fs::write(&main_path, "import dep.util; fn main() void { ret; }")
            .expect("write main.void");

        let dep_root = root.join("dep");
        let dep_src = dep_root.join("src");
        fs::create_dir_all(&dep_src).expect("create dep src");
        fs::write(dep_src.join("util.void"), "fn util() void { ret; }")
            .expect("write util.void");
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

        let result = load_programs_with_resolver(&[main_path], Some(&resolver))
            .expect("load programs");
        assert!(
            result.loaded_files.iter().any(|p| p.ends_with("util.void")),
            "expected util.void to be loaded"
        );

        let _ = fs::remove_dir_all(root);
    }
}
