// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use crate::parser::ast::*;

use super::*;

fn should_ignore_warning(attributes: &[String], warning_code: &str) -> bool {
    for attr in attributes {
        if attr == "ignore" {
            return true;
        }
        // Check for specific ignore categories
        if warning_code == "W01" || warning_code == "W02" {
            // unused variable/parameter warnings
            if attr == "ignore" || attr.contains("unused_vars") || attr == "str_variadic" {
                return true;
            }
        }
        if warning_code == "W03" || warning_code == "W07" {
            // unused function / dead function warnings
            if attr == "ignore" || attr.contains("dead_code") || attr == "panic_handler" {
                return true;
            }
        }
        if warning_code == "W05" {
            // any-type warnings
            if attr == "ignore" || attr.contains("any_type") {
                return true;
            }
        }
    }
    false
}

impl Analyzer {
    pub(super) fn run_unused_pass(&mut self) {
        let local_scopes = self.finished_scopes.clone();
        for scope in local_scopes {
            self.emit_unused_warnings(scope, false, false);
        }

        let global_symbols: Vec<(String, Symbol)> = self
            .scopes
            .first()
            .expect("semantic analyzer must always keep global scope")
            .iter()
            .map(|(name, symbol)| (name.clone(), symbol.clone()))
            .collect();

        self.emit_unused_warnings(global_symbols, true, true);
    }

    pub(super) fn emit_unused_warnings(
        &mut self,
        symbols: Vec<(String, Symbol)>,
        include_functions: bool,
        include_imports: bool,
    ) {
        for (name, symbol) in symbols {
            if symbol.used {
                continue;
            }

            if symbol.is_import {
                if include_imports {
                    // Module import (e.g. `map`) is considered used if its conventionally-named
                    // exported type (e.g. `Map`) is used — capitalise the first letter and check.
                    let capitalized: String = {
                        let mut chars = name.chars();
                        match chars.next() {
                            None => String::new(),
                            Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                        }
                    };
                    let type_used = self
                        .scopes
                        .first()
                        .and_then(|s| s.get(&capitalized))
                        .is_some_and(|sym| sym.used && matches!(sym.kind, SymbolKind::TypeName));
                    if type_used {
                        continue;
                    }
                    let full = symbol.import_path.clone().unwrap_or_else(|| name.clone());
                    self.unused_import_paths.insert(full.clone());
                    self.push_warning_with_suggestion(
                        symbol.span,
                        "W03",
                        format!("unused import '{}' (path '{}')", name, full),
                        format!("remove import '{}' if it is not needed", full),
                    );
                }
                continue;
            }

            match symbol.kind {
                SymbolKind::Parameter => {
                    if !should_ignore_warning(&symbol.attributes, "W02") {
                        self.push_warning_with_suggestion(
                            symbol.span,
                            "W02",
                            format!("unused parameter '{}'", name),
                            format!("remove or use parameter '{}'", name),
                        );
                    }
                }
                SymbolKind::Variable { mutable } => {
                    if !should_ignore_warning(&symbol.attributes, "W01") {
                        let label = if mutable { "variable" } else { "const" };
                        self.push_warning_with_suggestion(
                            symbol.span,
                            "W01",
                            format!("unused {} '{}'", label, name),
                            format!("remove or use {} '{}'", label, name),
                        );
                    }
                }
                SymbolKind::Function => {
                    if include_functions
                        && name != "main"
                        && !should_ignore_warning(&symbol.attributes, "W03")
                    {
                        self.push_warning_with_suggestion(
                            symbol.span,
                            "W03",
                            format!("unused function '{}'", name),
                            format!("remove function '{}' or call it", name),
                        );
                    }
                }
                SymbolKind::TypeName => {}
            }
        }
    }

    pub(super) fn run_dead_code_pass(&mut self, program: &Program) {
        for item in &program.items {
            // Skip @cfg-disabled items.
            let attrs = match &item.node {
                ItemKind::Fn { attributes, .. } => Some(attributes),
                _ => None,
            };
            if let Some(attrs) = attrs
                && !super::item_should_include(attrs)
            {
                continue;
            }
            match &item.node {
                ItemKind::Fn {
                    body: Some(body), ..
                } => {
                    let _ = self.dead_code_block(body);
                }
                ItemKind::Impl { methods, .. } => {
                    for method in methods {
                        if let ItemKind::Fn {
                            body: Some(body), ..
                        } = &method.node
                        {
                            let _ = self.dead_code_block(body);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn dead_code_block(&mut self, block: &Block) -> bool {
        let mut reachable = true;
        let mut guaranteed_return = false;

        for stmt in &block.stmts {
            if !reachable {
                self.push_warning_with_suggestion(
                    stmt.span,
                    "W04",
                    "unreachable code".to_string(),
                    "remove or move the unreachable statement".to_string(),
                );
                continue;
            }

            if self.dead_code_stmt(stmt) {
                reachable = false;
                guaranteed_return = true;
            }
        }

        guaranteed_return
    }

    pub(super) fn dead_code_stmt(&mut self, stmt: &Stmt) -> bool {
        match &stmt.node {
            StmtKind::Return(_) => true,
            StmtKind::If {
                then_block,
                else_if,
                else_block,
                ..
            } => {
                let mut then_returns = self.dead_code_block(then_block);
                for (_, else_if_block) in else_if {
                    then_returns = self.dead_code_block(else_if_block) && then_returns;
                }
                let else_returns = if let Some(else_block) = else_block {
                    self.dead_code_block(else_block)
                } else {
                    false
                };
                then_returns && else_returns
            }
            StmtKind::For { body, kind } => {
                if let ForLoop::CStyle {
                    init: Some(init_stmt),
                    ..
                } = kind
                {
                    let _ = self.dead_code_stmt(init_stmt);
                }
                let _ = self.dead_code_block(body);
                false
            }
            StmtKind::UnsafeBlock { body } => {
                let _ = self.dead_code_block(body);
                false
            }
            StmtKind::Var { .. } | StmtKind::Const { .. } | StmtKind::ExprStmt(_) => false,
            StmtKind::Break | StmtKind::Continue => true,
            StmtKind::CfgBlock { body, condition } => {
                if crate::semantic::item_should_include(std::slice::from_ref(condition)) {
                    let _ = self.dead_code_block(body);
                }
                false
            }
        }
    }
}
