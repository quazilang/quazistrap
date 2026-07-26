// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::HashSet;

use crate::parser::ast::Program;
use crate::semantic::{Analyzer, SemanticReport, SourceFile};

pub fn analyze_program(
    src: &str,
    program: &Program,
    library_fn_names: HashSet<String>,
    library_char_ranges: Vec<std::ops::Range<usize>>,
) -> SemanticReport {
    analyze_program_with_source_files(
        src,
        program,
        library_fn_names,
        library_char_ranges,
        Vec::new(),
        HashSet::new(),
    )
}

pub fn analyze_program_with_source_files(
    _src: &str,
    program: &Program,
    library_fn_names: HashSet<String>,
    library_char_ranges: Vec<std::ops::Range<usize>>,
    source_files: Vec<SourceFile>,
    namespaced_paths: HashSet<String>,
) -> SemanticReport {
    let mut analyzer = Analyzer::new();
    analyzer.set_library_fns(library_fn_names);
    analyzer.set_library_char_ranges(library_char_ranges);
    analyzer.set_source_files(source_files);
    analyzer.set_namespaced_paths(namespaced_paths);
    analyzer.analyze_program(program)
}

pub fn format_quazi_source(input: &str) -> String {
    let lines: Vec<String> = input
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}
