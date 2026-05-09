// void - the programming language
// Copyright titago (C) 2026
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
                Os::Windows => "no linker found; install lld-link or link.exe, or set VOID_LINKER=/path/to/lld-link".to_string(),
                _ => "no linker found; install ld.lld, mold, or ld, or set VOID_LINKER=/path/to/linker".to_string(),
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
        if let Ok(path) = std::env::var("VOID_LINKER") {
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
    /// Search order: $VOID_LINKER → ld.lld → mold → ld  (Linux/macOS)
    ///               $VOID_LINKER → lld-link → link      (Windows)
    pub fn detect() -> Option<PathBuf> {
        Self::detect_for_target(&TargetSpec::host())
    }

    pub fn run(&self) -> Result<(), String> {
        let args = self.build_args();
        let status = std::process::Command::new(&self.linker)
            .args(&args)
            .status()
            .map_err(|e| format!("failed to run linker {}: {}", self.linker.display(), e))?;

        if !status.success() {
            return Err(format!(
                "linker {} failed with exit code {}",
                self.linker.display(),
                status.code().unwrap_or(-1)
            ));
        }
        Ok(())
    }

    fn build_args(&self) -> Vec<OsString> {
        use super::target::Os;
        let mut args: Vec<OsString> = Vec::new();

        match self.target.os {
            Os::Linux => {
                args.push("-o".into());
                args.push(self.output.as_os_str().into());
                args.push(self.object.as_os_str().into());
                args.push("-lc".into());
                args.push("-lm".into());
                if let Some(dl) = self.target.dynamic_linker() {
                    args.push("--dynamic-linker".into());
                    args.push(dl.into());
                }
            }
            Os::MacOs => {
                args.push("-o".into());
                args.push(self.output.as_os_str().into());
                args.push(self.object.as_os_str().into());
                args.push("-lc".into());
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
                args.push("kernel32.lib".into()); // ExitProcess
                args.push("ucrt.lib".into()); // strlen, snprintf, atoll, strtod, pow
            }
        }

        // Pass through flags from void.toml [build].flags.
        for flag in &self.extra_flags {
            args.push(flag.into());
        }

        args
    }
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
    let tmp = std::env::temp_dir().join(format!("void_{}_{}.o", stem, std::process::id()));
    std::fs::write(&tmp, bytes)
        .map_err(|e| format!("cannot write temp object {}: {}", tmp.display(), e))?;
    Ok(tmp)
}

/// Remove a path, silently ignoring errors.
pub fn remove_temp(path: &Path) {
    let _ = std::fs::remove_file(path);
}
