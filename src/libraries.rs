// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD
//
// Resolves the `[libraries]` section of quazi.toml: downloads (or reuses an
// already-downloaded) library into `build/lib/<name>` (or an explicit `path`)
// and registers it with the module resolver, the same way a `[dependencies]`
// entry is registered.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;

use crate::loader::{ModuleResolver, ModuleSpec};
use crate::project::{collect_modules, ResolvedDependency};

/// Paths already downloaded/update-checked during this process run.
/// `qz build` ends up loading the project graph more than once (once to
/// resolve the lockfile/dependencies, once more while actually compiling),
/// so without this a library would get re-fetched/re-update-checked
/// several times per single `qz build` invocation.
fn session_cache() -> &'static Mutex<HashSet<PathBuf>> {
    static CACHE: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Returns `true` the first time it's called for a given path in this
/// process, `false` on every subsequent call.
fn claim_once(target: &Path) -> bool {
    session_cache().lock().unwrap().insert(target.to_path_buf())
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawLibrary {
    #[serde(rename = "type")]
    pub kind: String,
    pub url: Option<String>,
    pub branch: Option<String>,
    pub path: Option<String>,
}

enum LibraryKind {
    File,
    Archive,
    Local,
    Git,
}

impl LibraryKind {
    fn parse(name: &str, s: &str) -> Result<Self, String> {
        match s {
            "file" => Ok(LibraryKind::File),
            "archive" => Ok(LibraryKind::Archive),
            "local" => Ok(LibraryKind::Local),
            "git" => Ok(LibraryKind::Git),
            other => Err(format!(
                "library '{}': unknown type '{}' (expected 'file', 'archive', 'local' or 'git')",
                name, other
            )),
        }
    }
}

/// Resolves the home directory in a platform-independent way, so a single
/// quazi.toml (e.g. `path = "~/.quazi/std"`) works both on Linux/macOS and
/// on Windows.
fn home_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    } else {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn expand_path(raw: &str, root: &Path) -> PathBuf {
    let expanded = if raw == "~" {
        home_dir().unwrap_or_else(|| PathBuf::from("~"))
    } else if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        match home_dir() {
            Some(home) => home.join(rest),
            None => PathBuf::from(raw),
        }
    } else if let Some(rest) = raw
        .strip_prefix("$HOME/")
        .or_else(|| raw.strip_prefix("${HOME}/"))
        .or_else(|| raw.strip_prefix("%USERPROFILE%/"))
        .or_else(|| raw.strip_prefix("%USERPROFILE%\\"))
    {
        match home_dir() {
            Some(home) => home.join(rest),
            None => PathBuf::from(raw),
        }
    } else {
        PathBuf::from(raw)
    };

    if expanded.is_absolute() {
        expanded
    } else {
        root.join(expanded)
    }
}

fn default_path(root: &Path, name: &str) -> PathBuf {
    root.join("build").join("lib").join(name)
}

fn is_present(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => true,
        Ok(meta) if meta.is_dir() => std::fs::read_dir(path)
            .map(|mut it| it.next().is_some())
            .unwrap_or(false),
        _ => false,
    }
}

fn run_output(cmd: &mut std::process::Command, what: &str) -> Result<String, String> {
    let output = cmd
        .output()
        .map_err(|e| format!("failed to run {}: {}", what, e))?;
    if !output.status.success() {
        return Err(format!("{} failed with status {}", what, output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn ask_yes_no(prompt: &str) -> bool {
    use std::io::Write;
    print!("{} [y/N]: ", prompt);
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// Fetches remote refs (without merging) and returns `Some(true)` if the
/// local checkout is behind its upstream tracking branch. Returns `None`
/// (and prints a warning) if the check itself fails, e.g. no network —
/// in that case the existing local copy is used as-is, silently.
fn git_has_update(name: &str, dest: &Path) -> Option<bool> {
    let mut fetch_cmd = std::process::Command::new("git");
    fetch_cmd
        .arg("-C")
        .arg(dest)
        .arg("fetch")
        .arg("--quiet");
    if run(&mut fetch_cmd, &format!("git fetch for library '{}'", name)).is_err() {
        eprintln!(
            "warning: could not check for updates for library '{}' (offline?)",
            name
        );
        return None;
    }

    let local = run_output(
        std::process::Command::new("git").arg("-C").arg(dest).arg("rev-parse").arg("HEAD"),
        "git rev-parse HEAD",
    )
    .ok()?;
    let remote = run_output(
        std::process::Command::new("git")
            .arg("-C")
            .arg(dest)
            .arg("rev-parse")
            .arg("@{u}"),
        "git rev-parse @{u}",
    )
    .ok()?;

    Some(local != remote)
}

fn git_update(name: &str, dest: &Path) -> Result<(), String> {
    println!("updating library '{}'...", name);
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(dest).arg("reset").arg("--hard").arg("@{u}");
    run(&mut cmd, &format!("git reset for library '{}'", name))
}

fn run(cmd: &mut std::process::Command, what: &str) -> Result<(), String> {
    let status = cmd
        .status()
        .map_err(|e| format!("failed to run {}: {}", what, e))?;
    if !status.success() {
        return Err(format!("{} failed with status {}", what, status));
    }
    Ok(())
}

fn download_git(name: &str, url: &str, branch: &Option<String>, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create '{}': {}", parent.display(), e))?;
    }
    println!("downloading library '{}' from {} (git)", name, url);
    let mut cmd = std::process::Command::new("git");
    cmd.arg("clone").arg("--depth").arg("1");
    if let Some(branch) = branch {
        cmd.arg("--branch").arg(branch);
    }
    cmd.arg(url).arg(dest);
    run(&mut cmd, &format!("git clone for library '{}'", name))
}

fn download_file(name: &str, url: &str, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create '{}': {}", parent.display(), e))?;
    }
    println!("downloading library '{}' from {} (file)", name, url);
    let mut cmd = std::process::Command::new("curl");
    cmd.arg("-fsSL").arg("-o").arg(dest).arg(url);
    run(&mut cmd, &format!("curl download for library '{}'", name))
}

fn download_archive(name: &str, url: &str, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest)
        .map_err(|e| format!("cannot create '{}': {}", dest.display(), e))?;

    let file_name = url
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("archive");
    let tmp_dir = std::env::temp_dir();
    let tmp_archive = tmp_dir.join(format!("quazi-lib-{}-{}", name, file_name));

    println!("downloading library '{}' from {} (archive)", name, url);
    let mut curl = std::process::Command::new("curl");
    curl.arg("-fsSL").arg("-o").arg(&tmp_archive).arg(url);
    run(&mut curl, &format!("curl download for library '{}'", name))?;

    let lower = file_name.to_ascii_lowercase();
    let result = if lower.ends_with(".zip") {
        let mut cmd = std::process::Command::new("unzip");
        cmd.arg("-q").arg("-o").arg(&tmp_archive).arg("-d").arg(dest);
        run(&mut cmd, &format!("unzip for library '{}'", name))
    } else {
        // .tar, .tar.gz, .tgz, .tar.xz, .tar.bz2, ...
        let mut cmd = std::process::Command::new("tar");
        cmd.arg("-xf").arg(&tmp_archive).arg("-C").arg(dest);
        run(&mut cmd, &format!("tar extract for library '{}'", name))
    };

    let _ = std::fs::remove_file(&tmp_archive);
    result
}

/// Resolves every entry of `[libraries]`, downloading/extracting/cloning
/// (only if not already present under the resolved path) and registers
/// each one into `resolver`, mirroring how `[dependencies]` are registered.
/// Network/filesystem side effects (download, update-check) for a given
/// resolved path only ever run once per process (`qz build` loads the
/// project graph more than once internally).
pub fn resolve_libraries(
    root: &Path,
    libraries: &BTreeMap<String, RawLibrary>,
    resolver: &mut ModuleResolver,
    visited: &mut HashSet<PathBuf>,
    root_name: &str,
    dep_versions: &mut HashMap<String, ResolvedDependency>,
) -> Result<(), String> {
    for (name, raw) in libraries {
        let kind = LibraryKind::parse(name, &raw.kind)?;

        match kind {
            LibraryKind::Local => {
                if raw.url.is_some() {
                    return Err(format!(
                        "library '{}': type 'local' does not accept 'url'",
                        name
                    ));
                }
                let path_str = raw.path.as_ref().ok_or_else(|| {
                    format!("library '{}': type 'local' requires 'path'", name)
                })?;
                let target = expand_path(path_str, root);
                if !target.exists() {
                    return Err(format!(
                        "library '{}': local path '{}' does not exist",
                        name,
                        target.display()
                    ));
                }
                register_module_dir(name, &target, resolver, visited, root_name, dep_versions)?;
            }
            LibraryKind::Git => {
                let url = raw
                    .url
                    .as_ref()
                    .ok_or_else(|| format!("library '{}': type 'git' requires 'url'", name))?;
                let target = raw
                    .path
                    .as_ref()
                    .map(|p| expand_path(p, root))
                    .unwrap_or_else(|| default_path(root, name));
                if claim_once(&target) {
                    if !is_present(&target) {
                        download_git(name, url, &raw.branch, &target)?;
                    } else {
                        println!("library '{}' already present at {}, checking for updates", name, target.display());
                        if let Some(true) = git_has_update(name, &target) {
                            if ask_yes_no(&format!(
                                "Update available for library \"{}\", update it?",
                                name
                            )) {
                                git_update(name, &target)?;
                            }
                        }
                    }
                }
                register_module_dir(name, &target, resolver, visited, root_name, dep_versions)?;
            }
            LibraryKind::Archive => {
                if raw.branch.is_some() {
                    return Err(format!(
                        "library '{}': 'branch' is only valid for type 'git'",
                        name
                    ));
                }
                let url = raw
                    .url
                    .as_ref()
                    .ok_or_else(|| format!("library '{}': type 'archive' requires 'url'", name))?;
                let target = raw
                    .path
                    .as_ref()
                    .map(|p| expand_path(p, root))
                    .unwrap_or_else(|| default_path(root, name));
                if claim_once(&target) {
                    if !is_present(&target) {
                        download_archive(name, url, &target)?;
                    } else {
                        println!("library '{}' already present at {}, skipping download", name, target.display());
                    }
                }
                register_module_dir(name, &target, resolver, visited, root_name, dep_versions)?;
            }
            LibraryKind::File => {
                if raw.branch.is_some() {
                    return Err(format!(
                        "library '{}': 'branch' is only valid for type 'git'",
                        name
                    ));
                }
                let url = raw
                    .url
                    .as_ref()
                    .ok_or_else(|| format!("library '{}': type 'file' requires 'url'", name))?;
                let target = raw
                    .path
                    .as_ref()
                    .map(|p| expand_path(p, root))
                    .unwrap_or_else(|| default_path(root, name));
                if claim_once(&target) {
                    if !is_present(&target) {
                        download_file(name, url, &target)?;
                    } else {
                        println!("library '{}' already present at {}, skipping download", name, target.display());
                    }
                }
                register_module_file(name, &target, resolver, root_name, dep_versions)?;
            }
        }
    }

    Ok(())
}

/// Registers a directory-based library (git/archive/local) as a module,
/// reusing the same recursive project-loading logic used for `[dependencies]`.
fn register_module_dir(
    name: &str,
    target: &Path,
    resolver: &mut ModuleResolver,
    visited: &mut HashSet<PathBuf>,
    root_name: &str,
    dep_versions: &mut HashMap<String, ResolvedDependency>,
) -> Result<(), String> {
    collect_modules(
        target,
        resolver,
        visited,
        Some((name, None)),
        root_name,
        dep_versions,
    )
}

/// Registers a single downloaded file as a self-contained module.
fn register_module_file(
    name: &str,
    target: &Path,
    resolver: &mut ModuleResolver,
    root_name: &str,
    dep_versions: &mut HashMap<String, ResolvedDependency>,
) -> Result<(), String> {
    let canonical = target
        .canonicalize()
        .map_err(|e| format!("cannot resolve '{}': {}", target.display(), e))?;
    let parent = canonical
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let spec = ModuleSpec {
        name: name.to_string(),
        root: parent.clone(),
        src_dir: parent.clone(),
        entry: canonical.clone(),
        version: None,
    };
    resolver.insert(spec)?;

    if name != root_name {
        dep_versions.insert(
            name.to_string(),
            ResolvedDependency {
                name: name.to_string(),
                root: parent,
                requested_version: None,
                version: None,
            },
        );
    }

    Ok(())
}
