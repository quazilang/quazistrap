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

/// Whether calls to a function with these parameters follow format-call
/// rules (template literal, display-converted arguments): an `@format`
/// function, or one whose final `...args: str` (or reference) follows a fixed
/// `str` parameter. A `...values: str` without one is an ordinary typed
/// variadic.
pub fn is_format_variadic(params: &[Param], attributes: &[Attribute]) -> bool {
    let Some(last) = params.last().filter(|param| param.variadic) else {
        return false;
    };
    let explicit = attributes.iter().any(|attribute| attribute.name == "format");
    match &last.ty.node {
        TypeKind::Any => explicit,
        TypeKind::Str | TypeKind::Ref { .. } | TypeKind::MutRef { .. } => {
            explicit
                || params[..params.len() - 1]
                    .iter()
                    .any(|param| matches!(param.ty.node, TypeKind::Str))
        }
        _ => false,
    }
}

/// Parse `{}` and `{expr}` / `{expr:spec}` placeholders in a format template string.
///
/// Positional `{}` placeholders consume explicit trailing arguments in order.
/// Named/expression `{name}`, `{x:X}`, `{a + b}` placeholders are parsed from
/// the string content and given file-accurate spans so that semantic analysis and
/// codegen type-map lookups produce the correct result.
///
/// NOTE: placeholder spans count Unicode scalars of the decoded string from the
/// opening quote, which is exact only when no escape sequence precedes them;
/// after `\n` or `\u{..}` they point a few characters too far left (still
/// inside the literal). Consumers that edit text at these spans (rename) must
/// check the text they replace.
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
                    // lookups from semantic analysis use the same key. Spans
                    // count Unicode scalar values, not bytes.
                    let name_start_in_s = s[..i + 1].chars().count();
                    let leading_spaces =
                        field.split_once(':').map(|(n, _)| n).unwrap_or(field).len()
                            - field
                                .split_once(':')
                                .map(|(n, _)| n)
                                .unwrap_or(field)
                                .trim_start()
                                .len();
                    let real_start = file_base + name_start_in_s + leading_spaces;
                    let real_end = real_start + name_part.chars().count();
                    // Lines and columns follow literal line breaks. With
                    // escapes the decoded text no longer matches the source
                    // (an escaped `\n` is not a line break), so positions
                    // stay on the literal's first line.
                    let prefix = &s[..i + 1];
                    let exact = s.chars().count() + 2
                        == template_span.end.saturating_sub(template_span.start);
                    let (line, col) = match prefix.rfind('\n').filter(|_| exact) {
                        Some(newline) => (
                            template_span.line + prefix.matches('\n').count(),
                            prefix[newline + 1..].chars().count() + leading_spaces + 1,
                        ),
                        None => (
                            template_span.line,
                            template_span.col + name_start_in_s + leading_spaces,
                        ),
                    };
                    let expr_span = Span::new(
                        line,
                        col,
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

/// Re-parse a short source fragment as an expression with file-accurate spans:
/// semantic analysis and codegen key types and resolutions by
/// `(span.start, span.end)`, so every nested expression must sit inside the
/// literal rather than at fragment-relative offsets that belong to other code.
pub fn parse_inline_format_expr(src: &str, file_span: Span) -> Option<Expr> {
    let mut tokens = crate::lexer::Lexer::new(src).tokenize();
    for token in &mut tokens {
        token.span.line = file_span.line;
        token.span.col = file_span.col + token.span.start;
        token.span.start += file_span.start;
        token.span.end += file_span.start;
    }
    let mut parser = crate::parser::Parser::new(tokens);
    match parser.parse_expr() {
        // The top-level span covers the whole trimmed placeholder text.
        Ok(expr) => Some(respan_expr(expr, file_span)),
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
    use super::{StringLiteralForm, classify_string_literal};

    #[test]
    fn placeholder_subexpressions_keep_file_spans() {
        let span = crate::parser::ast::Span::new(3, 20, 100, 109);
        let expr = super::parse_inline_format_expr("grown < 5", span).expect("expression");
        assert_eq!((expr.span.start, expr.span.end), (100, 109));
        let crate::parser::ast::ExprKind::Binary { left, right, .. } = &expr.node else {
            panic!("binary placeholder");
        };
        // Fragment-relative spans (0..5) would alias unrelated code.
        assert_eq!((left.span.start, left.span.end), (100, 105));
        assert_eq!((right.span.start, right.span.end), (108, 109));
    }

    #[test]
    fn literals_classify_under_d020() {
        assert_eq!(classify_string_literal("plain"), StringLiteralForm::Plain("plain".into()));
        assert_eq!(classify_string_literal("a {{b}} c"), StringLiteralForm::Plain("a {b} c".into()));
        assert_eq!(classify_string_literal("v = {x}"), StringLiteralForm::Interpolated);
        assert_eq!(classify_string_literal("{x:X} {{"), StringLiteralForm::Interpolated);
        for invalid in ["{", "}", "a {} b", "{:X}", "x}"] {
            assert!(
                matches!(classify_string_literal(invalid), StringLiteralForm::Invalid(_)),
                "{invalid}"
            );
        }
    }

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

/// How an ordinary string literal reads under D-020 (every literal is a
/// format string; `{{`/`}}` are literal braces).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StringLiteralForm {
    /// No placeholders: the text with `{{`/`}}` reduced to single braces.
    Plain(String),
    /// At least one `{expr}` placeholder: formats to an owned `String`.
    Interpolated,
    /// Unmatched brace or an argument-less `{}` placeholder.
    Invalid(String),
}

/// Classify a literal outside a format call's template position.
pub fn classify_string_literal(text: &str) -> StringLiteralForm {
    let bytes = text.as_bytes();
    let mut plain = Vec::with_capacity(bytes.len());
    let mut interpolated = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'{' if bytes.get(index + 1) == Some(&b'{') => {
                plain.push(b'{');
                index += 2;
            }
            b'}' if bytes.get(index + 1) == Some(&b'}') => {
                plain.push(b'}');
                index += 2;
            }
            b'{' => {
                let Some(length) = bytes[index + 1..].iter().position(|byte| *byte == b'}') else {
                    return StringLiteralForm::Invalid(
                        "unmatched `{` in string literal; write `{{` for a literal brace".into(),
                    );
                };
                let field = &text[index + 1..index + 1 + length];
                let name = field.split_once(':').map_or(field, |(name, _)| name).trim();
                if name.is_empty() {
                    return StringLiteralForm::Invalid(format!(
                        "`{{{field}}}` has no argument outside a format call; name a value (`{{x}}`) or write `{{{{` and `}}}}` for literal braces"
                    ));
                }
                interpolated = true;
                index += length + 2;
            }
            b'}' => {
                return StringLiteralForm::Invalid(
                    "unmatched `}` in string literal; write `}}` for a literal brace".into(),
                );
            }
            byte => {
                plain.push(byte);
                index += 1;
            }
        }
    }
    if interpolated {
        StringLiteralForm::Interpolated
    } else {
        StringLiteralForm::Plain(String::from_utf8(plain).unwrap_or_default())
    }
}
