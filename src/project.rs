// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::loader::{ModuleResolver, ModuleSpec};
use crate::package::{
    DependencyKind, LockedSource, materialize_path, materialize_qzi_interface, materialize_url,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProjectKind {
    Bin,
    Lib,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectArtifact {
    pub name: String,
    pub kind: ProjectKind,
    pub entry: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ProjectConfig {
    pub root: PathBuf,
    pub out_dir: PathBuf,
    pub name: String,
    pub version: Option<String>,
    pub package: PackageSettings,
    pub kind: ProjectKind,
    pub entry: PathBuf,
    pub src_dir: PathBuf,
    pub flags: Vec<String>,
    pub dependencies: Vec<ResolvedDependency>,
    pub qzi_dependencies: Vec<PathBuf>,
    pub cc: CcConfig,
    pub link: LinkConfig,
    pub artifacts: Vec<ProjectArtifact>,
    pub target_links: BTreeMap<String, LinkConfigOverride>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageSettings {
    pub std: bool,
    pub crash_handler: bool,
    pub mangling: bool,
}

impl Default for PackageSettings {
    fn default() -> Self {
        Self {
            std: true,
            crash_handler: true,
            mangling: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CcConfig {
    pub sources: Vec<PathBuf>,
    pub include_paths: Vec<PathBuf>,
    pub defines: Vec<String>,
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LinkConfig {
    pub linker: Option<String>,
    pub libc: bool,
    pub objects: Vec<PathBuf>,
    pub libraries: Vec<String>,
    pub library_paths: Vec<PathBuf>,
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LinkConfigOverride {
    pub linker: Option<String>,
    pub libc: Option<bool>,
    pub objects: Vec<PathBuf>,
    pub libraries: Vec<String>,
    pub library_paths: Vec<PathBuf>,
    pub flags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedDependency {
    pub name: String,
    pub identity: Option<String>,
    pub root: PathBuf,
    pub kind: DependencyKind,
    pub url: Option<String>,
    pub revision: Option<String>,
    pub checksum: Option<String>,
    pub version: Option<String>,
    pub selector: Option<String>,
    pub floating: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub config: ProjectConfig,
    pub resolver: ModuleResolver,
}

#[derive(Debug, Clone)]
pub struct ProjectPreview {
    pub root: PathBuf,
    pub name: String,
    pub kind: ProjectKind,
    pub entry: PathBuf,
    pub out_dir: PathBuf,
}

pub fn preview(start: &Path, bin: Option<&str>, lib: bool) -> Result<ProjectPreview, String> {
    let root = find_project_root(start)
        .ok_or_else(|| "quazi.toml not found in this directory or parents".to_string())?;
    let meta = load_project_meta(&root)?;
    let candidates: Vec<&ProjectArtifact> = meta
        .artifacts
        .iter()
        .filter(|artifact| {
            if lib {
                artifact.kind == ProjectKind::Lib
            } else if let Some(name) = bin {
                artifact.kind == ProjectKind::Bin && artifact.name == name
            } else {
                true
            }
        })
        .collect();
    let selected = if bin.is_some() || lib {
        candidates.first().copied().ok_or_else(|| {
            if lib {
                "project has no [lib] artifact".to_string()
            } else {
                format!("project has no binary named '{}'", bin.unwrap())
            }
        })?
    } else if candidates.len() == 1 {
        candidates[0]
    } else if let Some(default) = candidates
        .iter()
        .copied()
        .find(|artifact| artifact.kind == ProjectKind::Bin && artifact.name == meta.name)
    {
        default
    } else {
        return Err("project has multiple artifacts; select one with --bin <name> or --lib".into());
    };
    Ok(ProjectPreview {
        root,
        name: selected.name.clone(),
        kind: selected.kind.clone(),
        entry: selected.entry.clone(),
        out_dir: meta.out_dir,
    })
}

pub struct DependencyEdit {
    pub name: String,
    pub path: Option<PathBuf>,
    pub url: Option<String>,
    pub kind: Option<String>,
    pub version: Option<String>,
    pub revision: Option<String>,
    pub checksum: Option<String>,
}

pub fn infer_dependency_name(path: Option<&Path>, url: Option<&str>) -> Result<String, String> {
    let raw_name = if let Some(path) = path {
        if path.is_dir() {
            load_project_meta(path)?.name
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("qzi") {
            let bytes = std::fs::read(path)
                .map_err(|error| format!("cannot read '{}': {error}", path.display()))?;
            crate::bytecode::deserialize_qzi_module(&bytes)
                .map_err(|error| format!("invalid QZI '{}': {error}", path.display()))?
                .metadata
                .name
        } else {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| format!("cannot infer dependency name from '{}'", path.display()))?
                .to_string()
        }
    } else if let Some(url) = url {
        let clean = url
            .split(['?', '#'])
            .next()
            .unwrap_or(url)
            .trim_end_matches('/');
        let file = clean
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| format!("cannot infer dependency name from URL '{url}'"))?;
        file.trim_end_matches(".git")
            .trim_end_matches(".tar.gz")
            .trim_end_matches(".tar.xz")
            .trim_end_matches(".zip")
            .trim_end_matches(".qzi")
            .trim_end_matches(".qz")
            .to_string()
    } else {
        return Err("dependency path or URL is required".into());
    };

    let mut inferred = String::with_capacity(raw_name.len() + 1);
    for (index, character) in raw_name.chars().enumerate() {
        let valid = if index == 0 {
            character.is_ascii_alphabetic() || character == '_'
        } else {
            character.is_ascii_alphanumeric() || character == '_'
        };
        if valid {
            inferred.push(character);
        } else {
            inferred.push('_');
        }
    }
    if inferred.is_empty() {
        return Err("inferred dependency name is empty".into());
    }
    if inferred.as_bytes()[0].is_ascii_digit() {
        inferred.insert(0, '_');
    }
    Ok(inferred)
}

pub fn add_dependency(start: &Path, edit: DependencyEdit) -> Result<ProjectContext, String> {
    if !is_quazi_identifier(&edit.name) {
        return Err("dependency name must be a Quazi identifier".into());
    }
    if edit.path.is_some() == edit.url.is_some() {
        return Err("dependency must resolve to exactly one path or URL".into());
    }
    if edit.url.is_some() && edit.kind.is_none() {
        return Err("internet dependencies require --type git|archive|source|qzi".into());
    }
    mutate_dependencies(start, |dependencies| {
        if dependencies.contains_key(&edit.name) {
            return Err(format!("dependency '{}' already exists", edit.name));
        }
        let mut value = toml::map::Map::new();
        if let Some(path) = edit.path {
            value.insert(
                "path".into(),
                toml::Value::String(path.to_string_lossy().into_owned()),
            );
        }
        if let Some(url) = edit.url {
            value.insert("url".into(), toml::Value::String(url));
        }
        if let Some(kind) = edit.kind {
            value.insert("type".into(), toml::Value::String(kind));
        }
        if let Some(version) = edit.version {
            value.insert("version".into(), toml::Value::String(version));
        }
        if let Some(revision) = edit.revision {
            value.insert("rev".into(), toml::Value::String(revision));
        }
        if let Some(checksum) = edit.checksum {
            value.insert("checksum".into(), toml::Value::String(checksum));
        }
        dependencies.insert(edit.name, toml::Value::Table(value));
        Ok(())
    })
}

pub fn remove_dependency(start: &Path, name: &str) -> Result<ProjectContext, String> {
    mutate_dependencies(start, |dependencies| {
        dependencies
            .remove(name)
            .ok_or_else(|| format!("dependency '{name}' does not exist"))?;
        Ok(())
    })
}

fn mutate_dependencies(
    start: &Path,
    mutate: impl FnOnce(&mut toml::map::Map<String, toml::Value>) -> Result<(), String>,
) -> Result<ProjectContext, String> {
    let root = find_project_root(start)
        .ok_or_else(|| "quazi.toml not found in this directory or parents".to_string())?;
    let manifest_path = root.join("quazi.toml");
    let lock_path = root.join("quazi.lock");
    let old_manifest = std::fs::read(&manifest_path)
        .map_err(|error| format!("cannot read '{}': {error}", manifest_path.display()))?;
    let old_lock = std::fs::read(&lock_path).ok();
    let manifest_text = std::str::from_utf8(&old_manifest)
        .map_err(|error| format!("invalid UTF-8 in '{}': {error}", manifest_path.display()))?;
    let mut document: toml::Value = toml::from_str(manifest_text)
        .map_err(|error| format!("invalid '{}': {error}", manifest_path.display()))?;
    let table = document
        .as_table_mut()
        .ok_or("quazi.toml root must be a table")?;
    let dependencies = table
        .entry("dependencies")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .ok_or("[dependencies] must be a table")?;
    mutate(dependencies)?;
    let rendered = toml::to_string_pretty(&document)
        .map_err(|error| format!("cannot serialize quazi.toml: {error}"))?;
    std::fs::write(&manifest_path, rendered)
        .map_err(|error| format!("cannot write '{}': {error}", manifest_path.display()))?;
    let _ = std::fs::remove_file(&lock_path);
    let result = ProjectContext::load_from_root(&root).and_then(|context| {
        context.ensure_lockfile()?;
        Ok(context)
    });
    if result.is_err() {
        let _ = std::fs::write(&manifest_path, old_manifest);
        match old_lock {
            Some(lock) => {
                let _ = std::fs::write(&lock_path, lock);
            }
            None => {
                let _ = std::fs::remove_file(&lock_path);
            }
        }
    }
    result
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    package: Option<RawPackage>,
    build: Option<RawBuild>,
    dependencies: Option<BTreeMap<String, RawDependency>>,
    cc: Option<RawCc>,
    link: Option<RawLink>,
    lib: Option<RawArtifact>,
    #[serde(rename = "bin")]
    bins: Option<Vec<RawArtifact>>,
    target: Option<BTreeMap<String, RawTarget>>,
}

#[derive(Debug, Deserialize)]
struct RawArtifact {
    name: Option<String>,
    path: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawTarget {
    link: Option<RawLink>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
struct RawCc {
    sources: Option<Vec<String>>,
    include_paths: Option<Vec<String>>,
    defines: Option<Vec<String>>,
    flags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
struct RawLink {
    linker: Option<String>,
    libc: Option<bool>,
    objects: Option<Vec<String>>,
    libraries: Option<Vec<String>>,
    library_paths: Option<Vec<String>>,
    flags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct RawPackage {
    name: String,
    version: Option<String>,
    out_dir: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    std: Option<bool>,
    crash_handler: Option<bool>,
    mangling: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RawBuild {
    entry: Option<String>,
    src: Option<String>,
    flags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawDependency {
    Path(String),
    Table {
        #[serde(rename = "type")]
        kind: Option<DependencyKind>,
        path: Option<String>,
        url: Option<String>,
        version: Option<String>,
        rev: Option<String>,
        checksum: Option<String>,
    },
}

#[derive(Debug, Clone)]
struct ProjectMeta {
    name: String,
    version: Option<String>,
    package: PackageSettings,
    out_dir: PathBuf,
    kind: ProjectKind,
    entry: PathBuf,
    src_dir: PathBuf,
    flags: Vec<String>,
    dependencies: Vec<DependencySpec>,
    cc: CcConfig,
    link: LinkConfig,
    artifacts: Vec<ProjectArtifact>,
    target_links: BTreeMap<String, LinkConfigOverride>,
}

#[derive(Debug, Clone)]
struct DependencySpec {
    name: String,
    kind: Option<DependencyKind>,
    path: Option<String>,
    url: Option<String>,
    requested_version: Option<String>,
    revision: Option<String>,
    checksum: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Lockfile {
    package: Vec<LockPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LockPackage {
    name: String,
    #[serde(default)]
    identity: Option<String>,
    version: Option<String>,
    #[serde(default)]
    kind: DependencyKind,
    path: Option<String>,
    url: Option<String>,
    revision: Option<String>,
    checksum: Option<String>,
    #[serde(default)]
    selector: Option<String>,
}

impl ProjectContext {
    pub fn load(start: &Path) -> Result<Self, String> {
        let root = find_project_root(start)
            .ok_or_else(|| "quazi.toml not found in this directory or parents".to_string())?;
        Self::load_from_root(&root)
    }

    pub fn discover(start: &Path) -> Result<Option<Self>, String> {
        let Some(root) = find_project_root(start) else {
            return Ok(None);
        };
        Ok(Some(Self::load_from_root(&root)?))
    }

    pub fn ensure_lockfile(&self) -> Result<(), String> {
        if self.resolver.modules.len() <= 1 {
            return Ok(());
        }
        let lock_path = self.config.root.join("quazi.lock");
        if self
            .config
            .dependencies
            .iter()
            .any(|dependency| dependency.floating)
        {
            write_lockfile(&lock_path, &self.config.dependencies)
        } else if lock_path.exists() {
            let lock = load_lockfile(&lock_path)?;
            validate_lockfile(&lock, &self.config.dependencies)
        } else {
            write_lockfile(&lock_path, &self.config.dependencies)
        }
    }

    pub fn select_artifact(&mut self, bin: Option<&str>, lib: bool) -> Result<(), String> {
        let candidates: Vec<&ProjectArtifact> = self
            .config
            .artifacts
            .iter()
            .filter(|artifact| {
                if lib {
                    artifact.kind == ProjectKind::Lib
                } else if let Some(name) = bin {
                    artifact.kind == ProjectKind::Bin && artifact.name == name
                } else {
                    true
                }
            })
            .collect();
        let selected =
            if bin.is_some() || lib {
                candidates.first().copied().ok_or_else(|| {
                    if lib {
                        "project has no [lib] artifact".to_string()
                    } else {
                        format!("project has no binary named '{}'", bin.unwrap())
                    }
                })?
            } else if candidates.len() == 1 {
                candidates[0]
            } else if let Some(default) = candidates.iter().copied().find(|artifact| {
                artifact.kind == ProjectKind::Bin && artifact.name == self.config.name
            }) {
                default
            } else {
                return Err(
                    "project has multiple artifacts; select one with --bin <name> or --lib".into(),
                );
            };
        self.config.name = selected.name.clone();
        self.config.kind = selected.kind.clone();
        self.config.entry = selected.entry.clone();
        Ok(())
    }

    pub fn link_for_target(&self, triple: &str) -> LinkConfig {
        let mut link = self.config.link.clone();
        if let Some(override_) = self.config.target_links.get(triple) {
            if override_.linker.is_some() {
                link.linker = override_.linker.clone();
            }
            if let Some(libc) = override_.libc {
                link.libc = libc;
            }
            link.objects.extend(override_.objects.iter().cloned());
            link.libraries.extend(override_.libraries.iter().cloned());
            link.library_paths
                .extend(override_.library_paths.iter().cloned());
            link.flags.extend(override_.flags.iter().cloned());
        }
        link
    }

    pub fn incremental_cache_path(&self, triple: &str) -> PathBuf {
        self.config
            .out_dir
            .join("quazi")
            .join(triple)
            .join(&self.config.name)
            .join("incremental.qzc")
    }

    pub fn incremental_inputs(&self) -> Vec<PathBuf> {
        let mut inputs = vec![self.config.root.join("quazi.toml")];
        let lock = self.config.root.join("quazi.lock");
        if lock.exists() {
            inputs.push(lock);
        }
        for dependency in &self.config.dependencies {
            if dependency.root.is_file() {
                inputs.push(dependency.root.clone());
            } else {
                let manifest = dependency.root.join("quazi.toml");
                if manifest.exists() {
                    inputs.push(manifest);
                }
            }
        }
        inputs
    }

    fn load_from_root(root: &Path) -> Result<Self, String> {
        let meta = load_project_meta(root)?;
        let config = ProjectConfig {
            root: root.to_path_buf(),
            out_dir: meta.out_dir.clone(),
            name: meta.name.clone(),
            version: meta.version.clone(),
            package: meta.package,
            kind: meta.kind.clone(),
            entry: meta.entry.clone(),
            src_dir: meta.src_dir.clone(),
            flags: meta.flags.clone(),
            dependencies: Vec::new(),
            qzi_dependencies: Vec::new(),
            cc: meta.cc.clone(),
            link: meta.link.clone(),
            artifacts: meta.artifacts.clone(),
            target_links: meta.target_links.clone(),
        };

        let mut resolver = ModuleResolver::default();
        let mut visited = HashSet::new();
        let mut dep_versions: HashMap<String, ResolvedDependency> = HashMap::new();
        let lock_path = root.join("quazi.lock");
        let existing_lock = lock_path
            .exists()
            .then(|| load_lockfile(&lock_path))
            .transpose()?;
        let locked_packages: HashMap<String, LockPackage> = existing_lock
            .as_ref()
            .map(|lock| {
                lock.package
                    .iter()
                    .map(|package| (package.name.clone(), package.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let mut qzi_dependencies = Vec::new();
        let package_cache = config.out_dir.join("deps");
        let interface_cache = package_cache.join("interfaces");

        collect_modules(
            root,
            &mut resolver,
            &mut visited,
            None,
            &config.name,
            &mut dep_versions,
            &locked_packages,
            &mut qzi_dependencies,
            &interface_cache,
            &package_cache,
        )?;

        let mut dependencies: Vec<ResolvedDependency> = dep_versions.into_values().collect();
        dependencies.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(Self {
            config: ProjectConfig {
                dependencies,
                qzi_dependencies,
                ..config
            },
            resolver,
        })
    }
}

fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut dir = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };

    loop {
        // Empty path (after popping single component) means CWD.
        let check: &Path = if dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            &dir
        };

        if check.join("quazi.toml").exists() {
            return check
                .canonicalize()
                .ok()
                .or_else(|| Some(check.to_path_buf()));
        }

        // Can't go higher — empty dir means we already checked CWD.
        if dir.as_os_str().is_empty() || !dir.pop() {
            return None;
        }
    }
}

fn load_project_meta(root: &Path) -> Result<ProjectMeta, String> {
    let path = root.join("quazi.toml");
    let raw = read_raw_config(&path)?;
    let package = raw
        .package
        .ok_or_else(|| "quazi.toml missing [package] section".to_string())?;
    let out_dir_setting = Path::new(package.out_dir.as_deref().unwrap_or("build"));
    if out_dir_setting.is_absolute()
        || out_dir_setting
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("package.out_dir must stay inside the project".to_string());
    }
    let out_dir = root.join(out_dir_setting);
    let package_settings = PackageSettings {
        std: package.std.unwrap_or(true),
        crash_handler: package.crash_handler.unwrap_or(true),
        mangling: package.mangling.unwrap_or(true),
    };

    let legacy_kind = match package.kind.as_deref() {
        Some("lib") => ProjectKind::Lib,
        Some("bin") | None => ProjectKind::Bin,
        Some(other) => {
            return Err(format!(
                "quazi.toml: unknown package type '{}' (expected 'bin' or 'lib')",
                other
            ));
        }
    };

    let build = raw.build.unwrap_or(RawBuild {
        entry: None,
        src: None,
        flags: None,
    });

    let raw_cc = raw.cc.unwrap_or_default();
    let cc = CcConfig {
        sources: raw_cc
            .sources
            .unwrap_or_default()
            .into_iter()
            .map(|path| root.join(path))
            .collect(),
        include_paths: raw_cc
            .include_paths
            .unwrap_or_default()
            .into_iter()
            .map(|path| root.join(path))
            .collect(),
        defines: raw_cc.defines.unwrap_or_default(),
        flags: raw_cc.flags.unwrap_or_default(),
    };
    let make_link = |raw_link: RawLink| LinkConfigOverride {
        linker: raw_link.linker,
        libc: raw_link.libc,
        objects: raw_link
            .objects
            .unwrap_or_default()
            .into_iter()
            .map(|path| root.join(path))
            .collect(),
        libraries: raw_link.libraries.unwrap_or_default(),
        library_paths: raw_link
            .library_paths
            .unwrap_or_default()
            .into_iter()
            .map(|path| root.join(path))
            .collect(),
        flags: raw_link.flags.unwrap_or_default(),
    };

    let base = make_link(raw.link.unwrap_or_default());
    let link = LinkConfig {
        linker: base.linker,
        libc: base.libc.unwrap_or(false),
        objects: base.objects,
        libraries: base.libraries,
        library_paths: base.library_paths,
        flags: base.flags,
    };
    let target_links = raw
        .target
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(triple, target)| target.link.map(|link| (triple, make_link(link))))
        .collect();

    let has_legacy_entry = build.entry.is_some();
    let src_dir = root.join(build.src.unwrap_or_else(|| "src".to_string()));
    let default_entry = match legacy_kind {
        ProjectKind::Lib => "src/lib.qz",
        ProjectKind::Bin => "src/main.qz",
    };
    let entry = root.join(build.entry.unwrap_or_else(|| default_entry.to_string()));

    let mut artifacts = Vec::new();
    if let Some(lib) = raw.lib {
        artifacts.push(ProjectArtifact {
            name: lib.name.unwrap_or_else(|| package.name.clone()),
            kind: ProjectKind::Lib,
            entry: root.join(lib.path.unwrap_or_else(|| "src/lib.qz".into())),
        });
    }
    for bin in raw.bins.unwrap_or_default() {
        artifacts.push(ProjectArtifact {
            name: bin.name.unwrap_or_else(|| package.name.clone()),
            kind: ProjectKind::Bin,
            entry: root.join(bin.path.unwrap_or_else(|| "src/main.qz".into())),
        });
    }
    if artifacts.is_empty() {
        artifacts.push(ProjectArtifact {
            name: package.name.clone(),
            kind: legacy_kind.clone(),
            entry: entry.clone(),
        });
    } else if package.kind.is_some() || has_legacy_entry {
        return Err(
            "quazi.toml cannot mix [lib]/[[bin]] with legacy package.type/build.entry".into(),
        );
    }
    let mut names = HashSet::new();
    for artifact in &artifacts {
        if artifact.kind == ProjectKind::Lib && !is_quazi_identifier(&artifact.name) {
            return Err(format!(
                "library name '{}' must be a Quazi identifier because it is used by imports",
                artifact.name
            ));
        }
        if artifact.kind == ProjectKind::Lib && artifact.name != package.name {
            return Err(format!(
                "library name '{}' must match package name '{}'",
                artifact.name, package.name
            ));
        }
        if !names.insert((artifact.kind.clone(), artifact.name.clone())) {
            return Err(format!("duplicate artifact name '{}'", artifact.name));
        }
        if !artifact.entry.exists() {
            return Err(format!(
                "entry file not found: {}",
                artifact.entry.display()
            ));
        }
    }
    let selected = artifacts
        .iter()
        .find(|artifact| artifact.kind == ProjectKind::Bin && artifact.name == package.name)
        .or_else(|| artifacts.first())
        .expect("artifacts cannot be empty");
    let kind = selected.kind.clone();
    let entry = selected.entry.clone();

    if !entry.exists() {
        return Err(format!("entry file not found: {}", entry.to_string_lossy()));
    }

    let mut dependencies = Vec::new();
    if let Some(raw_deps) = raw.dependencies {
        for (name, spec) in raw_deps {
            if !is_quazi_identifier(&name) {
                return Err(format!(
                    "dependency name '{name}' must be a Quazi identifier because it is used by import"
                ));
            }
            let (kind, path, url, version, revision, checksum) = match spec {
                RawDependency::Path(path) => (None, Some(path), None, None, None, None),
                RawDependency::Table {
                    kind,
                    path,
                    url,
                    version,
                    rev,
                    checksum,
                } => (kind, path, url, version, rev, checksum),
            };
            if path.is_some() == url.is_some() {
                return Err(format!(
                    "dependency '{name}' must specify exactly one of 'path' or 'url'"
                ));
            }
            if url.is_some() && kind.is_none() {
                return Err(format!(
                    "dependency '{name}' uses 'url' and must specify type = \"git\", \"archive\", \"source\", or \"qzi\""
                ));
            }
            if path.is_some() && matches!(kind, Some(DependencyKind::Git | DependencyKind::Archive))
            {
                return Err(format!(
                    "dependency '{name}' cannot use type = \"{}\" with 'path'",
                    kind.unwrap().as_str()
                ));
            }
            dependencies.push(DependencySpec {
                name,
                kind,
                path,
                url,
                requested_version: version,
                revision,
                checksum,
            });
        }
    }

    Ok(ProjectMeta {
        name: package.name,
        version: package.version,
        package: package_settings,
        out_dir,
        kind,
        entry,
        src_dir,
        flags: build.flags.unwrap_or_default(),
        dependencies,
        cc,
        link,
        artifacts,
        target_links,
    })
}

fn is_quazi_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn read_raw_config(path: &Path) -> Result<RawConfig, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read '{}': {}", path.to_string_lossy(), e))?;
    toml::from_str(&text).map_err(|e| format!("cannot parse '{}': {}", path.to_string_lossy(), e))
}

fn collect_modules(
    root: &Path,
    resolver: &mut ModuleResolver,
    visited: &mut HashSet<PathBuf>,
    expected: Option<(&str, Option<&str>)>,
    root_name: &str,
    dep_versions: &mut HashMap<String, ResolvedDependency>,
    locked_packages: &HashMap<String, LockPackage>,
    qzi_dependencies: &mut Vec<PathBuf>,
    interface_cache: &Path,
    package_cache: &Path,
) -> Result<(), String> {
    let canonical = root
        .canonicalize()
        .map_err(|e| format!("cannot resolve '{}': {}", root.to_string_lossy(), e))?;

    let mut meta = load_project_meta(&canonical)?;
    if expected.is_some()
        && let Some(library) = meta
            .artifacts
            .iter()
            .find(|artifact| artifact.kind == ProjectKind::Lib)
    {
        meta.entry = library.entry.clone();
        meta.kind = ProjectKind::Lib;
    }

    let import_name = expected.map(|(alias, _)| alias).unwrap_or(&meta.name);
    if let Some((_, expect_version)) = expected {
        if let Some(expect_ver) = expect_version
            && meta.version.as_deref() != Some(expect_ver)
        {
            return Err(format!(
                "dependency '{}' version mismatch: expected {}, got {}",
                meta.name,
                expect_ver,
                meta.version.clone().unwrap_or_else(|| "<none>".to_string())
            ));
        }
    }

    let spec = ModuleSpec {
        name: import_name.to_string(),
        root: canonical.clone(),
        src_dir: meta.src_dir.clone(),
        entry: meta.entry.clone(),
        entry_is_package_root: true,
        version: meta.version.clone(),
    };
    resolver.insert(spec)?;

    if import_name != root_name {
        dep_versions.insert(
            import_name.to_string(),
            ResolvedDependency {
                name: import_name.to_string(),
                identity: Some(meta.name.clone()),
                root: canonical.clone(),
                kind: DependencyKind::Path,
                url: None,
                revision: None,
                checksum: None,
                version: meta.version.clone(),
                selector: None,
                floating: false,
            },
        );
    }

    if !visited.insert(canonical.clone()) {
        return Ok(());
    }

    for dep in meta.dependencies {
        let locked = locked_packages.get(&dep.name);
        let git_selector = (dep.kind == Some(DependencyKind::Git))
            .then(|| {
                if dep.requested_version.is_some() && dep.revision.is_some() {
                    return Err(format!(
                        "Git dependency '{}' cannot specify both version and rev",
                        dep.name
                    ));
                }
                Ok(dep
                    .requested_version
                    .clone()
                    .or_else(|| dep.revision.clone()))
            })
            .transpose()?
            .flatten();
        if let (Some(locked), Some(url)) = (locked, dep.url.as_deref()) {
            let requested_checksum = dep
                .checksum
                .as_deref()
                .map(|value| value.strip_prefix("sha256:").unwrap_or(value));
            let locked_checksum = locked
                .checksum
                .as_deref()
                .map(|value| value.strip_prefix("sha256:").unwrap_or(value));
            if locked.url.as_deref() != Some(url)
                || Some(locked.kind) != dep.kind
                || locked.selector != git_selector
                || requested_checksum.is_some_and(|checksum| locked_checksum != Some(checksum))
            {
                return Err(format!(
                    "dependency '{}' differs from quazi.lock; delete quazi.lock and run `qz fetch`",
                    dep.name
                ));
            }
        }
        let materialized = if let Some(path) = dep.path.as_deref() {
            materialize_path(&canonical, path, dep.kind)?
        } else {
            let url = dep.url.as_deref().expect("validated URL dependency");
            let kind = dep.kind.expect("validated URL dependency type");
            let locked_source = locked.map(|package| LockedSource {
                revision: package.revision.clone(),
                checksum: package.checksum.clone(),
            });
            materialize_url(
                &dep.name,
                kind,
                url,
                git_selector.as_deref(),
                dep.checksum.as_deref(),
                locked_source.as_ref(),
                package_cache,
            )?
        };
        match materialized.kind {
            DependencyKind::Qzi => {
                let bytes = std::fs::read(&materialized.path).map_err(|error| {
                    format!(
                        "cannot read QZI dependency '{}': {error}",
                        materialized.path.display()
                    )
                })?;
                let mut module = crate::bytecode::deserialize_qzi_module(&bytes)
                    .map_err(|error| format!("invalid QZI dependency '{}': {error}", dep.name))?;
                let identity =
                    (!module.metadata.name.is_empty()).then(|| module.metadata.name.clone());
                if module.metadata.kind != crate::bytecode::QziModuleKind::Library {
                    return Err(format!(
                        "QZI dependency '{}' is executable; dependencies must be library QZI modules",
                        dep.name
                    ));
                }
                if let Some(expected_version) = dep.requested_version.as_deref()
                    && module.metadata.version.as_deref() != Some(expected_version)
                {
                    return Err(format!(
                        "QZI dependency '{}' version mismatch: expected {}, got {}",
                        dep.name,
                        expected_version,
                        module.metadata.version.as_deref().unwrap_or("<none>")
                    ));
                }
                let qzi_path = if identity.as_deref().is_some_and(|name| name != dep.name) {
                    module.alias_library_namespace(&dep.name);
                    let alias_dir = package_cache.join("qzi-alias");
                    std::fs::create_dir_all(&alias_dir).map_err(|error| {
                        format!(
                            "cannot create QZI alias cache '{}': {error}",
                            alias_dir.display()
                        )
                    })?;
                    let source_hash = materialized
                        .checksum
                        .clone()
                        .unwrap_or_else(|| crate::package::sha256_bytes(&bytes));
                    let path = alias_dir.join(format!("{source_hash}-{}.qzi", dep.name));
                    let encoded = crate::bytecode::serialize_qzi_module(&module)?;
                    std::fs::write(&path, encoded).map_err(|error| {
                        format!("cannot write aliased QZI '{}': {error}", path.display())
                    })?;
                    path
                } else {
                    materialized.path.clone()
                };
                let interface = materialize_qzi_interface(
                    interface_cache,
                    &dep.name,
                    &qzi_path,
                    &module.interface,
                )?;
                resolver.insert(ModuleSpec {
                    name: dep.name.clone(),
                    root: interface.root,
                    src_dir: interface.src_dir,
                    entry: interface.entry,
                    entry_is_package_root: true,
                    version: module.metadata.version.clone(),
                })?;
                qzi_dependencies.push(qzi_path.clone());
                dep_versions.insert(
                    dep.name.clone(),
                    ResolvedDependency {
                        name: dep.name,
                        identity,
                        root: qzi_path,
                        kind: DependencyKind::Qzi,
                        url: dep.url,
                        revision: materialized.revision,
                        checksum: materialized.checksum,
                        version: module.metadata.version,
                        selector: None,
                        floating: false,
                    },
                );
            }
            DependencyKind::Source => {
                if dep.requested_version.is_some() {
                    return Err(format!(
                        "source dependency '{}' cannot request a package version",
                        dep.name
                    ));
                }
                let source_root = materialized
                    .path
                    .parent()
                    .unwrap_or(Path::new("."))
                    .to_path_buf();
                resolver.insert(ModuleSpec {
                    name: dep.name.clone(),
                    root: source_root.clone(),
                    src_dir: source_root,
                    entry: materialized.path.clone(),
                    entry_is_package_root: true,
                    version: None,
                })?;
                dep_versions.insert(
                    dep.name.clone(),
                    ResolvedDependency {
                        name: dep.name,
                        identity: None,
                        root: materialized.path,
                        kind: DependencyKind::Source,
                        url: dep.url,
                        revision: None,
                        checksum: materialized.checksum,
                        version: None,
                        selector: None,
                        floating: false,
                    },
                );
            }
            DependencyKind::Path | DependencyKind::Git | DependencyKind::Archive => {
                let expected_version = (materialized.kind != DependencyKind::Git)
                    .then_some(dep.requested_version.as_deref())
                    .flatten();
                let dep_expected = (dep.name.as_str(), expected_version);
                collect_modules(
                    &materialized.path,
                    resolver,
                    visited,
                    Some(dep_expected),
                    root_name,
                    dep_versions,
                    locked_packages,
                    qzi_dependencies,
                    interface_cache,
                    package_cache,
                )?;
                if let Some(resolved) = dep_versions.get_mut(&dep.name) {
                    resolved.kind = materialized.kind;
                    resolved.url = dep.url;
                    resolved.revision = materialized.revision;
                    resolved.checksum = materialized.checksum;
                    resolved.selector = git_selector.clone();
                    resolved.floating = git_selector.as_deref() == Some("latest");
                }
            }
        }
    }

    Ok(())
}

fn load_lockfile(path: &Path) -> Result<Lockfile, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read '{}': {}", path.to_string_lossy(), e))?;
    toml::from_str(&text).map_err(|e| format!("cannot parse '{}': {}", path.to_string_lossy(), e))
}

fn validate_lockfile(lock: &Lockfile, dependencies: &[ResolvedDependency]) -> Result<(), String> {
    if lock.package.len() != dependencies.len() {
        return Err(
            "lockfile dependency set differs from quazi.toml; delete quazi.lock and run `qz fetch`"
                .to_string(),
        );
    }
    let mut map: HashMap<&str, &LockPackage> = HashMap::new();
    for pkg in &lock.package {
        map.insert(pkg.name.as_str(), pkg);
    }

    for dependency in dependencies {
        let Some(pkg) = map.get(dependency.name.as_str()) else {
            return Err(format!("lockfile missing package '{}'", dependency.name));
        };
        if pkg.version.as_deref() != dependency.version.as_deref() {
            return Err(format!(
                "lockfile version mismatch for '{}': expected {}, got {}",
                dependency.name,
                dependency
                    .version
                    .clone()
                    .unwrap_or_else(|| "<none>".to_string()),
                pkg.version.clone().unwrap_or_else(|| "<none>".to_string())
            ));
        }
        if pkg.kind != dependency.kind
            || pkg.identity != dependency.identity
            || pkg.url != dependency.url
            || pkg.revision != dependency.revision
            || pkg.checksum != dependency.checksum
            || pkg.selector != dependency.selector
        {
            return Err(format!(
                "lockfile source mismatch for '{}'; delete quazi.lock and run `qz fetch`",
                dependency.name
            ));
        }
    }

    Ok(())
}

fn write_lockfile(path: &Path, dependencies: &[ResolvedDependency]) -> Result<(), String> {
    let mut packages: Vec<LockPackage> = dependencies
        .iter()
        .map(|dependency| LockPackage {
            name: dependency.name.clone(),
            identity: dependency.identity.clone(),
            version: dependency.version.clone(),
            kind: dependency.kind,
            // Local paths come from quazi.toml and may resolve differently on
            // another machine. Lock versions/checksums, never absolute paths.
            path: None,
            url: dependency.url.clone(),
            revision: dependency.revision.clone(),
            checksum: dependency.checksum.clone(),
            selector: dependency.selector.clone(),
        })
        .collect();
    packages.sort_by(|a, b| a.name.cmp(&b.name));

    let lock = Lockfile { package: packages };
    let text =
        toml::to_string_pretty(&lock).map_err(|e| format!("cannot serialize lockfile: {}", e))?;
    std::fs::write(path, text)
        .map_err(|e| format!("cannot write '{}': {}", path.to_string_lossy(), e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::analyze_program_with_source_files;

    #[test]
    fn infers_dependency_identifier_from_url() {
        assert_eq!(
            infer_dependency_name(None, Some("https://github.com/namnam1105/qz-test-lib.git"))
                .unwrap(),
            "qz_test_lib"
        );
    }
    use crate::bytecode::instruction::{ri16, rrr};
    use crate::bytecode::interface::{QziInterfaceBundle, QziInterfaceModule};
    use crate::bytecode::opcode::Opcode;
    use crate::bytecode::{Chunk, Codegen, QziMetadata, QziModule, QziModuleKind};
    use std::collections::{HashMap as StdHashMap, HashSet as StdHashSet};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        dir.push(format!("{}_{}_{}", prefix, std::process::id(), nanos));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn selects_named_artifacts_and_separates_cache_paths() {
        let root = temp_dir("qz_artifacts");
        fs::create_dir_all(root.join("src")).expect("create src");
        fs::write(root.join("src/lib.qz"), "pub fn value() i32 { ret 1; }").unwrap();
        fs::write(root.join("src/main.qz"), "fn main() i32 { ret 0; }").unwrap();
        fs::write(root.join("src/tool.qz"), "fn main() i32 { ret 0; }").unwrap();
        fs::write(
            root.join("quazi.toml"),
            r#"[package]
name = "app"
version = "0.1.0"

[lib]
name = "app"
path = "src/lib.qz"

[[bin]]
name = "app"
path = "src/main.qz"

[[bin]]
name = "tool"
path = "src/tool.qz"
"#,
        )
        .unwrap();
        let mut context = ProjectContext::load(&root).expect("load project");
        context
            .select_artifact(Some("tool"), false)
            .expect("select tool");
        assert_eq!(context.config.name, "tool");
        assert_eq!(
            context.config.out_dir,
            root.canonicalize().unwrap().join("build")
        );
        assert!(context.config.entry.ends_with(Path::new("src/tool.qz")));
        assert!(
            context
                .incremental_cache_path("x86_64-windows")
                .ends_with(Path::new("x86_64-windows/tool/incremental.qzc"))
        );
        context.select_artifact(None, true).expect("select library");
        assert_eq!(context.config.kind, ProjectKind::Lib);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_out_dir_is_configurable_but_cannot_escape_project() {
        let root = temp_dir("qz_out_dir");
        fs::create_dir_all(root.join("src")).expect("create src");
        fs::write(root.join("src/main.qz"), "fn main() i32 { ret 0; }").unwrap();
        fs::write(
            root.join("quazi.toml"),
            "[package]\nname = \"app\"\nout_dir = \"output\"\n",
        )
        .unwrap();
        let context = ProjectContext::load(&root).expect("custom output directory");
        assert_eq!(
            context.config.out_dir,
            root.canonicalize().unwrap().join("output")
        );
        assert_eq!(context.config.package, PackageSettings::default());

        fs::write(
            root.join("quazi.toml"),
            "[package]\nname = \"app\"\nstd = false\ncrash_handler = false\nmangling = false\n",
        )
        .unwrap();
        let context = ProjectContext::load(&root).expect("explicit package settings");
        assert_eq!(
            context.config.package,
            PackageSettings {
                std: false,
                crash_handler: false,
                mangling: false,
            }
        );

        fs::write(
            root.join("quazi.toml"),
            "[package]\nname = \"app\"\nout_dir = \"../outside\"\n",
        )
        .unwrap();
        assert!(ProjectContext::load(&root).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn add_and_remove_local_dependency_updates_manifest_and_lock() {
        let root = temp_dir("qz_dependency_edit");
        let app = root.join("app");
        let dependency = root.join("math");
        fs::create_dir_all(app.join("src")).expect("create app src");
        fs::create_dir_all(dependency.join("src")).expect("create dependency src");
        fs::write(
            app.join("quazi.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
        )
        .expect("write app manifest");
        fs::write(app.join("src/main.qz"), "fn main() void { ret; }").expect("write app source");
        fs::write(
            dependency.join("quazi.toml"),
            "[package]\nname = \"math\"\nversion = \"0.1.0\"\ntype = \"lib\"\n",
        )
        .expect("write dependency manifest");
        fs::write(
            dependency.join("src/lib.qz"),
            "pub fn answer() i32 { ret 42; }",
        )
        .expect("write dependency source");

        add_dependency(
            &app,
            DependencyEdit {
                name: "numbers".into(),
                path: Some(PathBuf::from("../math")),
                url: None,
                kind: None,
                version: Some("0.1.0".into()),
                revision: None,
                checksum: None,
            },
        )
        .expect("add local dependency");
        let manifest = fs::read_to_string(app.join("quazi.toml")).expect("read manifest");
        assert!(manifest.contains("[dependencies.numbers]"));
        assert!(app.join("quazi.lock").is_file());
        let lock = load_lockfile(&app.join("quazi.lock")).expect("read dependency lock");
        assert_eq!(lock.package[0].name, "numbers");
        assert_eq!(lock.package[0].identity.as_deref(), Some("math"));

        remove_dependency(&app, "numbers").expect("remove local dependency");
        let manifest = fs::read_to_string(app.join("quazi.toml")).expect("read manifest");
        assert!(!manifest.contains("[dependencies.numbers]"));
        assert!(!app.join("quazi.lock").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn loads_project_and_writes_lockfile() {
        let root = temp_dir("qz_project");
        let src_dir = root.join("src");
        fs::create_dir_all(&src_dir).expect("create src");
        fs::write(src_dir.join("main.qz"), "fn main() void { ret; }").expect("write main.qz");

        let dep_root = root.join("dep");
        let dep_src = dep_root.join("src");
        fs::create_dir_all(&dep_src).expect("create dep src");
        fs::write(dep_src.join("main.qz"), "fn dep_main() void { ret; }").expect("write dep main");

        fs::write(
            root.join("quazi.toml"),
            r#"[package]
name = "app"
version = "0.1.0"

[dependencies]
dep = { path = "dep", version = "1.2.3" }
"#,
        )
        .expect("write app quazi.toml");

        fs::write(
            dep_root.join("quazi.toml"),
            r#"[package]
name = "dep"
version = "1.2.3"
"#,
        )
        .expect("write dep quazi.toml");

        let ctx = ProjectContext::load(&root).expect("load project context");
        assert_eq!(ctx.config.name, "app");
        assert!(ctx.resolver.modules.contains_key("app"));
        assert!(ctx.resolver.modules.contains_key("dep"));
        assert_eq!(ctx.config.dependencies.len(), 1);
        assert_eq!(ctx.config.dependencies[0].name, "dep");
        assert_eq!(ctx.config.dependencies[0].version.as_deref(), Some("1.2.3"));

        ctx.ensure_lockfile().expect("ensure lockfile");
        let lock_path = root.join("quazi.lock");
        assert!(lock_path.exists(), "expected lockfile to be created");
        let lock = load_lockfile(&lock_path).expect("load lockfile");
        assert_eq!(lock.package.len(), 1);
        assert_eq!(lock.package[0].name, "dep");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn source_project_imports_and_links_qzi_library_dependency() {
        let root = temp_dir("qz_qzi_dependency");
        let src_dir = root.join("src");
        fs::create_dir_all(&src_dir).expect("create src");
        fs::write(
            src_dir.join("main.qz"),
            "import numbers.answer;\nfn main() i32 { ret answer(); }",
        )
        .expect("write main");

        let interface = toml::to_string(&QziInterfaceBundle {
            modules: vec![QziInterfaceModule {
                name: "dep".to_string(),
                exports: vec!["answer".to_string()],
                source: "pub fn answer() i32;\n".to_string(),
            }],
        })
        .expect("serialize interface");
        let mut answer = Chunk::new("dep.answer");
        answer.reg_count = 1;
        answer.emit(ri16(Opcode::MovI, 0, 42));
        answer.emit(rrr(Opcode::Ret, 0, 0, 0));
        let dependency = QziModule {
            metadata: QziMetadata {
                name: "dep".to_string(),
                version: Some("1.0.0".to_string()),
                kind: QziModuleKind::Library,
                main_takes_args: false,
            },
            interface,
            call_relocations: Vec::new(),
            chunks: vec![answer],
        };
        let dependency_path = root.join("dep.qzi");
        fs::write(
            &dependency_path,
            crate::bytecode::serialize_qzi_module(&dependency).expect("serialize dependency"),
        )
        .expect("write dependency");
        fs::write(
            root.join("quazi.toml"),
            r#"[package]
name = "app"
version = "0.1.0"

[dependencies]
numbers = { path = "dep.qzi" }
"#,
        )
        .expect("write manifest");

        let context = ProjectContext::load(&root).expect("load QZI project");
        assert_ne!(
            context.config.qzi_dependencies,
            [dependency_path.canonicalize().unwrap()]
        );
        assert!(
            context.config.qzi_dependencies[0]
                .to_string_lossy()
                .contains("qzi-alias")
        );
        let loaded = crate::loader::load_programs_with_resolver(
            std::slice::from_ref(&context.config.entry),
            Some(&context.resolver),
        )
        .expect("load source against QZI interface");
        let namespaced_paths: StdHashSet<String> = loaded
            .namespaced_paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        let report = analyze_program_with_source_files(
            &loaded.merged_source,
            &loaded.program,
            loaded.library_fn_names.clone(),
            loaded.library_char_ranges.clone(),
            loaded.source_files.clone(),
            namespaced_paths,
        );
        assert!(
            report.errors.is_empty(),
            "semantic errors: {:?}",
            report.errors
        );
        let mut codegen = Codegen::new(&report);
        let chunks = codegen
            .compile_program(&loaded.program, &loaded.source_files)
            .expect("compile application");
        assert!(
            codegen
                .external_call_relocations()
                .iter()
                .any(|relocation| relocation.symbol == "numbers.answer")
        );
        let generated = QziModule {
            metadata: QziMetadata {
                name: "app".to_string(),
                version: None,
                kind: QziModuleKind::Executable,
                main_takes_args: false,
            },
            interface: String::new(),
            call_relocations: codegen.external_call_relocations().to_vec(),
            chunks,
        };
        let aliased_dependency = crate::bytecode::deserialize_qzi_module(
            &fs::read(&context.config.qzi_dependencies[0]).expect("read aliased dependency"),
        )
        .expect("deserialize aliased dependency");
        let linked = crate::bytecode::link_qzi_modules(&[generated, aliased_dependency])
            .expect("link QZI dependency");
        let symbols: StdHashMap<_, _> = linked
            .iter()
            .enumerate()
            .map(|(index, chunk)| (chunk.name.as_str(), index as u16))
            .collect();
        let main = linked.iter().find(|chunk| chunk.name == "main").unwrap();
        let call = main
            .code
            .iter()
            .find(|instruction| instruction.opcode == Opcode::CallIdx as u8)
            .expect("main should call dependency");
        assert_eq!(call.ri16().1, symbols["numbers.answer"]);

        let _ = fs::remove_dir_all(root);
    }
}
