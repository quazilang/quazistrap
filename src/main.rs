// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

pub mod lexer;
pub mod parser;
pub mod semantic;

use lexer::Lexer;
use parser::Parser;
use semantic::Analyzer;

fn main() {
    let src = r#"
import std.io.stdout;

fn main() void {
    const z = "hi";
    const y: int32 = 10;
    var x: int32 = 5;

    while (x < y) {
        stdout.println("{} void! x = {}",z, x);
        x = x + 1;
    }
}
"#;

    println!("== void bootstrap frontend ==");

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();

    println!("lexed {} tokens", tokens.len());
    for token in &tokens {
        println!(
            "{} @ {}:{} [{}..{}]",
            token.kind, token.span.line, token.span.col, token.span.start, token.span.end
        );
    }

    let mut parser = Parser::new_with_source(tokens, src);
    match parser.parse() {
        Ok(program) => {
            println!("parse success");
            println!("{:#?}", program);

            let mut analyzer = Analyzer::new();
            let sema_report = analyzer.analyze_program(&program);

            if !sema_report.warnings.is_empty() {
                eprintln!(
                    "semantic analysis emitted {} warning(s):",
                    sema_report.warnings.len()
                );
                for warning in &sema_report.warnings {
                    eprintln!("- {}", warning);
                }
            }

            if !sema_report.suggestions.is_empty() {
                eprintln!(
                    "semantic analysis suggestions ({}):",
                    sema_report.suggestions.len()
                );
                for suggestion in &sema_report.suggestions {
                    eprintln!("- {}", suggestion.message);
                }
            }

            if !sema_report.used_imports.is_empty() {
                println!("used imports: {}", sema_report.used_imports.join(", "));
            }

            if !sema_report.used_imports_map.is_empty() {
                println!(
                    "used imports map entries: {}",
                    sema_report.used_imports_map.len()
                );
            }

            if !sema_report.unused_imports.is_empty() {
                eprintln!("unused imports: {}", sema_report.unused_imports.join(", "));
            }

            if !sema_report.inline_candidates.is_empty() {
                println!(
                    "inline candidates: {}",
                    sema_report
                        .inline_candidates
                        .iter()
                        .map(|c| c.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }

            if !sema_report.constant_evaluations.is_empty() {
                println!(
                    "constant evaluations: {}",
                    sema_report.constant_evaluations.len()
                );
            }

            println!(
                "symbol table entries: {}",
                sema_report.symbol_table.entries.len()
            );
            println!(
                "dependency edges: {}",
                sema_report.dependency_graph.edges.len()
            );
            println!(
                "optimization hints: const={}, inline={}, removable_imports={}",
                sema_report.optimization_hints.constant_evaluations.len(),
                sema_report.optimization_hints.inline_candidates.len(),
                sema_report.optimization_hints.removable_imports.len()
            );

            if !sema_report.errors.is_empty() {
                eprintln!(
                    "semantic analysis failed with {} error(s):",
                    sema_report.errors.len()
                );
                for err in sema_report.errors {
                    eprintln!("- {}", err);
                }
                std::process::exit(1);
            }

            println!("semantic analysis success");
        }
        Err(err) => {
            eprintln!("{}", err);
            std::process::exit(1);
        }
    }
}
