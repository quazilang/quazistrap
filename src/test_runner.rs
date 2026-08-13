use std::collections::HashSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::backend::TargetSpec;
use crate::backend::linker::link_object;
use crate::bytecode::chunk::{QziMetadata, QziModule, QziModuleKind};
use crate::bytecode::instruction::{ri16, rrr};
use crate::bytecode::opcode::Opcode;
use crate::bytecode::{Chunk, Codegen, deserialize_qzi_module, link_qzi_modules};
use crate::project::ProjectContext;

fn collect_qz_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(root)
        .map_err(|error| format!("cannot read '{}': {error}", root.display()))?
    {
        let path = entry
            .map_err(|error| format!("cannot read test source entry: {error}"))?
            .path();
        if path.is_dir() {
            collect_qz_files(&path, files)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("qz") {
            files.push(path);
        }
    }
    Ok(())
}

fn harness_for(chunks: &[Chunk], test_index: usize) -> Result<Vec<Chunk>, String> {
    let test_index = u16::try_from(test_index).map_err(|_| "too many test functions")?;
    let mut harness = chunks.to_vec();
    let mut main = Chunk::new("main");
    main.reg_count = 1;
    main.emit(ri16(Opcode::MovI, 0, 0));
    main.emit(ri16(Opcode::CallIdx, 0, test_index));
    main.emit(ri16(Opcode::MovI, 0, 0));
    main.emit(rrr(Opcode::Ret, 0, 0, 0));
    harness.push(main);
    Ok(harness)
}

fn file_name(name: &str, windows: bool) -> String {
    let safe: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect();
    if windows { format!("{safe}.exe") } else { safe }
}

pub fn run(filter: Option<&str>, no_color: bool, no_unicode: bool) -> Result<bool, String> {
    let cwd = std::env::current_dir().map_err(|error| format!("cannot read cwd: {error}"))?;
    let mut context = ProjectContext::load(&cwd)?;
    context.ensure_lockfile()?;

    let entry = context.config.entry.canonicalize().map_err(|error| {
        format!(
            "cannot resolve test entry '{}': {error}",
            context.config.entry.display()
        )
    })?;
    let mut additional_roots = Vec::new();
    collect_qz_files(&context.config.src_dir, &mut additional_roots)?;
    collect_qz_files(&context.config.root.join("tests"), &mut additional_roots)?;
    let mut seen = HashSet::new();
    additional_roots.retain(|path| {
        path.canonicalize()
            .is_ok_and(|path| path != entry && seen.insert(path))
    });

    let loaded = crate::loader::load_programs_configured(
        std::slice::from_ref(&entry),
        Some(&context.resolver),
        context.config.package.std,
        &additional_roots,
    )?;
    if let Some(error) = loaded.parse_error {
        return Err(error);
    }

    let target = crate::apply_package_settings(TargetSpec::host(), context.config.package);
    context.config.link = context.link_for_target(target.triple());
    let (target_os, target_abi) = match target.os {
        crate::backend::target::Os::Windows => ("windows", "win64"),
        crate::backend::target::Os::Linux => ("linux", "sysv"),
        crate::backend::target::Os::MacOs => ("macos", "sysv"),
    };
    let program = crate::semantic::strip_cfg_for(&loaded.program, target_os, "x86_64", target_abi);
    let namespaced_paths = loaded
        .namespaced_paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    let report = crate::analysis::analyze_program_with_source_files(
        &loaded.merged_source,
        &program,
        loaded.library_fn_names,
        loaded.library_char_ranges,
        loaded.source_files.clone(),
        namespaced_paths,
    );
    if !crate::report_diagnostics(&report, &loaded.merged_source, &loaded.source_files) {
        return Ok(false);
    }

    let mut tests: Vec<String> = report
        .test_functions
        .iter()
        .filter(|name| filter.is_none_or(|filter| name.contains(filter)))
        .cloned()
        .collect();
    tests.sort();
    tests.dedup();
    println!("running {} test{}", tests.len(), if tests.len() == 1 { "" } else { "s" });
    if tests.is_empty() {
        println!("test result: ok. 0 passed; 0 failed");
        return Ok(true);
    }

    let mut codegen = Codegen::new(&report);
    codegen.enable_test_mode();
    codegen.set_native_mangling(context.config.package.mangling);
    let generated = codegen.compile_program(&program, &loaded.source_files)?;
    let mut modules = vec![QziModule {
        metadata: QziMetadata {
            name: context.config.name.clone(),
            version: context.config.version.clone(),
            kind: QziModuleKind::Executable,
            main_takes_args: false,
        },
        interface: String::new(),
        call_relocations: codegen.external_call_relocations().to_vec(),
        chunks: generated,
    }];
    for dependency in &context.config.qzi_dependencies {
        let bytes = std::fs::read(dependency)
            .map_err(|error| format!("cannot read '{}': {error}", dependency.display()))?;
        modules.push(deserialize_qzi_module(&bytes)?);
    }
    let chunks = link_qzi_modules(&modules)?;
    let link_flags = crate::native_link_flags(&context)?;
    let output_dir = context.config.out_dir.join("tests");
    std::fs::create_dir_all(&output_dir)
        .map_err(|error| format!("cannot create '{}': {error}", output_dir.display()))?;

    let mut passed = 0usize;
    for test in &tests {
        print!("test {test} ... ");
        std::io::stdout().flush().ok();
        let Some(test_index) = chunks.iter().position(|chunk| chunk.name == *test) else {
            println!("FAILED");
            continue;
        };
        let harness = harness_for(&chunks, test_index)?;
        let output = output_dir.join(file_name(test, target.os == crate::backend::target::Os::Windows));
        let object = crate::compile_to_object(
            &harness,
            true,
            !context.config.package.crash_handler,
            None,
            false,
            &target,
        );
        let success = link_object(&object, &output, target.clone(), &link_flags, None)
            .and_then(|_| {
                std::process::Command::new(&output)
                    .status()
                    .map_err(|error| format!("cannot run '{}': {error}", output.display()))
            })
            .is_ok_and(|status| status.success());
        if success {
            passed += 1;
            if no_unicode {
                println!("[ok]");
            } else if no_color {
                println!("ok");
            } else {
                println!("\x1b[32mok\x1b[0m");
            }
        } else if no_color {
            println!("FAILED");
        } else {
            println!("\x1b[31mFAILED\x1b[0m");
        }
    }

    let failed = tests.len() - passed;
    println!();
    println!(
        "test result: {}. {passed} passed; {failed} failed",
        if failed == 0 { "ok" } else { "FAILED" }
    );
    Ok(failed == 0)
}
