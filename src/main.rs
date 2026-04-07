// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

pub mod lexer;
pub mod parser;
use parser::Parser;

use lexer::Lexer;

fn main() {
    println!("start");
    let src = r#"
import std.io.stdout;

fn main() void {
    let x: int32 = 5;
    stdout.println("hello, void! today's number is {}",x);
}
"#;
    println!("lexing");
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();
    println!("tokens: {:?}", tokens);
    let mut parser = Parser::new(tokens);
    println!("parsing");
    let program = parser.parse();
    println!("{:#?}", program);
}
