// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::TargetSpec;

/// Link a compiler-produced object plus optional native inputs. Plain Linux
/// builds, including lists of ELF `.o` files, use the in-process linker;
/// libraries or an explicit external linker opt into the external path.
pub fn link_object(
    object_bytes: &[u8],
    output: &Path,
    target: TargetSpec,
    extra_flags: &[String],
    explicit_linker: Option<&Path>,
) -> Result<(), String> {
    use super::target::Os;

    let environment_linker = std::env::var_os("QUAZI_LINKER").map(PathBuf::from);
    let requested_linker = explicit_linker
        .map(Path::to_path_buf)
        .or(environment_linker);
    let requests_builtin = requested_linker
        .as_deref()
        .is_some_and(|path| path == Path::new("builtin"));
    let objects_only = extra_flags.iter().all(|flag| is_elf_object_path(flag));
    let use_builtin =
        requests_builtin || (target.os == Os::Linux && requested_linker.is_none() && objects_only);

    if use_builtin {
        if target.os != Os::Linux {
            return Err(
                "the experimental built-in linker currently supports x86-64 Linux only".to_string(),
            );
        }
        if !objects_only {
            return Err(format!(
                "the built-in linker accepts ELF `.o` inputs but not these native flags: {}\n\
                 hint: select an external linker explicitly for archives/shared libraries",
                extra_flags.join(" ")
            ));
        }
        let native_objects = extra_flags
            .iter()
            .map(|path| {
                std::fs::read(path)
                    .map_err(|error| format!("cannot read native object {path}: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut objects = Vec::with_capacity(native_objects.len() + 1);
        objects.push(object_bytes);
        objects.extend(native_objects.iter().map(Vec::as_slice));
        return super::builtin_linker::link_elf_objects(&objects, output);
    }

    let stem = output
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("output");
    let object = write_temp_object(object_bytes, stem)?;
    let result = (|| {
        let linker = match requested_linker {
            Some(linker) => linker,
            None => LinkerInvocation::detect_for_target(&target).ok_or_else(|| match target.os {
                Os::Windows => "no linker found; install lld-link or link.exe, set QUAZI_LINKER, or pass --linker".to_string(),
                _ => "no linker found; install ld.lld, mold, or ld, set QUAZI_LINKER, or pass --linker".to_string(),
            })?,
        };
        LinkerInvocation {
            output: output.to_path_buf(),
            object: object.clone(),
            linker,
            extra_flags: extra_flags.to_vec(),
            target,
        }
        .run()
    })();
    remove_temp(&object);
    result
}

fn is_elf_object_path(flag: &str) -> bool {
    Path::new(flag)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("o"))
}

pub struct LinkerInvocation {
    pub output: PathBuf,
    pub object: PathBuf,
    pub linker: PathBuf,
    pub extra_flags: Vec<String>,
    pub target: TargetSpec,
}

impl LinkerInvocation {
    pub fn new(
        object: PathBuf,
        output: PathBuf,
        target: TargetSpec,
        extra_flags: Vec<String>,
    ) -> Result<Self, String> {
        let linker = Self::detect_for_target(&target).ok_or_else(|| {
            use super::target::Os;
            match target.os {
                Os::Windows => "no linker found; install lld-link or link.exe, or set QUAZI_LINKER=/path/to/lld-link".to_string(),
                _ => "no linker found; install ld.lld, mold, or ld, or set QUAZI_LINKER=/path/to/linker".to_string(),
            }
        })?;
        Ok(Self {
            output,
            object,
            linker,
            extra_flags,
            target,
        })
    }

    fn detect_for_target(target: &TargetSpec) -> Option<PathBuf> {
        if let Ok(path) = std::env::var("QUAZI_LINKER") {
            return Some(PathBuf::from(path));
        }
        use super::target::Os;
        let candidates: &[&str] = match target.os {
            Os::Windows => &["lld-link", "link"],
            _ => &["ld.lld", "mold", "ld"],
        };
        for candidate in candidates {
            if let Some(path) = find_in_path(candidate) {
                return Some(path);
            }
        }
        None
    }

    /// Detect the best available linker for the host target.
    /// Search order: $QUAZI_LINKER → ld.lld → mold → ld  (Linux/macOS)
    ///               $QUAZI_LINKER → lld-link → link      (Windows)
    pub fn detect() -> Option<PathBuf> {
        Self::detect_for_target(&TargetSpec::host())
    }

    pub fn run(&self) -> Result<(), String> {
        let args = self.build_args();
        let output = std::process::Command::new(&self.linker)
            .args(&args)
            .output()
            .map_err(|e| format!("failed to run linker {}: {}", self.linker.display(), e))?;

        if !output.status.success() {
            let mut msg = format!(
                "linker {} failed with exit code {}",
                self.linker.display(),
                output.status.code().unwrap_or(-1)
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            if !stderr.trim().is_empty() {
                msg.push('\n');
                msg.push_str(stderr.trim_end());
            }
            if !stdout.trim().is_empty() {
                msg.push('\n');
                msg.push_str(stdout.trim_end());
            }
            return Err(msg);
        }
        Ok(())
    }

    fn build_args(&self) -> Vec<OsString> {
        use super::target::Os;
        let mut args: Vec<OsString> = Vec::new();
        let is_shared = self.extra_flags.iter().any(|flag| flag == "-shared");
        match self.target.os {
            Os::Linux => {
                args.push("--gc-sections".into());
                args.push("--build-id=none".into());
                args.push("-o".into());
                args.push(self.output.as_os_str().into());
                args.push(self.object.as_os_str().into());
                for flag in &self.extra_flags {
                    args.push(flag.into());
                }
                let uses_dynamic_library = self
                    .extra_flags
                    .iter()
                    .any(|flag| flag.starts_with("-l") || flag.ends_with(".so"));
                if uses_dynamic_library && !is_shared {
                    if let Some(interpreter) = find_dynamic_linker() {
                        args.push("-dynamic-linker".into());
                        args.push(interpreter.into());
                    }
                    for directory in [
                        "/usr/lib/x86_64-linux-gnu",
                        "/lib/x86_64-linux-gnu",
                        "/usr/lib64",
                        "/lib64",
                        "/usr/lib",
                        "/lib",
                    ] {
                        if Path::new(directory).is_dir() {
                            args.push(format!("-L{directory}").into());
                        }
                    }
                }
                args.push("-z".into());
                args.push("noseparate-code".into());
                args.push("-z".into());
                args.push("max-page-size=0x1000".into()); // 4KB instead of default 2MB
            }
            Os::MacOs => {
                args.push("-o".into());
                args.push(self.output.as_os_str().into());
                args.push(self.object.as_os_str().into());
                for flag in &self.extra_flags {
                    args.push(flag.into());
                }
                args.push("-lm".into());
            }
            Os::Windows => {
                // lld-link / link.exe style (MSVC-compatible).
                // Requires LIB env var pointing to Windows SDK + MSVC lib dirs.
                let out_flag = format!("/out:{}", self.output.display());
                args.push(out_flag.into());
                args.push(self.object.as_os_str().into());
                for flag in &self.extra_flags {
                    if let Some(path) = flag.strip_prefix("-L") {
                        args.push(format!("/libpath:{path}").into());
                    } else if let Some(library) = flag.strip_prefix("-l") {
                        if library == "c" {
                            args.push("ucrt.lib".into());
                            args.push("legacy_stdio_definitions.lib".into());
                        } else {
                            args.push(format!("{library}.lib").into());
                        }
                    } else {
                        args.push(flag.into());
                    }
                }
                args.push("/subsystem:console".into());
                args.push("/entry:mainCRTStartup".into());
                args.push("/debug:none".into());
                args.push("/OPT:REF,ICF".into()); // dead-strip + fold identical functions
                args.push("/MERGE:.rdata=.text".into()); // fold rodata into text — saves 512B PE alignment
                args.push("kernel32.lib".into()); // ExitProcess
                args.push("shell32.lib".into()); // CommandLineToArgvW
                args.push("ws2_32.lib".into()); // Winsock used by std.net
            }
        }

        args
    }
}

fn find_dynamic_linker() -> Option<PathBuf> {
    [
        "/lib64/ld-linux-x86-64.so.2",
        "/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2",
        "/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
}

/// Find an executable by name without invoking a shell.
fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        // On Windows, also try with .exe extension.
        #[cfg(target_os = "windows")]
        {
            let with_exe = dir.join(format!("{}.exe", name));
            if with_exe.is_file() {
                return Some(with_exe);
            }
        }
    }
    None
}

/// Write object bytes to a temporary file and return its path.
pub fn write_temp_object(bytes: &[u8], stem: &str) -> Result<PathBuf, String> {
    let tmp = std::env::temp_dir().join(format!("qz_{}_{}.o", stem, std::process::id()));
    std::fs::write(&tmp, bytes)
        .map_err(|e| format!("cannot write temp object {}: {}", tmp.display(), e))?;
    Ok(tmp)
}

/// Remove a path, silently ignoring errors.
pub fn remove_temp(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::target::{Abi, Arch, Os};

    #[test]
    fn linux_external_link_args_do_not_add_implicit_libc() {
        let inv = LinkerInvocation {
            output: PathBuf::from("hello"),
            object: PathBuf::from("hello.o"),
            linker: PathBuf::from("ld.lld"),
            extra_flags: Vec::new(),
            target: TargetSpec {
                arch: Arch::X86_64,
                os: Os::Linux,
                abi: Abi::SysV,
                emit_start: true,
                no_crash: false,
            },
        };

        let args: Vec<String> = inv
            .build_args()
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert!(args.contains(&"--gc-sections".to_string()));
        assert!(args.contains(&"--build-id=none".to_string()));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-z", "noseparate-code"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-z", "max-page-size=0x1000"])
        );
        assert!(!args.contains(&"-lc".to_string()));
        assert!(!args.contains(&"-lm".to_string()));
        assert!(!args.contains(&"-dynamic-linker".to_string()));
    }

    #[test]
    fn windows_link_args_keep_crt_opt_in_and_translate_library_flags() {
        let inv = LinkerInvocation {
            output: PathBuf::from("hello.exe"),
            object: PathBuf::from("hello.obj"),
            linker: PathBuf::from("lld-link"),
            extra_flags: vec!["-LC:/native".to_string(), "-lucrt".to_string()],
            target: TargetSpec {
                arch: Arch::X86_64,
                os: Os::Windows,
                abi: Abi::Win64,
                emit_start: true,
                no_crash: false,
            },
        };
        let args: Vec<String> = inv
            .build_args()
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"shell32.lib".to_string()));
        assert!(args.contains(&"ws2_32.lib".to_string()));
        assert!(args.contains(&"/libpath:C:/native".to_string()));
        assert!(args.contains(&"ucrt.lib".to_string()));
        assert!(!args.contains(&"libcmt.lib".to_string()));
        assert!(!args.contains(&"legacy_stdio_definitions.lib".to_string()));
    }
}
