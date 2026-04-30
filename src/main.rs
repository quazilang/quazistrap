// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

mod aot;
pub mod bytecode;
mod cli;
mod loader;
pub mod lexer;
pub mod parser;
pub mod semantic;

use bytecode::{serialize_vbc, Codegen};
use clap::Parser as ClapParser;
use cli::Args;
use lexer::Lexer;
use parser::Parser;
use semantic::Analyzer;
use std::io::Write;

use cli::Command as CliCmd;
use cli::EmitType;

fn run_pipeline(src: &str, program: &parser::ast::Program, emit: EmitType, output_file_name: &str) {
    let mut analyzer = Analyzer::new();
    let sema_report = analyzer.analyze_program(program);

    for warning in &sema_report.warnings {
        eprintln!("{}", warning.render(src));
    }
    for suggestion in &sema_report.suggestions {
        eprintln!("suggestion: {}", suggestion.message);
    }

    if !sema_report.errors.is_empty() {
        for err in &sema_report.errors {
            eprintln!("{}", err.render(src));
        }
        std::process::exit(1);
    }

    let mut cg = Codegen::new(&sema_report);
    let chunks = cg.compile_program(program);

    match emit {
        EmitType::Bytecode => {
            let bytes = serialize_vbc(&chunks);
            let mut f = std::fs::File::create(output_file_name).unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m cannot create {}: {}", output_file_name, e);
                std::process::exit(1);
            });
            f.write_all(&bytes).unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m write failed: {}", e);
                std::process::exit(1);
            });
            println!("wrote {} bytes to {}", bytes.len(), output_file_name);
            for chunk in &chunks {
                print!("{}", chunk);
            }
        }
        EmitType::Assembly => {
            let emitter = aot::X86Emitter::new(&chunks);
            let asm = emitter.emit_asm();
            std::fs::write(output_file_name, &asm).unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m cannot write {}: {}", output_file_name, e);
                std::process::exit(1);
            });
            println!("wrote {}", output_file_name);
        }
        EmitType::Binary => {
            let emitter = aot::X86Emitter::new(&chunks);
            let asm = emitter.emit_asm();
            let asm_path = format!("{}.s", output_file_name);
            std::fs::write(&asm_path, &asm).unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m cannot write {}: {}", asm_path, e);
                std::process::exit(1);
            });
            let status = std::process::Command::new("gcc")
                .args(["-o", output_file_name, &asm_path])
                .status()
                .unwrap_or_else(|e| {
                    eprintln!("\x1b[31;1merror:\x1b[0m gcc not found: {}", e);
                    std::process::exit(1);
                });
            let _ = std::fs::remove_file(&asm_path);
            if !status.success() {
                eprintln!("\x1b[31;1merror:\x1b[0m gcc failed");
                std::process::exit(1);
            }
            println!("wrote {}", output_file_name);
        }
    }
}

fn main() {
    let args = Args::parse();

    match args.command {
        CliCmd::Compile {
            files,
            output,
            emit_asm,
            emit_bytecode,
            run: _,
        } => {
            if files.is_empty() {
                eprintln!("\x1b[31;1merror:\x1b[0m no input files");
                std::process::exit(1);
            }
            let emit = if emit_bytecode {
                EmitType::Bytecode
            } else if emit_asm {
                EmitType::Assembly
            } else {
                EmitType::Binary
            };

            let result = loader::load_programs(&files).unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                std::process::exit(1);
            });

            if result.loaded_files.len() > 1 {
                let names: Vec<_> = result.loaded_files.iter()
                    .map(|p| p.display().to_string())
                    .collect();
                eprintln!("compiling: {}", names.join(", "));
            }

            let out = output.clone().unwrap_or_else(|| {
                let stem = files[0].file_stem().unwrap_or_default().to_string_lossy().into_owned();
                match emit {
                    EmitType::Bytecode => format!("{}.vbc", stem),
                    EmitType::Assembly => format!("{}.s", stem),
                    EmitType::Binary => {
                        if cfg!(target_os = "windows") { format!("{}.exe", stem) } else { stem }
                    }
                }
            });

            run_pipeline(&result.merged_source, &result.program, emit, &out);
        }
        CliCmd::Debug {
            emit_asm,
            emit_bytecode,
        } => {
            let src = r#"
import std.io.stdout;

fn main() void {
    const z = "hi";
    const y: int32 = 10;
    var x: int32 = 5;

    while (x < y) {
        stdout.println("{} void! x = {}", z, x);
        x++;
    }
}
"#;
            let (emit, output) = if emit_bytecode {
                (EmitType::Bytecode, "dbg.vbc".to_owned())
            } else if emit_asm {
                (EmitType::Assembly, "dbg.s".to_owned())
            } else {
                (EmitType::Binary, if cfg!(target_os = "windows") { "dbg.exe".to_owned() } else { "dbg".to_owned() })
            };
            let mut lexer = Lexer::new(src);
            let tokens = lexer.tokenize();
            let mut parser = Parser::new_with_source(tokens, src);
            match parser.parse() {
                Ok(program) => run_pipeline(src, &program, emit, &output),
                Err(err) => {
                    eprintln!("{}", err);
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("\x1b[31;1merror: \x1b[0;1mnot implemented\x1b[0m");
            std::process::exit(2);
        }
    }
}
