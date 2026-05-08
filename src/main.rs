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
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use loader::LoadResult;

use cli::Command as CliCmd;
use cli::EmitType;
use project::ProjectContext;

fn analyze_program(_src: &str, program: &parser::ast::Program) -> semantic::SemanticReport {
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
                .args(["-o", output_file_name, &asm_path, "-lm"])
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

fn write_file(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap_or_else(|e| {
        eprintln!("\x1b[31;1merror:\x1b[0m cannot write {}: {}", path.display(), e);
        std::process::exit(1);
    });
}

fn create_new_project(name: &str) {
    let root = PathBuf::from(name);
    if root.exists() {
        eprintln!("\x1b[31;1merror:\x1b[0m path already exists: {}", root.display());
        std::process::exit(1);
    }

    let pkg_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(name);

    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap_or_else(|e| {
        eprintln!("\x1b[31;1merror:\x1b[0m cannot create {}: {}", src_dir.display(), e);
        std::process::exit(1);
    });

    let toml = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.void\"\nsrc = \"src\"\n",
        pkg_name
    );
    write_file(&root.join("void.toml"), &toml);

    let main_src = r#"@syscall("write")
fn sys_write(fd: i32, buf: str, len: isize) isize { }

fn main() i32 {
    var msg: str = "Hello World!\n";
    sys_write(1, msg, 13);
    ret 0;
}
"#;
    write_file(&src_dir.join("main.void"), main_src);

    println!("created project '{}'", root.display());
}

fn collect_void_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {}", dir.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read dir entry: {}", e))?;
        let path = entry.path();
        if path.is_dir() {
            collect_void_files(&path, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("void") {
            out.push(path);
        }
    }
    Ok(())
}

fn format_void_source(input: &str) -> String {
    let lines: Vec<String> = input.lines().map(|line| line.trim_end().to_string()).collect();
    if lines.is_empty() {
        return String::new();
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn format_project_sources() {
    let ctx = load_project_context();
    let mut files = Vec::new();
    collect_void_files(&ctx.config.src_dir, &mut files).unwrap_or_else(|e| {
        eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
        std::process::exit(1);
    });

    let mut changed = 0usize;
    for path in files {
        let src = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("\x1b[31;1merror:\x1b[0m cannot read {}: {}", path.display(), e);
            std::process::exit(1);
        });
        let formatted = format_void_source(&src);
        if formatted != src {
            write_file(&path, &formatted);
            changed += 1;
        }
    }

    println!("formatted {} file{}", changed, if changed == 1 { "" } else { "s" });
}

fn clean_project_artifacts() {
    let ctx = load_project_context();
    let root = &ctx.config.root;
    let bin_name = project_output_name(&ctx.config.name, EmitType::Binary);

    let mut targets: HashSet<PathBuf> = HashSet::new();
    targets.insert(root.join(&bin_name));
    targets.insert(root.join(format!("{}.s", bin_name)));
    targets.insert(root.join(format!("{}.s", ctx.config.name)));
    targets.insert(root.join(format!("{}.vbc", ctx.config.name)));

    let mut removed = 0usize;
    for path in targets {
        if path.exists() {
            std::fs::remove_file(&path).unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m cannot remove {}: {}", path.display(), e);
                std::process::exit(1);
            });
            removed += 1;
        }
    }

    if removed == 0 {
        println!("no build artifacts found");
    } else {
        println!("removed {} artifact{}", removed, if removed == 1 { "" } else { "s" });
    }
}

fn main() {
    let args = Args::parse();

    match args.command {
        CliCmd::Build {
            files,
            output,
            emit_asm,
            emit_bytecode,
            run,
        } => {
            let emit = if emit_bytecode {
                EmitType::Bytecode
            } else if emit_asm {
                EmitType::Assembly
            } else {
                EmitType::Binary
            };

            if files.is_empty() {
                // No files: build project from void.toml
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

                let out = output.clone().unwrap_or_else(|| project_output_name(&ctx.config.name, emit.clone()));
                run_pipeline(
                    &result.merged_source,
                    &result.program,
                    emit,
                    &out,
                    Some(&ctx.config.flags),
                );

                if run && !emit_bytecode && !emit_asm {
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
            } else {
                // Files given: compile files directly
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

                if run && !emit_bytecode && !emit_asm {
                    let status = std::process::Command::new(format!("./{}", out))
                        .status()
                        .unwrap_or_else(|e| {
                            eprintln!("\x1b[31;1merror:\x1b[0m failed to run binary: {}", e);
                            std::process::exit(1);
                        });
                    if !status.success() {
                        std::process::exit(status.code().unwrap_or(1));
                    }
                }
            }
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
        CliCmd::New { name } => {
            create_new_project(&name);
        }
        CliCmd::Fmt => {
            format_project_sources();
        }
        CliCmd::Clean => {
            clean_project_artifacts();
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

    for x < y {
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
    }
}
