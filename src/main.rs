// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

pub mod lexer;
pub mod parser;

use lexer::Lexer;
use parser::Parser;

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
        }
        Err(err) => {
            eprintln!("parse error: {}", err);
            std::process::exit(1);
        }
    }
}
