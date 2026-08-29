use std::collections::{HashMap, HashSet};
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
    let mut entries = std::fs::read_dir(root)
        .map_err(|error| format!("cannot read '{}': {error}", root.display()))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| format!("cannot read test source entry: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_qz_files(&path, files)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("qz") {
            files.push(path);
        }
    }
    Ok(())
}

fn relative_module_name(
    root: &Path,
    path: &Path,
    root_name: Option<&str>,
) -> Result<String, String> {
    let relative = path.strip_prefix(root).map_err(|_| {
        format!(
            "source '{}' is outside '{}'",
            path.display(),
            root.display()
        )
    })?;
    let mut parts = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let file = parts
        .last_mut()
        .ok_or_else(|| format!("invalid source path '{}'", path.display()))?;
    *file = Path::new(file)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| format!("invalid source path '{}'", path.display()))?
        .to_string();
    if file == "mod" {
        parts.pop();
    }
    let relative_name = parts.join(".");
    match (root_name, relative_name.is_empty()) {
        (Some(root_name), true) => Ok(root_name.to_string()),
        (Some(root_name), false) => Ok(format!("{root_name}.{relative_name}")),
        (None, true) => root
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .ok_or_else(|| format!("invalid source root '{}'", root.display())),
        (None, false) => Ok(relative_name),
    }
}

fn set_module_names(
    source_files: &mut [crate::semantic::SourceFile],
    module_names: &HashMap<PathBuf, String>,
) {
    for source_file in source_files {
        if let Some(module_name) = module_names.get(Path::new(&source_file.path)) {
            source_file.module_name = Some(module_name.clone());
        }
    }
}

fn report_failure(no_color: bool, detail: Option<&str>) {
    if no_color {
        println!("FAILED");
    } else {
        println!("\x1b[31mFAILED\x1b[0m");
    }
    if let Some(detail) = detail {
        println!("  {detail}");
    }
}

fn run_test_binary(
    chunks: &[Chunk],
    test_index: usize,
    output: &Path,
    target: &TargetSpec,
    no_crash: bool,
    link_flags: &[String],
) -> Result<std::process::ExitStatus, String> {
    let harness = harness_for(chunks, test_index)?;
    let object = crate::try_compile_to_object(&harness, true, no_crash, None, false, target)?;
    link_object(&object, output, target.clone(), link_flags, None)?;
    std::process::Command::new(output)
        .status()
        .map_err(|error| format!("cannot run '{}': {error}", output.display()))
}

pub fn run(filter: Option<&str>, no_color: bool, no_unicode: bool) -> Result<bool, String> {
    let cwd = std::env::current_dir().map_err(|error| format!("cannot read cwd: {error}"))?;
    run_project(&cwd, filter, no_color, no_unicode)
}

fn run_project(
    start: &Path,
    filter: Option<&str>,
    no_color: bool,
    no_unicode: bool,
) -> Result<bool, String> {
    let mut context = ProjectContext::load(start)?;
    context.ensure_lockfile()?;

    let entry = context.config.entry.canonicalize().map_err(|error| {
        format!(
            "cannot resolve test entry '{}': {error}",
            context.config.entry.display()
        )
    })?;
    let tests_root = context.config.root.join("tests");
    let mut source_roots = Vec::new();
    let mut test_roots = Vec::new();
    collect_qz_files(&context.config.src_dir, &mut source_roots)?;
    collect_qz_files(&tests_root, &mut test_roots)?;
    let source_module_names = source_roots
        .iter()
        .map(|path| {
            let canonical = path
                .canonicalize()
                .map_err(|error| format!("cannot resolve source '{}': {error}", path.display()))?;
            Ok((
                canonical,
                relative_module_name(&context.config.src_dir, path, None)?,
            ))
        })
        .collect::<Result<HashMap<_, _>, String>>()?;
    let test_module_names = test_roots
        .iter()
        .map(|path| {
            let canonical = path.canonicalize().map_err(|error| {
                format!("cannot resolve test source '{}': {error}", path.display())
            })?;
            Ok((
                canonical,
                relative_module_name(&tests_root, path, Some("tests"))?,
            ))
        })
        .collect::<Result<HashMap<_, _>, String>>()?;
    let mut additional_roots = source_roots;
    additional_roots.extend(test_roots);
    let mut seen = HashSet::new();
    additional_roots.retain(|path| {
        path.canonicalize()
            .is_ok_and(|path| path != entry && seen.insert(path))
    });

    let mut loaded = crate::loader::load_programs_configured(
        std::slice::from_ref(&entry),
        Some(&context.resolver),
        context.config.package.std,
        &additional_roots,
    )?;
    if let Some(error) = loaded.parse_error.take() {
        return Err(error);
    }
    set_module_names(&mut loaded.source_files, &source_module_names);
    set_module_names(&mut loaded.source_files, &test_module_names);

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
    println!(
        "running {} test{}",
        tests.len(),
        if tests.len() == 1 { "" } else { "s" }
    );
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
            report_failure(no_color, Some("compiled test function is missing"));
            continue;
        };
        let output = output_dir.join(file_name(
            test,
            target.os == crate::backend::target::Os::Windows,
        ));
        match run_test_binary(
            &chunks,
            test_index,
            &output,
            &target,
            !context.config.package.crash_handler,
            &link_flags,
        ) {
            Ok(status) if status.success() => {
                passed += 1;
                if no_unicode {
                    println!("[ok]");
                } else if no_color {
                    println!("ok");
                } else {
                    println!("\x1b[32mok\x1b[0m");
                }
            }
            Ok(status) => report_failure(no_color, Some(&format!("process exited with {status}"))),
            Err(error) => report_failure(no_color, Some(&error)),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let thread_name = file_name(std::thread::current().name().unwrap_or("test"), false);
        let path = std::env::temp_dir().join(format!(
            "quazi-test-runner-{name}-{}-{}",
            std::process::id(),
            thread_name
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn source_discovery_is_recursive_and_sorted() {
        let root = temp_dir("discovery");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("z.qz"), "").unwrap();
        std::fs::write(root.join("a.qz"), "").unwrap();
        std::fs::write(root.join("nested/m.qz"), "").unwrap();
        std::fs::write(root.join("ignored.txt"), "").unwrap();

        let mut files = Vec::new();
        collect_qz_files(&root, &mut files).unwrap();
        assert_eq!(
            files,
            [
                root.join("a.qz"),
                root.join("nested/m.qz"),
                root.join("z.qz")
            ]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tests_use_rooted_path_module_names() {
        let root = Path::new("project/tests");
        assert_eq!(
            relative_module_name(
                root,
                Path::new("project/tests/http/client.qz"),
                Some("tests")
            )
            .unwrap(),
            "tests.http.client"
        );
        assert_eq!(
            relative_module_name(root, Path::new("project/tests/http/mod.qz"), Some("tests"))
                .unwrap(),
            "tests.http"
        );
    }

    #[test]
    fn source_modules_keep_their_relative_directory_path() {
        let root = Path::new("project/src");
        assert_eq!(
            relative_module_name(root, Path::new("project/src/a/math.qz"), None).unwrap(),
            "a.math"
        );
        assert_eq!(
            relative_module_name(root, Path::new("project/src/b/math.qz"), None).unwrap(),
            "b.math"
        );
    }

    #[test]
    fn harness_calls_selected_chunk_and_returns_success() {
        let chunks = vec![Chunk::new("first"), Chunk::new("second")];
        let harness = harness_for(&chunks, 1).unwrap();
        let main = harness.last().unwrap();
        assert_eq!(main.name, "main");
        assert_eq!(main.code[1].opcode, Opcode::CallIdx as u8);
        assert_eq!(main.code[1].ri16().1, 1);
        assert_eq!(main.code.last().unwrap().opcode, Opcode::Ret as u8);
    }

    #[test]
    fn generated_file_names_are_portable() {
        assert_eq!(
            file_name("tests.http.connects", false),
            "tests_http_connects"
        );
        assert_eq!(
            file_name("tests.http.connects", true),
            "tests_http_connects.exe"
        );
    }

    #[test]
    fn runs_tests_from_a_real_project_without_changing_cwd() {
        let root = temp_dir("end-to-end");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(
            root.join("quazi.toml"),
            "[package]\nname = \"runner_e2e\"\nstd = false\ncrash_handler = false\n\n\
             [[bin]]\nname = \"runner_e2e\"\npath = \"src/main.qz\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src/main.qz"), "").unwrap();
        std::fs::write(
            root.join("tests/basic.qz"),
            "@test\nfn passes() void { ret; }\n",
        )
        .unwrap();

        assert!(run_project(&root, None, true, true).unwrap());
        assert!(
            root.join("build/tests")
                .join(file_name("tests.basic.passes", cfg!(target_os = "windows")))
                .exists()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
