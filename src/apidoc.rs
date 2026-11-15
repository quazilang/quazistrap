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

/// A public type with the public methods implemented for it in this module.
struct TypeSection {
    name: String,
    kind: &'static str,
    entry: Option<Entry>,
    methods: Vec<Entry>,
}

/// Render the reference page for one module, or the parse error.
///
/// Layout: `# module`, the module comment, a contents list, `## Functions`
/// with one `###` per free function, then one `##` section per public type
/// (its declaration and comment) with one `###` per method. Methods
/// implemented here for a type declared elsewhere get their own section.
pub fn render_module(module: &str, source: &str) -> Result<String, String> {
    let program = Parser::new_with_source(Lexer::new(source).tokenize(), source).parse()?;
    let chars: Vec<char> = source.chars().collect();
    let lines: Vec<&str> = source.lines().collect();
    let mut constants = Vec::new();
    let mut functions = Vec::new();
    let mut types: Vec<TypeSection> = Vec::new();
    for item in &program.items {
        collect(item, &chars, &lines, &mut constants, &mut functions, &mut types);
    }
    // A struct with methods is used through them: its fields are internal.
    for section in &mut types {
        if section.kind == "struct"
            && !section.methods.is_empty()
            && let Some(entry) = &mut section.entry
            && let Some(open) = entry.signature.find('{')
        {
            entry.signature = format!("{}{{ /* internal fields */ }}", &entry.signature[..open]);
        }
    }
    let mut page = format!("# `{module}`\n\n");
    let intro = module_comment(&lines);
    if !intro.is_empty() {
        page.push_str(&intro.join("\n"));
        page.push_str("\n\n");
    }
    if constants.is_empty() && functions.is_empty() && types.is_empty() {
        page.push_str("This module has no public declarations.\n");
        return Ok(page);
    }
    page.push_str("## Contents\n\n");
    if !constants.is_empty() {
        page.push_str("- [Constants](#constants)\n");
    }
    if !functions.is_empty() {
        page.push_str("- [Functions](#functions)\n");
    }
    for section in &types {
        page.push_str(&format!(
            "- [`{}`](#{}) ({})\n",
            section.name,
            anchor(&section.name),
            section.kind
        ));
    }
    page.push('\n');
    if !constants.is_empty() {
        page.push_str("## Constants\n\n");
        for entry in &constants {
            render_entry(&mut page, "###", entry);
        }
    }
    if !functions.is_empty() {
        page.push_str("## Functions\n\n");
        for entry in &functions {
            render_entry(&mut page, "###", entry);
        }
    }
    for section in &types {
        page.push_str(&format!("## `{}`\n\n", section.name));
        if let Some(entry) = &section.entry {
            render_body(&mut page, entry);
        }
        for method in &section.methods {
            render_entry(&mut page, "###", method);
        }
    }
    Ok(page.trim_end().to_string() + "\n")
}

fn render_entry(page: &mut String, level: &str, entry: &Entry) {
    page.push_str(&format!("{level} `{}`\n\n", entry.heading));
    render_body(page, entry);
}

fn render_body(page: &mut String, entry: &Entry) {
    page.push_str(&format!("```quazi\n{}\n```\n\n", entry.signature));
    if entry.doc.is_empty() {
        page.push_str("_Undocumented._\n\n");
    } else {
        page.push_str(&entry.doc.join("\n"));
        page.push_str("\n\n");
    }
}

/// GitHub-style heading anchor of a code-span heading.
fn anchor(name: &str) -> String {
    name.chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                Some(c.to_ascii_lowercase())
            } else if c == ' ' {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

/// The leading `//` comment of the file after the license header.
fn module_comment(lines: &[&str]) -> Vec<String> {
    let mut doc = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        let Some(text) = trimmed.strip_prefix("//") else {
            if trimmed.is_empty() && doc.is_empty() {
                continue;
            }
            break;
        };
        let text = text.strip_prefix(' ').unwrap_or(text).trim_end();
        if doc.is_empty()
            && (text.starts_with("Quazi Programming Language")
                || text.starts_with("Copyright")
                || text.starts_with("SPDX-License-Identifier")
                || text.is_empty())
        {
            continue;
        }
        doc.push(text.to_string());
    }
    while doc.last().is_some_and(|line| line.is_empty()) {
        doc.pop();
    }
    doc
}

fn section<'a>(types: &'a mut Vec<TypeSection>, name: &str, kind: &'static str) -> &'a mut TypeSection {
    if let Some(index) = types.iter().position(|section| section.name == name) {
        return &mut types[index];
    }
    types.push(TypeSection {
        name: name.to_string(),
        kind,
        entry: None,
        methods: Vec::new(),
    });
    types.last_mut().expect("pushed section")
}

fn collect(
    item: &Item,
    chars: &[char],
    lines: &[&str],
    constants: &mut Vec<Entry>,
    functions: &mut Vec<Entry>,
    types: &mut Vec<TypeSection>,
) {
    let entry = |heading: String, whole: bool| Entry {
        heading,
        signature: signature(chars, item.span, whole),
        doc: doc_comment(lines, item.span),
    };
    match &item.node {
        // `__name` items are public only for the compiler and runtime.
        ItemKind::Fn { name, .. }
        | ItemKind::Struct { name, .. }
        | ItemKind::TypeAlias { name, .. }
            if name.starts_with("__") => {}
        ItemKind::Fn {
            name, pub_fn: true, ..
        } => functions.push(entry(name.clone(), false)),
        ItemKind::Const {
            name, public: true, ..
        } => constants.push(entry(name.clone(), true)),
        ItemKind::Struct {
            name, public: true, ..
        } => {
            let declared = entry(name.clone(), true);
            let target = section(types, name, "struct");
            target.kind = "struct";
            target.entry = Some(declared);
        }
        ItemKind::Enum {
            name, public: true, ..
        } => {
            let declared = entry(name.clone(), true);
            let target = section(types, name, "enum");
            target.kind = "enum";
            target.entry = Some(declared);
        }
        ItemKind::Trait {
            name, public: true, ..
        } => {
            let declared = entry(name.clone(), true);
            let target = section(types, name, "trait");
            target.kind = "trait";
            target.entry = Some(declared);
        }
        ItemKind::TypeAlias {
            name, public: true, ..
        } => {
            let declared = entry(name.clone(), true);
            let target = section(types, name, "type alias");
            target.kind = "type alias";
            target.entry = Some(declared);
        }
        ItemKind::Impl {
            for_ty, methods, ..
        } => {
            let owner = for_ty.node.to_string();
            let owner_name = owner.split('[').next().unwrap_or(&owner).to_string();
            let mut public = Vec::new();
            for method in methods {
                if let ItemKind::Fn {
                    name, pub_fn: true, ..
                } = &method.node
                    && !name.starts_with("__")
                {
                    public.push(Entry {
                        heading: format!("{owner_name}.{name}"),
                        signature: signature(chars, method.span, false),
                        doc: doc_comment(lines, method.span),
                    });
                }
            }
            if !public.is_empty() {
                section(types, &owner_name, "methods").methods.extend(public);
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
        assert!(page.contains("## Functions\n\n### `add`\n\n```quazi\npub fn add(a: i32, b: i32) i32\n```\n\nAdds two numbers.\nWraps on overflow."), "{page}");
        assert!(page.contains("- [`Point`](#point) (struct)"), "{page}");
        assert!(
            page.contains("## `Point`\n\n```quazi\npub struct Point { /* internal fields */ }\n```\n\nA 2D point."),
            "{page}"
        );
        assert!(page.contains("### `Point.norm`"), "{page}");
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
    fn module_comment_follows_the_license_header() {
        let source = "// Quazi Programming Language\n// Copyright (c) 2026 quazilang\n// SPDX-License-Identifier: 0BSD\n\n// Directory listing.\n// Second line.\nimport std.core;\n\npub struct Data { value: i32, }\n";
        let page = render_module("std.fs", source).expect("render");
        assert!(page.starts_with("# `std.fs`\n\nDirectory listing.\nSecond line.\n\n"), "{page}");
        assert!(page.contains("pub struct Data { value: i32, }"), "{page}");
        assert!(page.contains("_Undocumented._"), "{page}");
    }

    #[test]
    fn attributes_between_comment_and_item_keep_the_comment() {
        let source = "// Linux only.\n@cfg(target_os = \"linux\")\npub fn tid() i32 { ret 0; }\n";
        let page = render_module("os", source).expect("render");
        assert!(page.contains("Linux only."), "{page}");
    }
}
