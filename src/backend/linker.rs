// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::TargetSpec;

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

        match self.target.os {
            Os::Linux => {
                args.push("--gc-sections".into());
                args.push("--build-id=none".into());
                args.push("-o".into());
                args.push(self.output.as_os_str().into());
                args.push(self.object.as_os_str().into());
                // Ensure dynamic linker is set up so PLT/GOT resolves correctly.
                if let Some(interp) = find_dynamic_linker() {
                    args.push("-dynamic-linker".into());
                    args.push(interp.into());
                }
                // Common multiarch and legacy library directories.
                for dir in &[
                    "/usr/lib/x86_64-linux-gnu",
                    "/lib/x86_64-linux-gnu",
                    "/usr/lib64",
                    "/lib64",
                    "/usr/lib",
                    "/lib",
                ] {
                    if Path::new(dir).is_dir() {
                        args.push(format!("-L{}", dir).into());
                    }
                }
                args.push("-z".into());
                args.push("noseparate-code".into());
                args.push("-z".into());
                args.push("max-page-size=0x1000".into()); // 4KB instead of default 2MB
                // ld.lld cannot parse GNU linker scripts, so link the actual
                // ELF shared objects by full path.
                if let Some(libc) = find_system_so("libc") {
                    args.push(libc.into());
                } else {
                    args.push("-lc".into());
                }
                if let Some(libm) = find_system_so("libm") {
                    args.push(libm.into());
                } else {
                    args.push("-lm".into());
                }
            }
            Os::MacOs => {
                args.push("-o".into());
                args.push(self.output.as_os_str().into());
                args.push(self.object.as_os_str().into());
                args.push("-lm".into());
            }
            Os::Windows => {
                // lld-link / link.exe style (MSVC-compatible).
                // Requires LIB env var pointing to Windows SDK + MSVC lib dirs.
                let out_flag = format!("/out:{}", self.output.display());
                args.push(out_flag.into());
                args.push(self.object.as_os_str().into());
                args.push("/subsystem:console".into());
                args.push("/entry:mainCRTStartup".into());
                args.push("/debug:none".into());
                args.push("/OPT:REF,ICF".into()); // dead-strip + fold identical functions
                args.push("/MERGE:.rdata=.text".into()); // fold rodata into text — saves 512B PE alignment
                args.push("kernel32.lib".into()); // ExitProcess
                args.push("ucrt.lib".into()); // malloc, strlen, atoll, strtod, pow, calloc
                args.push("vcruntime.lib".into()); // memset, memcpy, memmove, memcmp
                args.push("legacy_stdio_definitions.lib".into()); // sprintf, printf (CRT stdio forwarders)
            }
        }

        // Pass through flags from quazi.toml [build].flags.
        for flag in &self.extra_flags {
            args.push(flag.into());
        }

        args
    }
}

/// Search common Linux directories for the dynamic linker (ld-linux.so).
fn find_dynamic_linker() -> Option<PathBuf> {
    let candidates: &[&str] = &[
        "/lib64/ld-linux-x86-64.so.2",
        "/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2",
        "/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2",
    ];
    candidates
        .iter()
        .map(Path::new)
        .find(|p| p.exists())
        .map(Path::to_path_buf)
}

/// Search common Linux directories for the actual ELF shared object of a system library
/// (e.g. "libc" → /lib/x86_64-linux-gnu/libc.so.6).
/// This avoids GNU linker scripts that ld.lld cannot parse.
fn find_system_so(name: &str) -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = [
        "/lib/x86_64-linux-gnu",
        "/usr/lib/x86_64-linux-gnu",
        "/lib64",
        "/usr/lib64",
        "/lib",
        "/usr/lib",
    ]
    .iter()
    .map(|dir| Path::new(dir).join(format!("{}.so.6", name)))
    .collect();
    candidates.into_iter().find(|p| p.exists())
}

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
    fn linux_link_args_strip_and_keep_small_page_alignment() {
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
    }
}
