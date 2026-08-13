// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Animated build progress display.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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

fn short_label(
    path: &Path,
    lib_prefix: Option<&Path>,
    display_names: &HashMap<PathBuf, String>,
) -> String {
    if let Some(name) = display_names.get(path) {
        return name.clone();
    }
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
    display_names: &HashMap<PathBuf, String>,
    visited: &mut HashSet<PathBuf>,
) -> TreeNode {
    let is_lib = lib_files.contains(file);
    let label = short_label(file, if is_lib { lib_prefix } else { None }, display_names);
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
        .map(|to| build_inner(to, edges, lib_files, lib_prefix, display_names, visited))
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
    display_names: &HashMap<PathBuf, String>,
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
        display_names,
        &mut HashSet::new(),
    )
}

pub fn tree_file_count(root: &TreeNode) -> usize {
    if root.is_dup {
        0
    } else {
        1 + root.children.iter().map(tree_file_count).sum::<usize>()
    }
}

fn render_node(node: &TreeNode, prefix: &str, is_root: bool, is_last: bool, color: bool) {
    let line = if is_root {
        if color {
            format!("  \x1b[1m{}\x1b[0m", node.label)
        } else {
            format!("  {}", node.label)
        }
    } else {
        let connector = if is_last { "└─" } else { "├─" };
        let conn = if color {
            format!("\x1b[2m{}\x1b[0m ", connector)
        } else {
            format!("{} ", connector)
        };
        format!("  {}{}{}", prefix, conn, node.label)
    };
    let tag_text = if node.is_dup {
        "dup"
    } else if node.is_lib {
        "lib"
    } else {
        ""
    };
    let tag = if tag_text.is_empty() {
        String::new()
    } else if color {
        format!("  \x1b[2m{}\x1b[0m", tag_text)
    } else {
        format!("  {}", tag_text)
    };
    eprintln!("{}{}", line, tag);
    let _ = std::io::stderr().flush();

    if !node.is_dup {
        let cp = if is_root {
            String::new()
        } else if is_last {
            format!("{}   ", prefix)
        } else {
            if color {
                format!("{}\x1b[2m│\x1b[0m  ", prefix)
            } else {
                format!("{}│  ", prefix)
            }
        };
        for (i, child) in node.children.iter().enumerate() {
            render_node(child, &cp, false, i == node.children.len() - 1, color);
        }
    }
}

// ── BuildProgress ─────────────────────────────────────────────────────────────

pub struct BuildProgress {
    pub is_tty: bool,
    pub silent: bool,
    no_unicode: bool,
    no_progress: bool,
    current_label: String,
    build_started: Instant,
    stage_started: Instant,
}

impl BuildProgress {
    pub fn new(silent: bool, no_color: bool, no_unicode: bool, no_progress: bool) -> Self {
        Self {
            is_tty: !no_color && stderr_is_tty(),
            silent,
            no_unicode,
            no_progress,
            current_label: String::new(),
            build_started: Instant::now(),
            stage_started: Instant::now(),
        }
    }

    fn elapsed_label(duration: Duration) -> String {
        if duration.as_secs() >= 1 {
            format!("{:.2}s", duration.as_secs_f64())
        } else {
            format!("{}ms", duration.as_millis())
        }
    }

    /// Print the compact build header: `quazi  ›  input  →  output`.
    pub fn header(&self, input: &str, output: &str) {
        if self.silent || self.no_progress {
            return;
        }
        if self.no_unicode {
            eprintln!("\nquazi  >  {}  ->  {}\n", input, output);
            return;
        }
        if self.is_tty {
            eprintln!(
                "\n  \x1b[1;36mquazi\x1b[0m  \x1b[2m›\x1b[0m  \x1b[1m{}\x1b[0m  \x1b[2m→\x1b[0m  \x1b[1m{}\x1b[0m\n",
                input, output
            );
        } else {
            eprintln!("\nquazi  ›  {}  →  {}\n", input, output);
        }
    }

    /// Print "  ⠋ label" (no newline) — overwritable by done/fail.
    pub fn begin(&mut self, label: &str) {
        self.current_label = label.to_string();
        self.stage_started = Instant::now();
        if self.silent || self.no_progress {
            return;
        }
        if self.is_tty && !self.no_unicode {
            eprint!("  \x1b[2m{}\x1b[0m  \x1b[1m{}\x1b[0m", SPIN[0], label);
            let _ = std::io::stderr().flush();
        }
    }

    /// Overwrite spinner with "  ✓  label  ·  info".
    pub fn done(&mut self, info: &str) {
        if self.silent || self.no_progress {
            return;
        }
        let elapsed = Self::elapsed_label(self.stage_started.elapsed());
        let info = if self.no_unicode {
            info.replace('·', "|")
        } else {
            info.to_string()
        };
        let details = if info.is_empty() {
            elapsed
        } else {
            format!("{} · {}", info, elapsed)
        };
        if self.no_unicode {
            let label = self.current_label.replace('·', ".");
            eprintln!("[ok] {:<12}  {}", label, details.replace('·', "|"));
        } else if self.is_tty {
            eprintln!(
                "\r\x1b[K  \x1b[32m◆\x1b[0m  \x1b[1m{:<12}\x1b[0m  \x1b[2m{}\x1b[0m",
                self.current_label, details
            );
        } else {
            eprintln!("ok  {:<12}  {}", self.current_label, details);
        }
    }

    /// Overwrite spinner with "  ✗  label  ·  info".
    pub fn fail(&mut self, info: &str) {
        if self.silent || self.no_progress {
            return;
        }
        if self.no_unicode {
            let label = self.current_label.replace('·', ".");
            if info.is_empty() {
                eprintln!("[fail] {}", label);
            } else {
                eprintln!("[fail] {}  -  {}", label, info.replace('·', "|"));
            }
        } else if self.is_tty {
            if info.is_empty() {
                eprintln!(
                    "\r\x1b[K  \x1b[31m◆\x1b[0m  \x1b[1m{}\x1b[0m",
                    self.current_label
                );
            } else {
                eprintln!(
                    "\r\x1b[K  \x1b[31m◆\x1b[0m  \x1b[1m{}\x1b[0m  \x1b[2m·\x1b[0m  \x1b[2m{}\x1b[0m",
                    self.current_label, info
                );
            }
        } else if info.is_empty() {
            eprintln!("◆  {}", self.current_label);
        } else {
            eprintln!("◆  {}  ·  {}", self.current_label, info);
        }
    }

    /// Render the dependency tree below the last ✓ line.
    pub fn dep_tree(&self, root: &TreeNode) {
        if self.silent || self.no_progress {
            return;
        }
        eprintln!();
        if self.is_tty {
            eprint!("\x1b[?25l"); // hide cursor during animation
            let _ = std::io::stderr().flush();
        }
        if self.no_unicode {
            render_ascii_node(root, "", true, true);
        } else {
            render_node(root, "", true, true, self.is_tty);
        }
        eprintln!();
        if self.is_tty {
            eprint!("\x1b[?25h");
            let _ = std::io::stderr().flush();
        }
    }

    /// Print "  ✓  output  (N KB)" final line.
    pub fn success(&self, output: &str, size_bytes: Option<u64>) {
        if self.silent {
            return;
        }
        let name = Path::new(output)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(output);
        let elapsed = Self::elapsed_label(self.build_started.elapsed());
        if self.no_progress {
            eprintln!("built {}", name);
            return;
        }
        eprintln!();
        if self.no_unicode {
            match size_bytes {
                Some(bytes) => eprintln!(
                    "[built] {}  {:.1} KB | {}",
                    name,
                    bytes as f64 / 1024.0,
                    elapsed
                ),
                None => eprintln!("[built] {}  {}", name, elapsed),
            }
        } else if self.is_tty {
            match size_bytes {
                Some(b) => eprintln!(
                    "  \x1b[30;42m built \x1b[0m  \x1b[1m{}\x1b[0m  \x1b[2m{:.1} KB · {}\x1b[0m",
                    name,
                    b as f64 / 1024.0,
                    elapsed
                ),
                None => eprintln!(
                    "  \x1b[30;42m built \x1b[0m  \x1b[1m{}\x1b[0m  \x1b[2m{}\x1b[0m",
                    name, elapsed
                ),
            }
        } else {
            match size_bytes {
                Some(b) => eprintln!("built  {}  {:.1} KB  {}", name, b as f64 / 1024.0, elapsed),
                None => eprintln!("built  {}  {}", name, elapsed),
            }
        }
        eprintln!();
    }
}

fn render_ascii_node(node: &TreeNode, prefix: &str, is_root: bool, is_last: bool) {
    let line = if is_root {
        format!("  {}", node.label)
    } else {
        let connector = if is_last { "`--" } else { "|--" };
        format!("  {}{} {}", prefix, connector, node.label)
    };
    let tag = if node.is_dup {
        "  dup"
    } else if node.is_lib {
        "  lib"
    } else {
        ""
    };
    eprintln!("{}{}", line, tag);
    if !node.is_dup {
        let child_prefix = if is_root {
            String::new()
        } else if is_last {
            format!("{}    ", prefix)
        } else {
            format!("{}|   ", prefix)
        };
        for (index, child) in node.children.iter().enumerate() {
            render_ascii_node(
                child,
                &child_prefix,
                false,
                index == node.children.len() - 1,
            );
        }
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
