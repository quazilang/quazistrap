use crate::lexer::Lexer;
use crate::lexer::token::TokenKind;
use crate::parser::Parser;
use crate::parser::ast::{ImportItems, ItemKind, Program, Span};
use crate::semantic::{Analyzer, SemanticReport, Symbol, SymbolKind};

use std::collections::HashSet;
use std::path::PathBuf;

pub fn analyze_source(source: &str) -> Result<SemanticReport, String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new_with_source(tokens, source);
    let program = parser.parse()?;
    let mut analyzer = Analyzer::new();
    let (library_fn_names, library_symbols) = std_symbols_for_source(source, &program);
    analyzer.set_library_fns(library_fn_names);
    analyzer.set_library_symbols(library_symbols);
    Ok(analyzer.analyze_program(&program))
}

fn std_symbols_for_source(
    source: &str,
    program: &Program,
) -> (HashSet<String>, Vec<(String, Symbol)>) {
    let Some(std_src_dir) = find_std_src_dir() else {
        return (HashSet::new(), Vec::new());
    };

    if source_contains_no_std(source) {
        return (HashSet::new(), Vec::new());
    }

    let user_fn_names = user_function_names(program);
    let explicitly_imported = explicitly_imported_std_leaf_names(program);
    let shadowed_names: HashSet<String> = user_fn_names
        .difference(&explicitly_imported)
        .cloned()
        .collect();

    let mut modules = used_std_modules(source);
    for item in &program.items {
        let ItemKind::Import(import_path) = &item.node else {
            continue;
        };

        let Some((base, remainder)) = import_base_and_remainder(import_path) else {
            continue;
        };

        if base != "std" {
            continue;
        }

        let module = remainder
            .first()
            .cloned()
            .unwrap_or_else(|| "core".to_string());
        modules.insert(module);
    }

    if modules.is_empty() {
        modules.insert("core".to_string());
    }

    let mut names = HashSet::new();
    let mut symbols = Vec::new();
    for module in modules {
        let path = std_src_dir.join(format!("{module}.void"));
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(lib_program) = parse_program(&src) else {
            continue;
        };

        for item in lib_program.items {
            if let ItemKind::Fn {
                name,
                return_ty,
                params,
                attributes,
                pub_fn,
                unsafe_fn,
                ..
            } = item.node
            {
                if shadowed_names.contains(&name) {
                    continue;
                }
                let is_syscall_or_api = attributes
                    .iter()
                    .any(|a| matches!(a.name.as_str(), "syscall" | "api"));
                names.insert(name.clone());
                symbols.push((
                    name,
                    Symbol {
                        kind: SymbolKind::Function,
                        ty: Some(return_ty.node),
                        span: zero_span(),
                        params: params.into_iter().map(|p| p.ty.node).collect(),
                        used: true,
                        initialized: true,
                        is_import: false,
                        import_path: None,
                        const_value: None,
                        variadic: false,
                        attributes: attributes.into_iter().map(|a| a.name).collect(),
                        public: pub_fn,
                        unsafe_fn: unsafe_fn || is_syscall_or_api,
                    },
                ));
            }
        }
    }

    (names, symbols)
}

fn user_function_names(program: &Program) -> HashSet<String> {
    program
        .items
        .iter()
        .filter_map(|item| match &item.node {
            ItemKind::Fn { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

fn explicitly_imported_std_leaf_names(program: &Program) -> HashSet<String> {
    let mut names = HashSet::new();
    for item in &program.items {
        let ItemKind::Import(import_path) = &item.node else {
            continue;
        };
        let Some((base, _)) = import_base_and_remainder(import_path) else {
            continue;
        };
        if base != "std" {
            continue;
        }
        match &import_path.items {
            ImportItems::Single(name) | ImportItems::Aliased(name, _) => {
                names.insert(name.clone());
            }
            ImportItems::Multiple(items) => {
                names.extend(items.iter().cloned());
            }
            ImportItems::All => {}
        }
    }
    names
}

fn source_contains_no_std(source: &str) -> bool {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    tokens.windows(2).any(|pair| {
        matches!(pair[0].kind, TokenKind::At)
            && matches!(&pair[1].kind, TokenKind::Ident(name) if name == "no_std")
    })
}

fn used_std_modules(source: &str) -> HashSet<String> {
    let mut lexer = Lexer::new(source);
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

fn parse_program(source: &str) -> Result<Program, String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new_with_source(tokens, source);
    parser.parse()
}

fn import_base_and_remainder(
    import_path: &crate::parser::ast::ImportPath,
) -> Option<(String, Vec<String>)> {
    match &import_path.items {
        ImportItems::Single(name) | ImportItems::Aliased(name, _) => {
            let mut segments = import_path.path.clone();
            segments.push(name.clone());
            let base = segments.first()?.clone();
            let remainder = segments.into_iter().skip(1).collect();
            Some((base, remainder))
        }
        ImportItems::Multiple(_) | ImportItems::All => {
            let base = import_path.path.first()?.clone();
            let remainder = import_path.path.iter().skip(1).cloned().collect();
            Some((base, remainder))
        }
    }
}

fn find_std_src_dir() -> Option<PathBuf> {
    if let Ok(root) = std::env::var("VOID_STD_ROOT") {
        let path = PathBuf::from(root).join("src");
        if path.exists() {
            return Some(path);
        }
    }

    let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("std")
        .join("src");
    if manifest_path.exists() {
        return Some(manifest_path);
    }

    let cwd_path = std::env::current_dir().ok()?.join("std").join("src");
    if cwd_path.exists() {
        return Some(cwd_path);
    }

    None
}

fn zero_span() -> Span {
    Span {
        line: 0,
        col: 0,
        start: 0,
        end: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_explicit_std_function_import() {
        let report = analyze_source(
            r#"
import std.core.write;

fn main() void {
    write(1, "x", 1);
    ret;
}
"#,
        )
        .expect("analyze source");

        assert!(
            report.errors.is_empty(),
            "expected std.core.write to resolve, got {:?}",
            report.errors
        );
    }

    #[test]
    fn resolves_qualified_std_function_call() {
        let report = analyze_source(
            r#"
import std;

fn main() void {
    std.core.write(1, "x", 1);
    ret;
}
"#,
        )
        .expect("analyze source");

        assert!(
            report.errors.is_empty(),
            "expected std.core.write qualified call to resolve, got {:?}",
            report.errors
        );
    }

    #[test]
    fn keeps_unqualified_std_function_out_of_scope_without_leaf_import() {
        let report = analyze_source(
            r#"
import std;

fn main() void {
    write(1, "x", 1);
    ret;
}
"#,
        )
        .expect("analyze source");

        assert!(
            report
                .errors
                .iter()
                .any(|err| err.message.contains("'write' is not in scope")),
            "expected unqualified std write to require explicit import, got {:?}",
            report.errors
        );
    }

    #[test]
    fn user_function_shadows_unimported_std_function() {
        let report = analyze_source(
            r#"
import std.core.write;
import std;

fn exit() void {
    unsafe { std.windows.exit_process(67); }
}

fn main() i32 {
    const msg: str = "hey!\n";
    write(1, msg, msg.len());
    exit();
    ret 0;
}
"#,
        )
        .expect("analyze source");

        assert!(
            report.errors.is_empty(),
            "expected user exit to shadow unimported std exit, got {:?}",
            report.errors
        );
    }
}
