// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use crate::parser::ast::*;

#[derive(Debug, Clone)]
pub struct ExpandedFormatArgs {
    pub clean_template: String,
    pub args: Vec<Expr>,
    pub specs: Vec<String>,
}

/// Parse `{}` and `{expr}` / `{expr:spec}` placeholders in a format template string.
///
/// Positional `{}` placeholders consume explicit trailing arguments in order.
/// Named/expression `{name}`, `{x:X}`, `{a + b}` placeholders are parsed from
/// the string content and given file-accurate spans so that semantic analysis and
/// codegen type-map lookups produce the correct result.
///
/// NOTE: span arithmetic assumes the template is an unescaped or minimally-escaped
/// double-quoted string. Each byte in the decoded `s` maps to
/// `template_span.start + 1 + byte_offset` in the file (the +1 skips the `"`).
/// Escape sequences that compress multiple source bytes into one decoded byte will
/// make inner-placeholder spans slightly inaccurate, which is acceptable for the
/// initial implementation.
pub fn expand_format_call_args(args: &[Expr]) -> Option<ExpandedFormatArgs> {
    if args.is_empty() {
        return None;
    }
    let (s, template_span) = match &args[0].node {
        ExprKind::Literal(Literal::String(s)) => (s.as_str(), args[0].span),
        _ => return None,
    };

    // File offset of the first byte of the string content (past the opening `"`).
    let file_base: usize = template_span.start + 1;

    let explicit_args = &args[1..];
    let bytes = s.as_bytes();
    let mut i = 0;
    // NOTE: this buffer holds raw UTF-8 *bytes*, not chars. The template may
    // contain multi-byte UTF-8 sequences (box-drawing chars, Cyrillic, emoji,
    // etc.) outside of `{...}` placeholders, and those bytes must be copied
    // through verbatim rather than reinterpreted one byte at a time as a
    // `char` (which corrupts anything above ASCII — see bug report where
    // `╭` etc. turned into `â` mojibake).
    let mut clean_template_bytes: Vec<u8> = Vec::with_capacity(s.len());
    let mut out_args: Vec<Expr> = Vec::new();
    let mut out_specs: Vec<String> = Vec::new();
    let mut pos_arg_idx = 0;

    while i < bytes.len() {
        // `{{` → escaped `{`
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            clean_template_bytes.extend_from_slice(b"{{");
            i += 2;
            continue;
        }
        // `}}` → escaped `}`
        if bytes[i] == b'}' && i + 1 < bytes.len() && bytes[i + 1] == b'}' {
            clean_template_bytes.extend_from_slice(b"}}");
            i += 2;
            continue;
        }
        // Placeholder `{...}`
        if bytes[i] == b'{' {
            let mut end = i + 1;
            while end < bytes.len() && bytes[end] != b'}' {
                end += 1;
            }
            if end < bytes.len() {
                let field = &s[i + 1..end];
                let (name_part, spec_part) = if let Some((n, sp)) = field.split_once(':') {
                    (n.trim(), sp.trim())
                } else {
                    (field.trim(), "")
                };

                if name_part.is_empty() {
                    // Positional placeholder `{}` or `{:spec}` — consume next explicit arg.
                    if pos_arg_idx < explicit_args.len() {
                        out_args.push(explicit_args[pos_arg_idx].clone());
                        out_specs.push(spec_part.to_string());
                        pos_arg_idx += 1;
                    }
                    clean_template_bytes.extend_from_slice(b"{}");
                } else {
                    // Named / expression placeholder `{expr}` or `{expr:spec}`.
                    // Compute real file span for this name_part so that type_map
                    // lookups from semantic analysis use the same key.
                    let name_start_in_s = i + 1;
                    let leading_spaces =
                        field.split_once(':').map(|(n, _)| n).unwrap_or(field).len()
                            - field
                                .split_once(':')
                                .map(|(n, _)| n)
                                .unwrap_or(field)
                                .trim_start()
                                .len();
                    let real_start = file_base + name_start_in_s + leading_spaces;
                    let real_end = real_start + name_part.len();
                    let expr_span = Span::new(
                        template_span.line,
                        template_span.col + name_start_in_s + leading_spaces,
                        real_start,
                        real_end,
                    );

                    if let Some(parsed_expr) = parse_inline_format_expr(name_part, expr_span) {
                        out_args.push(parsed_expr);
                        out_specs.push(spec_part.to_string());
                        clean_template_bytes.extend_from_slice(b"{}");
                    } else {
                        // Parse failed — fall back to next positional arg.
                        if pos_arg_idx < explicit_args.len() {
                            out_args.push(explicit_args[pos_arg_idx].clone());
                            out_specs.push(spec_part.to_string());
                            pos_arg_idx += 1;
                        }
                        clean_template_bytes.extend_from_slice(b"{}");
                    }
                }
                i = end + 1;
                continue;
            }
        }
        // Copy the raw byte through as-is (NOT `bytes[i] as char` — that
        // reinterprets the byte as a Latin-1 codepoint and mangles any
        // multi-byte UTF-8 sequence it's part of).
        clean_template_bytes.push(bytes[i]);
        i += 1;
    }

    // `clean_template_bytes` is built entirely out of slices of the original
    // (valid UTF-8) `s` plus ASCII-only literals (`{{`, `}}`, `{}`), so it is
    // guaranteed to be valid UTF-8 itself.
    let clean_template = String::from_utf8(clean_template_bytes)
        .expect("clean_template_bytes is built from valid UTF-8 fragments only");

    Some(ExpandedFormatArgs {
        clean_template,
        args: out_args,
        specs: out_specs,
    })
}

/// Re-parse a short source fragment as an expression and assign it a file-accurate
/// span so that the type_map key `(span.start, span.end)` matches what semantic
/// analysis stored when it visited the original identifier.
pub fn parse_inline_format_expr(src: &str, file_span: Span) -> Option<Expr> {
    let tokens = crate::lexer::Lexer::new(src).tokenize();
    let mut parser = crate::parser::Parser::new(tokens);
    match parser.parse_expr() {
        Ok(expr) => {
            // Rewrite the top-level span to the real file position.
            // Sub-expression spans (e.g. in binary ops) remain relative to `src`;
            // that is acceptable — only the top-level span is used for coercion.
            Some(respan_expr(expr, file_span))
        }
        Err(_) => None,
    }
}

/// Replace the span of an expression's top node with `new_span`.
/// For simple identifiers and literals this is the only span that matters for
/// type-map lookup in codegen. Nested spans in binary ops stay approximate.
fn respan_expr(mut expr: Expr, new_span: Span) -> Expr {
    expr.span = new_span;
    expr
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_span(start: usize, end: usize) -> Span {
        Span::new(1, 1, start, end)
    }

    fn str_lit_at(s: &str, start: usize) -> Expr {
        let span = make_span(start, start + s.len() + 2); // +2 for quotes
        Spanned::new(ExprKind::Literal(Literal::String(s.to_string())), span)
    }

    fn str_lit(s: &str) -> Expr {
        str_lit_at(s, 0)
    }

    fn ident_expr(s: &str) -> Expr {
        Spanned::new(ExprKind::Ident(s.to_string()), make_span(0, s.len()))
    }

    #[test]
    fn test_expand_format_inline_ident() {
        // Template: "hello {name}" at file offset 5 (i.e. `"hello {name}"` starts at byte 5)
        // The opening `"` is at byte 5, content starts at 6.
        // `{name}` starts at byte 6+6=12 in content (after "hello ").
        // name_part "name" starts at 12+1=13, length 4 → file bytes 13..17.
        let template = str_lit("hello {name}");
        let args = vec![template];
        let expanded = expand_format_call_args(&args).unwrap();
        assert_eq!(expanded.clean_template, "hello {}");
        assert_eq!(expanded.args.len(), 1);
        assert_eq!(expanded.specs, vec![""]);
        if let ExprKind::Ident(name) = &expanded.args[0].node {
            assert_eq!(name, "name");
        } else {
            panic!("expected ident, got {:?}", expanded.args[0].node);
        }
    }

    #[test]
    fn test_expand_format_inline_spec() {
        let args = vec![str_lit("val: {x:X}")];
        let expanded = expand_format_call_args(&args).unwrap();
        assert_eq!(expanded.clean_template, "val: {}");
        assert_eq!(expanded.args.len(), 1);
        assert_eq!(expanded.specs, vec!["X"]);
    }

    #[test]
    fn test_expand_format_mixed_positional_and_inline() {
        let args = vec![str_lit("a = {}, b = {y}"), ident_expr("x")];
        let expanded = expand_format_call_args(&args).unwrap();
        assert_eq!(expanded.clean_template, "a = {}, b = {}");
        assert_eq!(expanded.args.len(), 2);
        assert_eq!(expanded.specs, vec!["", ""]);
        // First arg is positional x
        if let ExprKind::Ident(n) = &expanded.args[0].node {
            assert_eq!(n, "x");
        } else {
            panic!("expected x ident");
        }
        // Second arg is inline y
        if let ExprKind::Ident(n) = &expanded.args[1].node {
            assert_eq!(n, "y");
        } else {
            panic!("expected y ident");
        }
    }

    #[test]
    fn test_expand_format_only_template_no_args() {
        let args = vec![str_lit("no placeholders here")];
        let expanded = expand_format_call_args(&args).unwrap();
        assert_eq!(expanded.clean_template, "no placeholders here");
        assert!(expanded.args.is_empty());
    }

    #[test]
    fn test_expand_format_escaped_braces() {
        let args = vec![str_lit("{{}} {x}")];
        let expanded = expand_format_call_args(&args).unwrap();
        assert_eq!(expanded.clean_template, "{{}} {}");
        assert_eq!(expanded.args.len(), 1);
    }
}
