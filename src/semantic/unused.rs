// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use crate::parser::ast::*;

use super::*;

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
                    let full = symbol
                        .import_path
                        .clone()
                        .unwrap_or_else(|| name.clone());
                    self.unused_import_paths.insert(full.clone());
                    self.push_warning(
                        symbol.span,
                        format!("unused import '{}' (path '{}')", name, full),
                    );
                    self.push_suggestion(
                        Some(symbol.span),
                        format!("remove import '{}' if it is not needed", full),
                    );
                }
                continue;
            }

            match symbol.kind {
                SymbolKind::Parameter => {
                    self.push_warning(symbol.span, format!("unused parameter '{}'", name));
                    self.push_suggestion(
                        Some(symbol.span),
                        format!("remove or use parameter '{}'", name),
                    );
                }
                SymbolKind::Variable { mutable } => {
                    let label = if mutable { "variable" } else { "const" };
                    self.push_warning(symbol.span, format!("unused {} '{}'", label, name));
                    self.push_suggestion(
                        Some(symbol.span),
                        format!("remove or use {} '{}'", label, name),
                    );
                }
                SymbolKind::Function => {
                    if include_functions && name != "main" {
                        self.push_warning(symbol.span, format!("unused function '{}'", name));
                        self.push_suggestion(
                            Some(symbol.span),
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
            match &item.node {
                ItemKind::Fn { body, .. } => {
                    let _ = self.dead_code_block(body);
                }
                ItemKind::Impl { methods, .. } => {
                    for method in methods {
                        if let ItemKind::Fn { body, .. } = &method.node {
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
                self.push_warning(stmt.span, "unreachable code".to_string());
                self.push_suggestion(
                    Some(stmt.span),
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
                else_block,
                ..
            } => {
                let then_returns = self.dead_code_block(then_block);
                let else_returns = if let Some(else_block) = else_block {
                    self.dead_code_block(else_block)
                } else {
                    false
                };
                then_returns && else_returns
            }
            StmtKind::While { body, .. } => {
                let _ = self.dead_code_block(body);
                false
            }
            StmtKind::Var { .. } | StmtKind::Const { .. } | StmtKind::ExprStmt(_) => false,
        }
    }
}
