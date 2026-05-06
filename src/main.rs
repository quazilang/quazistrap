// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

mod aot;
pub mod bytecode;
mod cli;
mod loader;
mod project;
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
use std::path::PathBuf;
use loader::LoadResult;

use cli::Command as CliCmd;
use cli::EmitType;
use project::ProjectContext;

fn analyze_program(src: &str, program: &parser::ast::Program) -> semantic::SemanticReport {
    let mut analyzer = Analyzer::new();
    analyzer.analyze_program(program)
}

fn report_diagnostics(report: &semantic::SemanticReport, src: &str) -> bool {
    for warning in &report.warnings {
        eprintln!("{}", warning.render(src));
    }
    for suggestion in &report.suggestions {
        eprintln!("suggestion: {}", suggestion.message);
    }

    if !report.errors.is_empty() {
        for err in &report.errors {
            eprintln!("{}", err.render(src));
        }
        return false;
    }
    true
}

fn run_pipeline(
    src: &str,
    program: &parser::ast::Program,
    emit: EmitType,
    output_file_name: &str,
    link_flags: Option<&[String]>,
) {
    let sema_report = analyze_program(src, program);
    if !report_diagnostics(&sema_report, src) {
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
            let mut cmd = std::process::Command::new("gcc");
            if let Some(flags) = link_flags {
                cmd.args(flags);
            }
            let status = cmd
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

fn run_check(src: &str, program: &parser::ast::Program) {
    let report = analyze_program(src, program);
    if !report_diagnostics(&report, src) {
        std::process::exit(1);
    }
}

fn project_output_name(name: &str, emit: EmitType) -> String {
    match emit {
        EmitType::Bytecode => format!("{}.vbc", name),
        EmitType::Assembly => format!("{}.s", name),
        EmitType::Binary => {
            if cfg!(target_os = "windows") {
                format!("{}.exe", name)
            } else {
                name.to_string()
            }
        }
    }
}

fn load_with_optional_project(files: &[PathBuf]) -> LoadResult {
    let ctx = ProjectContext::discover(&files[0]).unwrap_or_else(|e| {
        eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
        std::process::exit(1);
    });
    let resolver = ctx.as_ref().map(|c| &c.resolver);
    loader::load_programs_with_resolver(files, resolver).unwrap_or_else(|e| {
        eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
        std::process::exit(1);
    })
}

fn load_project_context() -> ProjectContext {
    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        eprintln!("\x1b[31;1merror:\x1b[0m cannot read cwd: {}", e);
        std::process::exit(1);
    });
    ProjectContext::load(&cwd).unwrap_or_else(|e| {
        eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
        std::process::exit(1);
    })
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

            let result = load_with_optional_project(&files);

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

            run_pipeline(&result.merged_source, &result.program, emit, &out, None);
        }
        CliCmd::Build {
            output,
            emit_asm,
            emit_bytecode,
        } => {
            let emit = if emit_bytecode {
                EmitType::Bytecode
            } else if emit_asm {
                EmitType::Assembly
            } else {
                EmitType::Binary
            };

            let ctx = load_project_context();
            ctx.ensure_lockfile().unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                std::process::exit(1);
            });

            let entry = ctx.config.entry.clone();
            let result = loader::load_programs_with_resolver(&[entry], Some(&ctx.resolver))
                .unwrap_or_else(|e| {
                    eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                    std::process::exit(1);
                });

            let out = output.unwrap_or_else(|| project_output_name(&ctx.config.name, emit.clone()));
            run_pipeline(
                &result.merged_source,
                &result.program,
                emit,
                &out,
                Some(&ctx.config.flags),
            );
        }
        CliCmd::Run => {
            let ctx = load_project_context();
            ctx.ensure_lockfile().unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                std::process::exit(1);
            });

            let entry = ctx.config.entry.clone();
            let result = loader::load_programs_with_resolver(&[entry], Some(&ctx.resolver))
                .unwrap_or_else(|e| {
                    eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                    std::process::exit(1);
                });

            let out = project_output_name(&ctx.config.name, EmitType::Binary);
            run_pipeline(
                &result.merged_source,
                &result.program,
                EmitType::Binary,
                &out,
                Some(&ctx.config.flags),
            );

            let status = std::process::Command::new(&out)
                .status()
                .unwrap_or_else(|e| {
                    eprintln!("\x1b[31;1merror:\x1b[0m cannot run {}: {}", out, e);
                    std::process::exit(1);
                });
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
        CliCmd::Check => {
            let ctx = load_project_context();
            let entry = ctx.config.entry.clone();
            let result = loader::load_programs_with_resolver(&[entry], Some(&ctx.resolver))
                .unwrap_or_else(|e| {
                    eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                    std::process::exit(1);
                });
            run_check(&result.merged_source, &result.program);
        }
        CliCmd::Debug {
            emit_asm,
            emit_bytecode,
        } => {
            let src = r#"
import std.io.stdout;

fn main() void {
    const z = "hi";
    const y: i32 = 10;
    var x: i32 = 5;

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
                Ok(program) => run_pipeline(src, &program, emit, &output, None),
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
