// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! `qz build -i`: one compilation for every supported operating system.
//!
//! The loader resolves each target's imports into one program, the
//! target-view pass makes it analyzable once, and analysis and code
//! generation run a single time. The result is written as a format-16 QZI
//! directly: there is no per-target build, staging file or merge.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

// Diagnostics follow `--no-color` and plain-text pipes like the rest of the CLI.
macro_rules! eprintln {
    ($($arg:tt)*) => {
        crate::print_stderr_line(format_args!($($arg)*))
    };
}

use crate::analysis::analyze_program_with_source_files;
use crate::bytecode::chunk::{QZI_UNIFIED_VERSION, UnifiedQzi, decode_unified_qzi};
use crate::bytecode::unified::{UnifiedInput, assemble_unified_qzi};
use crate::bytecode::{self, Codegen, QziMetadata, QziModuleKind};
use crate::loader::{self, CfgTarget, ModuleResolver};
use crate::progress::BuildProgress;
use crate::semantic::target_views::{
    TargetViewInput, build_target_views, program_for_target, strip_variant_markers,
};

/// The operating systems a QZI serves, in mask bit order.
pub const QZI_TARGETS: [CfgTarget<'static>; 3] = [
    CfgTarget {
        os: "linux",
        arch: "x86_64",
        abi: "sysv",
    },
    CfgTarget {
        os: "windows",
        arch: "x86_64",
        abi: "win64",
    },
    CfgTarget {
        os: "netbsd",
        arch: "x86_64",
        abi: "sysv",
    },
];

pub struct UniversalBuild<'a> {
    pub entries: &'a [PathBuf],
    pub out: &'a str,
    pub resolver: Option<&'a ModuleResolver>,
    pub include_std: bool,
    pub native_mangling: bool,
    /// Project identity; `None` builds an executable without a package name.
    pub metadata: Option<QziMetadata>,
    pub dependencies: &'a [PathBuf],
    pub debug: bool,
    /// The caller already printed the build header.
    pub header_printed: bool,
    /// Exact-hit cache of the finished QZI, and the configuration files that
    /// invalidate it besides the loaded sources and dependencies.
    pub cache: Option<(PathBuf, Vec<PathBuf>)>,
}

fn write_output(out: &str, bytes: &[u8], progress: &mut BuildProgress) {
    let path = Path::new(out);
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).unwrap_or_else(|error| {
            fail(
                progress,
                &format!("cannot create {}: {error}", parent.display()),
            )
        });
    }
    std::fs::write(path, bytes)
        .unwrap_or_else(|error| fail(progress, &format!("cannot write {out}: {error}")));
}

fn fail(progress: &mut BuildProgress, message: &str) -> ! {
    progress.fail("error");
    if !message.is_empty() {
        eprintln!("\x1b[31;1merror:\x1b[0m {message}");
    }
    std::process::exit(1)
}

fn decode_dependency(path: &Path, bytes: &[u8]) -> Result<UnifiedQzi, String> {
    let invalid = |error: String| format!("invalid QZI dependency '{}': {error}", path.display());
    if bytes.get(4) == Some(&QZI_UNIFIED_VERSION) {
        return decode_unified_qzi(bytes).map_err(invalid);
    }
    // A single-program dependency serves every target.
    let module = bytecode::deserialize_qzi_module(bytes).map_err(invalid)?;
    let masks = vec![(1u8 << QZI_TARGETS.len()) - 1; module.chunks.len()];
    Ok(UnifiedQzi {
        targets: QZI_TARGETS
            .iter()
            .map(|target| target.os.to_string())
            .collect(),
        module,
        masks,
        interface_overrides: Vec::new(),
    })
}

pub fn build(request: UniversalBuild<'_>, progress: &mut BuildProgress) {
    let input_name = request.entries[0]
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("?");
    let out_name = Path::new(request.out)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(request.out);
    if !request.header_printed {
        progress.header(input_name, out_name);
    }

    progress.begin("frontend");
    let (loaded, file_masks) = loader::load_program_for_all_targets(
        request.entries,
        request.resolver,
        request.include_std,
        &QZI_TARGETS,
    )
    .unwrap_or_else(|error| fail(progress, &error));
    if let Some(error) = &loaded.parse_error {
        progress.fail("");
        eprintln!("{error}");
        std::process::exit(1);
    }
    progress.done(&format!("{} files", loaded.source_files.len()));

    let dependency_bytes = request
        .dependencies
        .iter()
        .map(|path| {
            std::fs::read(path).unwrap_or_else(|error| {
                fail(progress, &format!("cannot read QZI dependency '{}': {error}", path.display()))
            })
        })
        .collect::<Vec<_>>();
    let cache_context = request.cache.as_ref().and_then(|(_, configuration)| {
        let mut hash = Sha256::new();
        hash.update(b"universal-qzi-exact/1");
        hash.update(format!(
            "{:?}",
            (
                request.entries,
                request.include_std,
                request.native_mangling,
                &request.metadata,
                QZI_TARGETS,
                configuration,
                request.dependencies,
                &loaded.source_files,
                &file_masks,
            )
        ));
        for file in &loaded.source_files {
            hash.update(loaded.source_hashes[&file.path]);
        }
        for bytes in &dependency_bytes {
            hash.update(Sha256::digest(bytes));
        }
        for path in configuration {
            match std::fs::read(path) {
                Ok(bytes) => {
                    hash.update([1]);
                    hash.update(Sha256::digest(bytes));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => hash.update([0]),
                Err(_) => return None,
            }
        }
        Some(hash.finalize().into())
    });
    if let (Some((cache_path, _)), Some(context)) = (&request.cache, cache_context) {
        progress.begin("cache lookup");
        match crate::incremental::load_exact(cache_path, Some(context)) {
            Ok(Some(hit)) if hit.qzi.get(4) == Some(&QZI_UNIFIED_VERSION) => {
                progress.done("hit");
                write_output(request.out, &hit.qzi, progress);
                progress.success_as(request.out, Some(hit.qzi.len() as u64), Some("QZI"));
                return;
            }
            Ok(_) => progress.done("miss"),
            Err(error) => {
                progress.done("ignored");
                if !progress.silent {
                    eprintln!("\x1b[33;1mwarning:\x1b[0m ignoring QZC: {error}");
                }
            }
        }
    }

    progress.begin("target views");
    let views = build_target_views(&TargetViewInput {
        program: &loaded.program,
        source_files: &loaded.source_files,
        library_char_ranges: &loaded.library_char_ranges,
        file_masks: &file_masks,
        targets: &QZI_TARGETS,
    })
    .unwrap_or_else(|errors| {
        progress.fail("error");
        for error in errors {
            eprintln!(
                "{}",
                crate::semantic::types::SemanticError {
                    code: "S14",
                    message: error.message,
                    span: error.span,
                }
                .render_with_source_files(&loaded.merged_source, &loaded.source_files)
            );
        }
        std::process::exit(1);
    });
    progress.done(&format!("{} items", views.program.items.len()));

    progress.begin("analysis");
    let namespaced_paths: HashSet<String> = loaded
        .namespaced_paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    // Compiler-owned codec impls, as for single-target analysis.
    let program = crate::semantic::expand_serialize_derives(&views.program);
    let sema = analyze_program_with_source_files(
        &loaded.merged_source,
        &program,
        loaded.library_fn_names.clone(),
        views.library_char_ranges.clone(),
        views.source_files.clone(),
        namespaced_paths,
    );
    // Copies of one declaration report the same finding once, at its source.
    let render = |code: &'static str, message: &str, span, kind: &str| -> String {
        let span = views.layout.original(span);
        let message = strip_variant_markers(message);
        if kind == "error" {
            crate::semantic::types::SemanticError {
                code,
                message,
                span,
            }
            .render_with_source_files(&loaded.merged_source, &loaded.source_files)
        } else {
            crate::semantic::types::SemanticWarning {
                code,
                message,
                span,
                suggestions: Vec::new(),
            }
            .render_with_source_files(&loaded.merged_source, &loaded.source_files)
        }
    };
    let mut shown = HashSet::new();
    if sema.errors.is_empty() {
        progress.done(&format!(
            "{} warning{}",
            sema.warnings.len(),
            if sema.warnings.len() == 1 { "" } else { "s" }
        ));
    } else {
        progress.fail("error");
    }
    if !progress.silent {
        for warning in &sema.warnings {
            let text = render(warning.code, &warning.message, warning.span, "warning");
            if shown.insert(text.clone()) {
                eprintln!("{text}");
            }
        }
    }
    if !sema.errors.is_empty() {
        for error in &sema.errors {
            let text = render(error.code, &error.message, error.span, "error");
            if shown.insert(text.clone()) {
                eprintln!("{text}");
            }
        }
        std::process::exit(1);
    }

    progress.begin("bytecode");
    let library = request
        .metadata
        .as_ref()
        .is_some_and(|metadata| metadata.kind == QziModuleKind::Library);
    let mut codegen = Codegen::new(&sema);
    codegen.set_native_mangling(request.native_mangling);
    if library {
        codegen.retain_public_library_api(loaded.library_file_paths.iter().cloned().collect());
    }
    let chunks = codegen
        .compile_program(&program, &views.source_files)
        .unwrap_or_else(|error| fail(progress, &format!("code generation failed: {error}")));
    let declared = codegen.declared_targets();
    let external_relocations = codegen.external_call_relocations().to_vec();
    let library_roots = codegen.library_roots().to_vec();
    progress.done(&format!("{} functions", chunks.len()));

    let mut metadata = request.metadata.clone().unwrap_or(QziMetadata {
        name: String::new(),
        version: None,
        kind: QziModuleKind::Executable,
        main_takes_args: false,
    });
    metadata.main_takes_args = sema.main_takes_args;
    let interfaces = if library {
        let excluded: HashSet<PathBuf> = loaded.library_file_paths.iter().cloned().collect();
        (0..QZI_TARGETS.len())
            .map(|bit| {
                bytecode::build_qzi_interface(
                    &metadata.name,
                    &program_for_target(&program, &views.layout, bit as u8),
                    &views.source_files,
                    &loaded.namespaced_paths,
                    &excluded,
                )
                .unwrap_or_else(|error| {
                    fail(progress, &format!("cannot build QZI interface: {error}"))
                })
            })
            .collect()
    } else {
        vec![String::new(); QZI_TARGETS.len()]
    };

    progress.begin("qzi");
    let dependencies = request
        .dependencies
        .iter()
        .zip(&dependency_bytes)
        .map(|(path, bytes)| decode_dependency(path, bytes))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|error| fail(progress, &error));
    let targets: Vec<&str> = QZI_TARGETS.iter().map(|target| target.os).collect();
    let mut unified = assemble_unified_qzi(UnifiedInput {
        targets: &targets,
        chunks,
        declared,
        external_relocations,
        library_roots,
        dependencies,
        metadata,
        interfaces,
    })
    .unwrap_or_else(|error| fail(progress, &error));
    unified.module.qualify_library_root_symbols();
    if request.debug {
        for (chunk, mask) in unified.module.chunks.iter().zip(&unified.masks) {
            eprintln!("; targets {mask:#05b}");
            eprint!("{chunk}");
        }
    }
    let bytes = unified
        .encode()
        .unwrap_or_else(|error| fail(progress, &format!("cannot write QZI: {error}")));
    let shared = unified
        .masks
        .iter()
        .filter(|mask| **mask == (1u8 << QZI_TARGETS.len()) - 1)
        .count();
    progress.done(&format!(
        "{} bodies · {} shared",
        unified.masks.len(),
        shared
    ));

    write_output(request.out, &bytes, progress);
    if let (Some((cache_path, configuration)), Some(context)) = (&request.cache, cache_context) {
        let mut inputs = loaded.loaded_files.clone();
        inputs.extend(configuration.iter().cloned());
        if let Err(error) = crate::incremental::store(
            cache_path,
            &inputs,
            &loaded.source_hashes,
            &bytes,
            false,
            context,
            &[],
        ) && !progress.silent
        {
            eprintln!("\x1b[33;1mwarning:\x1b[0m cannot update QZC: {error}");
        }
    }
    // One artifact for every supported system: no target list.
    progress.success_as(request.out, Some(bytes.len() as u64), Some("QZI"));
}
