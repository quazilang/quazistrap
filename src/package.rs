// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyKind {
    #[default]
    Path,
    Git,
    Archive,
    Source,
    Qzi,
}

impl DependencyKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Git => "git",
            Self::Archive => "archive",
            Self::Source => "source",
            Self::Qzi => "qzi",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LockedSource {
    pub revision: Option<String>,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MaterializedDependency {
    pub kind: DependencyKind,
    pub path: PathBuf,
    pub revision: Option<String>,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MaterializedInterface {
    pub root: PathBuf,
    pub src_dir: PathBuf,
    pub entry: PathBuf,
}

pub fn materialize_qzi_interface(
    cache_root: &Path,
    package_name: &str,
    qzi_path: &Path,
    interface: &str,
) -> Result<MaterializedInterface, String> {
    if interface.is_empty() {
        return Err(format!(
            "QZI dependency '{}' has no public interface; rebuild it with QZI v6",
            qzi_path.display()
        ));
    }
    let bundle = crate::bytecode::parse_qzi_interface(interface)?;
    let checksum = sha256_file(qzi_path)?;
    let root = cache_root.join(checksum);
    let src_dir = root.join("src");
    let entry = src_dir.join("mod.qz");
    if !entry.exists() {
        fs::create_dir_all(&src_dir).map_err(|error| {
            format!(
                "cannot create QZI interface cache '{}': {error}",
                src_dir.display()
            )
        })?;
        let mut gateway = String::new();
        for module in bundle.modules {
            if !module
                .name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
            {
                return Err(format!(
                    "QZI interface has invalid module name '{}'",
                    module.name
                ));
            }
            fs::write(src_dir.join(format!("{}.qz", module.name)), module.source)
                .map_err(|error| format!("cannot write cached QZI interface module: {error}"))?;
            if module.exports.is_empty() {
                continue;
            }
            gateway.push_str("pub import ./");
            gateway.push_str(&module.name);
            if module.exports.len() == 1 {
                gateway.push('.');
                gateway.push_str(&module.exports[0]);
            } else {
                gateway.push_str(".{");
                gateway.push_str(&module.exports.join(", "));
                gateway.push('}');
            }
            gateway.push_str(";\n");
        }
        if gateway.is_empty() {
            return Err(format!(
                "QZI dependency '{package_name}' exports no source-visible symbols"
            ));
        }
        fs::write(&entry, gateway)
            .map_err(|error| format!("cannot write cached QZI interface gateway: {error}"))?;
    }
    Ok(MaterializedInterface {
        root,
        src_dir,
        entry,
    })
}

pub fn infer_path_kind(path: &Path) -> Result<DependencyKind, String> {
    if path.is_dir() {
        return Ok(DependencyKind::Path);
    }
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("qz") => Ok(DependencyKind::Source),
        Some("qzi") => Ok(DependencyKind::Qzi),
        _ => Err(format!(
            "cannot infer dependency type from '{}' (expected a project directory, .qz, or .qzi)",
            path.display()
        )),
    }
}

pub fn materialize_path(
    project_root: &Path,
    path: &str,
    explicit_kind: Option<DependencyKind>,
) -> Result<MaterializedDependency, String> {
    let joined = project_root.join(path);
    let canonical = joined
        .canonicalize()
        .map_err(|error| format!("cannot resolve dependency '{}': {error}", joined.display()))?;
    let inferred = infer_path_kind(&canonical)?;
    if let Some(kind) = explicit_kind
        && kind != inferred
        && !(kind == DependencyKind::Path && inferred == DependencyKind::Path)
    {
        return Err(format!(
            "dependency '{}' has type '{}', but its path is '{}'",
            path,
            kind.as_str(),
            inferred.as_str()
        ));
    }
    let checksum = canonical
        .is_file()
        .then(|| sha256_file(&canonical))
        .transpose()?;
    Ok(MaterializedDependency {
        kind: inferred,
        path: canonical,
        revision: None,
        checksum,
    })
}

pub fn materialize_url(
    name: &str,
    kind: DependencyKind,
    url: &str,
    requested_revision: Option<&str>,
    requested_checksum: Option<&str>,
    locked: Option<&LockedSource>,
) -> Result<MaterializedDependency, String> {
    if matches!(kind, DependencyKind::Path) {
        return Err("URL dependency cannot use type 'path'".to_string());
    }
    let cache = package_cache_root()?;
    fs::create_dir_all(&cache)
        .map_err(|error| format!("cannot create package cache '{}': {error}", cache.display()))?;
    let cache_key = sha256_bytes(url.as_bytes());
    let locked_revision = locked.and_then(|entry| entry.revision.as_deref());
    let locked_checksum = locked.and_then(|entry| entry.checksum.as_deref());
    let revision = locked_revision.or(requested_revision);
    let checksum = locked_checksum.or(requested_checksum);

    match kind {
        DependencyKind::Git => {
            materialize_git(name, url, revision, locked.is_some(), &cache, &cache_key)
        }
        DependencyKind::Archive => materialize_archive(name, url, checksum, &cache, &cache_key),
        DependencyKind::Source | DependencyKind::Qzi => {
            materialize_file(name, kind, url, checksum, &cache, &cache_key)
        }
        DependencyKind::Path => unreachable!(),
    }
}

fn package_cache_root() -> Result<PathBuf, String> {
    if let Some(root) = std::env::var_os("QUAZI_HOME") {
        return Ok(PathBuf::from(root).join("cache").join("packages"));
    }
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .ok_or_else(|| "cannot locate home directory for Quazi package cache".to_string())?;
    Ok(PathBuf::from(home)
        .join(".quazi")
        .join("cache")
        .join("packages"))
}

fn materialize_git(
    name: &str,
    url: &str,
    revision: Option<&str>,
    locked: bool,
    cache: &Path,
    cache_key: &str,
) -> Result<MaterializedDependency, String> {
    let destination = cache.join("git").join(cache_key);
    let cached = destination.join(".git").exists();
    if !cached {
        fs::create_dir_all(destination.parent().unwrap()).map_err(|error| {
            format!(
                "cannot create git package cache '{}': {error}",
                cache.display()
            )
        })?;
        eprintln!("  Fetching  {name}  ·  {url}");
        let temporary = temporary_sibling(&destination);
        if temporary.exists() {
            fs::remove_dir_all(&temporary).map_err(|error| {
                format!(
                    "cannot clear temporary package '{}': {error}",
                    temporary.display()
                )
            })?;
        }
        let status = Command::new("git")
            .args(["clone", "--quiet", "--no-checkout", url])
            .arg(&temporary)
            .status()
            .map_err(|error| format!("cannot start git for dependency '{name}': {error}"))?;
        if !status.success() {
            return Err(format!("git clone failed for dependency '{name}'"));
        }
        if destination.exists() {
            fs::remove_dir_all(&temporary)
                .map_err(|error| format!("cannot clear duplicate package download: {error}"))?;
        } else {
            fs::rename(&temporary, &destination).map_err(|error| {
                format!("cannot install git dependency '{name}' into cache: {error}")
            })?;
        }
    }
    if cached && !locked {
        eprintln!("  Updating  {name}  ·  {url}");
        let status = Command::new("git")
            .arg("-C")
            .arg(&destination)
            .args(["fetch", "--quiet", "--tags", "origin"])
            .status()
            .map_err(|error| format!("cannot update dependency '{name}': {error}"))?;
        if !status.success() {
            return Err(format!("git fetch failed for dependency '{name}'"));
        }
    }
    let checkout = revision.unwrap_or("origin/HEAD");
    let status = Command::new("git")
        .arg("-C")
        .arg(&destination)
        .args(["checkout", "--quiet", "--detach", checkout])
        .status()
        .map_err(|error| format!("cannot checkout dependency '{name}': {error}"))?;
    if !status.success() {
        return Err(format!(
            "cannot checkout revision '{checkout}' for dependency '{name}'"
        ));
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(&destination)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|error| format!("cannot resolve git revision for '{name}': {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cannot resolve git revision for dependency '{name}'"
        ));
    }
    let resolved_revision = String::from_utf8(output.stdout)
        .map_err(|_| format!("git returned invalid UTF-8 for dependency '{name}'"))?
        .trim()
        .to_string();
    Ok(MaterializedDependency {
        kind: DependencyKind::Git,
        path: find_package_root(&destination)?,
        revision: Some(resolved_revision),
        checksum: None,
    })
}

fn materialize_archive(
    name: &str,
    url: &str,
    expected_checksum: Option<&str>,
    cache: &Path,
    cache_key: &str,
) -> Result<MaterializedDependency, String> {
    let directory = cache.join("archive").join(cache_key);
    let archive_path = directory.with_extension(archive_extension(url));
    let actual_checksum = download_cached(name, url, &archive_path, expected_checksum)?;
    if !directory.join(".quazi-unpacked").exists() {
        eprintln!("  Unpacking {name}");
        let temporary = temporary_sibling(&directory);
        if temporary.exists() {
            fs::remove_dir_all(&temporary)
                .map_err(|error| format!("cannot clear temporary archive directory: {error}"))?;
        }
        fs::create_dir_all(&temporary)
            .map_err(|error| format!("cannot create temporary archive directory: {error}"))?;
        extract_archive(&archive_path, &temporary)?;
        File::create(temporary.join(".quazi-unpacked"))
            .map_err(|error| format!("cannot finish archive extraction: {error}"))?;
        if directory.exists() {
            fs::remove_dir_all(&directory)
                .map_err(|error| format!("cannot replace cached archive: {error}"))?;
        }
        fs::rename(&temporary, &directory)
            .map_err(|error| format!("cannot install archive dependency '{name}': {error}"))?;
    }
    Ok(MaterializedDependency {
        kind: DependencyKind::Archive,
        path: find_package_root(&directory)?,
        revision: None,
        checksum: Some(actual_checksum),
    })
}

fn materialize_file(
    name: &str,
    kind: DependencyKind,
    url: &str,
    expected_checksum: Option<&str>,
    cache: &Path,
    cache_key: &str,
) -> Result<MaterializedDependency, String> {
    let extension = if kind == DependencyKind::Qzi {
        "qzi"
    } else {
        "qz"
    };
    let path = cache
        .join(kind.as_str())
        .join(format!("{cache_key}.{extension}"));
    let checksum = download_cached(name, url, &path, expected_checksum)?;
    Ok(MaterializedDependency {
        kind,
        path,
        revision: None,
        checksum: Some(checksum),
    })
}

fn download_cached(
    name: &str,
    url: &str,
    destination: &Path,
    expected_checksum: Option<&str>,
) -> Result<String, String> {
    if destination.exists() {
        let checksum = sha256_file(destination)?;
        verify_checksum(name, &checksum, expected_checksum)?;
        return Ok(checksum);
    }
    fs::create_dir_all(destination.parent().unwrap()).map_err(|error| {
        format!(
            "cannot create download cache '{}': {error}",
            destination.display()
        )
    })?;
    eprintln!("  Downloading {name}  ·  {url}");
    let response = ureq::get(url)
        .call()
        .map_err(|error| format!("cannot download dependency '{name}' from '{url}': {error}"))?;
    let temporary = temporary_sibling(destination);
    let mut input = response.into_reader();
    let mut output = File::create(&temporary)
        .map_err(|error| format!("cannot create temporary download: {error}"))?;
    io::copy(&mut input, &mut output)
        .map_err(|error| format!("cannot save dependency '{name}': {error}"))?;
    output
        .flush()
        .map_err(|error| format!("cannot flush dependency '{name}': {error}"))?;
    let checksum = sha256_file(&temporary)?;
    verify_checksum(name, &checksum, expected_checksum)?;
    fs::rename(&temporary, destination)
        .map_err(|error| format!("cannot install downloaded dependency '{name}': {error}"))?;
    Ok(checksum)
}

fn verify_checksum(name: &str, actual: &str, expected: Option<&str>) -> Result<(), String> {
    if let Some(expected) = expected {
        let expected = expected.strip_prefix("sha256:").unwrap_or(expected);
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(format!(
                "checksum mismatch for dependency '{name}': expected sha256:{expected}, got sha256:{actual}"
            ));
        }
    }
    Ok(())
}

fn extract_archive(path: &Path, destination: &Path) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if name.ends_with(".zip") {
        let file = File::open(path)
            .map_err(|error| format!("cannot open archive '{}': {error}", path.display()))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|error| format!("invalid zip archive '{}': {error}", path.display()))?;
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|error| format!("cannot read zip entry: {error}"))?;
            let relative = entry
                .enclosed_name()
                .ok_or_else(|| "zip archive contains an unsafe path".to_string())?;
            let output = destination.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(&output)
                    .map_err(|error| format!("cannot create archive directory: {error}"))?;
            } else {
                if let Some(parent) = output.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|error| format!("cannot create archive directory: {error}"))?;
                }
                let mut file = File::create(&output)
                    .map_err(|error| format!("cannot create archive file: {error}"))?;
                io::copy(&mut entry, &mut file)
                    .map_err(|error| format!("cannot extract archive file: {error}"))?;
            }
        }
        return Ok(());
    }
    let file = File::open(path)
        .map_err(|error| format!("cannot open archive '{}': {error}", path.display()))?;
    let reader: Box<dyn Read> = if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Box::new(GzDecoder::new(file))
    } else if name.ends_with(".tar") {
        Box::new(file)
    } else {
        return Err(format!(
            "unsupported archive '{}': expected .zip, .tar, .tar.gz, or .tgz",
            path.display()
        ));
    };
    let mut archive = tar::Archive::new(reader);
    for entry in archive
        .entries()
        .map_err(|error| format!("cannot read tar archive: {error}"))?
    {
        let mut entry = entry.map_err(|error| format!("cannot read tar entry: {error}"))?;
        if !entry
            .unpack_in(destination)
            .map_err(|error| format!("cannot extract tar entry: {error}"))?
        {
            return Err("tar archive contains an unsafe path".to_string());
        }
    }
    Ok(())
}

fn find_package_root(extracted: &Path) -> Result<PathBuf, String> {
    if extracted.join("quazi.toml").exists() {
        return extracted
            .canonicalize()
            .map_err(|error| format!("cannot resolve package root: {error}"));
    }
    let mut candidates = fs::read_dir(extracted)
        .map_err(|error| format!("cannot inspect package '{}': {error}", extracted.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join("quazi.toml").exists());
    let root = candidates.next().ok_or_else(|| {
        format!(
            "downloaded package '{}' has no quazi.toml",
            extracted.display()
        )
    })?;
    if candidates.next().is_some() {
        return Err(format!(
            "downloaded package '{}' contains multiple project roots",
            extracted.display()
        ));
    }
    root.canonicalize()
        .map_err(|error| format!("cannot resolve package root: {error}"))
}

fn archive_extension(url: &str) -> &'static str {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    if path.ends_with(".tar.gz") {
        "tar.gz"
    } else if path.ends_with(".tgz") {
        "tgz"
    } else if path.ends_with(".tar") {
        "tar"
    } else {
        "zip"
    }
}

fn temporary_sibling(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("package");
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|error| format!("cannot hash '{}': {error}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot hash '{}': {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
