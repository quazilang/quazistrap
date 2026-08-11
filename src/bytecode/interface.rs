// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::parser::ast::{AttrArg, AttrVal, Attribute, Item, ItemKind, Param, Program};
use crate::semantic::SourceFile;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QziInterfaceBundle {
    pub modules: Vec<QziInterfaceModule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QziInterfaceModule {
    pub name: String,
    pub exports: Vec<String>,
    pub source: String,
}

pub fn build_qzi_interface(
    package_name: &str,
    program: &Program,
    source_files: &[SourceFile],
    namespaced_paths: &HashSet<PathBuf>,
    excluded_paths: &HashSet<PathBuf>,
) -> Result<String, String> {
    if !is_quazi_identifier(package_name) {
        return Err(format!(
            "QZI library name `{package_name}` must be a Quazi identifier"
        ));
    }
    let mut modules: BTreeMap<String, (Vec<String>, String)> = BTreeMap::new();
    for item in &program.items {
        let unsupported_generic = match &item.node {
            ItemKind::Fn {
                name,
                generic_params,
                pub_fn: true,
                ..
            }
            | ItemKind::Struct {
                name,
                generic_params,
                public: true,
                ..
            }
            | ItemKind::Trait {
                name,
                generic_params,
                public: true,
                ..
            }
            | ItemKind::Enum {
                name,
                generic_params,
                public: true,
                ..
            }
            | ItemKind::TypeAlias {
                name,
                generic_params,
                public: true,
                ..
            } if !generic_params.is_empty() => Some(name),
            _ => None,
        };
        if let Some(name) = unsupported_generic {
            return Err(format!(
                "public generic `{name}` cannot be exported in QZI v6 yet; publish this library as source"
            ));
        }
        let Some((declaration, exports)) = render_public_item(item) else {
            continue;
        };
        let source_path = source_files
            .iter()
            .find(|source| source.contains(item.span))
            .map(|source| PathBuf::from(&source.path));
        if source_path
            .as_ref()
            .is_some_and(|path| excluded_paths.contains(path))
        {
            continue;
        }
        let module = source_path
            .as_deref()
            .map(|path| {
                if namespaced_paths.contains(path) {
                    module_name(path).unwrap_or_else(|| package_name.to_string())
                } else {
                    package_name.to_string()
                }
            })
            .unwrap_or_else(|| package_name.to_string());
        let entry = modules.entry(module).or_default();
        entry.0.extend(exports);
        entry.1.push_str(&declaration);
        entry.1.push('\n');
    }
    let modules = modules
        .into_iter()
        .map(|(name, (mut exports, source))| {
            exports.sort();
            exports.dedup();
            QziInterfaceModule {
                name,
                exports,
                source,
            }
        })
        .collect();
    toml::to_string(&QziInterfaceBundle { modules })
        .map_err(|error| format!("cannot serialize QZI interface: {error}"))
}

fn is_quazi_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

pub fn parse_qzi_interface(text: &str) -> Result<QziInterfaceBundle, String> {
    toml::from_str(text).map_err(|error| format!("cannot parse QZI interface: {error}"))
}

fn module_name(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    if stem == "mod" {
        path.parent()?.file_name()?.to_str().map(str::to_string)
    } else {
        Some(stem.to_string())
    }
}

fn render_public_item(item: &Item) -> Option<(String, Vec<String>)> {
    match &item.node {
        ItemKind::Fn {
            name,
            generic_params,
            params,
            return_ty,
            attributes,
            unsafe_fn,
            pub_fn,
            c_variadic,
            ..
        } if *pub_fn => {
            let mut output = render_attributes(attributes);
            output.push_str("pub ");
            if *unsafe_fn {
                output.push_str("unsafe ");
            }
            output.push_str("fn ");
            output.push_str(name);
            render_generic_params(&mut output, generic_params);
            output.push('(');
            render_params(&mut output, params, *c_variadic);
            output.push_str(") ");
            output.push_str(&return_ty.node.to_string());
            output.push_str(";\n");
            Some((output, vec![name.clone()]))
        }
        ItemKind::Struct {
            name,
            generic_params,
            fields,
            bit_widths,
            is_union,
            attributes,
            public,
        } if *public => {
            let mut output = render_attributes(attributes);
            output.push_str(if *is_union {
                "pub union "
            } else {
                "pub struct "
            });
            output.push_str(name);
            render_generic_params(&mut output, generic_params);
            output.push_str(" {\n");
            for (index, (field, ty, is_const)) in fields.iter().enumerate() {
                output.push_str("    ");
                if *is_const {
                    output.push_str("const ");
                }
                output.push_str(field);
                output.push_str(": ");
                output.push_str(&ty.node.to_string());
                if let Some(width) = bit_widths.get(index).and_then(|width| *width) {
                    output.push_str(" : ");
                    output.push_str(&width.to_string());
                }
                output.push_str(",\n");
            }
            output.push_str("}\n");
            Some((output, vec![name.clone()]))
        }
        ItemKind::Trait {
            name,
            generic_params,
            methods,
            attributes,
            public,
        } if *public => {
            let mut output = render_attributes(attributes);
            output.push_str("pub trait ");
            output.push_str(name);
            render_generic_params(&mut output, generic_params);
            output.push_str(" {\n");
            for method in methods {
                output.push_str("    fn ");
                output.push_str(&method.name);
                render_generic_params(&mut output, &method.generic_params);
                output.push('(');
                for (index, ty) in method.params.iter().enumerate() {
                    if index > 0 {
                        output.push_str(", ");
                    }
                    output.push_str("arg");
                    output.push_str(&index.to_string());
                    output.push_str(": ");
                    output.push_str(&ty.node.to_string());
                }
                output.push_str(") ");
                output.push_str(&method.return_ty.node.to_string());
                output.push_str(";\n");
            }
            output.push_str("}\n");
            Some((output, vec![name.clone()]))
        }
        ItemKind::Enum {
            name,
            generic_params,
            variants,
            attributes,
            public,
        } if *public => {
            let mut output = render_attributes(attributes);
            output.push_str("pub enum ");
            output.push_str(name);
            render_generic_params(&mut output, generic_params);
            output.push_str(" {\n");
            for variant in variants {
                output.push_str("    ");
                output.push_str(&variant.name);
                if !variant.payload_types.is_empty() {
                    output.push('(');
                    for (index, ty) in variant.payload_types.iter().enumerate() {
                        if index > 0 {
                            output.push_str(", ");
                        }
                        output.push_str(&ty.node.to_string());
                    }
                    output.push(')');
                }
                output.push_str(",\n");
            }
            output.push_str("}\n");
            Some((output, vec![name.clone()]))
        }
        ItemKind::TypeAlias {
            name,
            generic_params,
            aliased_type,
            attributes,
            public,
        } if *public => {
            let mut output = render_attributes(attributes);
            output.push_str("pub type ");
            output.push_str(name);
            render_generic_params(&mut output, generic_params);
            output.push_str(" = ");
            output.push_str(&aliased_type.node.to_string());
            output.push_str(";\n");
            Some((output, vec![name.clone()]))
        }
        ItemKind::ForeignGlobal {
            name,
            ty,
            attributes,
            public,
        } if *public => {
            let mut output = render_attributes(attributes);
            output.push_str("pub var ");
            output.push_str(name);
            output.push_str(": ");
            output.push_str(&ty.node.to_string());
            output.push_str(";\n");
            Some((output, vec![name.clone()]))
        }
        ItemKind::Impl {
            trait_ty,
            for_ty,
            methods,
        } => {
            let public_methods: Vec<&Item> = methods
                .iter()
                .filter(|method| {
                    matches!(&method.node, ItemKind::Fn { pub_fn: true, .. }) || trait_ty.is_some()
                })
                .collect();
            if public_methods.is_empty() {
                return None;
            }
            let mut output = String::from("impl ");
            if let Some(trait_ty) = trait_ty {
                output.push_str(&trait_ty.node.to_string());
                output.push_str(" for ");
            }
            output.push_str(&for_ty.node.to_string());
            output.push_str(" {\n");
            for method in public_methods {
                if let ItemKind::Fn {
                    name,
                    generic_params,
                    params,
                    return_ty,
                    attributes,
                    unsafe_fn,
                    c_variadic,
                    ..
                } = &method.node
                {
                    for line in render_attributes(attributes).lines() {
                        output.push_str("    ");
                        output.push_str(line);
                        output.push('\n');
                    }
                    output.push_str("    pub ");
                    if *unsafe_fn {
                        output.push_str("unsafe ");
                    }
                    output.push_str("fn ");
                    output.push_str(name);
                    render_generic_params(&mut output, generic_params);
                    output.push('(');
                    render_params(&mut output, params, *c_variadic);
                    output.push_str(") ");
                    output.push_str(&return_ty.node.to_string());
                    output.push_str(";\n");
                }
            }
            output.push_str("}\n");
            Some((output, Vec::new()))
        }
        _ => None,
    }
}

fn render_generic_params(output: &mut String, params: &[String]) {
    if !params.is_empty() {
        output.push('[');
        output.push_str(&params.join(", "));
        output.push(']');
    }
}

fn render_params(output: &mut String, params: &[Param], c_variadic: bool) {
    for (index, param) in params.iter().enumerate() {
        if index > 0 {
            output.push_str(", ");
        }
        output.push_str(&render_attributes(&param.attributes).replace('\n', " "));
        if param.variadic {
            output.push_str("...");
        }
        output.push_str(&param.name);
        output.push_str(": ");
        output.push_str(&param.ty.node.to_string());
    }
    if c_variadic {
        if !params.is_empty() {
            output.push_str(", ");
        }
        output.push_str("...");
    }
}

fn render_attributes(attributes: &[Attribute]) -> String {
    let mut output = String::new();
    for attribute in attributes {
        // Code-generation-only export identity remains in QZI metadata. Keeping
        // it on a bodyless interface declaration would create a second adapter.
        if matches!(attribute.name.as_str(), "export" | "no_mangle" | "inline") {
            continue;
        }
        output.push('@');
        output.push_str(&attribute.name);
        if !attribute.args.is_empty() {
            output.push('(');
            for (index, argument) in attribute.args.iter().enumerate() {
                if index > 0 {
                    output.push_str(", ");
                }
                match argument {
                    AttrArg::Positional(value) => render_attr_value(&mut output, value),
                    AttrArg::KeyValue(key, value) => {
                        output.push_str(key);
                        output.push_str(" = ");
                        render_attr_value(&mut output, value);
                    }
                }
            }
            output.push(')');
        }
        output.push('\n');
    }
    output
}

fn render_attr_value(output: &mut String, value: &AttrVal) {
    match value {
        AttrVal::Str(value) => output.push_str(&format!("{value:?}")),
        AttrVal::Int(value) => output.push_str(&value.to_string()),
        AttrVal::Ident(value) => output.push_str(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    #[test]
    fn public_function_interface_is_parseable_and_hides_body() {
        let source = "pub fn add(a: i32, b: i32) i32 { ret a + b; }\nfn hidden() void {}";
        let mut lexer = Lexer::new(source);
        let mut parser = Parser::new(lexer.tokenize());
        let program = parser.parse().expect("parse source");
        let files = vec![SourceFile {
            path: "src/lib.qz".to_string(),
            start: 0,
            end: source.chars().count(),
            line_start: 1,
        }];
        let encoded =
            build_qzi_interface("math", &program, &files, &HashSet::new(), &HashSet::new())
                .expect("build interface");
        let bundle = parse_qzi_interface(&encoded).expect("parse interface bundle");
        assert_eq!(bundle.modules.len(), 1);
        assert!(bundle.modules[0].source.contains("pub fn add"));
        assert!(!bundle.modules[0].source.contains("ret a"));
        assert!(!bundle.modules[0].source.contains("hidden"));
        let mut lexer = Lexer::new(&bundle.modules[0].source);
        Parser::new(lexer.tokenize())
            .parse()
            .expect("generated interface should parse");
    }

    #[test]
    fn public_type_and_method_interface_is_parseable() {
        let source = r#"
pub struct Point { x: i32, y: i32, }
pub enum Axis { X, Y, }
pub trait Length { fn length(self: Point) i32; }
impl Point { pub fn sum(self: Point) i32 { ret self.x + self.y; } }
pub type Coordinate = i32;
"#;
        let mut lexer = Lexer::new(source);
        let program = Parser::new(lexer.tokenize()).parse().expect("parse source");
        let files = vec![SourceFile {
            path: "src/types.qz".to_string(),
            start: 0,
            end: source.chars().count(),
            line_start: 1,
        }];
        let encoded = build_qzi_interface(
            "geometry",
            &program,
            &files,
            &HashSet::new(),
            &HashSet::new(),
        )
        .expect("build interface");
        let bundle = parse_qzi_interface(&encoded).expect("parse interface bundle");
        assert_eq!(
            bundle.modules[0].exports,
            ["Axis", "Coordinate", "Length", "Point"]
        );
        let mut lexer = Lexer::new(&bundle.modules[0].source);
        Parser::new(lexer.tokenize())
            .parse()
            .expect("generated type interface should parse");
    }

    #[test]
    fn public_generic_requires_source_distribution() {
        let source = "pub fn identity[T](value: T) T { ret value; }";
        let program = Parser::new(Lexer::new(source).tokenize())
            .parse()
            .expect("parse generic");
        let error = build_qzi_interface("generic", &program, &[], &HashSet::new(), &HashSet::new())
            .expect_err("generic QZI API should be rejected");
        assert!(error.contains("publish this library as source"));
    }
}
