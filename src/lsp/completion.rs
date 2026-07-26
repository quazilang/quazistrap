use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, CompletionResponse, Position};

use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::parser::ast::ItemKind;

use super::span::position_to_byte_offset;

pub fn complete_at(source: &str, pos: Position) -> Option<CompletionResponse> {
    let offset = position_to_byte_offset(pos, source)?;
    let before = &source[..offset.min(source.len())];
    let chain = dotted_chain_before_cursor(before)?;

    if chain.first().map(String::as_str) != Some("std") {
        return None;
    }

    let std_src = find_std_src_dir()?;
    let items = complete_std_chain(&std_src, &chain);
    if items.is_empty() {
        None
    } else {
        Some(CompletionResponse::Array(items))
    }
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
    use super::complete_at;
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
}
