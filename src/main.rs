// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

pub mod lexer;
pub mod parser;
pub mod sema;

use lexer::Lexer;
use parser::Parser;
use sema::Analyzer;

fn main() {
    let src = r#"
import std.io.stdout;

fn main() void {
    const y: int32 = 10;
    var x: int32 = 5;

    while (x < y) {
        stdout.println("hello, void! x = {}", x);
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
            "{:?} @ {}:{} [{}..{}]",
            token.kind, token.span.line, token.span.col, token.span.start, token.span.end
        );
    }

    let mut parser = Parser::new(tokens);
    match parser.parse() {
        Ok(program) => {
            println!("parse success");
            println!("{:#?}", program);

            let mut analyzer = Analyzer::new();
            let sema_errors = analyzer.analyze_program(&program);
            if !sema_errors.is_empty() {
                eprintln!("semantic analysis failed with {} error(s):", sema_errors.len());
                for err in sema_errors {
                    eprintln!("- {}", err);
                }
                std::process::exit(1);
            }

            println!("semantic analysis success");
        }
        Err(err) => {
            eprintln!("parse error: {}", err);
            std::process::exit(1);
        }
    }
}
