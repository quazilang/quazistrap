// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

mod abi;
pub mod analysis;
mod backend;
pub mod bytecode;
pub mod cli;
mod header;
mod incremental;
pub mod lexer;
pub mod loader;
mod lsp;
mod package;
pub mod parser;
mod progress;
mod project;
pub mod semantic;
mod test_runner;

use analysis::{analyze_program_with_source_files, format_quazi_source};
use backend::linker::{LinkerInvocation, link_object, remove_temp, write_temp_object};
use backend::{TargetSpec, select_backend};
use bytecode::{Codegen, serialize_qzi};
use clap::Parser as ClapParser;
use cli::Args;
use lexer::Lexer;
use parser::Parser;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use cli::Command as CliCmd;
use cli::EmitType;
use project::ProjectContext;

static NO_COLOR_OUTPUT: AtomicBool = AtomicBool::new(false);

fn print_stderr_line(arguments: std::fmt::Arguments<'_>) {
    let rendered = arguments.to_string();
    if NO_COLOR_OUTPUT.load(Ordering::Relaxed) || std::env::var_os("NO_COLOR").is_some() {
        std::eprintln!("{}", lsp::diagnostics::strip_ansi(&rendered));
    } else {
        std::eprintln!("{rendered}");
    }
}

macro_rules! eprintln {
    () => {
        print_stderr_line(format_args!(""))
    };
    ($($arg:tt)*) => {
        print_stderr_line(format_args!($($arg)*))
    };
}

fn report_diagnostics(
    report: &semantic::SemanticReport,
    src: &str,
    source_files: &[semantic::types::SourceFile],
) -> bool {
    for warning in &report.warnings {
        eprintln!("{}", warning.render_with_source_files(src, source_files));
    }
    for suggestion in &report.suggestions {
        eprintln!("\x1b[2mhint: {}\x1b[0m", suggestion.message);
    }

    if !report.errors.is_empty() {
        for err in &report.errors {
            eprintln!("{}", err.render_with_source_files(src, source_files));
        }
        return false;
    }
    true
}

fn run_pipeline(
    src: &str,
    program: &parser::ast::Program,
    library_fn_names: HashSet<String>,
    library_char_ranges: Vec<std::ops::Range<usize>>,
    source_files: Vec<semantic::types::SourceFile>,
    namespaced_paths: HashSet<String>,
    debug: bool,
    emit: EmitType,
    output_file_name: &str,
    link_flags: Option<&[String]>,
    explicit_linker: Option<&Path>,
) {
    let program = &semantic::strip_cfg(program);
    let sema_report = analyze_program_with_source_files(
        src,
        program,
        library_fn_names,
        library_char_ranges,
        source_files.clone(),
        namespaced_paths,
    );
    if !report_diagnostics(&sema_report, src, &source_files) {
        std::process::exit(1);
    }

    let mut cg = Codegen::new(&sema_report);
    cg.set_native_mangling(true);
    let chunks = cg
        .compile_program(program, &source_files)
        .unwrap_or_else(|error| {
            eprintln!("\x1b[31;1merror:\x1b[0m code generation failed: {error}");
            std::process::exit(1);
        });

    if debug {
        for chunk in &chunks {
            eprint!("{}", chunk);
        }
    }

    match emit {
        EmitType::Bytecode => {
            let bytes = serialize_qzi(&chunks).unwrap_or_else(|error| {
                eprintln!("\x1b[31;1merror:\x1b[0m cannot serialize QZI: {error}");
                std::process::exit(1);
            });
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
            let obj_bytes = compile_to_object(
                &chunks,
                false,
                false,
                Some(&sema_report),
                sema_report.main_takes_args,
                &TargetSpec::host(),
            );
            write_or_package_object(
                &obj_bytes,
                output_file_name,
                link_flags.unwrap_or(&[]),
                explicit_linker,
                &TargetSpec::host(),
                &sema_report
                    .exported_symbols
                    .values()
                    .cloned()
                    .collect::<Vec<_>>(),
            );
            println!(
                "\x1b[1;32mbuilt\x1b[0m  \x1b[1m{output_file_name}\x1b[0m  \x1b[2m[object]\x1b[0m"
            );
        }

        EmitType::Binary => {
            let obj_bytes = compile_to_object(
                &chunks,
                true,
                false,
                Some(&sema_report),
                sema_report.main_takes_args,
                &TargetSpec::host(),
            );
            let flags = link_flags.unwrap_or(&[]);
            link_object(
                &obj_bytes,
                Path::new(output_file_name),
                TargetSpec::host(),
                flags,
                explicit_linker,
            )
            .unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                std::process::exit(1);
            });
            println!("\x1b[1;32mbuilt\x1b[0m  \x1b[1m{output_file_name}\x1b[0m");
        }
    }
}

fn write_or_package_object(
    bytes: &[u8],
    output: &str,
    link_flags: &[String],
    explicit_linker: Option<&Path>,
    target: &TargetSpec,
    export_symbols: &[String],
) {
    match Path::new(output).extension().and_then(|ext| ext.to_str()) {
        Some("a") => {
            let object = write_temp_object(bytes, "ffi_static").unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m {e}");
                std::process::exit(1);
            });
            let ar = std::env::var_os("AR").unwrap_or_else(|| "ar".into());
            // `ar r` replaces only a member with the same member name; our temporary
            // object name changes per process, so recreate the requested archive.
            if Path::new(output).exists() {
                std::fs::remove_file(output).unwrap_or_else(|e| {
                    remove_temp(&object);
                    eprintln!("\x1b[31;1merror:\x1b[0m cannot replace {output}: {e}");
                    std::process::exit(1);
                });
            }
            let mut command = std::process::Command::new(&ar);
            command.args(["rcs", output]).arg(&object);
            for native_object in link_flags.iter().filter(|flag| flag.ends_with(".o")) {
                command.arg(native_object);
            }
            let result = command.output();
            remove_temp(&object);
            let result = result.unwrap_or_else(|e| {
                eprintln!(
                    "\x1b[31;1merror:\x1b[0m failed to run archiver {:?}: {e}",
                    ar
                );
                std::process::exit(1);
            });
            if !result.status.success() {
                eprintln!(
                    "\x1b[31;1merror:\x1b[0m archiver failed\n{}{}",
                    String::from_utf8_lossy(&result.stderr),
                    String::from_utf8_lossy(&result.stdout)
                );
                std::process::exit(1);
            }
        }
        Some("so" | "dll") => {
            let object = write_temp_object(bytes, "ffi_shared").unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m {e}");
                std::process::exit(1);
            });
            let mut flags = vec!["-shared".to_string()];
            flags.extend_from_slice(link_flags);
            if target.os == backend::target::Os::Windows {
                for symbol in export_symbols {
                    flags.push(format!("/export:{symbol}"));
                }
            }
            let mut invocation =
                LinkerInvocation::new(object.clone(), PathBuf::from(output), target.clone(), flags)
                    .unwrap_or_else(|e| {
                        remove_temp(&object);
                        eprintln!("\x1b[31;1merror:\x1b[0m {e}");
                        std::process::exit(1);
                    });
            if let Some(linker) = explicit_linker {
                invocation.linker = linker.to_path_buf();
            }
            invocation.run().unwrap_or_else(|e| {
                remove_temp(&object);
                eprintln!("\x1b[31;1merror:\x1b[0m {e}");
                std::process::exit(1);
            });
            remove_temp(&object);
        }
        _ => std::fs::write(output, bytes).unwrap_or_else(|e| {
            eprintln!("\x1b[31;1merror:\x1b[0m cannot write {output}: {e}");
            std::process::exit(1);
        }),
    }
}

fn strip_binary(path: &Path) {
    let candidates = if cfg!(target_os = "windows") {
        ["llvm-strip.exe", "strip.exe"]
    } else {
        ["llvm-strip", "strip"]
    };

    let found = candidates.iter().find_map(|name| {
        // Check PATH
        std::env::var_os("PATH")
            .and_then(|paths| {
                std::env::split_paths(&paths).find_map(|dir| {
                    let full = dir.join(name);
                    if full.is_file() { Some(full) } else { None }
                })
            })
            .or_else(|| {
                // Check current dir / common locations
                let local = PathBuf::from(name);
                if local.is_file() { Some(local) } else { None }
            })
    });

    match found {
        Some(strip_path) => {
            let _ = std::process::Command::new(&strip_path).arg(path).status();
        }
        None => {
            eprintln!(
                "\r\x1b[K\x1b[33;1mwarning:\x1b[0m no strip tool found; debug symbols retained"
            );
        }
    }
}

fn compile_to_object(
    chunks: &[crate::bytecode::Chunk],
    emit_start: bool,
    no_crash: bool,
    report: Option<&crate::semantic::SemanticReport>,
    main_takes_args: bool,
    target: &TargetSpec,
) -> Vec<u8> {
    let mut target = target.clone();
    if !emit_start {
        target = target.without_start();
    }
    if no_crash {
        target = target.with_no_crash();
    }
    let backend = select_backend(&target);
    backend
        .compile(chunks, &target, report, main_takes_args)
        .unwrap_or_else(|e| {
            eprintln!("\x1b[31;1merror:\x1b[0m codegen failed: {}", e);
            std::process::exit(1);
        })
        .bytes
}

/// Emit chunks to bytecode, object, or binary (same as run_pipeline but skips analysis/codegen).
fn emit_chunks(
    chunks: &[bytecode::Chunk],
    emit: EmitType,
    output_file_name: &str,
    link_flags: Option<&[String]>,
    explicit_linker: Option<&Path>,
    debug: bool,
    no_crash: bool,
    main_takes_args: bool,
    announce: bool,
    target: &TargetSpec,
) {
    if debug {
        for chunk in chunks {
            eprint!("{}", chunk);
        }
    }

    match emit {
        EmitType::Bytecode => {
            let bytes = serialize_qzi(chunks).unwrap_or_else(|error| {
                eprintln!("\x1b[31;1merror:\x1b[0m cannot serialize QZI: {error}");
                std::process::exit(1);
            });
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
            if announce {
                println!(
                    "\x1b[1;32mbuilt\x1b[0m  \x1b[1m{output_file_name}\x1b[0m  \x1b[2m({} bytes)\x1b[0m",
                    bytes.len()
                );
            }
        }

        EmitType::Object => {
            let obj_bytes =
                compile_to_object(chunks, false, no_crash, None, main_takes_args, target);
            write_or_package_object(
                &obj_bytes,
                output_file_name,
                link_flags.unwrap_or(&[]),
                explicit_linker,
                target,
                &chunks
                    .iter()
                    .filter_map(|chunk| chunk.export.as_ref().map(|export| export.symbol.clone()))
                    .collect::<Vec<_>>(),
            );
            if announce {
                println!(
                    "\x1b[1;32mbuilt\x1b[0m  \x1b[1m{output_file_name}\x1b[0m  \x1b[2m[object]\x1b[0m"
                );
            }
        }

        EmitType::Binary => {
            let obj_bytes =
                compile_to_object(chunks, true, no_crash, None, main_takes_args, target);
            let flags = link_flags.unwrap_or(&[]);
            link_object(
                &obj_bytes,
                Path::new(output_file_name),
                target.clone(),
                flags,
                explicit_linker,
            )
            .unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                std::process::exit(1);
            });
            if announce {
                println!("\x1b[1;32mbuilt\x1b[0m  \x1b[1m{output_file_name}\x1b[0m");
            }
        }
    }
}

fn run_check(
    src: &str,
    program: &parser::ast::Program,
    library_fn_names: HashSet<String>,
    library_char_ranges: Vec<std::ops::Range<usize>>,
    source_files: Vec<semantic::types::SourceFile>,
    namespaced_paths: HashSet<String>,
) {
    let program = &semantic::strip_cfg(program);
    let report = analyze_program_with_source_files(
        src,
        program,
        library_fn_names,
        library_char_ranges,
        source_files.clone(),
        namespaced_paths,
    );
    if !report_diagnostics(&report, src, &source_files) {
        std::process::exit(1);
    }
}

fn target_output_name(name: &str, emit: EmitType, target: &TargetSpec) -> String {
    match emit {
        EmitType::Bytecode => format!("{name}.qzi"),
        EmitType::Object => match target.os {
            backend::target::Os::Windows => format!("{name}.obj"),
            _ => format!("{name}.o"),
        },
        EmitType::Binary => match target.os {
            backend::target::Os::Windows => format!("{name}.exe"),
            _ => name.into(),
        },
    }
}

fn shared_library_name(name: &str, target: &TargetSpec) -> String {
    match target.os {
        backend::target::Os::Windows => format!("{name}.dll"),
        backend::target::Os::Linux => format!("lib{name}.so"),
        backend::target::Os::MacOs => format!("lib{name}.dylib"),
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

/// Windows canonical paths use the `\\?\` namespace, which several otherwise
/// supported GNU-style tools parse as switches or malformed UNC paths. Keep
/// extended paths internally and normalize only at the external-tool boundary.
fn external_tool_path(path: &std::path::Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let value = path.to_string_lossy();
        if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = value.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

fn load_with_optional_project(files: &[PathBuf]) -> Result<loader::LoadResult, String> {
    let ctx = ProjectContext::discover(&files[0])?;
    let settings = ctx
        .as_ref()
        .map(|context| context.config.package)
        .unwrap_or_default();
    let resolver_owned = ctx.map(|context| context.resolver);
    loader::load_programs_configured(files, resolver_owned.as_ref(), settings.std, &[])
}

fn apply_package_settings(
    mut target: TargetSpec,
    settings: project::PackageSettings,
) -> TargetSpec {
    if !settings.crash_handler {
        target = target.with_no_crash();
    }
    target
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

fn native_link_flags(ctx: &ProjectContext) -> Result<Vec<String>, String> {
    let mut flags = ctx.config.flags.clone();
    let object_dir = ctx.config.out_dir.join("ffi");
    if !ctx.config.cc.sources.is_empty() {
        std::fs::create_dir_all(&object_dir).map_err(|e| {
            format!(
                "cannot create native object directory {}: {e}",
                object_dir.display()
            )
        })?;
    }

    let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    for source in &ctx.config.cc.sources {
        let stem = source
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("invalid C source path: {}", source.display()))?;
        let object = object_dir.join(format!("{stem}.o"));
        let mut command = std::process::Command::new(&cc);
        command
            .arg("-c")
            .arg(external_tool_path(source))
            .arg("-o")
            .arg(external_tool_path(&object));
        if !cfg!(target_os = "windows") {
            command.arg("-fPIC");
        }
        for include in &ctx.config.cc.include_paths {
            command.arg(format!("-I{}", external_tool_path(include).display()));
        }
        for define in &ctx.config.cc.defines {
            command.arg(format!("-D{define}"));
        }
        command.args(&ctx.config.cc.flags);
        let output = command.output().map_err(|e| {
            format!(
                "failed to run C compiler {:?} for {}: {e}",
                cc,
                source.display()
            )
        })?;
        if !output.status.success() {
            return Err(format!(
                "C compiler failed for {}\n{}{}",
                source.display(),
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout)
            ));
        }
        flags.push(external_tool_path(&object).to_string_lossy().into_owned());
    }

    flags.extend(
        ctx.config
            .link
            .objects
            .iter()
            .map(|path| external_tool_path(path).to_string_lossy().into_owned()),
    );
    flags.extend(
        ctx.config
            .link
            .library_paths
            .iter()
            .map(|path| format!("-L{}", external_tool_path(path).display())),
    );
    flags.extend(
        ctx.config
            .link
            .libraries
            .iter()
            .map(|name| format!("-l{name}")),
    );
    flags.extend(ctx.config.link.flags.iter().cloned());
    if ctx.config.link.libc {
        flags.push("-lc".into());
    }
    Ok(flags)
}

fn compile_direct_c_sources(sources: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    let mut objects: Vec<PathBuf> = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        let object = std::env::temp_dir().join(format!("qz_cc_{}_{}.o", std::process::id(), index));
        let mut command = std::process::Command::new(&cc);
        command
            .arg("-c")
            .arg(external_tool_path(source))
            .arg("-o")
            .arg(external_tool_path(&object));
        if !cfg!(target_os = "windows") {
            command.arg("-fPIC");
        }
        let output = command
            .output()
            .map_err(|e| format!("failed to run C compiler {:?}: {e}", cc))?;
        if !output.status.success() {
            for prior in &objects {
                remove_temp(prior);
            }
            return Err(format!(
                "C compiler failed for {}\n{}{}",
                source.display(),
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout)
            ));
        }
        objects.push(object);
    }
    Ok(objects)
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

fn scaffold_library_name(name: &str) -> String {
    let mut result = String::with_capacity(name.len() + 1);
    for (index, character) in name.chars().enumerate() {
        let valid = if index == 0 {
            character.is_ascii_alphabetic() || character == '_'
        } else {
            character.is_ascii_alphanumeric() || character == '_'
        };
        result.push(if valid { character } else { '_' });
    }
    if result.is_empty() {
        result.push('_');
    }
    result
}

const PROJECT_GITIGNORE: &str = "# Quazi build output\n/build/\n\n# Compiled artifacts\n*.qzi\n*.o\n*.obj\n*.a\n*.so\n*.dll\n*.dylib\n*.exe\n*.pdb\n";

fn scaffold_project(root: &Path, pkg_name: &str, lib: bool) {
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap_or_else(|e| {
        eprintln!(
            "\x1b[31;1merror:\x1b[0m cannot create {}: {}",
            src_dir.display(),
            e
        );
        std::process::exit(1);
    });
    write_file(&root.join(".gitignore"), PROJECT_GITIGNORE);

    if lib {
        let library_name = scaffold_library_name(pkg_name);
        let toml = format!(
            "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nout_dir = \"build\"\nstd = true\ncrash_handler = true\nmangling = true\n\n[lib]\nname = \"{}\"\npath = \"src/lib.qz\"\n",
            library_name, library_name
        );
        write_file(&root.join("quazi.toml"), &toml);

        let lib_src = format!(
            "// {name} — quazilang library\n\npub fn add(a: i64, b: i64) i64 {{\n    ret a + b;\n}}\n",
            name = pkg_name
        );
        write_file(&src_dir.join("lib.qz"), &lib_src);
    } else {
        let toml = format!(
            "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nout_dir = \"build\"\nstd = true\ncrash_handler = true\nmangling = true\n\n[[bin]]\nname = \"{}\"\npath = \"src/main.qz\"\n",
            pkg_name, pkg_name
        );
        write_file(&root.join("quazi.toml"), &toml);

        let main_src = "import std.io;\n\nfn main() i32 {\n    io.println(\"Hello, World!\");\n    ret 0;\n}\n";
        write_file(&src_dir.join("main.qz"), main_src);
    }
}

fn initialize_git_repository(root: &Path) {
    if root.join(".git").exists() {
        return;
    }

    match std::process::Command::new("git")
        .arg("init")
        .arg("--quiet")
        .arg("--initial-branch=main")
        .arg(root)
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!(
            "\x1b[33;1mwarning:\x1b[0m git init failed with status {}; project was still created",
            status
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => eprintln!(
            "\x1b[33;1mwarning:\x1b[0m git was not found; project was created without a repository"
        ),
        Err(error) => {
            eprintln!("\x1b[33;1mwarning:\x1b[0m cannot initialize Git repository: {error}")
        }
    }
}

fn create_new_project(name: &str, lib: bool) {
    let root = PathBuf::from(name);
    if root.exists() {
        eprintln!(
            "\x1b[31;1merror:\x1b[0m path already exists: {}",
            root.display()
        );
        std::process::exit(1);
    }

    let pkg_name = root.file_name().and_then(|n| n.to_str()).unwrap_or(name);
    scaffold_project(&root, pkg_name, lib);
    initialize_git_repository(&root);
    let kind = if lib { "library" } else { "binary" };
    println!("created {} project '{}'", kind, root.display());
}

fn init_project(lib: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        eprintln!(
            "\x1b[31;1merror:\x1b[0m cannot get current directory: {}",
            e
        );
        std::process::exit(1);
    });

    if cwd.join("quazi.toml").exists() {
        eprintln!("\x1b[31;1merror:\x1b[0m quazi.toml already exists in this directory");
        std::process::exit(1);
    }

    let pkg_name = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");
    scaffold_project(&cwd, pkg_name, lib);
    initialize_git_repository(&cwd);
    let kind = if lib { "library" } else { "binary" };
    println!("initialized {} project '{}'", kind, pkg_name);
}

fn collect_quazi_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {}", dir.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read dir entry: {}", e))?;
        let path = entry.path();
        if path.is_dir() {
            collect_quazi_files(&path, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("qz") {
            out.push(path);
        }
    }
    Ok(())
}

fn format_project_sources() {
    let ctx = load_project_context();
    let mut files = Vec::new();
    collect_quazi_files(&ctx.config.src_dir, &mut files).unwrap_or_else(|e| {
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
        let formatted = format_quazi_source(&src);
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
    let preview = project::preview(Path::new("."), None, false).unwrap_or_else(|error| {
        eprintln!("\x1b[31;1merror:\x1b[0m {error}");
        std::process::exit(1);
    });
    if !preview.out_dir.exists() {
        println!("no build artifacts found");
    } else {
        std::fs::remove_dir_all(&preview.out_dir).unwrap_or_else(|error| {
            eprintln!(
                "\x1b[31;1merror:\x1b[0m cannot remove {}: {error}",
                preview.out_dir.display()
            );
            std::process::exit(1);
        });
        println!("removed {}", preview.out_dir.display());
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

fn build_with_progress(
    files: &[PathBuf],
    out: &str,
    emit: EmitType,
    debug: bool,
    link_flags: Option<&[String]>,
    explicit_linker: Option<&Path>,
    do_strip: bool,
    qzi_metadata: Option<bytecode::QziMetadata>,
    qzi_dependencies: &[PathBuf],
    qzc_path: Option<&Path>,
    extra_cache_inputs: &[PathBuf],
    target: &TargetSpec,
    silent: bool,
    no_color: bool,
    no_unicode: bool,
    no_progress: bool,
    initial_progress: Option<progress::BuildProgress>,
    project_resolver: Option<&loader::ModuleResolver>,
    package_settings: project::PackageSettings,
) {
    use progress::{
        BuildProgress, arch_label, build_dep_tree, codegen_stats, common_lib_prefix, fmt_count,
        tree_file_count,
    };

    let header_already_printed = initial_progress.is_some();
    let mut prog = initial_progress
        .unwrap_or_else(|| BuildProgress::new(silent, no_color, no_unicode, no_progress));
    if let Some(parent) = Path::new(out).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).unwrap_or_else(|error| {
            eprintln!(
                "\x1b[31;1merror:\x1b[0m cannot create {}: {error}",
                parent.display()
            );
            std::process::exit(1);
        });
    }

    // Header line.
    let input_name = files[0].file_name().and_then(|s| s.to_str()).unwrap_or("?");
    let out_name = Path::new(out)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(out);
    if !header_already_printed {
        prog.header(input_name, out_name);
    }

    if let Some(cache_path) = qzc_path {
        prog.begin("cache lookup");
        match incremental::load(cache_path) {
            Ok(Some(hit)) => {
                let module = bytecode::deserialize_qzi_module(&hit.qzi).unwrap_or_else(|error| {
                    prog.fail("invalid");
                    eprintln!("\x1b[31;1merror:\x1b[0m invalid cached QZI: {error}");
                    std::process::exit(1);
                });
                prog.done(&format!("hit · {} functions", module.chunks.len()));
                if debug {
                    for chunk in &module.chunks {
                        eprint!("{chunk}");
                    }
                }
                if matches!(emit, EmitType::Bytecode) {
                    std::fs::write(out, &hit.qzi).unwrap_or_else(|error| {
                        eprintln!("\x1b[31;1merror:\x1b[0m cannot write {out}: {error}");
                        std::process::exit(1);
                    });
                } else {
                    emit_chunks(
                        &module.chunks,
                        emit.clone(),
                        out,
                        link_flags,
                        explicit_linker,
                        debug,
                        hit.no_crash,
                        module.metadata.main_takes_args,
                        false,
                        target,
                    );
                    if do_strip {
                        strip_binary(Path::new(out));
                    }
                }
                let size = std::fs::metadata(out).ok().map(|metadata| metadata.len());
                prog.success(out, size);
                return;
            }
            Ok(None) => prog.done(if incremental::has_codegen_units(cache_path) {
                "partial"
            } else {
                "miss"
            }),
            Err(error) => {
                prog.fail("ignored");
                if !prog.silent {
                    eprintln!("\x1b[33;1mwarning:\x1b[0m ignoring QZC: {error}");
                }
            }
        }
    }

    // ── Step 1: Lexing (file I/O + tokenize) ─────────────────────────────────
    prog.begin("frontend");
    let result = match project_resolver
        .map(|resolver| {
            loader::load_programs_configured(
                files,
                Some(resolver),
                package_settings.std,
                &[],
            )
        })
        .unwrap_or_else(|| load_with_optional_project(files))
    {
        Ok(r) => r,
        Err(e) => {
            prog.fail("");
            eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
            std::process::exit(1);
        }
    };
    if debug {
        print_debug_files(&result.loaded_files, &result.library_file_paths);
    }
    let lib_prefix = common_lib_prefix(&result.library_file_paths);
    let tree_root = build_dep_tree(
        &files[0],
        &result.dep_edges,
        &result.library_file_paths,
        lib_prefix.as_deref(),
        &result.display_names,
    );
    let visible_files = tree_file_count(&tree_root);
    let tok_info = format!(
        "{} tokens · {} file{}",
        fmt_count(result.token_count),
        visible_files,
        if visible_files == 1 { "" } else { "s" }
    );
    prog.done(&tok_info);
    prog.dep_tree(&tree_root);

    // ── Step 2: Parsing ───────────────────────────────────────────────────────
    prog.begin("parsing");
    if let Some(parse_err) = &result.parse_error {
        prog.fail("");
        eprintln!("{}", parse_err);
        std::process::exit(1);
    }
    let (target_os, target_abi) = match target.os {
        backend::target::Os::Windows => ("windows", "win64"),
        backend::target::Os::Linux => ("linux", "sysv"),
        backend::target::Os::MacOs => ("macos", "sysv"),
    };
    let target_program = semantic::strip_cfg_for(&result.program, target_os, "x86_64", target_abi);
    let source_hashes = result.source_hashes.clone();
    let mut context_inputs = extra_cache_inputs.to_vec();
    context_inputs.extend_from_slice(qzi_dependencies);
    let incremental_context =
        incremental::semantic_context_hash(&target_program, target.triple(), &context_inputs)
            .unwrap_or_else(|error| {
                prog.fail("error");
                eprintln!("\x1b[31;1merror:\x1b[0m cannot fingerprint semantic context: {error}");
                std::process::exit(1);
            });
    let cached_codegen_units = qzc_path
        .map(|cache_path| {
            incremental::load_codegen_units(cache_path, incremental_context, &source_hashes)
                .unwrap_or_else(|error| {
                    if !prog.silent {
                        eprintln!("\x1b[33;1mwarning:\x1b[0m ignoring reusable QZC units: {error}");
                    }
                    Vec::new()
                })
        })
        .unwrap_or_default();
    let user_items = result
        .program
        .items
        .iter()
        .filter(|item| {
            !result
                .library_char_ranges
                .iter()
                .any(|r| r.contains(&item.span.start))
        })
        .count();
    prog.done(&format!(
        "{} item{}",
        user_items,
        if user_items == 1 { "" } else { "s" }
    ));

    // ── Step 3: Codegen (analyze + bytecode) ─────────────────────────────────
    prog.begin("analysis");
    let namespaced_paths: HashSet<String> = result
        .namespaced_paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let sema = analyze_program_with_source_files(
        &result.merged_source,
        &target_program,
        result.library_fn_names,
        result.library_char_ranges.clone(),
        result.source_files.clone(),
        namespaced_paths,
    );
    let has_errors = !sema.errors.is_empty();
    if has_errors {
        prog.fail("error");
    } else {
        prog.done(&format!(
            "{} warning{}",
            sema.warnings.len(),
            if sema.warnings.len() == 1 { "" } else { "s" }
        ));
    }

    if !prog.silent {
        for w in &sema.warnings {
            eprintln!(
                "{}",
                w.render_with_source_files(&result.merged_source, &result.source_files)
            );
        }
        for hint in &sema.suggestions {
            eprintln!("\x1b[2mhint: {}\x1b[0m", hint.message);
        }
    }
    if has_errors {
        for e in &sema.errors {
            eprintln!(
                "{}",
                e.render_with_source_files(&result.merged_source, &result.source_files)
            );
        }
        std::process::exit(1);
    }

    prog.begin("bytecode");
    let mut cg = bytecode::Codegen::new(&sema);
    cg.set_native_mangling(package_settings.mangling);
    cg.set_incremental_codegen(cached_codegen_units, source_hashes);
    if qzi_metadata
        .as_ref()
        .is_some_and(|metadata| metadata.kind == bytecode::QziModuleKind::Library)
    {
        cg.retain_public_library_api(result.library_file_paths.iter().cloned().collect());
    }
    let mut chunks = cg
        .compile_program(&target_program, &result.source_files)
        .unwrap_or_else(|error| {
            prog.fail("error");
            eprintln!("\x1b[31;1merror:\x1b[0m code generation failed: {error}");
            std::process::exit(1);
        });
    let external_call_relocations = cg.external_call_relocations().to_vec();
    let incremental_stats = cg.incremental_codegen_stats();
    let incremental_snapshot = cg.incremental_codegen_snapshot().to_vec();
    prog.done(&format!(
        "{} · {} restored · {} compiled",
        codegen_stats(&chunks, prog.is_tty),
        incremental_stats.restored,
        incremental_stats.compiled
    ));

    if !qzi_dependencies.is_empty() {
        prog.begin("qzi linking");
        let generated = bytecode::QziModule {
            metadata: qzi_metadata.clone().unwrap_or(bytecode::QziMetadata {
                name: String::new(),
                version: None,
                kind: bytecode::QziModuleKind::Executable,
                main_takes_args: sema.main_takes_args,
            }),
            interface: String::new(),
            call_relocations: external_call_relocations,
            chunks,
        };
        let mut modules = vec![generated];
        for dependency in qzi_dependencies {
            let bytes = std::fs::read(dependency).unwrap_or_else(|error| {
                prog.fail("error");
                eprintln!(
                    "\x1b[31;1merror:\x1b[0m cannot read QZI dependency '{}': {error}",
                    dependency.display()
                );
                std::process::exit(1);
            });
            modules.push(
                bytecode::deserialize_qzi_module(&bytes).unwrap_or_else(|error| {
                    prog.fail("error");
                    eprintln!(
                        "\x1b[31;1merror:\x1b[0m invalid QZI dependency '{}': {error}",
                        dependency.display()
                    );
                    std::process::exit(1);
                }),
            );
        }
        chunks = bytecode::link_qzi_modules(&modules).unwrap_or_else(|error| {
            prog.fail("error");
            eprintln!("\x1b[31;1merror:\x1b[0m cannot link QZI dependencies: {error}");
            std::process::exit(1);
        });
        prog.done(&format!(
            "{} module{}",
            modules.len(),
            if modules.len() == 1 { "" } else { "s" }
        ));
    }

    if debug {
        for chunk in &chunks {
            eprint!("{}", chunk);
        }
    }
    let no_crash = !package_settings.crash_handler;
    let project_qzi = qzi_metadata.clone().map(|mut metadata| {
        metadata.main_takes_args = sema.main_takes_args;
        let interface = if metadata.kind == bytecode::QziModuleKind::Library {
            let excluded_paths: HashSet<PathBuf> =
                result.library_file_paths.iter().cloned().collect();
            bytecode::build_qzi_interface(
                &metadata.name,
                &target_program,
                &result.source_files,
                &result.namespaced_paths,
                &excluded_paths,
            )
            .unwrap_or_else(|error| {
                prog.fail("error");
                eprintln!("\x1b[31;1merror:\x1b[0m cannot build QZI interface: {error}");
                std::process::exit(1);
            })
        } else {
            String::new()
        };
        let mut module = bytecode::QziModule {
            metadata,
            interface,
            call_relocations: Vec::new(),
            chunks: chunks.clone(),
        };
        module.qualify_library_root_symbols();
        bytecode::serialize_qzi_module(&module).unwrap_or_else(|error| {
            prog.fail("error");
            eprintln!("\x1b[31;1merror:\x1b[0m cannot serialize QZI: {error}");
            std::process::exit(1);
        })
    });

    if let (Some(cache_path), Some(qzi)) = (qzc_path, project_qzi.as_deref()) {
        prog.begin("cache write");
        let mut cache_inputs = result.loaded_files.clone();
        cache_inputs.extend_from_slice(extra_cache_inputs);
        cache_inputs.extend_from_slice(qzi_dependencies);
        if let Err(error) = incremental::store(
            cache_path,
            &cache_inputs,
            &result.source_hashes,
            qzi,
            no_crash,
            incremental_context,
            &incremental_snapshot,
        ) {
            prog.fail("not saved");
            if !prog.silent {
                eprintln!("\x1b[33;1mwarning:\x1b[0m cannot update QZC: {error}");
            }
        } else {
            prog.done("saved");
        }
    }

    match emit {
        EmitType::Bytecode => {
            let bytes = project_qzi.unwrap_or_else(|| {
                bytecode::serialize_qzi(&chunks).unwrap_or_else(|error| {
                    prog.fail("error");
                    eprintln!("\x1b[31;1merror:\x1b[0m cannot serialize QZI: {error}");
                    std::process::exit(1);
                })
            });
            if bytes.is_empty() {
                prog.fail("error");
                eprintln!("\x1b[31;1merror:\x1b[0m cannot serialize empty QZI");
                std::process::exit(1);
            }
            let mut f = std::fs::File::create(out).unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m cannot create {}: {}", out, e);
                std::process::exit(1);
            });
            use std::io::Write as _;
            f.write_all(&bytes).unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m write failed: {}", e);
                std::process::exit(1);
            });
            prog.success(out, Some(bytes.len() as u64));
        }

        EmitType::Object => {
            // ── Step 3: Native ────────────────────────────────────────────────
            let arch = arch_label();
            prog.begin(&format!("native  {}", arch));
            let obj_bytes = compile_to_object(
                &chunks,
                false,
                no_crash,
                Some(&sema),
                sema.main_takes_args,
                target,
            );
            write_or_package_object(
                &obj_bytes,
                out,
                link_flags.unwrap_or(&[]),
                explicit_linker,
                target,
                &sema.exported_symbols.values().cloned().collect::<Vec<_>>(),
            );
            prog.done(&format!("{:.1} KB", obj_bytes.len() as f64 / 1024.0));
            prog.success(out, Some(obj_bytes.len() as u64));
        }

        EmitType::Binary => {
            // ── Step 3: Native ────────────────────────────────────────────────
            let arch = arch_label();
            prog.begin(&format!("native  {}", arch));
            let obj_bytes = compile_to_object(
                &chunks,
                true,
                no_crash,
                Some(&sema),
                sema.main_takes_args,
                target,
            );
            prog.done(&format!(
                "{:.1} KB  object",
                obj_bytes.len() as f64 / 1024.0
            ));

            // ── Step 4: Linking ───────────────────────────────────────────────
            prog.begin("linking");
            let flags = link_flags.unwrap_or(&[]);
            backend::linker::link_object(
                &obj_bytes,
                Path::new(out),
                target.clone(),
                flags,
                explicit_linker,
            )
            .unwrap_or_else(|e| {
                prog.fail("error");
                eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                std::process::exit(1);
            });

            let bin_size = std::fs::metadata(out).map(|m| m.len()).ok();
            let link_info = match bin_size {
                Some(b) => format!("{:.1} KB", b as f64 / 1024.0),
                None => String::new(),
            };
            prog.done(&link_info);

            if do_strip {
                prog.begin("stripping");
                strip_binary(Path::new(out));
                prog.done("");
            }

            let final_size = std::fs::metadata(out).map(|m| m.len()).ok();
            prog.success(out, final_size);
        }
    }
}

fn main() {
    #[cfg(windows)]
    {
        unsafe extern "system" {
            fn SetConsoleOutputCP(wCodePageID: u32) -> i32;
        }
        unsafe {
            SetConsoleOutputCP(65001);
        }
    }
    let args = Args::parse();
    let no_color_output = matches!(
        &args.command,
        CliCmd::Build { no_color: true, .. }
            | CliCmd::Run { no_color: true, .. }
            | CliCmd::Test { no_color: true, .. }
    );
    NO_COLOR_OUTPUT.store(no_color_output, Ordering::Relaxed);

    let command = match args.command {
        CliCmd::Run {
            files,
            linker,
            strip,
            library_paths,
            libraries,
            no_incremental,
            bin,
            lib,
            target,
            silent,
            no_color,
            no_unicode,
            no_progress,
        } => CliCmd::Build {
            files,
            output: None,
            emit_bytecode: false,
            emit_object: false,
            run: true,
            debug: false,
            linker,
            strip,
            library_paths,
            libraries,
            static_lib: false,
            shared_lib: false,
            no_incremental,
            bin,
            lib,
            target,
            silent,
            no_color,
            no_unicode,
            no_progress,
        },
        command => command,
    };

    match command {
        CliCmd::Build {
            files,
            output,
            emit_bytecode,
            emit_object,
            run,
            debug,
            linker,
            strip,
            library_paths,
            libraries,
            static_lib,
            shared_lib,
            no_incremental,
            bin,
            lib,
            target,
            silent,
            no_color,
            no_unicode,
            no_progress,
        } => {
            package::configure_output(silent, no_color, no_unicode, no_progress);
            let target = match target {
                Some(cli::TargetTriple::X86_64Linux) => TargetSpec::x86_64_linux(),
                Some(cli::TargetTriple::X86_64Windows) => TargetSpec::x86_64_windows(),
                None => TargetSpec::host(),
            };
            if run && target.triple() != TargetSpec::host().triple() {
                eprintln!(
                    "\x1b[31;1merror:\x1b[0m cannot run target '{}' on host '{}'; use `qz build`",
                    target.triple(),
                    TargetSpec::host().triple()
                );
                std::process::exit(1);
            }
            let emit = if emit_bytecode {
                EmitType::Bytecode
            } else if emit_object || static_lib || shared_lib {
                EmitType::Object
            } else {
                EmitType::Binary
            };

            let cli_linker = linker.as_deref();
            let explicit_linker = cli_linker;
            let do_strip = strip && matches!(emit, EmitType::Binary);

            if files.is_empty() {
                let preview =
                    project::preview(Path::new("."), bin.as_deref(), lib).unwrap_or_else(|error| {
                        eprintln!("\x1b[31;1merror:\x1b[0m {error}");
                        std::process::exit(1);
                    });
                let preview_is_lib = preview.kind == project::ProjectKind::Lib;
                let preview_emit = if preview_is_lib
                    && matches!(emit, EmitType::Binary)
                    && !emit_bytecode
                    && !emit_object
                {
                    EmitType::Bytecode
                } else {
                    emit.clone()
                };
                let preview_output = output.clone().unwrap_or_else(|| {
                    let name = if static_lib {
                        format!("lib{}.a", preview.name)
                    } else if shared_lib {
                        shared_library_name(&preview.name, &target)
                    } else {
                        target_output_name(&preview.name, preview_emit, &target)
                    };
                    preview.out_dir.join(name).to_string_lossy().into_owned()
                });
                let preview_progress =
                    progress::BuildProgress::new(silent, no_color, no_unicode, no_progress);
                preview_progress.header(
                    preview
                        .entry
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("?"),
                    Path::new(&preview_output)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(&preview_output),
                );
                let mut ctx = load_project_context();
                ctx.select_artifact(bin.as_deref(), lib)
                    .unwrap_or_else(|e| {
                        eprintln!("\x1b[31;1merror:\x1b[0m {e}");
                        std::process::exit(1);
                    });
                let target = apply_package_settings(target, ctx.config.package);
                let manifest_link = ctx.link_for_target(target.triple());
                let manifest_linker = manifest_link
                    .linker
                    .as_ref()
                    .filter(|linker| linker.as_str() != "auto")
                    .map(PathBuf::from);
                let explicit_linker = cli_linker.or(manifest_linker.as_deref());
                ctx.config.link = manifest_link;
                ctx.ensure_lockfile().unwrap_or_else(|e| {
                    eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                    std::process::exit(1);
                });

                // Library projects default to bytecode output; binaries default to native binary.
                let is_lib = ctx.config.kind == project::ProjectKind::Lib;
                let effective_emit =
                    if is_lib && matches!(emit, EmitType::Binary) && !emit_bytecode && !emit_object
                    {
                        EmitType::Bytecode
                    } else {
                        emit.clone()
                    };
                let effective_strip = strip && matches!(effective_emit, EmitType::Binary);

                if !silent
                    && is_lib
                    && matches!(emit, EmitType::Binary)
                    && !emit_bytecode
                    && !emit_object
                {
                    eprintln!(
                        "\x1b[33;1mnote:\x1b[0m library project — emitting bytecode (.qzi). Use -c for object file."
                    );
                }

                let entry = ctx.config.entry.clone();
                let out = output.clone().unwrap_or_else(|| {
                    let name = if static_lib {
                        format!("lib{}.a", ctx.config.name)
                    } else if shared_lib {
                        shared_library_name(&ctx.config.name, &target)
                    } else {
                        target_output_name(&ctx.config.name, effective_emit.clone(), &target)
                    };
                    ctx.config.out_dir.join(name).to_string_lossy().into_owned()
                });
                // Portable bytecode contains no native objects. Requiring a C
                // compiler for `qz build -i` made FFI projects impossible to
                // validate on machines that only consume the QZI elsewhere.
                let mut link_flags = if matches!(effective_emit, EmitType::Bytecode) {
                    Vec::new()
                } else {
                    native_link_flags(&ctx).unwrap_or_else(|e| {
                        eprintln!("\x1b[31;1merror:\x1b[0m {e}");
                        std::process::exit(1);
                    })
                };
                link_flags.extend(
                    library_paths
                        .iter()
                        .map(|path| format!("-L{}", path.display())),
                );
                link_flags.extend(libraries.iter().map(|name| format!("-l{name}")));
                let qzc_path =
                    (!no_incremental).then(|| ctx.incremental_cache_path(target.triple()));
                let cache_inputs = ctx.incremental_inputs();
                build_with_progress(
                    &[entry],
                    &out,
                    effective_emit,
                    debug,
                    Some(&link_flags),
                    explicit_linker,
                    effective_strip,
                    Some(bytecode::QziMetadata {
                        name: ctx.config.name.clone(),
                        version: ctx.config.version.clone(),
                        kind: if is_lib {
                            bytecode::QziModuleKind::Library
                        } else {
                            bytecode::QziModuleKind::Executable
                        },
                        main_takes_args: false,
                    }),
                    &ctx.config.qzi_dependencies,
                    qzc_path.as_deref(),
                    &cache_inputs,
                    &target,
                    silent,
                    no_color,
                    no_unicode,
                    no_progress,
                    Some(preview_progress),
                    Some(&ctx.resolver),
                    ctx.config.package,
                );
                let _ = (emit, do_strip);

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
            } else if files
                .iter()
                .any(|f| f.extension().is_some_and(|e| e == "qzi"))
            {
                // .qzi input — deserialize and compile to native directly;
                // ELF objects may be supplied alongside it for final linking.
                for input in &files {
                    if !input.extension().is_some_and(|extension| {
                        matches!(extension.to_str(), Some("qzi" | "o" | "obj"))
                    }) {
                        eprintln!(
                            "\x1b[31;1merror:\x1b[0m .qzi builds accept only .qzi and .o inputs: {}",
                            input.display()
                        );
                        std::process::exit(1);
                    }
                }

                let mut qzi_modules = Vec::new();
                for qzi_file in files
                    .iter()
                    .filter(|path| path.extension().is_some_and(|extension| extension == "qzi"))
                {
                    let bytes = std::fs::read(qzi_file).unwrap_or_else(|e| {
                        eprintln!(
                            "\x1b[31;1merror:\x1b[0m cannot read '{}': {}",
                            qzi_file.display(),
                            e
                        );
                        std::process::exit(1);
                    });
                    let module = bytecode::deserialize_qzi_module(&bytes).unwrap_or_else(|e| {
                        eprintln!(
                            "\x1b[31;1merror:\x1b[0m invalid .qzi '{}': {}",
                            qzi_file.display(),
                            e
                        );
                        std::process::exit(1);
                    });
                    qzi_modules.push(module);
                }
                let all_chunks = bytecode::link_qzi_modules(&qzi_modules).unwrap_or_else(|e| {
                    eprintln!("\x1b[31;1merror:\x1b[0m cannot link QZI modules: {e}");
                    std::process::exit(1);
                });
                let main_takes_args = qzi_modules.iter().any(|module| {
                    module.metadata.kind == bytecode::QziModuleKind::Executable
                        && module.metadata.main_takes_args
                });

                let out = output.clone().unwrap_or_else(|| {
                    let stem = files
                        .iter()
                        .find(|path| path.extension().is_some_and(|extension| extension == "qzi"))
                        .expect("qzi branch has a qzi input")
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    target_output_name(&stem, emit.clone(), &target)
                });

                let mut qzi_link_flags: Vec<String> = files
                    .iter()
                    .filter(|path| {
                        path.extension().is_some_and(|extension| {
                            matches!(extension.to_str(), Some("o" | "obj"))
                        })
                    })
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect();
                let cwd = std::env::current_dir().unwrap_or_default();
                let qzi_context = ProjectContext::discover(&cwd).unwrap_or_else(|e| {
                    eprintln!("\x1b[31;1merror:\x1b[0m {e}");
                    std::process::exit(1);
                });
                let package_settings = qzi_context
                    .as_ref()
                    .map(|context| context.config.package)
                    .unwrap_or_default();
                let target = apply_package_settings(target, package_settings);
                if matches!(emit, EmitType::Binary) {
                    if let Some(ctx) = qzi_context {
                        qzi_link_flags.extend(native_link_flags(&ctx).unwrap_or_else(|e| {
                            eprintln!("\x1b[31;1merror:\x1b[0m {e}");
                            std::process::exit(1);
                        }));
                    }
                    qzi_link_flags.extend(
                        library_paths
                            .iter()
                            .map(|path| format!("-L{}", path.display())),
                    );
                    qzi_link_flags.extend(libraries.iter().map(|name| format!("-l{name}")));
                }
                let mut seen_objects = HashSet::new();
                qzi_link_flags.retain(|flag| {
                    !Path::new(flag)
                        .extension()
                        .is_some_and(|extension| matches!(extension.to_str(), Some("o" | "obj")))
                        || seen_objects.insert(
                            std::fs::canonicalize(flag).unwrap_or_else(|_| PathBuf::from(flag)),
                        )
                });
                emit_chunks(
                    &all_chunks,
                    emit,
                    &out,
                    Some(&qzi_link_flags),
                    explicit_linker,
                    debug,
                    !package_settings.crash_handler,
                    main_takes_args,
                    true,
                    &target,
                );

                if do_strip {
                    strip_binary(Path::new(&out));
                }

                if run && !emit_bytecode && !emit_object {
                    let status = std::process::Command::new(abs_path(&out))
                        .status()
                        .unwrap_or_else(|e| {
                            eprintln!("\x1b[31;1merror:\x1b[0m cannot run binary: {}", e);
                            std::process::exit(1);
                        });
                    if !status.success() {
                        std::process::exit(status.code().unwrap_or(1));
                    }
                }
            } else {
                let source_files: Vec<PathBuf> = files
                    .iter()
                    .filter(|path| path.extension().is_some_and(|ext| ext == "qz"))
                    .cloned()
                    .collect();
                let native_objects: Vec<String> = files
                    .iter()
                    .filter(|path| {
                        path.extension().is_some_and(|ext| {
                            matches!(ext.to_str(), Some("o" | "a" | "so" | "obj" | "lib"))
                        })
                    })
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect();
                let c_sources: Vec<PathBuf> = files
                    .iter()
                    .filter(|path| path.extension().is_some_and(|ext| ext == "c"))
                    .cloned()
                    .collect();
                let compiled_c_objects = compile_direct_c_sources(&c_sources).unwrap_or_else(|e| {
                    eprintln!("\x1b[31;1merror:\x1b[0m {e}");
                    std::process::exit(1);
                });
                if source_files.is_empty() {
                    if !matches!(emit, EmitType::Binary)
                        || (native_objects.is_empty() && compiled_c_objects.is_empty())
                    {
                        eprintln!(
                            "\x1b[31;1merror:\x1b[0m build requires a .qz source or a linkable object"
                        );
                        std::process::exit(1);
                    }
                    let mut objects = native_objects;
                    objects.extend(
                        compiled_c_objects
                            .iter()
                            .map(|path| path.to_string_lossy().into_owned()),
                    );
                    let primary_index = objects
                        .iter()
                        .position(|path| {
                            Path::new(path).extension().is_some_and(|extension| {
                                matches!(extension.to_str(), Some("o" | "obj"))
                            })
                        })
                        .unwrap_or_else(|| {
                            eprintln!(
                                "\x1b[31;1merror:\x1b[0m object-only builds require at least one .o input"
                            );
                            std::process::exit(1);
                        });
                    let primary = objects.remove(primary_index);
                    let primary_bytes = std::fs::read(&primary).unwrap_or_else(|error| {
                        eprintln!("\x1b[31;1merror:\x1b[0m cannot read object {primary}: {error}");
                        std::process::exit(1);
                    });
                    objects.extend(
                        library_paths
                            .iter()
                            .map(|path| format!("-L{}", path.display())),
                    );
                    objects.extend(libraries.iter().map(|name| format!("-l{name}")));
                    let out = output.clone().unwrap_or_else(|| {
                        let stem = Path::new(&primary)
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy();
                        target_output_name(&stem, EmitType::Binary, &target)
                    });
                    link_object(
                        &primary_bytes,
                        Path::new(&out),
                        target.clone(),
                        &objects,
                        explicit_linker,
                    )
                    .unwrap_or_else(|error| {
                        eprintln!("\x1b[31;1merror:\x1b[0m {error}");
                        std::process::exit(1);
                    });
                    for object in &compiled_c_objects {
                        remove_temp(object);
                    }
                    if do_strip {
                        strip_binary(Path::new(&out));
                    }
                    println!("\x1b[1;32mbuilt\x1b[0m  \x1b[1m{out}\x1b[0m");
                    if run {
                        let status = std::process::Command::new(abs_path(&out))
                            .status()
                            .unwrap_or_else(|error| {
                                eprintln!("\x1b[31;1merror:\x1b[0m failed to run binary: {error}");
                                std::process::exit(1);
                            });
                        if !status.success() {
                            std::process::exit(status.code().unwrap_or(1));
                        }
                    }
                    return;
                }
                let out = output.clone().unwrap_or_else(|| {
                    if static_lib {
                        let stem = source_files[0]
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy();
                        return format!("lib{stem}.a");
                    }
                    if shared_lib {
                        let stem = source_files[0]
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy();
                        return shared_library_name(&stem, &target);
                    }
                    let stem = source_files[0]
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    match emit {
                        EmitType::Bytecode => format!("{}.qzi", stem),
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

                let mut link_flags = native_objects;
                link_flags.extend(
                    compiled_c_objects
                        .iter()
                        .map(|path| path.to_string_lossy().into_owned()),
                );
                link_flags.extend(
                    library_paths
                        .iter()
                        .map(|path| format!("-L{}", path.display())),
                );
                link_flags.extend(libraries.iter().map(|name| format!("-l{name}")));
                let package_settings = ProjectContext::discover(&source_files[0])
                    .unwrap_or_else(|error| {
                        eprintln!("\x1b[31;1merror:\x1b[0m {error}");
                        std::process::exit(1);
                    })
                    .map(|context| context.config.package)
                    .unwrap_or_default();
                let target = apply_package_settings(target, package_settings);
                build_with_progress(
                    &source_files,
                    &out,
                    emit,
                    debug,
                    Some(&link_flags),
                    explicit_linker,
                    do_strip,
                    None,
                    &[],
                    None,
                    &[],
                    &target,
                    silent,
                    no_color,
                    no_unicode,
                    no_progress,
                    None,
                    None,
                    package_settings,
                );
                for object in &compiled_c_objects {
                    remove_temp(object);
                }

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

        CliCmd::Run { .. } => unreachable!("run commands are normalized to build --run"),

        CliCmd::Header {
            files,
            output,
            target,
        } => {
            let result = if files.is_empty() {
                let ctx = load_project_context();
                loader::load_programs_configured(
                    &[ctx.config.entry],
                    Some(&ctx.resolver),
                    ctx.config.package.std,
                    &[],
                )
            } else {
                load_with_optional_project(&files)
            }
            .unwrap_or_else(|error| {
                eprintln!("\x1b[31;1merror:\x1b[0m {error}");
                std::process::exit(1);
            });
            if let Some(error) = &result.parse_error {
                eprintln!("{error}");
                std::process::exit(1);
            }
            let namespaced_paths: HashSet<String> = result
                .namespaced_paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            let (target_os, target_abi) = match target {
                cli::HeaderTarget::X86_64Linux => ("linux", "sysv"),
                cli::HeaderTarget::X86_64Windows => ("windows", "win64"),
            };
            let target_program =
                semantic::strip_cfg_for(&result.program, target_os, "x86_64", target_abi);
            let report = analyze_program_with_source_files(
                &result.merged_source,
                &target_program,
                result.library_fn_names.clone(),
                result.library_char_ranges.clone(),
                result.source_files.clone(),
                namespaced_paths,
            );
            if !report_diagnostics(&report, &result.merged_source, &result.source_files) {
                std::process::exit(1);
            }
            let guard = output
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("quazi.h");
            let generated =
                header::generate(&result.program, &result.library_char_ranges, target, guard)
                    .unwrap_or_else(|errors| {
                        for error in errors {
                            eprintln!("\x1b[31;1merror:\x1b[0m {error}");
                        }
                        std::process::exit(1);
                    });
            std::fs::write(&output, generated).unwrap_or_else(|error| {
                eprintln!(
                    "\x1b[31;1merror:\x1b[0m cannot write '{}': {error}",
                    output.display()
                );
                std::process::exit(1);
            });
            println!(
                "\x1b[1;32mgenerated\x1b[0m  \x1b[1m{}\x1b[0m",
                output.display()
            );
        }

        CliCmd::Check { bin, lib, target } => {
            let mut ctx = load_project_context();
            ctx.select_artifact(bin.as_deref(), lib)
                .unwrap_or_else(|error| {
                    eprintln!("\x1b[31;1merror:\x1b[0m {error}");
                    std::process::exit(1);
                });
            let entry = ctx.config.entry.clone();
            let result = loader::load_programs_configured(
                &[entry],
                Some(&ctx.resolver),
                ctx.config.package.std,
                &[],
            )
                .unwrap_or_else(|e| {
                    eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                    std::process::exit(1);
                });
            if let Some(e) = &result.parse_error {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            let namespaced_paths: HashSet<String> = result
                .namespaced_paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            let target = match target {
                Some(cli::TargetTriple::X86_64Linux) => TargetSpec::x86_64_linux(),
                Some(cli::TargetTriple::X86_64Windows) => TargetSpec::x86_64_windows(),
                None => TargetSpec::host(),
            };
            let (target_os, target_abi) = match target.os {
                backend::target::Os::Windows => ("windows", "win64"),
                backend::target::Os::Linux => ("linux", "sysv"),
                backend::target::Os::MacOs => ("macos", "sysv"),
            };
            let target_program =
                semantic::strip_cfg_for(&result.program, target_os, "x86_64", target_abi);
            run_check(
                &result.merged_source,
                &target_program,
                result.library_fn_names,
                result.library_char_ranges,
                result.source_files,
                namespaced_paths,
            );
        }

        CliCmd::Test {
            filter,
            no_color,
            no_unicode,
        } => {
            let success = test_runner::run(filter.as_deref(), no_color, no_unicode)
                .unwrap_or_else(|error| {
                    eprintln!("\x1b[31;1merror:\x1b[0m {error}");
                    std::process::exit(1);
                });
            if !success {
                std::process::exit(1);
            }
        }

        CliCmd::Fetch => {
            let preview = project::preview(Path::new("."), None, false).unwrap_or_else(|error| {
                eprintln!("\x1b[31;1merror:\x1b[0m {error}");
                std::process::exit(1);
            });
            progress::BuildProgress::new(false, false, false, false).header(
                "quazi.toml",
                preview
                    .out_dir
                    .join("deps")
                    .strip_prefix(&preview.root)
                    .unwrap_or_else(|_| Path::new("deps"))
                    .to_string_lossy()
                    .as_ref(),
            );
            let ctx = load_project_context();
            ctx.ensure_lockfile().unwrap_or_else(|error| {
                eprintln!("\x1b[31;1merror:\x1b[0m {error}");
                std::process::exit(1);
            });
            if ctx.config.dependencies.is_empty() {
                eprintln!("  Dependencies  ·  none");
            } else {
                eprintln!(
                    "  \x1b[32m◆\x1b[0m  dependencies  ·  {} resolved  ·  quazi.lock updated",
                    ctx.config.dependencies.len()
                );
            }
        }

        CliCmd::Deps => {
            let ctx = load_project_context();
            ctx.ensure_lockfile().unwrap_or_else(|error| {
                eprintln!("\x1b[31;1merror:\x1b[0m {error}");
                std::process::exit(1);
            });
            eprintln!(
                "\n  \x1b[1mDependencies\x1b[0m  ({})\n",
                ctx.config.dependencies.len()
            );
            for dependency in &ctx.config.dependencies {
                let version = dependency.version.as_deref().unwrap_or("unversioned");
                eprintln!(
                    "  \x1b[36m{}\x1b[0m  \x1b[2m{} · {}\x1b[0m\n      \x1b[2m{}\x1b[0m",
                    dependency.name,
                    version,
                    dependency.kind.as_str(),
                    dependency.root.display()
                );
            }
            eprintln!();
        }

        CliCmd::Add {
            dependency,
            kind,
            alias,
            version,
            checksum,
        } => {
            let kind = kind.map(|kind| format!("{kind:?}").to_ascii_lowercase());
            let (inferred_name, path, url) = if dependency.starts_with("https://")
                || dependency.starts_with("http://")
            {
                let url = Some(dependency);
                let name =
                    project::infer_dependency_name(None, url.as_deref()).unwrap_or_else(|error| {
                        eprintln!("\x1b[31;1merror:\x1b[0m {error}");
                        std::process::exit(1);
                    });
                (name, None, url)
            } else {
                let path = Some(PathBuf::from(dependency));
                let name =
                    project::infer_dependency_name(path.as_deref(), None).unwrap_or_else(|error| {
                        eprintln!("\x1b[31;1merror:\x1b[0m {error}");
                        std::process::exit(1);
                    });
                (name, path, None)
            };
            let name = alias.unwrap_or(inferred_name);
            if url.is_some() {
                let preview =
                    project::preview(Path::new("."), None, false).unwrap_or_else(|error| {
                        eprintln!("\x1b[31;1merror:\x1b[0m {error}");
                        std::process::exit(1);
                    });
                progress::BuildProgress::new(false, false, false, false).header(
                    "quazi.toml",
                    preview
                        .out_dir
                        .join("deps")
                        .strip_prefix(&preview.root)
                        .unwrap_or_else(|_| Path::new("deps"))
                        .to_string_lossy()
                        .as_ref(),
                );
            }
            let context = project::add_dependency(
                Path::new("."),
                project::DependencyEdit {
                    name: name.clone(),
                    path,
                    url,
                    kind,
                    version,
                    revision: None,
                    checksum,
                },
            )
            .unwrap_or_else(|error| {
                eprintln!("\x1b[31;1merror:\x1b[0m {error}");
                std::process::exit(1);
            });
            let dependency = context
                .config
                .dependencies
                .iter()
                .find(|dependency| dependency.name == name)
                .expect("added dependency resolved");
            println!(
                "\x1b[32;1madded\x1b[0m  {}  \x1b[2m{} · {}\x1b[0m",
                dependency.name,
                dependency.kind.as_str(),
                dependency.root.display()
            );
        }

        CliCmd::Remove { name } => {
            project::remove_dependency(Path::new("."), &name).unwrap_or_else(|error| {
                eprintln!("\x1b[31;1merror:\x1b[0m {error}");
                std::process::exit(1);
            });
            println!("\x1b[32;1mremoved\x1b[0m  {name}");
        }

        CliCmd::New { name, lib } => {
            create_new_project(&name, lib);
        }

        CliCmd::Init { lib } => {
            init_project(lib);
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
import std.io;

fn main() i32 {
    const z = "hi";
    const y: i32 = 10;
    var x: i32 = 5;

    for x < y {
        io.println("{} quazi! x = {}", z, x);
        x++;
    }
    ret 0;
}
"#;
            let (emit, output) = if emit_bytecode {
                (EmitType::Bytecode, "dbg.qzi".to_owned())
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
                    Vec::new(),
                    Vec::new(),
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
