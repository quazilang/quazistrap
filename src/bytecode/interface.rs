// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::parser::ast::{AttrArg, AttrVal, Attribute, Item, ItemKind, Param, Program, TypeKind};
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
    // Only a same-source nominal declaration proves a private identity here.
    // Aliases and generic argument spellings need checked semantic resolution;
    // they must never cause silent interface omission.
    let mut private_types: HashMap<Option<usize>, HashSet<&str>> = HashMap::new();
    for item in &program.items {
        let source = source_files
            .iter()
            .position(|file| file.contains(item.span));
        if source.is_none() && !source_files.is_empty() {
            continue;
        }
        if source.is_some_and(|index| excluded_paths.contains(Path::new(&source_files[index].path)))
        {
            continue;
        }
        let name = match &item.node {
            ItemKind::Struct {
                name,
                public: false,
                ..
            }
            | ItemKind::Enum {
                name,
                public: false,
                ..
            }
            | ItemKind::Trait {
                name,
                public: false,
                ..
            } => name,
            _ => continue,
        };
        private_types.entry(source).or_default().insert(name);
    }
    let no_private_types = HashSet::new();
    let mut modules: BTreeMap<String, (Vec<String>, String)> = BTreeMap::new();
    for item in &program.items {
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
        let source = source_files
            .iter()
            .position(|file| file.contains(item.span));
        let Some(interface_item) = private_impl_interface(
            item,
            private_types.get(&source).unwrap_or(&no_private_types),
        )?
        else {
            continue;
        };
        let item = interface_item.as_ref();
        let unsupported_generic = public_item_with_unsupported_generic(item);
        if let Some(name) = unsupported_generic {
            return Err(format!(
                "QZI v7 cannot export public generic `{name}`; depend on this package's sources instead (use a path, git, or archive project dependency), or make `{name}` non-public or non-generic"
            ));
        }
        if let Some(name) = public_item_with_runtime_any(item) {
            return Err(format!(
                "QZI v7 cannot export `{name}` because runtime `any` has no portable representation; publish this package as source or replace `any` with a concrete, generic, or trait type"
            ));
        }
        let Some((declaration, exports)) = render_public_item(item) else {
            continue;
        };
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

fn private_impl_interface<'a>(
    item: &'a Item,
    private_types: &HashSet<&str>,
) -> Result<Option<Cow<'a, Item>>, String> {
    let ItemKind::Impl {
        trait_ty,
        for_ty,
        methods,
    } = &item.node
    else {
        return Ok(Some(Cow::Borrowed(item)));
    };
    fn is_private(ty: &TypeKind, private_types: &HashSet<&str>) -> bool {
        match ty {
            TypeKind::Named { name, .. } => private_types.contains(name.as_str()),
            TypeKind::Ref { inner } | TypeKind::MutRef { inner } => {
                is_private(&inner.node, private_types)
            }
            _ => false,
        }
    }
    let has_public_method = methods
        .iter()
        .any(|method| matches!(method.node, ItemKind::Fn { pub_fn: true, .. }));
    if is_private(&for_ty.node, private_types) {
        if has_public_method {
            return Err(format!(
                "QZI cannot export public methods of private target `{}`; make the target public or depend on this package's sources instead",
                for_ty.node
            ));
        }
        return Ok(None);
    }
    if trait_ty
        .as_ref()
        .is_some_and(|ty| is_private(&ty.node, private_types))
    {
        if !has_public_method {
            return Ok(None);
        }
        let public_methods = methods
            .iter()
            .filter(|method| matches!(method.node, ItemKind::Fn { pub_fn: true, .. }))
            .cloned()
            .collect();
        // Public methods remain callable by Type.method, independently of the
        // inaccessible private trait. Render only that public inherent surface.
        return Ok(Some(Cow::Owned(Item {
            span: item.span,
            node: ItemKind::Impl {
                trait_ty: None,
                for_ty: for_ty.clone(),
                methods: public_methods,
            },
        })));
    }
    Ok(Some(Cow::Borrowed(item)))
}

fn public_item_with_unsupported_generic(item: &Item) -> Option<String> {
    match &item.node {
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
        } if !generic_params.is_empty() => Some(name.clone()),
        ItemKind::Trait {
            name,
            generic_params,
            methods,
            public: true,
            ..
        } => {
            if !generic_params.is_empty() {
                return Some(name.clone());
            }
            methods
                .iter()
                .find(|method| !method.generic_params.is_empty())
                .map(|method| format!("{name}.{}", method.name))
        }
        ItemKind::Impl {
            trait_ty, methods, ..
        } => methods.iter().find_map(|method| match &method.node {
            ItemKind::Fn {
                name,
                generic_params,
                pub_fn,
                ..
            } if (*pub_fn || trait_ty.is_some()) && !generic_params.is_empty() => {
                Some(name.clone())
            }
            _ => None,
        }),
        _ => None,
    }
}

fn public_item_with_runtime_any(item: &Item) -> Option<&str> {
    match &item.node {
        ItemKind::Fn {
            name,
            params,
            return_ty,
            attributes,
            pub_fn: true,
            ..
        } => (fn_params_contain_runtime_any(params, attributes)
            || type_contains_any(&return_ty.node))
        .then_some(name),
        ItemKind::Struct {
            name,
            fields,
            public: true,
            ..
        } => fields
            .iter()
            .any(|field| type_contains_any(&field.ty.node))
            .then_some(name),
        ItemKind::Trait {
            name,
            methods,
            public: true,
            ..
        } => methods
            .iter()
            .any(|method| {
                method.params.iter().any(|ty| type_contains_any(&ty.node))
                    || type_contains_any(&method.return_ty.node)
            })
            .then_some(name),
        ItemKind::Enum {
            name,
            variants,
            public: true,
            ..
        } => variants
            .iter()
            .flat_map(|variant| &variant.payload_types)
            .any(|ty| type_contains_any(&ty.node))
            .then_some(name),
        ItemKind::TypeAlias {
            name,
            aliased_type,
            public: true,
            ..
        } => type_contains_any(&aliased_type.node).then_some(name),
        ItemKind::ForeignGlobal {
            name,
            ty,
            public: true,
            ..
        } => type_contains_any(&ty.node).then_some(name),
        ItemKind::Impl {
            trait_ty,
            for_ty,
            methods,
        } => {
            let unsafe_header = trait_ty
                .as_ref()
                .is_some_and(|ty| type_contains_any(&ty.node))
                || type_contains_any(&for_ty.node);
            let unsafe_method = methods.iter().any(|method| match &method.node {
                ItemKind::Fn {
                    params,
                    return_ty,
                    attributes,
                    pub_fn,
                    ..
                } if *pub_fn || trait_ty.is_some() => {
                    fn_params_contain_runtime_any(params, attributes)
                        || type_contains_any(&return_ty.node)
                }
                _ => false,
            });
            (unsafe_header || unsafe_method).then_some("public impl")
        }
        _ => None,
    }
}

fn fn_params_contain_runtime_any(params: &[Param], attributes: &[Attribute]) -> bool {
    let format_erased = attributes
        .iter()
        .any(|attribute| attribute.name == "format");
    params.iter().enumerate().any(|(index, param)| {
        let allowed_format_tail = format_erased
            && index + 1 == params.len()
            && param.variadic
            && matches!(param.ty.node, TypeKind::Any);
        !allowed_format_tail && type_contains_any(&param.ty.node)
    })
}

fn type_contains_any(ty: &TypeKind) -> bool {
    match ty {
        TypeKind::Any => true,
        TypeKind::Ref { inner } | TypeKind::RawPtr { inner } => type_contains_any(&inner.node),
        TypeKind::Array { elem_ty, .. }
        | TypeKind::FlexibleArray { elem_ty }
        | TypeKind::Slice { elem_ty } => type_contains_any(&elem_ty.node),
        TypeKind::Named { type_args, .. } => {
            type_args.iter().any(|arg| type_contains_any(&arg.node))
        }
        TypeKind::Fn { params, return_ty } | TypeKind::CFn { params, return_ty } => {
            params.iter().any(|param| type_contains_any(&param.node))
                || type_contains_any(&return_ty.node)
        }
        _ => false,
    }
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

pub fn qzi_v6_interface_has_ambiguous_trait_receivers(text: &str) -> Result<bool, String> {
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    let bundle = parse_qzi_interface(text)?;
    for module in bundle.modules {
        let mut lexer = Lexer::new(&module.source);
        let program = Parser::new(lexer.tokenize()).parse().map_err(|error| {
            format!(
                "cannot parse QZI v6 interface module `{}` while checking trait receivers: {error}",
                module.name
            )
        })?;
        if program.items.iter().any(|item| {
            matches!(
                &item.node,
                ItemKind::Trait { methods, .. }
                    if methods.iter().any(|method| !method.params.is_empty())
            )
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn qzi_v6_interface_has_runtime_any(text: &str) -> Result<bool, String> {
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    let bundle = parse_qzi_interface(text)?;
    for module in bundle.modules {
        let mut lexer = Lexer::new(&module.source);
        let program = Parser::new(lexer.tokenize()).parse().map_err(|error| {
            format!(
                "cannot parse QZI v6 interface module `{}` while checking runtime `any`: {error}",
                module.name
            )
        })?;
        if program
            .items
            .iter()
            .any(|item| public_item_with_runtime_any(item).is_some())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn qzi_v6_interface_has_owned_function_values(text: &str) -> Result<bool, String> {
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    let bundle = parse_qzi_interface(text)?;
    for module in bundle.modules {
        let mut lexer = Lexer::new(&module.source);
        let program = Parser::new(lexer.tokenize()).parse().map_err(|error| {
            format!(
                "cannot parse QZI v6 interface module `{}` while checking function-value ownership: {error}",
                module.name
            )
        })?;
        if program.items.iter().any(public_item_contains_owned_fn) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn public_item_contains_owned_fn(item: &Item) -> bool {
    match &item.node {
        ItemKind::Fn {
            params,
            return_ty,
            pub_fn: true,
            ..
        } => {
            params
                .iter()
                .any(|param| type_contains_owned_fn(&param.ty.node))
                || type_contains_owned_fn(&return_ty.node)
        }
        ItemKind::Struct {
            fields,
            public: true,
            ..
        } => fields
            .iter()
            .any(|field| type_contains_owned_fn(&field.ty.node)),
        ItemKind::Trait {
            methods,
            public: true,
            ..
        } => methods.iter().any(|method| {
            method
                .params
                .iter()
                .any(|param| type_contains_owned_fn(&param.node))
                || type_contains_owned_fn(&method.return_ty.node)
        }),
        ItemKind::Enum {
            variants,
            public: true,
            ..
        } => variants
            .iter()
            .flat_map(|variant| &variant.payload_types)
            .any(|ty| type_contains_owned_fn(&ty.node)),
        ItemKind::TypeAlias {
            aliased_type,
            public: true,
            ..
        } => type_contains_owned_fn(&aliased_type.node),
        ItemKind::Impl {
            trait_ty, methods, ..
        } => methods.iter().any(|method| {
            matches!(&method.node, ItemKind::Fn { params, return_ty, pub_fn, .. }
                if (*pub_fn || trait_ty.is_some())
                    && (params.iter().any(|param| type_contains_owned_fn(&param.ty.node))
                        || type_contains_owned_fn(&return_ty.node)))
        }),
        _ => false,
    }
}

fn type_contains_owned_fn(ty: &TypeKind) -> bool {
    match ty {
        TypeKind::Fn { .. } => true,
        TypeKind::Ref { inner } | TypeKind::RawPtr { inner } => type_contains_owned_fn(&inner.node),
        TypeKind::Array { elem_ty, .. }
        | TypeKind::FlexibleArray { elem_ty }
        | TypeKind::Slice { elem_ty } => type_contains_owned_fn(&elem_ty.node),
        TypeKind::Named { type_args, .. } => type_args
            .iter()
            .any(|argument| type_contains_owned_fn(&argument.node)),
        TypeKind::CFn { params, return_ty } => {
            params
                .iter()
                .any(|param| type_contains_owned_fn(&param.node))
                || type_contains_owned_fn(&return_ty.node)
        }
        _ => false,
    }
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
            for field in fields {
                output.push_str("    ");
                if field.is_const {
                    output.push_str("const ");
                }
                output.push_str(&field.name);
                output.push_str(": ");
                output.push_str(&field.ty.node.to_string());
                if let Some(width) = field.bit_width {
                    output.push_str(" : ");
                    output.push_str(&width.to_string());
                }
                render_inline_attributes(&mut output, &field.attributes);
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
                    output.push_str(
                        method
                            .param_names
                            .get(index)
                            .map(String::as_str)
                            .unwrap_or("arg"),
                    );
                    if method.param_names.get(index).is_none() {
                        output.push_str(&index.to_string());
                    }
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
        if matches!(attribute.name.as_str(), "export" | "inline" | "test") {
            continue;
        }
        render_attribute(&mut output, attribute);
        output.push('\n');
    }
    output
}

/// Render opaque field metadata on the same declaration line. Unlike item
/// attributes, no names are compiler-reserved at this position, so every
/// attribute must survive a QZI interface round trip unchanged.
fn render_inline_attributes(output: &mut String, attributes: &[Attribute]) {
    for attribute in attributes {
        output.push(' ');
        render_attribute(output, attribute);
    }
}

fn render_attribute(output: &mut String, attribute: &Attribute) {
    output.push('@');
    output.push_str(&attribute.name);
    if !attribute.args.is_empty() {
        output.push('(');
        for (index, argument) in attribute.args.iter().enumerate() {
            if index > 0 {
                output.push_str(", ");
            }
            match argument {
                AttrArg::Positional(value) => render_attr_value(output, value),
                AttrArg::KeyValue(key, value) => {
                    output.push_str(key);
                    output.push_str(" = ");
                    render_attr_value(output, value);
                }
            }
        }
        output.push(')');
    }
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
            module_name: None,
            implicit_prelude: false,
            runtime_string: None,
            type_imports: Vec::new(),
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
    fn private_trait_and_target_implementations_stay_out_of_public_interface() {
        let source = r#"
struct PrivateCounter { value: i32 }
trait PrivateReadable { fn hidden(self: &Self) i32; }
pub struct PublicCounter { value: i32 }
pub trait PublicReadable { fn read(self: &Self) i32; }
impl PrivateReadable for PrivateCounter { fn hidden(self: &PrivateCounter) i32 { ret self.value; } }
impl PrivateReadable for PublicCounter { fn hidden(self: &PublicCounter) i32 { ret self.value; } }
impl PublicReadable for PrivateCounter { fn read(self: &PrivateCounter) i32 { ret self.value; } }
impl PrivateCounter { fn generic[T](self: &PrivateCounter, value: T) T { ret value; } }
impl PublicReadable for PublicCounter { fn read(self: &PublicCounter) i32 { ret self.value; } }
pub fn answer(seed: i32) i32 { ret seed; }
"#;
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        let encoded = build_qzi_interface(
            "private_dispatch",
            &program,
            &[],
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap();
        let bundle = parse_qzi_interface(&encoded).unwrap();
        assert_eq!(bundle.modules.len(), 1);
        let interface = &bundle.modules[0];
        assert_eq!(
            interface.exports,
            ["PublicCounter", "PublicReadable", "answer"]
        );
        assert!(
            !interface.source.contains("Private"),
            "{}",
            interface.source
        );
        assert!(!interface.source.contains("generic"));
        assert!(
            interface
                .source
                .contains("impl PublicReadable for PublicCounter")
        );
        let program = Parser::new(Lexer::new(&interface.source).tokenize())
            .parse()
            .unwrap();
        let report = crate::semantic::Analyzer::new().analyze_program(&program);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }

    #[test]
    fn private_trait_keeps_explicit_public_methods_as_inherent_surface() {
        let source = "pub struct Api {} trait Hidden { fn exposed(self: &Self) i32; fn internal(self: &Self) i32; } impl Hidden for Api { pub fn exposed(self: &Api) i32 { ret 7; } fn internal(self: &Api) i32 { ret 1; } }";
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        let encoded =
            build_qzi_interface("api", &program, &[], &HashSet::new(), &HashSet::new()).unwrap();
        let bundle = parse_qzi_interface(&encoded).unwrap();
        let source = &bundle.modules[0].source;
        assert!(source.contains("impl Api"), "{source}");
        assert!(source.contains("pub fn exposed"));
        assert!(!source.contains("Hidden") && !source.contains("internal"));
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        let report = crate::semantic::Analyzer::new().analyze_program(&program);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }

    #[test]
    fn public_methods_on_private_targets_need_actionable_export_error() {
        let source =
            "struct Hidden {} impl Hidden { pub fn exposed(self: &Hidden) i32 { ret 7; } }";
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        let error = build_qzi_interface("api", &program, &[], &HashSet::new(), &HashSet::new())
            .unwrap_err();
        assert!(error.contains("private target") && error.contains("make the target public"));
    }

    #[test]
    fn aliases_and_generic_argument_spellings_never_prove_private_targets() {
        for source in [
            "type Alias = i32; impl Alias { pub fn exposed(self: &Alias) i32 { ret 7; } }",
            "struct T {} impl Holder[T] { pub fn exposed(self: &Holder[T]) i32 { ret 7; } }",
        ] {
            let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
            let encoded =
                build_qzi_interface("api", &program, &[], &HashSet::new(), &HashSet::new())
                    .unwrap();
            let bundle = parse_qzi_interface(&encoded).unwrap();
            assert!(
                bundle.modules[0].source.contains("pub fn exposed"),
                "{source}"
            );
        }
    }

    #[test]
    fn unrelated_source_private_names_do_not_erase_public_implementations() {
        let private = "struct Api {}";
        let public = "pub struct Api {} impl Api { pub fn exposed(self: &Api) i32 { ret 7; } }";
        let source = format!("{private}\n{public}");
        let program = Parser::new(Lexer::new(&source).tokenize()).parse().unwrap();
        let offset = private.chars().count() + 1;
        let files = vec![
            SourceFile {
                path: "private.qz".into(),
                module_name: Some("private".into()),
                implicit_prelude: false,
                runtime_string: None,
                type_imports: Vec::new(),
                start: 0,
                end: offset,
                line_start: 1,
            },
            SourceFile {
                path: "public.qz".into(),
                module_name: Some("public".into()),
                implicit_prelude: false,
                runtime_string: None,
                type_imports: Vec::new(),
                start: offset,
                end: source.chars().count(),
                line_start: 2,
            },
        ];
        for excluded in [HashSet::new(), HashSet::from([PathBuf::from("private.qz")])] {
            let encoded = build_qzi_interface(
                "api",
                &program,
                &files,
                &HashSet::from([PathBuf::from("public.qz")]),
                &excluded,
            )
            .unwrap();
            let bundle = parse_qzi_interface(&encoded).unwrap();
            assert!(
                bundle
                    .modules
                    .iter()
                    .any(|module| module.name == "public"
                        && module.source.contains("pub fn exposed"))
            );
        }
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
            module_name: None,
            implicit_prelude: false,
            runtime_string: None,
            type_imports: Vec::new(),
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
        assert!(bundle.modules[0].source.contains("    X,"));
        assert!(
            bundle.modules[0]
                .source
                .contains("fn length(self: Point) i32;")
        );
        let mut lexer = Lexer::new(&bundle.modules[0].source);
        Parser::new(lexer.tokenize())
            .parse()
            .expect("generated type interface should parse");
    }

    #[test]
    fn public_field_attributes_survive_interface_round_trip() {
        let source = r#"pub struct User {
    name: str @ini("username") @json(name="user_name"),
}"#;
        let program = Parser::new(Lexer::new(source).tokenize())
            .parse()
            .expect("parse source");
        let encoded = build_qzi_interface("users", &program, &[], &HashSet::new(), &HashSet::new())
            .expect("build interface");
        let bundle = parse_qzi_interface(&encoded).expect("parse interface bundle");
        assert!(bundle.modules[0].source.contains("@ini(\"username\")"));
        assert!(
            bundle.modules[0]
                .source
                .contains("@json(name = \"user_name\")")
        );

        let reparsed = Parser::new(Lexer::new(&bundle.modules[0].source).tokenize())
            .parse()
            .expect("generated interface should parse");
        let ItemKind::Struct { fields, .. } = &reparsed.items[0].node else {
            panic!("expected public struct");
        };
        assert_eq!(fields[0].attributes.len(), 2);
        assert_eq!(fields[0].attributes[0].name, "ini");
        assert_eq!(fields[0].attributes[1].name, "json");
    }

    #[test]
    fn public_generic_requires_source_distribution() {
        let source = "pub fn identity[T](value: T) T { ret value; }";
        let program = Parser::new(Lexer::new(source).tokenize())
            .parse()
            .expect("parse generic");
        let error = build_qzi_interface("generic", &program, &[], &HashSet::new(), &HashSet::new())
            .expect_err("generic QZI API should be rejected");
        assert!(error.contains("depend on this package's sources instead"));
        assert!(error.contains("path, git, or archive project dependency"));
    }

    #[test]
    fn public_generic_methods_require_source_distribution() {
        for source in [
            "pub trait Factory { fn make[T](value: T) T; }",
            "pub struct Api {} impl Api { pub fn id[T](self: Api, value: T) T { ret value; } }",
        ] {
            let program = Parser::new(Lexer::new(source).tokenize())
                .parse()
                .expect("parse generic method");
            let error =
                build_qzi_interface("generic", &program, &[], &HashSet::new(), &HashSet::new())
                    .expect_err("generic method templates cannot be exported without bodies");
            assert!(error.contains("depend on this package's sources instead"));
        }
    }

    #[test]
    fn public_runtime_any_requires_source_distribution() {
        for source in [
            "pub fn erase(value: any) any { ret value; }",
            "pub fn erase(value: fn(any) void) void {}",
            "pub fn erase(value: Box[any]) void {}",
            "pub struct Erased { value: any, }",
            "pub enum Erased { Value(any), }",
            "pub type Erased = any;",
            "pub trait Sink { fn put(value: any) void; }",
            "pub struct Api {} impl Api { pub fn erase(self: Api, value: any) void {} }",
            "pub trait Sink { fn put(value: any) void; } pub struct Api {} impl Sink for Api { fn put(self: Api, value: any) void {} }",
        ] {
            let program = Parser::new(Lexer::new(source).tokenize())
                .parse()
                .expect("parse runtime any API");
            let error = build_qzi_interface(
                "unsafe_api",
                &program,
                &[],
                &HashSet::new(),
                &HashSet::new(),
            )
            .expect_err("runtime any QZI API should be rejected");
            assert!(error.contains("runtime `any` has no portable representation"));
            assert!(error.contains("publish this package as source"));
        }
    }

    #[test]
    fn format_erased_any_remains_exportable() {
        let source = r#"
@format pub fn print(template: str, ...args: any) void;
pub struct Printer {}
impl Printer {
    @format pub fn print(self: Printer, template: str, ...args: any) void {}
}
"#;
        let program = Parser::new(Lexer::new(source).tokenize())
            .parse()
            .expect("parse format API");
        let encoded = build_qzi_interface(
            "format_api",
            &program,
            &[],
            &HashSet::new(),
            &HashSet::new(),
        )
        .expect("format erased tail does not cross the runtime ABI");
        let bundle = parse_qzi_interface(&encoded).expect("parse format interface");
        assert_eq!(bundle.modules[0].source.matches("@format").count(), 2);
        let mut lexer = Lexer::new(&bundle.modules[0].source);
        Parser::new(lexer.tokenize())
            .parse()
            .expect("generated format interface should parse");
    }

    #[test]
    fn excluded_dependency_generics_do_not_enter_library_interface() {
        let source = "pub trait Eq[T] { fn equals(left: T, right: T) bool; }";
        let program = Parser::new(Lexer::new(source).tokenize())
            .parse()
            .expect("parse dependency generic");
        let path = PathBuf::from("std/traits.qz");
        let files = vec![SourceFile {
            path: path.to_string_lossy().into_owned(),
            module_name: None,
            implicit_prelude: false,
            runtime_string: None,
            type_imports: Vec::new(),
            start: 0,
            end: source.chars().count(),
            line_start: 1,
        }];
        let excluded = HashSet::from([path]);
        let encoded = build_qzi_interface("library", &program, &files, &HashSet::new(), &excluded)
            .expect("excluded dependency API should be ignored");
        let bundle = parse_qzi_interface(&encoded).expect("parse empty interface");
        assert!(bundle.modules.is_empty());
    }
}
