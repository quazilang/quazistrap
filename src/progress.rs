// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Animated build progress display.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

const SPIN: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ── TTY ───────────────────────────────────────────────────────────────────────

pub fn stderr_is_tty() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            let handle = std::io::stderr().as_raw_handle() as *mut std::ffi::c_void;
            unsafe extern "system" {
                fn GetConsoleMode(h: *mut std::ffi::c_void, mode: *mut u32) -> i32;
                fn SetConsoleMode(h: *mut std::ffi::c_void, mode: u32) -> i32;
            }
            let mut mode: u32 = 0;
            if GetConsoleMode(handle, &mut mode) != 0 {
                SetConsoleMode(handle, mode | 0x0004); // ENABLE_VIRTUAL_TERMINAL_PROCESSING
                return true;
            }
            return false;
        }
    }
    #[cfg(unix)]
    {
        unsafe {
            unsafe extern "C" {
                fn isatty(fd: i32) -> i32;
            }
            isatty(2) != 0
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        false
    }
}

// ── Dependency tree ────────────────────────────────────────────────────────────

pub struct TreeNode {
    pub label: String,
    pub is_lib: bool,
    pub is_dup: bool,
    pub children: Vec<TreeNode>,
}

pub fn common_lib_prefix(lib_files: &[PathBuf]) -> Option<PathBuf> {
    if lib_files.is_empty() {
        return None;
    }
    let mut prefix = lib_files[0].parent()?.to_path_buf();
    for path in &lib_files[1..] {
        while !path.starts_with(&prefix) {
            if !prefix.pop() {
                return None;
            }
        }
    }
    Some(prefix)
}

fn short_label(path: &Path, lib_prefix: Option<&Path>) -> String {
    if let Some(prefix) = lib_prefix
        && let Ok(rel) = path.strip_prefix(prefix)
    {
        let no_ext = rel.with_extension("");
        let parts: Vec<String> = no_ext
            .components()
            .map(|c| c.as_os_str().to_str().unwrap_or("?").to_string())
            // `.quazi` is the cache/install root directory (`~/.quazi/...`).
            // If `common_lib_prefix` ends up above the actual `std`/`prelude`
            // root (e.g. because lib files span multiple install
            // subdirectories), this component leaks into the label as
            // `.quazi.std.ffi` instead of `std.ffi`. Strip it like the
            // other structural/noise components.
            .filter(|c| c != "src" && c != "mod" && c != ".quazi")
            .collect();
        if !parts.is_empty() {
            return parts.join(".");
        }
    }
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_string()
}

fn build_inner(
    file: &Path,
    edges: &HashMap<PathBuf, Vec<PathBuf>>,
    lib_files: &HashSet<PathBuf>,
    lib_prefix: Option<&Path>,
    visited: &mut HashSet<PathBuf>,
) -> TreeNode {
    let is_lib = lib_files.contains(file);
    let label = short_label(file, if is_lib { lib_prefix } else { None });
    if !visited.insert(file.to_path_buf()) {
        return TreeNode {
            label,
            is_lib,
            is_dup: true,
            children: vec![],
        };
    }
    let children = edges
        .get(file)
        .into_iter()
        .flatten()
        .map(|to| build_inner(to, edges, lib_files, lib_prefix, visited))
        .collect();
    TreeNode {
        label,
        is_lib,
        is_dup: false,
        children,
    }
}

pub fn build_dep_tree(
    entry: &Path,
    edges: &[(PathBuf, PathBuf)],
    lib_files: &[PathBuf],
    lib_prefix: Option<&Path>,
) -> TreeNode {
    let mut adjacency: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for (from, to) in edges {
        adjacency.entry(from.clone()).or_default().push(to.clone());
    }
    let library_set: HashSet<PathBuf> = lib_files.iter().cloned().collect();
    build_inner(
        entry,
        &adjacency,
        &library_set,
        lib_prefix,
        &mut HashSet::new(),
    )
}

fn render_node(node: &TreeNode, prefix: &str, is_root: bool, is_last: bool) {
    let line = if is_root {
        format!("  \x1b[1m{}\x1b[0m", node.label)
    } else {
        let conn = if is_last {
            "\x1b[2m└─\x1b[0m ".to_string()
        } else {
            "\x1b[2m├─\x1b[0m ".to_string()
        };
        format!("  {}{}{}", prefix, conn, node.label)
    };
    let tag = if node.is_dup {
        "  \x1b[2mdup\x1b[0m"
    } else if node.is_lib {
        "  \x1b[2mlib\x1b[0m"
    } else {
        ""
    };
    eprintln!("{}{}", line, tag);
    let _ = std::io::stderr().flush();

    if !node.is_dup {
        let cp = if is_root {
            String::new()
        } else if is_last {
            format!("{}   ", prefix)
        } else {
            format!("{}\x1b[2m│\x1b[0m  ", prefix)
        };
        for (i, child) in node.children.iter().enumerate() {
            render_node(child, &cp, false, i == node.children.len() - 1);
        }
    }
}

// ── BuildProgress ─────────────────────────────────────────────────────────────

pub struct BuildProgress {
    pub is_tty: bool,
    current_label: String,
}

impl BuildProgress {
    pub fn new() -> Self {
        Self {
            is_tty: stderr_is_tty(),
            current_label: String::new(),
        }
    }

    /// Print header: `  quazi  ›  input → output`
    pub fn header(&self, input: &str, output: &str) {
        if self.is_tty {
            eprintln!(
                "\n  \x1b[2mquazi\x1b[0m  \x1b[2m›\x1b[0m  \x1b[1m{}\x1b[0m  \x1b[2m→\x1b[0m  \x1b[1m{}\x1b[0m\n",
                input, output
            );
        } else {
            eprintln!("\nquazi  {}  →  {}\n", input, output);
        }
    }

    /// Print "  ⠋ label" (no newline) — overwritable by done/fail.
    pub fn begin(&mut self, label: &str) {
        self.current_label = label.to_string();
        if self.is_tty {
            eprint!("  \x1b[2m{}\x1b[0m  \x1b[1m{}\x1b[0m", SPIN[0], label);
            let _ = std::io::stderr().flush();
        }
    }

    /// Overwrite spinner with "  ✓  label  ·  info".
    pub fn done(&mut self, info: &str) {
        if self.is_tty {
            if info.is_empty() {
                eprintln!(
                    "\r\x1b[K  \x1b[32m✓\x1b[0m  \x1b[1m{}\x1b[0m",
                    self.current_label
                );
            } else {
                eprintln!(
                    "\r\x1b[K  \x1b[32m✓\x1b[0m  \x1b[1m{}\x1b[0m  \x1b[2m·\x1b[0m  \x1b[2m{}\x1b[0m",
                    self.current_label, info
                );
            }
        } else if info.is_empty() {
            eprintln!("✓  {}", self.current_label);
        } else {
            eprintln!("✓  {}  ·  {}", self.current_label, info);
        }
    }

    /// Overwrite spinner with "  ✗  label  ·  info".
    pub fn fail(&mut self, info: &str) {
        if self.is_tty {
            if info.is_empty() {
                eprintln!(
                    "\r\x1b[K  \x1b[31m✗\x1b[0m  \x1b[1m{}\x1b[0m",
                    self.current_label
                );
            } else {
                eprintln!(
                    "\r\x1b[K  \x1b[31m✗\x1b[0m  \x1b[1m{}\x1b[0m  \x1b[2m·\x1b[0m  \x1b[2m{}\x1b[0m",
                    self.current_label, info
                );
            }
        } else if info.is_empty() {
            eprintln!("✗  {}", self.current_label);
        } else {
            eprintln!("✗  {}  ·  {}", self.current_label, info);
        }
    }

    /// Render the dependency tree below the last ✓ line.
    pub fn dep_tree(&self, root: &TreeNode) {
        eprintln!();
        if self.is_tty {
            eprint!("\x1b[?25l"); // hide cursor during animation
            let _ = std::io::stderr().flush();
        }
        render_node(root, "", true, true);
        eprintln!();
        if self.is_tty {
            eprint!("\x1b[?25h");
            let _ = std::io::stderr().flush();
        }
    }

    /// Print "  ✓  output  (N KB)" final line.
    pub fn success(&self, output: &str, size_bytes: Option<u64>) {
        let name = Path::new(output)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(output);
        eprintln!();
        if self.is_tty {
            match size_bytes {
                Some(b) => eprintln!(
                    "  \x1b[32m✓\x1b[0m  \x1b[1m{}\x1b[0m  \x1b[2m({:.1} KB)\x1b[0m",
                    name,
                    b as f64 / 1024.0
                ),
                None => eprintln!("  \x1b[32m✓\x1b[0m  \x1b[1m{}\x1b[0m", name),
            }
        } else {
            match size_bytes {
                Some(b) => eprintln!("✓  {}  ({:.1} KB)", name, b as f64 / 1024.0),
                None => eprintln!("✓  {}", name),
            }
        }
        eprintln!();
    }
}

// ── Nerd stats helpers ─────────────────────────────────────────────────────────

pub fn fmt_count(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}

pub fn codegen_stats(chunks: &[crate::bytecode::Chunk], is_tty: bool) -> String {
    let n_fns = chunks.len();
    let total_instr: usize = chunks.iter().map(|c| c.code.len()).sum();
    let max_regs: u8 = chunks.iter().map(|c| c.reg_count).max().unwrap_or(0);
    if is_tty {
        format!(
            "\x1b[36m{}\x1b[0m fn{} · \x1b[36m{}\x1b[0m instr · \x1b[36mr{}\x1b[0m",
            n_fns,
            if n_fns == 1 { "" } else { "s" },
            fmt_count(total_instr),
            max_regs
        )
    } else {
        format!(
            "{} fn{} · {} instr · r{}",
            n_fns,
            if n_fns == 1 { "" } else { "s" },
            fmt_count(total_instr),
            max_regs
        )
    }
}

pub fn arch_label() -> &'static str {
    if cfg!(target_arch = "x86_64") && cfg!(target_os = "windows") {
        "x86_64·windows"
    } else if cfg!(target_arch = "x86_64") && cfg!(target_os = "linux") {
        "x86_64·linux"
    } else if cfg!(target_arch = "x86_64") && cfg!(target_os = "macos") {
        "x86_64·macos"
    } else {
        "unknown"
    }
}
