// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::loader::{ModuleResolver, ModuleSpec};

#[derive(Debug, Clone)]
pub struct ProjectConfig {
    pub root: PathBuf,
    pub name: String,
    pub version: Option<String>,
    pub entry: PathBuf,
    pub src_dir: PathBuf,
    pub flags: Vec<String>,
    pub dependencies: Vec<ResolvedDependency>,
}

#[derive(Debug, Clone)]
pub struct ResolvedDependency {
    pub name: String,
    pub root: PathBuf,
    pub requested_version: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub config: ProjectConfig,
    pub resolver: ModuleResolver,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    package: Option<RawPackage>,
    build: Option<RawBuild>,
    dependencies: Option<BTreeMap<String, RawDependency>>,
}

#[derive(Debug, Deserialize)]
struct RawPackage {
    name: String,
    version: Option<String>,
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
    Table { path: String, version: Option<String> },
}

#[derive(Debug, Clone)]
struct ProjectMeta {
    name: String,
    version: Option<String>,
    entry: PathBuf,
    src_dir: PathBuf,
    flags: Vec<String>,
    dependencies: Vec<DependencySpec>,
}

#[derive(Debug, Clone)]
struct DependencySpec {
    name: String,
    path: PathBuf,
    requested_version: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Lockfile {
    package: Vec<LockPackage>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LockPackage {
    name: String,
    version: Option<String>,
    path: String,
}

impl ProjectContext {
    pub fn load(start: &Path) -> Result<Self, String> {
        let root = find_project_root(start)
            .ok_or_else(|| "void.toml not found in this directory or parents".to_string())?;
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
        let lock_path = self.config.root.join("void.lock");
        if lock_path.exists() {
            let lock = load_lockfile(&lock_path)?;
            validate_lockfile(&lock, &self.resolver, &self.config.name)
        } else {
            write_lockfile(&lock_path, &self.resolver, &self.config.name)
        }
    }

    fn load_from_root(root: &Path) -> Result<Self, String> {
        let meta = load_project_meta(root)?;
        let config = ProjectConfig {
            root: root.to_path_buf(),
            name: meta.name.clone(),
            version: meta.version.clone(),
            entry: meta.entry.clone(),
            src_dir: meta.src_dir.clone(),
            flags: meta.flags.clone(),
            dependencies: Vec::new(),
        };

        let mut resolver = ModuleResolver::default();
        let mut visited = HashSet::new();
        let mut dep_versions: HashMap<String, ResolvedDependency> = HashMap::new();

        collect_modules(
            root,
            &mut resolver,
            &mut visited,
            None,
            &config.name,
            &mut dep_versions,
        )?;

        let mut dependencies: Vec<ResolvedDependency> = dep_versions.into_values().collect();
        dependencies.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(Self {
            config: ProjectConfig { dependencies, ..config },
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
        if dir.join("void.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn load_project_meta(root: &Path) -> Result<ProjectMeta, String> {
    let path = root.join("void.toml");
    let raw = read_raw_config(&path)?;
    let package = raw
        .package
        .ok_or_else(|| "void.toml missing [package] section".to_string())?;

    let build = raw.build.unwrap_or(RawBuild {
        entry: None,
        src: None,
        flags: None,
    });

    let src_dir = root.join(build.src.unwrap_or_else(|| "src".to_string()));
    let entry = root.join(build.entry.unwrap_or_else(|| "src/main.void".to_string()));

    if !entry.exists() {
        return Err(format!(
            "entry file not found: {}",
            entry.to_string_lossy()
        ));
    }

    let mut dependencies = Vec::new();
    if let Some(raw_deps) = raw.dependencies {
        for (name, spec) in raw_deps {
            let (path, version) = match spec {
                RawDependency::Path(path) => (path, None),
                RawDependency::Table { path, version } => (path, version),
            };
            let dep_root = root.join(path);
            dependencies.push(DependencySpec {
                name,
                path: dep_root,
                requested_version: version,
            });
        }
    }

    Ok(ProjectMeta {
        name: package.name,
        version: package.version,
        entry,
        src_dir,
        flags: build.flags.unwrap_or_default(),
        dependencies,
    })
}

fn read_raw_config(path: &Path) -> Result<RawConfig, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read '{}': {}", path.to_string_lossy(), e))?;
    toml::from_str(&text)
        .map_err(|e| format!("cannot parse '{}': {}", path.to_string_lossy(), e))
}

fn collect_modules(
    root: &Path,
    resolver: &mut ModuleResolver,
    visited: &mut HashSet<PathBuf>,
    expected: Option<(&str, Option<&str>)>,
    root_name: &str,
    dep_versions: &mut HashMap<String, ResolvedDependency>,
) -> Result<(), String> {
    let canonical = root
        .canonicalize()
        .map_err(|e| format!("cannot resolve '{}': {}", root.to_string_lossy(), e))?;

    let meta = load_project_meta(&canonical)?;

    let requested_version = expected.and_then(|(_, v)| v.map(|s| s.to_string()));

    if let Some((expect_name, expect_version)) = expected {
        if meta.name != expect_name {
            return Err(format!(
                "dependency name mismatch: expected '{}', got '{}'",
                expect_name, meta.name
            ));
        }
        if let Some(expect_ver) = expect_version {
            if meta.version.as_deref() != Some(expect_ver) {
                return Err(format!(
                    "dependency '{}' version mismatch: expected {}, got {}",
                    meta.name,
                    expect_ver,
                    meta.version.clone().unwrap_or_else(|| "<none>".to_string())
                ));
            }
        }
    }

    let spec = ModuleSpec {
        name: meta.name.clone(),
        root: canonical.clone(),
        src_dir: meta.src_dir.clone(),
        entry: meta.entry.clone(),
        version: meta.version.clone(),
    };
    resolver.insert(spec)?;

    if meta.name != root_name {
        dep_versions.insert(
            meta.name.clone(),
            ResolvedDependency {
                name: meta.name.clone(),
                root: canonical.clone(),
                requested_version,
                version: meta.version.clone(),
            },
        );
    }

    if !visited.insert(canonical.clone()) {
        return Ok(());
    }

    for dep in meta.dependencies {
        let dep_expected = (dep.name.as_str(), dep.requested_version.as_deref());
        collect_modules(
            dep.path.as_path(),
            resolver,
            visited,
            Some(dep_expected),
            root_name,
            dep_versions,
        )?;
    }

    Ok(())
}

fn load_lockfile(path: &Path) -> Result<Lockfile, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read '{}': {}", path.to_string_lossy(), e))?;
    toml::from_str(&text)
        .map_err(|e| format!("cannot parse '{}': {}", path.to_string_lossy(), e))
}

fn validate_lockfile(
    lock: &Lockfile,
    resolver: &ModuleResolver,
    root_name: &str,
) -> Result<(), String> {
    let mut map: HashMap<&str, &LockPackage> = HashMap::new();
    for pkg in &lock.package {
        map.insert(pkg.name.as_str(), pkg);
    }

    for (name, spec) in &resolver.modules {
        if name == root_name {
            continue;
        }
        let Some(pkg) = map.get(name.as_str()) else {
            return Err(format!("lockfile missing package '{}'", name));
        };
        if pkg.version.as_deref() != spec.version.as_deref() {
            return Err(format!(
                "lockfile version mismatch for '{}': expected {}, got {}",
                name,
                spec.version.clone().unwrap_or_else(|| "<none>".to_string()),
                pkg.version.clone().unwrap_or_else(|| "<none>".to_string())
            ));
        }
    }

    Ok(())
}

fn write_lockfile(
    path: &Path,
    resolver: &ModuleResolver,
    root_name: &str,
) -> Result<(), String> {
    let mut packages: Vec<LockPackage> = resolver
        .modules
        .iter()
        .filter(|(name, _)| name.as_str() != root_name)
        .map(|(name, spec)| LockPackage {
            name: name.clone(),
            version: spec.version.clone(),
            path: spec.root.to_string_lossy().into_owned(),
        })
        .collect();
    packages.sort_by(|a, b| a.name.cmp(&b.name));

    let lock = Lockfile { package: packages };
    let text = toml::to_string_pretty(&lock)
        .map_err(|e| format!("cannot serialize lockfile: {}", e))?;
    std::fs::write(path, text)
        .map_err(|e| format!("cannot write '{}': {}", path.to_string_lossy(), e))
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn loads_project_and_writes_lockfile() {
        let root = temp_dir("void_project");
        let src_dir = root.join("src");
        fs::create_dir_all(&src_dir).expect("create src");
        fs::write(src_dir.join("main.void"), "fn main() void { ret; }")
            .expect("write main.void");

        let dep_root = root.join("dep");
        let dep_src = dep_root.join("src");
        fs::create_dir_all(&dep_src).expect("create dep src");
        fs::write(dep_src.join("main.void"), "fn dep_main() void { ret; }")
            .expect("write dep main");

        fs::write(
            root.join("void.toml"),
            r#"[package]
name = "app"
version = "0.1.0"

[dependencies]
dep = { path = "dep", version = "1.2.3" }
"#,
        )
        .expect("write app void.toml");

        fs::write(
            dep_root.join("void.toml"),
            r#"[package]
name = "dep"
version = "1.2.3"
"#,
        )
        .expect("write dep void.toml");

        let ctx = ProjectContext::load(&root).expect("load project context");
        assert_eq!(ctx.config.name, "app");
        assert!(ctx.resolver.modules.contains_key("app"));
        assert!(ctx.resolver.modules.contains_key("dep"));
        assert_eq!(ctx.config.dependencies.len(), 1);
        assert_eq!(ctx.config.dependencies[0].name, "dep");
        assert_eq!(ctx.config.dependencies[0].version.as_deref(), Some("1.2.3"));

        ctx.ensure_lockfile().expect("ensure lockfile");
        let lock_path = root.join("void.lock");
        assert!(lock_path.exists(), "expected lockfile to be created");
        let lock = load_lockfile(&lock_path).expect("load lockfile");
        assert_eq!(lock.package.len(), 1);
        assert_eq!(lock.package[0].name, "dep");

        let _ = fs::remove_dir_all(root);
    }
}
