// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

pub mod analysis;
mod backend;
pub mod bytecode;
pub mod cli;
pub mod lexer;
pub mod loader;
mod lsp;
pub mod parser;
mod project;
pub mod semantic;

use analysis::{analyze_program, format_void_source};
use backend::linker::{LinkerInvocation, remove_temp, write_temp_object};
use backend::{TargetSpec, select_backend};
use bytecode::{Codegen, serialize_vbc};
use clap::Parser as ClapParser;
use cli::Args;
use lexer::Lexer;
use loader::LoadResult;
use parser::Parser;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use cli::Command as CliCmd;
use cli::EmitType;
use project::ProjectContext;

fn report_diagnostics(report: &semantic::SemanticReport, src: &str) -> bool {
    for warning in &report.warnings {
        eprintln!("{}", warning.render(src));
    }
    for suggestion in &report.suggestions {
        eprintln!("\x1b[35;1msuggestion:\x1b[0;1m {}\x1b[0m", suggestion.message);
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
    library_fn_names: HashSet<String>,
    debug: bool,
    emit: EmitType,
    output_file_name: &str,
    link_flags: Option<&[String]>,
    explicit_linker: Option<&Path>,
) {
    let sema_report = analyze_program(src, program, library_fn_names);
    if !report_diagnostics(&sema_report, src) {
        std::process::exit(1);
    }

    let mut cg = Codegen::new(&sema_report);
    let chunks = cg.compile_program(program);

    if debug {
        for chunk in &chunks {
            eprint!("{}", chunk);
        }
    }

    match emit {
        EmitType::Bytecode => {
            let bytes = serialize_vbc(&chunks);
            let mut f = std::fs::File::create(output_file_name).unwrap_or_else(|e| {
                eprintln!(
                    "\x1b[31;1merror:\x1b[0m cannot create {}: {}",
                    output_file_name, e
                );
                std::process::exit(1);
            });
            f.write_all(&bytes).unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m write failed: {}", e);
                std::process::exit(1);
            });
            println!(
                "\x1b[1;32mbuilt\x1b[0m  \x1b[1m{output_file_name}\x1b[0m  \x1b[2m({} bytes)\x1b[0m",
                bytes.len()
            );
            for chunk in &chunks {
                print!("{}", chunk);
            }
        }

        EmitType::Object => {
            let obj_bytes = compile_to_object(&chunks, false);
            std::fs::write(output_file_name, &obj_bytes).unwrap_or_else(|e| {
                eprintln!(
                    "\x1b[31;1merror:\x1b[0m cannot write {}: {}",
                    output_file_name, e
                );
                std::process::exit(1);
            });
            println!(
                "\x1b[1;32mbuilt\x1b[0m  \x1b[1m{output_file_name}\x1b[0m  \x1b[2m[object]\x1b[0m"
            );
        }

        EmitType::Binary => {
            let obj_bytes = compile_to_object(&chunks, true);
            let stem = Path::new(output_file_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(output_file_name);
            let tmp_obj = write_temp_object(&obj_bytes, stem).unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                std::process::exit(1);
            });

            // Override linker if the user passed --linker.
            let flags = link_flags.unwrap_or(&[]);
            let mut inv = LinkerInvocation::new(
                tmp_obj.clone(),
                PathBuf::from(output_file_name),
                TargetSpec::host(),
                flags.to_vec(),
            )
            .unwrap_or_else(|e| {
                remove_temp(&tmp_obj);
                eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                std::process::exit(1);
            });

            if let Some(lnk) = explicit_linker {
                inv.linker = lnk.to_path_buf();
            }

            inv.run().unwrap_or_else(|e| {
                remove_temp(&tmp_obj);
                eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                std::process::exit(1);
            });

            remove_temp(&tmp_obj);
            println!("\x1b[1;32mbuilt\x1b[0m  \x1b[1m{output_file_name}\x1b[0m");
        }
    }
}

fn compile_to_object(chunks: &[crate::bytecode::Chunk], emit_start: bool) -> Vec<u8> {
    let mut target = TargetSpec::host();
    if !emit_start {
        target = target.without_start();
    }
    let backend = select_backend(&target);
    backend
        .compile(chunks, &target)
        .unwrap_or_else(|e| {
            eprintln!("\x1b[31;1merror:\x1b[0m codegen failed: {}", e);
            std::process::exit(1);
        })
        .bytes
}

fn run_check(src: &str, program: &parser::ast::Program, library_fn_names: HashSet<String>) {
    let report = analyze_program(src, program, library_fn_names);
    if !report_diagnostics(&report, src) {
        std::process::exit(1);
    }
}

fn project_output_name(name: &str, emit: EmitType) -> String {
    match emit {
        EmitType::Bytecode => format!("{}.vbc", name),
        EmitType::Object => format!("{}.o", name),
        EmitType::Binary => {
            if cfg!(target_os = "windows") {
                format!("{}.exe", name)
            } else {
                name.to_string()
            }
        }
    }
}

fn abs_path(name: &str) -> PathBuf {
    let p = PathBuf::from(name);
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    }
}

fn load_with_optional_project(files: &[PathBuf]) -> LoadResult {
    let ctx = ProjectContext::discover(&files[0]).unwrap_or_else(|e| {
        eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
        std::process::exit(1);
    });
    let resolver_owned: Option<loader::ModuleResolver> = ctx.map(|c| c.resolver);
    loader::load_programs_with_resolver(files, resolver_owned.as_ref()).unwrap_or_else(|e| {
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
        eprintln!(
            "\x1b[31;1merror:\x1b[0m cannot write {}: {}",
            path.display(),
            e
        );
        std::process::exit(1);
    });
}

fn create_new_project(name: &str) {
    let root = PathBuf::from(name);
    if root.exists() {
        eprintln!(
            "\x1b[31;1merror:\x1b[0m path already exists: {}",
            root.display()
        );
        std::process::exit(1);
    }

    let pkg_name = root.file_name().and_then(|n| n.to_str()).unwrap_or(name);

    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap_or_else(|e| {
        eprintln!(
            "\x1b[31;1merror:\x1b[0m cannot create {}: {}",
            src_dir.display(),
            e
        );
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
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {}", dir.display(), e))?;
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
            eprintln!(
                "\x1b[31;1merror:\x1b[0m cannot read {}: {}",
                path.display(),
                e
            );
            std::process::exit(1);
        });
        let formatted = format_void_source(&src);
        if formatted != src {
            write_file(&path, &formatted);
            changed += 1;
        }
    }

    println!(
        "formatted {} file{}",
        changed,
        if changed == 1 { "" } else { "s" }
    );
}

fn clean_project_artifacts() {
    let ctx = load_project_context();
    let root = &ctx.config.root;
    let bin_name = project_output_name(&ctx.config.name, EmitType::Binary);

    let mut targets: HashSet<PathBuf> = HashSet::new();
    targets.insert(root.join(&bin_name));
    targets.insert(root.join(format!("{}.o", ctx.config.name)));
    targets.insert(root.join(format!("{}.vbc", ctx.config.name)));

    let mut removed = 0usize;
    for path in targets {
        if path.exists() {
            std::fs::remove_file(&path).unwrap_or_else(|e| {
                eprintln!(
                    "\x1b[31;1merror:\x1b[0m cannot remove {}: {}",
                    path.display(),
                    e
                );
                std::process::exit(1);
            });
            removed += 1;
        }
    }

    if removed == 0 {
        println!("no build artifacts found");
    } else {
        println!(
            "removed {} artifact{}",
            removed,
            if removed == 1 { "" } else { "s" }
        );
    }
}

fn print_debug_files(loaded: &[PathBuf], library_paths: &[PathBuf]) {
    let n = loaded.len();
    eprintln!("\x1b[1;34mfiles\x1b[0m \x1b[2m({n}):\x1b[0m");
    for path in loaded {
        let is_lib = library_paths.iter().any(|lp| lp == path);
        if is_lib {
            eprintln!("  \x1b[2m+\x1b[0m {}  \x1b[2m[lib]\x1b[0m", path.display());
        } else {
            eprintln!("    {}", path.display());
        }
    }
}

fn main() {
    let args = Args::parse();

    match args.command {
        CliCmd::Build {
            files,
            output,
            emit_bytecode,
            emit_object,
            run,
            debug,
            linker,
        } => {
            let emit = if emit_bytecode {
                EmitType::Bytecode
            } else if emit_object {
                EmitType::Object
            } else {
                EmitType::Binary
            };

            let explicit_linker = linker.as_deref();

            if files.is_empty() {
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

                if debug {
                    print_debug_files(&result.loaded_files, &result.library_file_paths);
                }

                let out = output
                    .clone()
                    .unwrap_or_else(|| project_output_name(&ctx.config.name, emit.clone()));
                run_pipeline(
                    &result.merged_source,
                    &result.program,
                    result.library_fn_names,
                    debug,
                    emit,
                    &out,
                    Some(&ctx.config.flags),
                    explicit_linker,
                );

                if run && !emit_bytecode && !emit_object {
                    let status = std::process::Command::new(abs_path(&out))
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
                let result = load_with_optional_project(&files);
                print_debug_files(&result.loaded_files, &result.library_file_paths);

                let out = output.clone().unwrap_or_else(|| {
                    let stem = files[0]
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    match emit {
                        EmitType::Bytecode => format!("{}.vbc", stem),
                        EmitType::Object => format!("{}.o", stem),
                        EmitType::Binary => {
                            if cfg!(target_os = "windows") {
                                format!("{}.exe", stem)
                            } else {
                                stem
                            }
                        }
                    }
                });

                run_pipeline(
                    &result.merged_source,
                    &result.program,
                    result.library_fn_names,
                    debug,
                    emit,
                    &out,
                    None,
                    explicit_linker,
                );

                if run && !emit_bytecode && !emit_object {
                    let status = std::process::Command::new(abs_path(&out))
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

        CliCmd::Run { linker } => {
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
                result.library_fn_names,
                false,
                EmitType::Binary,
                &out,
                Some(&ctx.config.flags),
                linker.as_deref(),
            );

            let status = std::process::Command::new(abs_path(&out))
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
            run_check(
                &result.merged_source,
                &result.program,
                result.library_fn_names,
            );
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

        CliCmd::Lsp { .. } => {
            lsp::run_lsp_server();
        }

        CliCmd::Debug { emit_bytecode } => {
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
            } else {
                (
                    EmitType::Binary,
                    if cfg!(target_os = "windows") {
                        "dbg.exe".to_owned()
                    } else {
                        "dbg".to_owned()
                    },
                )
            };
            let mut lexer = Lexer::new(src);
            let tokens = lexer.tokenize();
            let mut parser = Parser::new_with_source(tokens, src);
            match parser.parse() {
                Ok(program) => run_pipeline(
                    src,
                    &program,
                    HashSet::new(),
                    false,
                    emit,
                    &output,
                    None,
                    None,
                ),
                Err(err) => {
                    eprintln!("{}", err);
                    std::process::exit(1);
                }
            }
        }
    }
}
