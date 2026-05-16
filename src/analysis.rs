// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::HashSet;

use crate::parser::ast::Program;
use crate::semantic::{Analyzer, SemanticReport};

pub fn analyze_program(
    _src: &str,
    program: &Program,
    library_fn_names: HashSet<String>,
    library_char_ranges: Vec<std::ops::Range<usize>>,
) -> SemanticReport {
    let mut analyzer = Analyzer::new();
    analyzer.set_library_fns(library_fn_names);
    analyzer.set_library_char_ranges(library_char_ranges);
    analyzer.analyze_program(program)
}

pub fn format_void_source(input: &str) -> String {
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
