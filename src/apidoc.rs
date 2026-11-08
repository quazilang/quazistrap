// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! `qz doc`: Markdown reference pages for the public declarations of a project.
//!
//! Each source file becomes one page listing its public functions, types,
//! traits, aliases and public methods. A declaration's documentation is the
//! block of `//` comment lines directly above it (attributes may sit between
//! the comment and the declaration). Signatures are copied from the source, so
//! the page always matches the code it was generated from.

use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::parser::ast::{Item, ItemKind, Span};

/// One documented declaration.
struct Entry {
    heading: String,
    signature: String,
    doc: Vec<String>,
}

/// Render the reference page for one module, or the parse error.
pub fn render_module(module: &str, source: &str) -> Result<String, String> {
    let program = Parser::new_with_source(Lexer::new(source).tokenize(), source).parse()?;
    let chars: Vec<char> = source.chars().collect();
    let lines: Vec<&str> = source.lines().collect();
    let mut entries = Vec::new();
    for item in &program.items {
        collect(item, &chars, &lines, &mut entries);
    }
    let mut page = format!("# `{module}`\n\n");
    if entries.is_empty() {
        page.push_str("This module has no public declarations.\n");
        return Ok(page);
    }
    for entry in entries {
        page.push_str(&format!(
            "## `{}`\n\n```quazi\n{}\n```\n\n",
            entry.heading, entry.signature
        ));
        if !entry.doc.is_empty() {
            page.push_str(&entry.doc.join("\n"));
            page.push_str("\n\n");
        }
    }
    Ok(page)
}

fn collect(item: &Item, chars: &[char], lines: &[&str], entries: &mut Vec<Entry>) {
    let entry = |heading: String, whole: bool| Entry {
        heading,
        signature: signature(chars, item.span, whole),
        doc: doc_comment(lines, item.span),
    };
    match &item.node {
        ItemKind::Fn {
            name, pub_fn: true, ..
        } => entries.push(entry(name.clone(), false)),
        ItemKind::Struct {
            name, public: true, ..
        }
        | ItemKind::Enum {
            name, public: true, ..
        }
        | ItemKind::Trait {
            name, public: true, ..
        }
        | ItemKind::TypeAlias {
            name, public: true, ..
        } => entries.push(entry(name.clone(), true)),
        ItemKind::Impl {
            for_ty, methods, ..
        } => {
            let owner = for_ty.node.to_string();
            for method in methods {
                if let ItemKind::Fn {
                    name, pub_fn: true, ..
                } = &method.node
                {
                    entries.push(Entry {
                        heading: format!("{owner}.{name}"),
                        signature: signature(chars, method.span, false),
                        doc: doc_comment(lines, method.span),
                    });
                }
            }
        }
        _ => {}
    }
}

/// Source text of a declaration. Functions stop before their body; other
/// declarations (`whole`) keep their complete text, such as struct fields.
fn signature(chars: &[char], span: Span, whole: bool) -> String {
    let end = span.end.min(chars.len());
    let start = span.start.min(end);
    let text: String = chars[start..end].iter().collect();
    let text = if whole {
        text
    } else {
        match text.find('{') {
            Some(body) => text[..body].to_string(),
            None => text.trim_end_matches(';').to_string(),
        }
    };
    let text = text
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    let text = text.trim();
    // Item spans start after the visibility keyword; only public items are listed.
    if text.starts_with("pub ") {
        text.to_string()
    } else {
        format!("pub {text}")
    }
}

/// The `//` comment block directly above the declaration on `span.line`.
fn doc_comment(lines: &[&str], span: Span) -> Vec<String> {
    let mut index = span.line.saturating_sub(1);
    // Attributes such as `@cfg(...)` may separate the comment from the item.
    while index > 0
        && lines
            .get(index - 1)
            .is_some_and(|line| line.trim_start().starts_with('@'))
    {
        index -= 1;
    }
    let mut doc = Vec::new();
    while index > 0 {
        let line = lines[index - 1].trim_start();
        let Some(text) = line.strip_prefix("//") else {
            break;
        };
        doc.push(
            text.strip_prefix(' ')
                .unwrap_or(text)
                .trim_end()
                .to_string(),
        );
        index -= 1;
    }
    doc.reverse();
    doc
}

#[cfg(test)]
mod tests {
    use super::render_module;

    #[test]
    fn documents_public_items_with_their_comments() {
        let source = "// Adds two numbers.\n// Wraps on overflow.\npub fn add(a: i32, b: i32) i32 {\n    a + b\n}\n\nfn hidden() void {}\n\n// A 2D point.\npub struct Point { x: i32, y: i32, }\n\nimpl Point {\n    // Distance from the origin, squared.\n    pub fn norm(self: &Point) i32 { ret self.x * self.x + self.y * self.y; }\n    fn private(self: &Point) void {}\n}\n";
        let page = render_module("geometry", source).expect("render");
        assert!(page.starts_with("# `geometry`"), "{page}");
        assert!(page.contains("## `add`\n\n```quazi\npub fn add(a: i32, b: i32) i32\n```\n\nAdds two numbers.\nWraps on overflow."), "{page}");
        assert!(
            page.contains("pub struct Point { x: i32, y: i32, }"),
            "{page}"
        );
        assert!(page.contains("## `Point.norm`"), "{page}");
        assert!(
            page.contains("Distance from the origin, squared."),
            "{page}"
        );
        assert!(
            !page.contains("hidden") && !page.contains("private"),
            "{page}"
        );
    }

    #[test]
    fn attributes_between_comment_and_item_keep_the_comment() {
        let source = "// Linux only.\n@cfg(target_os = \"linux\")\npub fn tid() i32 { ret 0; }\n";
        let page = render_module("os", source).expect("render");
        assert!(page.contains("Linux only."), "{page}");
    }
}
