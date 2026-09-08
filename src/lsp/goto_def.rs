// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Url};

use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::parser::ast::{ImportItems, ItemKind, Span};
use crate::semantic::{SemanticReport, SymbolTableEntry};

use super::analysis::LoadedSnapshot;
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

/// Resolve a binding using the compiler's complete configured import graph.
/// The request position is converted from the open document into merged-source
/// coordinates, then the semantic declaration span is rebased through the
/// loader-owned source map.
pub fn loaded_goto_definition(
    snapshot: &LoadedSnapshot,
    source: &str,
    uri: &Url,
    pos: Position,
) -> Option<GotoDefinitionResponse> {
    let local_offset = position_to_char_offset(pos, source)?;
    let path = uri.to_file_path().ok()?.canonicalize().ok()?;
    let file = snapshot
        .source_files
        .iter()
        .find(|file| std::path::Path::new(&file.path) == path)?;
    let merged_offset = file.start.checked_add(local_offset)?;
    if merged_offset >= file.end {
        return None;
    }
    let binding = snapshot
        .report
        .annotated_exprs
        .iter()
        .filter(|annotation| {
            annotation.span.start <= merged_offset && merged_offset < annotation.span.end
        })
        .min_by_key(|annotation| annotation.span.end - annotation.span.start)
        .and_then(|annotation| annotation.resolved_binding.as_ref())?;
    location_for_loaded_span(snapshot, binding.span).map(GotoDefinitionResponse::Scalar)
}

fn location_for_loaded_span(snapshot: &LoadedSnapshot, span: Span) -> Option<Location> {
    let file = snapshot
        .source_files
        .iter()
        .find(|file| file.contains(span))?;
    let path = std::path::PathBuf::from(&file.path);
    let source = snapshot.effective_sources.get(&path)?;
    let start = span.start.checked_sub(file.start)?;
    let end = span.end.checked_sub(file.start)?;
    if end > source.chars().count() {
        return None;
    }
    Some(Location {
        uri: Url::from_file_path(path).ok()?,
        range: span_to_range(Span::new(0, 0, start, end), source),
    })
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
    use std::collections::HashMap;
    use std::fs;

    use super::{
        goto_definition, loaded_goto_definition, public_top_level_definition,
        relative_leaf_import_target,
    };
    use crate::lsp::analysis::{analyze_loaded_document, analyze_source};
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

    #[test]
    fn resolves_std_imports_with_loader_source_map_and_utf16_ranges() {
        let root = std::env::temp_dir().join(format!(
            "quazi_lsp_loaded_std_definition_{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create workspace");
        let main_path = root.join("main.qz");
        let source = "import std.core.write;\n// 🚀\nfn main() void { unsafe { write(1, \"x\", 1); } ret; }\n";
        fs::write(&main_path, source).expect("write source");
        let snapshot = analyze_loaded_document(&main_path, &HashMap::new()).expect("load source");
        let uri = Url::from_file_path(&main_path).expect("URI");

        let Some(GotoDefinitionResponse::Scalar(location)) =
            loaded_goto_definition(&snapshot, source, &uri, Position::new(2, 27))
        else {
            panic!("expected standard-library definition");
        };
        let std_core = crate::loader::find_builtin_std_root()
            .expect("standard library")
            .join("src/core.qz");
        assert_eq!(
            location.uri,
            Url::from_file_path(std_core).expect("std core URI")
        );
        assert_eq!(location.range.start.line, 13);
        fs::remove_dir_all(root).expect("remove workspace");
    }

    #[test]
    fn loaded_definition_uses_unsaved_importer_and_target_overlays() {
        let root = std::env::temp_dir().join(format!(
            "quazi_lsp_loaded_overlay_definition_{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create workspace");
        let helper_path = root.join("helper.qz");
        fs::write(&helper_path, "pub fn exported() i32 { ret 1; }\n").expect("write helper");
        let main_path = root.join("main.qz");
        let main_source = "import helper.exported as local;\nfn main() i32 { ret local(); }\n";
        fs::write(&main_path, "fn main() i32 { ret 0; }\n").expect("write disk main");
        let overlay_source = "// unsaved\n\npub fn exported() i32 { ret 2; }\n";
        let mut overlays = HashMap::new();
        overlays.insert(
            main_path.canonicalize().expect("canonical main"),
            main_source.to_string(),
        );
        overlays.insert(
            helper_path.canonicalize().expect("canonical helper"),
            overlay_source.to_string(),
        );
        let snapshot = analyze_loaded_document(&main_path, &overlays).expect("load source");
        let uri = Url::from_file_path(&main_path).expect("URI");

        assert!(
            snapshot.report.errors.is_empty(),
            "unexpected analysis errors: {:?}",
            snapshot.report.errors
        );

        let Some(GotoDefinitionResponse::Scalar(location)) =
            loaded_goto_definition(&snapshot, main_source, &uri, Position::new(1, 21))
        else {
            panic!("expected overlay definition");
        };
        assert_eq!(
            location.uri,
            Url::from_file_path(&helper_path).expect("helper URI")
        );
        assert_eq!(location.range.start.line, 2);
        fs::remove_dir_all(root).expect("remove workspace");
    }

    #[test]
    fn loaded_snapshot_uses_unsaved_importer_and_gateway_overlays() {
        let root = std::env::temp_dir().join(format!(
            "quazi_lsp_loaded_gateway_overlay_{}",
            std::process::id()
        ));
        let library = root.join("library");
        fs::create_dir_all(&library).expect("create workspace");
        let main_path = root.join("main.qz");
        fs::write(&main_path, "fn main() i32 { ret 0; }\n").expect("write disk main");
        let gateway_path = library.join("mod.qz");
        fs::write(&gateway_path, "pub import old.exported;\n").expect("write disk gateway");
        fs::write(
            &library.join("old.qz"),
            "pub fn exported() i32 { ret 1; }\n",
        )
        .expect("write old target");
        let new_path = library.join("new.qz");
        fs::write(&new_path, "pub fn exported() i32 { ret 2; }\n").expect("write new target");
        let main_source = "// unsaved importer prefix\nimport library.exported;\nfn main() i32 { ret exported(); }\n";
        let mut overlays = HashMap::new();
        overlays.insert(
            main_path.canonicalize().expect("canonical main"),
            main_source.to_string(),
        );
        overlays.insert(
            gateway_path.canonicalize().expect("canonical gateway"),
            "pub import new.exported;\n".to_string(),
        );
        let snapshot = analyze_loaded_document(&main_path, &overlays).expect("load source");
        assert!(
            snapshot
                .effective_sources
                .contains_key(&new_path.canonicalize().expect("canonical new target"))
        );
        assert!(
            !snapshot.effective_sources.contains_key(
                &library
                    .join("old.qz")
                    .canonicalize()
                    .expect("canonical old target")
            )
        );
        assert_eq!(
            snapshot
                .effective_sources
                .get(&main_path.canonicalize().expect("canonical main")),
            Some(&main_source.to_string())
        );
        fs::remove_dir_all(root).expect("remove workspace");
    }

    #[test]
    fn resolves_a_local_package_dependency_through_the_loader() {
        let root = std::env::temp_dir().join(format!(
            "quazi_lsp_loaded_package_definition_{}",
            std::process::id()
        ));
        let app_src = root.join("app/src");
        let dep_src = root.join("dep/src");
        fs::create_dir_all(&app_src).expect("create app source");
        fs::create_dir_all(&dep_src).expect("create dependency source");
        fs::write(
            root.join("app/quazi.toml"),
            "[package]\nname = \"app\"\nstd = false\n\n[[bin]]\nname = \"app\"\npath = \"src/main.qz\"\n\n[dependencies]\ndep = { path = \"../dep\" }\n",
        )
        .expect("write app manifest");
        fs::write(
            root.join("dep/quazi.toml"),
            "[package]\nname = \"dep\"\nstd = false\n\n[lib]\nname = \"dep\"\npath = \"src/lib.qz\"\n",
        )
        .expect("write dependency manifest");
        let dependency_path = dep_src.join("lib.qz");
        fs::write(&dependency_path, "pub fn answer() i32 { ret 42; }\n")
            .expect("write dependency source");
        let main_path = app_src.join("main.qz");
        let main_source = "import dep.answer;\nfn main() i32 { ret answer(); }\n";
        fs::write(&main_path, main_source).expect("write app source");
        let snapshot = analyze_loaded_document(&main_path, &HashMap::new()).expect("load source");
        let uri = Url::from_file_path(&main_path).expect("URI");

        let Some(GotoDefinitionResponse::Scalar(location)) =
            loaded_goto_definition(&snapshot, main_source, &uri, Position::new(1, 24))
        else {
            panic!("expected package definition");
        };
        assert_eq!(
            location.uri,
            Url::from_file_path(&dependency_path).expect("dependency URI")
        );
        assert_eq!(location.range.start.line, 0);
        fs::remove_dir_all(root).expect("remove workspace");
    }
}
