// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Url};

use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::parser::ast::{ImportItems, ItemKind};
use crate::semantic::{SemanticReport, SymbolTableEntry};

use super::hover::word_at_offset;
use super::span::{position_to_byte_offset, position_to_char_offset, span_to_range};
use super::workspace::WorkspaceIndex;

pub fn goto_definition(
    report: &SemanticReport,
    source: &str,
    uri: &Url,
    pos: Position,
) -> Option<GotoDefinitionResponse> {
    let offset = position_to_byte_offset(pos, source)?;
    let char_offset = position_to_char_offset(pos, source)?;
    let word = word_at_offset(source, offset)?;

    let resolved_call = report
        .annotated_exprs
        .iter()
        .filter(|annotation| {
            annotation.span.start <= char_offset && char_offset < annotation.span.end
        })
        .min_by_key(|annotation| annotation.span.end - annotation.span.start)
        .and_then(|annotation| annotation.resolved_fn.as_deref());

    let entry = resolved_call
        .and_then(|name| definition_for_name(report, name, char_offset))
        .or_else(|| definition_for_name(report, word, char_offset))?;

    Some(GotoDefinitionResponse::Scalar(Location {
        uri: uri.clone(),
        range: span_to_range(entry.symbol.span, source),
    }))
}

/// Returns the imported target URI and original exported name for a cursor on
/// a relative one-file leaf import binding. Broader imports deliberately stay
/// unsupported until the LSP uses the compiler loader's source mapping.
pub fn relative_leaf_import_target(
    source: &str,
    uri: &Url,
    pos: Position,
    workspace: &WorkspaceIndex,
) -> Option<(Url, String)> {
    let offset = position_to_byte_offset(pos, source)?;
    let word = word_at_offset(source, offset)?;
    let mut lexer = Lexer::new(source);
    let mut parser = Parser::new_with_source(lexer.tokenize(), source);
    let program = parser.parse().ok()?;

    for item in program.items {
        let ItemKind::Import(import) = item.node else {
            continue;
        };
        if !import.relative || import.path.len() != 1 {
            continue;
        }
        let (exported, local) = match import.items {
            ImportItems::Single(name) => (name.clone(), name),
            ImportItems::Aliased(name, alias) => (name, alias),
            ImportItems::Multiple(_) | ImportItems::All => continue,
        };
        if local != word {
            continue;
        }
        let target = workspace.relative_leaf_target(uri, &import.path[0])?;
        return Some((target, exported));
    }
    None
}

pub fn public_top_level_definition(
    report: &SemanticReport,
    source: &str,
    uri: &Url,
    name: &str,
) -> Option<GotoDefinitionResponse> {
    let mut entries = report.symbol_table.entries.iter().filter(|entry| {
        entry.scope_depth == 0
            && entry.name == name
            && entry.symbol.public
            && entry.symbol.span.end > entry.symbol.span.start
    });
    let entry = entries.next()?;
    if entries.next().is_some() {
        return None;
    }
    Some(GotoDefinitionResponse::Scalar(Location {
        uri: uri.clone(),
        range: span_to_range(entry.symbol.span, source),
    }))
}

fn definition_for_name<'a>(
    report: &'a SemanticReport,
    name: &str,
    cursor: usize,
) -> Option<&'a SymbolTableEntry> {
    report
        .symbol_table
        .entries
        .iter()
        .filter(|entry| {
            (entry.name == name || entry.name.rsplit('.').next() == Some(name))
                && entry.symbol.span.end > entry.symbol.span.start
        })
        .max_by_key(|entry| {
            (
                entry.scope_depth,
                usize::from(entry.symbol.span.start <= cursor),
                entry.symbol.span.start,
            )
        })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{goto_definition, public_top_level_definition, relative_leaf_import_target};
    use crate::lsp::analysis::analyze_source;
    use crate::lsp::workspace::WorkspaceIndex;
    use tower_lsp::lsp_types::{
        GotoDefinitionResponse, InitializeParams, Position, Url, WorkspaceFolder,
    };

    #[test]
    fn resolves_a_method_call_using_the_semantic_function_name() {
        let source = r#"
struct Counter { value: i32 }
impl Counter {
    fn get(self: Counter) i32 { ret self.value; }
}
fn main() i32 {
    var counter = Counter { value: 1 };
    ret counter.get();
}
"#;
        let report = analyze_source(source).expect("analyze source");
        let position = Position::new(7, 16);
        let uri = Url::parse("file:///workspace/main.qz").expect("URI");

        let Some(GotoDefinitionResponse::Scalar(location)) =
            goto_definition(&report, source, &uri, position)
        else {
            panic!("expected a definition location");
        };
        assert_eq!(location.uri, uri);
        assert_eq!(location.range.start.line, 3);
    }

    #[test]
    fn prefers_the_current_function_local_binding() {
        let source = r#"
fn first() i32 {
    const value: i32 = 1;
    ret value;
}
fn main() i32 {
    const value: i32 = 2;
    ret value;
}
"#;
        let report = analyze_source(source).expect("analyze source");
        let position = Position::new(7, 8);
        let uri = Url::parse("file:///workspace/main.qz").expect("URI");

        let Some(GotoDefinitionResponse::Scalar(location)) =
            goto_definition(&report, source, &uri, position)
        else {
            panic!("expected a definition location");
        };
        assert_eq!(location.range.start.line, 6);
    }

    #[test]
    fn resolves_only_relative_leaf_imports_to_public_target_declarations() {
        let root = std::env::temp_dir().join(format!(
            "quazi_lsp_relative_definition_{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create workspace");
        let target_path = root.join("helper.qz");
        let target_source = "pub fn exported() i32 { ret 1; }\nfn private() i32 { ret 2; }";
        fs::write(&target_path, target_source).expect("write target");
        let importer_path = root.join("main.qz");
        fs::write(
            &importer_path,
            "import ./helper.exported as local;\nfn main() i32 { ret local(); }",
        )
        .expect("write importer");
        let importer_uri = Url::from_file_path(&importer_path).expect("importer URI");
        let workspace = WorkspaceIndex::from_initialize_params(&InitializeParams {
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: Url::from_file_path(&root).expect("workspace URI"),
                name: "test".to_string(),
            }]),
            ..Default::default()
        });
        let source = "import ./helper.exported as local;\nfn main() i32 { ret local(); }";
        let (uri, exported) =
            relative_leaf_import_target(source, &importer_uri, Position::new(1, 20), &workspace)
                .expect("relative alias target");
        assert_eq!(uri, Url::from_file_path(&target_path).expect("target URI"));
        assert_eq!(exported, "exported");
        let target = workspace.documents().get(&uri).expect("indexed target");
        let Some(GotoDefinitionResponse::Scalar(location)) =
            public_top_level_definition(&target.report, &target.source, &uri, &exported)
        else {
            panic!("public target definition");
        };
        assert_eq!(location.uri, uri);
        assert_eq!(location.range.start.line, 0);
        assert!(
            public_top_level_definition(&target.report, &target.source, &uri, "private").is_none()
        );
        fs::remove_dir_all(root).expect("remove workspace");
    }
}
