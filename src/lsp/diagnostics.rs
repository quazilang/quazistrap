// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

use crate::semantic::SemanticReport;

use super::span::span_to_range;

pub fn to_lsp_diagnostics(report: &SemanticReport, source: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();

    for err in &report.errors {
        out.push(Diagnostic {
            range: span_to_range(err.span, source),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(err.code.to_string())),
            source: Some("quazilang".to_string()),
            message: strip_ansi(&err.message),
            ..Default::default()
        });
    }

    for warn in &report.warnings {
        let msg = if warn.suggestions.is_empty() {
            warn.message.clone()
        } else {
            format!("{}\n  hint: {}", warn.message, warn.suggestions.join("; "))
        };
        out.push(Diagnostic {
            range: span_to_range(warn.span, source),
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String(warn.code.to_string())),
            source: Some("quazilang".to_string()),
            message: strip_ansi(&msg),
            ..Default::default()
        });
    }

    out
}

pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            out.push(ch);
            continue;
        }

        if chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }

    out
}

pub fn parse_error_range(rendered: &str, source: &str) -> Range {
    let plain = strip_ansi(rendered);
    let Some((line, col)) = parse_rendered_location(&plain) else {
        return Range::new(Position::new(0, 0), Position::new(0, 1));
    };

    let line_idx = line.saturating_sub(1);
    let char_idx = col.saturating_sub(1);
    let max_line = source.lines().count().saturating_sub(1) as u32;
    let line_idx = line_idx.min(max_line);
    let line_len = source
        .lines()
        .nth(line_idx as usize)
        .map(|line| line.chars().count() as u32)
        .unwrap_or(0);
    let char_idx = char_idx.min(line_len);
    let end = if char_idx < line_len {
        char_idx + 1
    } else {
        char_idx
    };

    Range::new(
        Position::new(line_idx, char_idx),
        Position::new(line_idx, end),
    )
}

fn parse_rendered_location(message: &str) -> Option<(u32, u32)> {
    let marker = "-->";
    let start = message.find(marker)? + marker.len();
    let rest = message[start..].trim_start();
    let location = rest.split_whitespace().next()?;
    let (line, col) = location.split_once(':')?;
    Some((line.parse().ok()?, col.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::{parse_error_range, strip_ansi};

    #[test]
    fn strips_ansi_escape_sequences() {
        let input = "\x1b[1;31merror\x1b[0m\x1b[1m[E02]\x1b[0m: expected fn";
        assert_eq!(strip_ansi(input), "error[E02]: expected fn");
    }

    #[test]
    fn parse_error_range_uses_rendered_location() {
        let source = "fn main() void {\n    ret;\n}\n}";
        let input = "\x1b[1;31merror\x1b[0m[E01]: expected identifier\n  \x1b[1;34m-->\x1b[0m 4:1";
        let range = parse_error_range(input, source);
        assert_eq!(range.start.line, 3);
        assert_eq!(range.start.character, 0);
    }
}
