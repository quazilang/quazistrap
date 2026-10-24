// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use super::*;
use crate::{lexer::Lexer, parser::Parser};

fn analyze(source: &str, runtime: Option<&str>) -> SemanticReport {
    let files = vec![SourceFile {
        path: "runtime.qz".into(),
        module_name: None,
        implicit_prelude: false,
        runtime_string: runtime.map(str::to_owned),
        type_imports: Vec::new(),
        start: 0,
        end: source.len(),
        line_start: 1,
    }];
    let program =
        Parser::new_with_source_files(Lexer::new(source).tokenize(), source, files.clone())
            .parse()
            .unwrap_or_else(|errors| panic!("{source}: {errors:?}"));
    let mut analyzer = Analyzer::new();
    analyzer.set_source_files(files);
    analyzer.analyze_program(&program)
}

fn error(source: &str, runtime: Option<&str>, code: &str) {
    let report = analyze(source, runtime);
    assert!(
        report.errors.iter().any(|error| error.code == code),
        "{source}: expected {code}, got {:?}",
        report.errors
    );
}

#[test]
fn unselected_string_spellings_do_not_grant_string_abi() {
    for fields in ["value:i32", "data:*u8", "data:str"] {
        for body in [
            "fn compare(a:String,b:String)bool{ret a<b;}",
            "type Alias=String;fn compare(a:Alias,b:Alias)bool{ret a<b;}",
            "fn compare[T](a:T,b:T)bool{ret a<b;}fn force(a:String,b:String)bool{ret compare[String](a,b);}",
        ] {
            error(
                &format!("pub struct String{{{fields},}}{body}"),
                None,
                "S06",
            );
        }
    }
}

#[test]
fn arbitrary_reference_is_not_a_builtin_string_view() {
    error(
        "struct Owner{value:i32,}fn view(a:&Owner)void{a.as_str();}",
        None,
        "S06",
    );
    error(
        "struct Owner{value:i32,}fn view(a:&Owner)void{a.as_string();}",
        None,
        "S06",
    );
}

#[test]
fn selected_runtime_string_validates_its_declared_layout() {
    for declaration in [
        "pub struct String{data:i32,}",
        "pub struct String{data:*u8,}",
        "pub struct String{value:i32,}",
        "pub struct String[T]{data:str,}",
        "struct String{data:str,}",
    ] {
        error(declaration, Some("String"), "S14");
    }
}

#[test]
fn checked_string_can_have_nonzero_data_offset_and_exported_original_name() {
    for (name, fields) in [("String", "tag:i32,data:str"), ("Text", "data:str")] {
        let source =
            format!("pub struct {name}{{{fields},}}fn compare(a:{name},b:{name})bool{{ret a<b;}}");
        let report = analyze(&source, Some(name));
        assert!(report.errors.is_empty(), "{source}: {:?}", report.errors);
        let abi = report
            .checked_string_abi
            .expect("selected declaration must publish checked ABI");
        assert_eq!(abi.declaration.name, name);
        assert_eq!(abi.data_offset, if name == "String" { 8 } else { 0 });
    }
}

#[test]
fn qualified_alias_cannot_authenticate_a_global_runtime_name() {
    let source = "pub struct String{data:str,}type Alias=String;fn compare(a:fake.Alias,b:fake.Alias)bool{ret a<b;}";
    let report = analyze(source, Some("String"));
    assert!(
        !report.errors.is_empty(),
        "unresolved qualified alias must not acquire the global runtime ABI"
    );
}

#[test]
fn format_rejects_unsupported_values_before_runtime_lowering() {
    let definitions = "struct Owner{value:i32,}@format fn sink(...parts:str)void{}";
    for body in [
        "fn main()void{sink(Owner{value:1});}",
        "fn main()void{var owner=Owner{value:1};sink(&owner);}",
        "unsafe fn main()void{var pointer:*u8=0;sink(pointer);}",
        "fn display[T](value:T)void{sink(value);}fn main()void{display[Owner](Owner{value:1});}",
        "fn display[T](value:&T)void{sink(value);}fn main()void{var owner=Owner{value:1};display[Owner](&owner);}",
        "fn display[T](value:T)void{sink(value);}unsafe fn main()void{var pointer:*u8=0;display[*u8](pointer);}",
    ] {
        error(&format!("{definitions}{body}"), None, "S06");
    }
}

#[test]
fn analyzer_reuse_does_not_retain_selected_runtime_role() {
    let source = "pub struct String{data:str,}fn compare(a:String,b:String)bool{ret a<b;}";
    let mut analyzer = Analyzer::new();
    for runtime in [Some("String"), None] {
        let files = vec![SourceFile {
            path: "runtime.qz".into(),
            module_name: None,
            implicit_prelude: false,
            runtime_string: runtime.map(str::to_owned),
            type_imports: Vec::new(),
            start: 0,
            end: source.len(),
            line_start: 1,
        }];
        let program =
            Parser::new_with_source_files(Lexer::new(source).tokenize(), source, files.clone())
                .parse()
                .unwrap();
        analyzer.set_source_files(files);
        let report = analyzer.analyze_program(&program);
        if runtime.is_some() {
            assert!(report.errors.is_empty(), "{:?}", report.errors);
            assert!(report.checked_string_abi.is_some());
        } else {
            assert!(report.errors.iter().any(|error| error.code == "S06"));
            assert!(report.checked_string_abi.is_none());
        }
    }
}

#[test]
fn nested_owned_string_references_do_not_collapse_two_loads_into_one() {
    for reference in ["&&String", "&&String!"] {
        let source = format!(
            "pub struct String{{data:str,}}fn compare(a:{reference},b:{reference})bool{{ret a<b;}}fn main()void{{}}"
        );
        error(&source, Some("String"), "S06");
    }
}

#[test]
fn a_runtime_type_name_cannot_bypass_a_static_namespace_binder() {
    error(
        "pub struct String{data:str,}impl String{pub fn new()String{ret String{data:\"\"};}}fn create[String]()String{ret String.new();}fn main()void{}",
        Some("String"),
        "S14",
    );
}
