// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

pub mod analysis;
mod backend;
pub mod bytecode;
pub mod cli;
pub mod lexer;
pub mod loader;
mod lsp;
pub mod parser;
mod progress;
mod project;
pub mod semantic;

use analysis::{analyze_program_with_source_files, format_quazi_source};
use backend::linker::{LinkerInvocation, remove_temp, write_temp_object};
use backend::{TargetSpec, select_backend};
use bytecode::{Codegen, serialize_qzi};
use clap::Parser as ClapParser;
use cli::Args;
use lexer::Lexer;
use parser::Parser;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use cli::Command as CliCmd;
use cli::EmitType;
use project::ProjectContext;

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
    let chunks = cg.compile_program(program, &source_files);

    if debug {
        for chunk in &chunks {
            eprint!("{}", chunk);
        }
    }

    match emit {
        EmitType::Bytecode => {
            let bytes = serialize_qzi(&chunks);
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
            let no_crash = source_contains_no_crash(src);
            let obj_bytes = compile_to_object(&chunks, false, no_crash, Some(&sema_report));
            write_or_package_object(
                &obj_bytes,
                output_file_name,
                link_flags.unwrap_or(&[]),
                explicit_linker,
            );
            println!(
                "\x1b[1;32mbuilt\x1b[0m  \x1b[1m{output_file_name}\x1b[0m  \x1b[2m[object]\x1b[0m"
            );
        }

        EmitType::Binary => {
            let no_crash = source_contains_no_crash(src);
            let obj_bytes = compile_to_object(&chunks, true, no_crash, Some(&sema_report));
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

fn write_or_package_object(
    bytes: &[u8],
    output: &str,
    link_flags: &[String],
    explicit_linker: Option<&Path>,
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
        Some("so") => {
            let object = write_temp_object(bytes, "ffi_shared").unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m {e}");
                std::process::exit(1);
            });
            let mut flags = vec!["-shared".to_string()];
            flags.extend_from_slice(link_flags);
            let mut invocation = LinkerInvocation::new(
                object.clone(),
                PathBuf::from(output),
                TargetSpec::host(),
                flags,
            )
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

fn source_contains_no_crash(src: &str) -> bool {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();
    tokens.windows(2).any(|pair| {
        matches!(pair[0].kind, lexer::token::TokenKind::At)
            && matches!(&pair[1].kind, lexer::token::TokenKind::Ident(name) if name == "no_crash")
    })
}

fn compile_to_object(
    chunks: &[crate::bytecode::Chunk],
    emit_start: bool,
    no_crash: bool,
    report: Option<&crate::semantic::SemanticReport>,
) -> Vec<u8> {
    let mut target = TargetSpec::host();
    if !emit_start {
        target = target.without_start();
    }
    if no_crash {
        target = target.with_no_crash();
    }
    let backend = select_backend(&target);
    backend
        .compile(chunks, &target, report)
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
) {
    if debug {
        for chunk in chunks {
            eprint!("{}", chunk);
        }
    }

    match emit {
        EmitType::Bytecode => {
            let bytes = serialize_qzi(chunks);
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
            for chunk in chunks {
                print!("{}", chunk);
            }
        }

        EmitType::Object => {
            let obj_bytes = compile_to_object(chunks, false, false, None);
            write_or_package_object(
                &obj_bytes,
                output_file_name,
                link_flags.unwrap_or(&[]),
                explicit_linker,
            );
            println!(
                "\x1b[1;32mbuilt\x1b[0m  \x1b[1m{output_file_name}\x1b[0m  \x1b[2m[object]\x1b[0m"
            );
        }

        EmitType::Binary => {
            let obj_bytes = compile_to_object(chunks, true, false, None);
            let stem = Path::new(output_file_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(output_file_name);
            let tmp_obj = write_temp_object(&obj_bytes, stem).unwrap_or_else(|e| {
                eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                std::process::exit(1);
            });

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

fn project_output_name(name: &str, emit: EmitType) -> String {
    match emit {
        EmitType::Bytecode => format!("{}.qzi", name),
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

fn load_with_optional_project(files: &[PathBuf]) -> Result<loader::LoadResult, String> {
    let ctx = ProjectContext::discover(&files[0])?;
    let resolver_owned: Option<loader::ModuleResolver> = ctx.map(|c| c.resolver);
    loader::load_programs_with_resolver(files, resolver_owned.as_ref())
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
    let object_dir = ctx.config.root.join("target").join("ffi");
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
            .arg(source)
            .arg("-o")
            .arg(&object)
            .arg("-fPIC");
        for include in &ctx.config.cc.include_paths {
            command.arg(format!("-I{}", include.display()));
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
        flags.push(object.to_string_lossy().into_owned());
    }

    flags.extend(
        ctx.config
            .link
            .objects
            .iter()
            .map(|path| path.to_string_lossy().into_owned()),
    );
    flags.extend(
        ctx.config
            .link
            .library_paths
            .iter()
            .map(|path| format!("-L{}", path.display())),
    );
    flags.extend(
        ctx.config
            .link
            .libraries
            .iter()
            .map(|name| format!("-l{name}")),
    );
    Ok(flags)
}

fn compile_direct_c_sources(sources: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    let mut objects: Vec<PathBuf> = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        let object = std::env::temp_dir().join(format!("qz_cc_{}_{}.o", std::process::id(), index));
        let output = std::process::Command::new(&cc)
            .arg("-c")
            .arg(source)
            .arg("-o")
            .arg(&object)
            .arg("-fPIC")
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

fn scaffold_project(root: &PathBuf, pkg_name: &str, lib: bool) {
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap_or_else(|e| {
        eprintln!(
            "\x1b[31;1merror:\x1b[0m cannot create {}: {}",
            src_dir.display(),
            e
        );
        std::process::exit(1);
    });

    if lib {
        let toml = format!(
            "[package]\nname = \"{}\"\nversion = \"0.1.0\"\ntype = \"lib\"\n\n[build]\nentry = \"src/lib.qz\"\nsrc = \"src\"\n",
            pkg_name
        );
        write_file(&root.join("quazi.toml"), &toml);

        let lib_src = format!(
            "// {name} — quazilang library\n\npub fn add(a: i64, b: i64) i64 {{\n    ret a + b;\n}}\n",
            name = pkg_name
        );
        write_file(&src_dir.join("lib.qz"), &lib_src);
    } else {
        let toml = format!(
            "[package]\nname = \"{}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.qz\"\nsrc = \"src\"\n",
            pkg_name
        );
        write_file(&root.join("quazi.toml"), &toml);

        let main_src = "import std.io;\n\nfn main() i32 {\n    io.println(\"Hello, World!\");\n    ret 0;\n}\n";
        write_file(&src_dir.join("main.qz"), main_src);
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
    let ctx = load_project_context();
    let root = &ctx.config.root;
    let bin_name = project_output_name(&ctx.config.name, EmitType::Binary);

    let mut targets: HashSet<PathBuf> = HashSet::new();
    targets.insert(root.join(&bin_name));
    targets.insert(root.join(format!("{}.o", ctx.config.name)));
    targets.insert(root.join(format!("{}.qzi", ctx.config.name)));

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

fn build_with_progress(
    files: &[PathBuf],
    out: &str,
    emit: EmitType,
    debug: bool,
    link_flags: Option<&[String]>,
    explicit_linker: Option<&Path>,
    do_strip: bool,
) {
    use progress::{
        BuildProgress, arch_label, build_dep_tree, codegen_stats, common_lib_prefix, fmt_count,
    };

    let mut prog = BuildProgress::new();

    // Header line.
    let input_name = files[0].file_name().and_then(|s| s.to_str()).unwrap_or("?");
    let out_name = Path::new(out)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(out);
    prog.header(input_name, out_name);

    // ── Step 1: Lexing (file I/O + tokenize) ─────────────────────────────────
    prog.begin("Lexing");
    let result = match load_with_optional_project(files) {
        Ok(r) => r,
        Err(e) => {
            prog.fail("");
            eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
            std::process::exit(1);
        }
    };
    let tok_info = format!(
        "{} tokens · {} file{}",
        fmt_count(result.token_count),
        result.loaded_files.len(),
        if result.loaded_files.len() == 1 {
            ""
        } else {
            "s"
        }
    );
    prog.done(&tok_info);

    // Dependency tree (animated).
    let lib_prefix = common_lib_prefix(&result.library_file_paths);
    let tree_root = build_dep_tree(
        &files[0],
        &result.dep_edges,
        &result.library_file_paths,
        lib_prefix.as_deref(),
    );
    prog.dep_tree(&tree_root);

    // ── Step 2: Parsing ───────────────────────────────────────────────────────
    prog.begin("Parsing");
    if let Some(parse_err) = &result.parse_error {
        prog.fail("");
        eprintln!("{}", parse_err);
        std::process::exit(1);
    }
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
    prog.begin("Codegen");
    let namespaced_paths: HashSet<String> = result
        .namespaced_paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let sema = analyze_program_with_source_files(
        &result.merged_source,
        &result.program,
        result.library_fn_names,
        result.library_char_ranges,
        result.source_files.clone(),
        namespaced_paths,
    );
    let has_errors = !sema.errors.is_empty();
    let mut cg = bytecode::Codegen::new(&sema);
    let chunks = cg.compile_program(&result.program, &result.source_files);

    if has_errors {
        prog.fail("error");
    } else {
        prog.done(&codegen_stats(&chunks, prog.is_tty));
    }

    if debug {
        for chunk in &chunks {
            eprint!("{}", chunk);
        }
    }

    // Print warnings + errors.
    for w in &sema.warnings {
        eprintln!(
            "{}",
            w.render_with_source_files(&result.merged_source, &result.source_files)
        );
    }
    for hint in &sema.suggestions {
        eprintln!("\x1b[2mhint: {}\x1b[0m", hint.message);
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

    match emit {
        EmitType::Bytecode => {
            let bytes = bytecode::serialize_qzi(&chunks);
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
            prog.begin(&format!("Native  {}", arch));
            let no_crash = source_contains_no_crash(&result.merged_source);
            let obj_bytes = compile_to_object(&chunks, false, no_crash, Some(&sema));
            write_or_package_object(&obj_bytes, out, link_flags.unwrap_or(&[]), explicit_linker);
            prog.done(&format!("{:.1} KB", obj_bytes.len() as f64 / 1024.0));
            prog.success(out, Some(obj_bytes.len() as u64));
        }

        EmitType::Binary => {
            // ── Step 3: Native ────────────────────────────────────────────────
            let arch = arch_label();
            prog.begin(&format!("Native  {}", arch));
            let no_crash = source_contains_no_crash(&result.merged_source);
            let obj_bytes = compile_to_object(&chunks, true, no_crash, Some(&sema));
            prog.done(&format!(
                "{:.1} KB  object",
                obj_bytes.len() as f64 / 1024.0
            ));

            // ── Step 4: Linking ───────────────────────────────────────────────
            prog.begin("Linking");
            let stem = Path::new(out)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(out);
            let tmp_obj =
                backend::linker::write_temp_object(&obj_bytes, stem).unwrap_or_else(|e| {
                    prog.fail("error");
                    eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                    std::process::exit(1);
                });

            let flags = link_flags.unwrap_or(&[]);
            let mut inv = backend::linker::LinkerInvocation::new(
                tmp_obj.clone(),
                PathBuf::from(out),
                TargetSpec::host(),
                flags.to_vec(),
            )
            .unwrap_or_else(|e| {
                backend::linker::remove_temp(&tmp_obj);
                prog.fail("error");
                eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                std::process::exit(1);
            });
            if let Some(lnk) = explicit_linker {
                inv.linker = lnk.to_path_buf();
            }

            inv.run().unwrap_or_else(|e| {
                backend::linker::remove_temp(&tmp_obj);
                prog.fail("error");
                eprintln!("\x1b[31;1merror:\x1b[0m {}", e);
                std::process::exit(1);
            });
            backend::linker::remove_temp(&tmp_obj);

            let bin_size = std::fs::metadata(out).map(|m| m.len()).ok();
            let link_info = match bin_size {
                Some(b) => format!("{:.1} KB", b as f64 / 1024.0),
                None => String::new(),
            };
            prog.done(&link_info);

            if do_strip {
                prog.begin("Stripping");
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

    match args.command {
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
        } => {
            let emit = if emit_bytecode {
                EmitType::Bytecode
            } else if emit_object || static_lib || shared_lib {
                EmitType::Object
            } else {
                EmitType::Binary
            };

            let explicit_linker = linker.as_deref();
            let do_strip = strip && matches!(emit, EmitType::Binary);

            if files.is_empty() {
                let ctx = load_project_context();
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

                if is_lib && matches!(emit, EmitType::Binary) && !emit_bytecode && !emit_object {
                    eprintln!(
                        "\x1b[33;1mnote:\x1b[0m library project — emitting bytecode (.qzi). Use -c for object file."
                    );
                }

                let entry = ctx.config.entry.clone();
                let out = output.clone().unwrap_or_else(|| {
                    if static_lib {
                        format!("lib{}.a", ctx.config.name)
                    } else if shared_lib {
                        format!("lib{}.so", ctx.config.name)
                    } else {
                        project_output_name(&ctx.config.name, effective_emit.clone())
                    }
                });
                let mut link_flags = native_link_flags(&ctx).unwrap_or_else(|e| {
                    eprintln!("\x1b[31;1merror:\x1b[0m {e}");
                    std::process::exit(1);
                });
                link_flags.extend(
                    library_paths
                        .iter()
                        .map(|path| format!("-L{}", path.display())),
                );
                link_flags.extend(libraries.iter().map(|name| format!("-l{name}")));
                build_with_progress(
                    &[entry],
                    &out,
                    effective_emit,
                    debug,
                    Some(&link_flags),
                    explicit_linker,
                    effective_strip,
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
                // .qzi input — deserialize and compile to native directly.
                for qzi_file in &files {
                    if qzi_file.extension().is_some_and(|e| e != "qzi") {
                        eprintln!(
                            "\x1b[31;1merror:\x1b[0m cannot mix .qzi and source files: {}",
                            qzi_file.display()
                        );
                        std::process::exit(1);
                    }
                }

                let mut all_chunks: Vec<bytecode::Chunk> = Vec::new();
                for qzi_file in &files {
                    let bytes = std::fs::read(qzi_file).unwrap_or_else(|e| {
                        eprintln!(
                            "\x1b[31;1merror:\x1b[0m cannot read '{}': {}",
                            qzi_file.display(),
                            e
                        );
                        std::process::exit(1);
                    });
                    let chunks = bytecode::deserialize_qzi(&bytes).unwrap_or_else(|e| {
                        eprintln!(
                            "\x1b[31;1merror:\x1b[0m invalid .qzi '{}': {}",
                            qzi_file.display(),
                            e
                        );
                        std::process::exit(1);
                    });
                    all_chunks.extend(chunks);
                }

                let out = output.clone().unwrap_or_else(|| {
                    let stem = files[0]
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

                emit_chunks(&all_chunks, emit, &out, None, explicit_linker, debug);

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
                        path.extension()
                            .is_some_and(|ext| matches!(ext.to_str(), Some("o" | "a" | "so")))
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
                    eprintln!(
                        "\x1b[31;1merror:\x1b[0m build requires at least one .qz source file"
                    );
                    std::process::exit(1);
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
                        return format!("lib{stem}.so");
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
                build_with_progress(
                    &source_files,
                    &out,
                    emit,
                    debug,
                    Some(&link_flags),
                    explicit_linker,
                    do_strip,
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

        CliCmd::Run { linker, strip } => {
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
            if let Some(e) = &result.parse_error {
                eprintln!("{}", e);
                std::process::exit(1);
            }

            let out = project_output_name(&ctx.config.name, EmitType::Binary);
            let namespaced_paths: HashSet<String> = result
                .namespaced_paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            run_pipeline(
                &result.merged_source,
                &result.program,
                result.library_fn_names,
                result.library_char_ranges,
                result.source_files,
                namespaced_paths,
                false,
                EmitType::Binary,
                &out,
                Some(&ctx.config.flags),
                linker.as_deref(),
            );

            if strip {
                strip_binary(Path::new(&out));
            }

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
            if let Some(e) = &result.parse_error {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            let namespaced_paths: HashSet<String> = result
                .namespaced_paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            run_check(
                &result.merged_source,
                &result.program,
                result.library_fn_names,
                result.library_char_ranges,
                result.source_files,
                namespaced_paths,
            );
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
