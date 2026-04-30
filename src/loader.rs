// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::lexer::Lexer;
use crate::parser::ast::{ItemKind, Program};
use crate::parser::Parser;

pub struct LoadResult {
    pub merged_source: String,
    pub program: Program,
    pub loaded_files: Vec<PathBuf>,
}

/// Load one or more entry files, recursively resolving local imports.
/// Local import: `import foo.bar` where `foo.void` exists next to the importing file.
/// Sources are merged in dependency-first order so definitions precede their uses.
pub fn load_programs(entries: &[PathBuf]) -> Result<LoadResult, String> {
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut sources: Vec<(PathBuf, String)> = Vec::new();

    for entry in entries {
        collect(entry, &mut visited, &mut sources)?;
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
) -> Result<(), String> {
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("cannot resolve '{}': {}", path.display(), e))?;

    if !visited.insert(canonical) {
        return Ok(());
    }

    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read '{}': {}", path.display(), e))?;

    for dep in local_import_paths(path, &src) {
        collect(&dep, visited, sources)?;
    }

    sources.push((path.to_path_buf(), src));
    Ok(())
}

/// Parse `src` just enough to find imports that resolve to local `.void` files.
fn local_import_paths(file: &Path, src: &str) -> Vec<PathBuf> {
    let dir = file.parent().unwrap_or(Path::new("."));

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens);
    let Ok(program) = parser.parse() else {
        return vec![];
    };

    program
        .items
        .iter()
        .filter_map(|item| {
            let ItemKind::Import(ip) = &item.node else {
                return None;
            };
            let first = ip.path.first()?;
            let candidate = dir.join(format!("{}.void", first));
            candidate.exists().then_some(candidate)
        })
        .collect()
}
