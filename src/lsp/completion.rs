// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, CompletionResponse, Position};

use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::parser::ast::ItemKind;
use crate::semantic::{SemanticReport, SymbolKind};

use super::span::{position_to_byte_offset, position_to_char_offset};

pub fn complete_at(source: &str, pos: Position) -> Option<CompletionResponse> {
    complete_with_report(source, pos, None)
}

/// Complete source identifiers from the current semantic snapshot. The report
/// is optional because malformed, in-progress documents may not parse; those
/// documents retain the narrower `std.*` completion behavior.
pub fn complete_with_report(
    source: &str,
    pos: Position,
    report: Option<&SemanticReport>,
) -> Option<CompletionResponse> {
    let byte_offset = position_to_byte_offset(pos, source)?;
    let char_offset = position_to_char_offset(pos, source)?;
    let before = &source[..byte_offset.min(source.len())];
    if let Some(chain) = dotted_chain_before_cursor(before)
        && chain.first().map(String::as_str) == Some("std")
    {
        let std_src = find_std_src_dir()?;
        let items = complete_std_chain(&std_src, &chain);
        return (!items.is_empty()).then_some(CompletionResponse::Array(items));
    }

    report
        .map(|report| symbol_items_from_report(report, char_offset))
        .filter(|items| !items.is_empty())
        .map(CompletionResponse::Array)
}

fn symbol_items_from_report(report: &SemanticReport, offset: usize) -> Vec<CompletionItem> {
    let mut items = BTreeMap::new();
    for entry in &report.symbol_table.entries {
        // Imported LSP library symbols have a synthetic zero span. All other
        // symbols must have been declared before the cursor to be useful.
        if entry.symbol.span.start != 0 && entry.symbol.span.start > offset {
            continue;
        }
        let kind = match entry.symbol.kind {
            SymbolKind::Function => CompletionItemKind::FUNCTION,
            SymbolKind::Variable { .. } => CompletionItemKind::VARIABLE,
            SymbolKind::Parameter => CompletionItemKind::VARIABLE,
            SymbolKind::TypeName => CompletionItemKind::CLASS,
        };
        let detail = match (&entry.symbol.kind, &entry.symbol.ty) {
            (SymbolKind::Function, Some(return_ty)) => {
                let params = entry
                    .symbol
                    .params
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                Some(format!("fn {}({params}) {return_ty}", entry.name))
            }
            (_, Some(ty)) => Some(ty.to_string()),
            _ => None,
        };
        items
            .entry(entry.name.clone())
            .or_insert_with(|| CompletionItem {
                label: entry.name.clone(),
                kind: Some(kind),
                detail,
                ..Default::default()
            });
    }
    items.into_values().collect()
}

fn dotted_chain_before_cursor(before: &str) -> Option<Vec<String>> {
    let trimmed = before.trim_end();
    if !trimmed.ends_with('.') {
        return None;
    }

    let chain_start = trimmed
        .char_indices()
        .rev()
        .find(|(_, ch)| !is_ident_char(*ch) && *ch != '.')
        .map(|(idx, ch)| idx + ch.len_utf8())
        .unwrap_or(0);
    let chain = &trimmed[chain_start..trimmed.len().saturating_sub(1)];
    if chain.is_empty() {
        return None;
    }

    let segments: Vec<String> = chain
        .split('.')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect();
    if segments.is_empty() {
        None
    } else {
        Some(segments)
    }
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn complete_std_chain(std_src: &Path, chain: &[String]) -> Vec<CompletionItem> {
    let relative = &chain[1..];
    let dir = path_for_segments(std_src, relative);
    let mut items = module_items_from_dir(&dir, "std", relative);

    if let Some(file) = module_file_for_segments(std_src, relative) {
        items.extend(symbol_items_from_file(&file));
    }

    items
}

fn path_for_segments(root: &Path, segments: &[String]) -> PathBuf {
    let mut path = root.to_path_buf();
    for segment in segments {
        path.push(segment);
    }
    path
}

fn module_file_for_segments(root: &Path, segments: &[String]) -> Option<PathBuf> {
    if segments.is_empty() {
        return None;
    }
    let mut path = path_for_segments(root, segments);
    path.set_extension("qz");
    path.exists().then_some(path)
}

fn module_items_from_dir(dir: &Path, base: &str, relative: &[String]) -> Vec<CompletionItem> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut names = BTreeSet::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                names.insert(name.to_string());
            }
        } else if path.extension().and_then(|s| s.to_str()) == Some("qz")
            && let Some(name) = path.file_stem().and_then(|s| s.to_str())
        {
            names.insert(name.to_string());
        }
    }

    names
        .into_iter()
        .map(|name| {
            let mut full_path = Vec::with_capacity(relative.len() + 2);
            full_path.push(base.to_string());
            full_path.extend(relative.iter().cloned());
            full_path.push(name.clone());
            CompletionItem {
                label: name,
                kind: Some(CompletionItemKind::MODULE),
                detail: Some(full_path.join(".")),
                ..Default::default()
            }
        })
        .collect()
}

fn symbol_items_from_file(path: &Path) -> Vec<CompletionItem> {
    let Ok(source) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens);
    let Ok(program) = parser.parse() else {
        return Vec::new();
    };

    let mut items = Vec::new();
    for item in program.items {
        match item.node {
            ItemKind::Fn {
                name,
                params,
                return_ty,
                pub_fn,
                ..
            } if pub_fn => {
                let params = params
                    .iter()
                    .map(|param| format!("{}: {}", param.name, param.ty.node))
                    .collect::<Vec<_>>()
                    .join(", ");
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(format!("fn {name}({params}) {}", return_ty.node)),
                    ..Default::default()
                });
            }
            ItemKind::Struct { name, .. } => {
                items.push(CompletionItem {
                    label: name,
                    kind: Some(CompletionItemKind::STRUCT),
                    ..Default::default()
                });
            }
            ItemKind::Enum { name, .. } => {
                items.push(CompletionItem {
                    label: name,
                    kind: Some(CompletionItemKind::ENUM),
                    ..Default::default()
                });
            }
            ItemKind::Trait { name, .. } => {
                items.push(CompletionItem {
                    label: name,
                    kind: Some(CompletionItemKind::INTERFACE),
                    ..Default::default()
                });
            }
            _ => {}
        }
    }
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

fn find_std_src_dir() -> Option<PathBuf> {
    crate::loader::find_builtin_std_root().map(|p| p.join("src"))
}

#[cfg(test)]
mod tests {
    use super::{complete_at, complete_with_report};
    use crate::lsp::analysis;
    use tower_lsp::lsp_types::{CompletionItemKind, CompletionResponse, Position};

    fn labels(response: CompletionResponse) -> Vec<String> {
        let CompletionResponse::Array(items) = response else {
            panic!("expected array completion");
        };
        items.into_iter().map(|item| item.label).collect()
    }

    #[test]
    fn completes_std_modules_after_dot_from_std_src() {
        let response = complete_at("import std.", Position::new(0, 11)).expect("completion");
        let labels = labels(response);
        assert!(labels.contains(&"core".to_string()));
        assert!(labels.contains(&"unix".to_string()));
        assert!(labels.contains(&"windows".to_string()));
    }

    #[test]
    fn completes_symbols_from_std_module_file() {
        let response = complete_at("import std.core.", Position::new(0, 16)).expect("completion");
        let CompletionResponse::Array(items) = response else {
            panic!("expected array completion");
        };
        let write = items
            .iter()
            .find(|item| item.label == "write")
            .expect("write completion");
        assert_eq!(write.kind, Some(CompletionItemKind::FUNCTION));
    }

    #[test]
    fn does_not_complete_outside_std_dot() {
        assert!(complete_at("import st", Position::new(0, 9)).is_none());
    }

    #[test]
    fn completes_symbols_declared_before_the_cursor() {
        let source = r#"
fn helper(value: i32) i32 { ret value; }

fn main() i32 {
    const answer: i32 = 42;
    helper(answer);
    ret answer;
}
"#;
        let report = analysis::analyze_source(source).expect("analyze source");
        let response =
            complete_with_report(source, Position::new(4, 4), Some(&report)).expect("completion");
        let CompletionResponse::Array(items) = response else {
            panic!("expected array completion");
        };
        let answer = items
            .iter()
            .find(|item| item.label == "answer")
            .expect("local completion");
        assert_eq!(answer.kind, Some(CompletionItemKind::VARIABLE));
        assert!(items.iter().any(|item| item.label == "helper"));
    }
}
