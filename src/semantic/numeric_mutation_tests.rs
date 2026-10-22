// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use super::*;
use crate::{lexer::Lexer, parser::Parser};

fn parse(source: &str) -> Program {
    Parser::new(Lexer::new(source).tokenize())
        .parse()
        .unwrap_or_else(|errors| panic!("{source}: {errors:?}"))
}

fn analyze(source: &str) -> SemanticReport {
    Analyzer::new().analyze_program(&parse(source))
}

fn assert_valid(source: &str) {
    let report = analyze(source);
    assert!(report.errors.is_empty(), "{source}: {:?}", report.errors);
}

fn assert_error(source: &str, code: &str) {
    let report = analyze(source);
    assert!(
        report.errors.iter().any(|error| error.code == code),
        "{source}: expected {code}, got {:?}",
        report.errors
    );
}

#[test]
fn numeric_mutations_reject_known_nonnumeric_increment_targets() {
    for source in [
        "struct Owner{value:i32,} fn main()void{var value=Owner{value:1};value++;}",
        "enum Choice{One,} fn main()void{var value:Choice=Choice.One;--value;}",
        "fn main()void{var value=[1,2];value++;}",
        "fn main()void{var value=true;value--;}",
        "fn main()void{var value=\"text\";++value;}",
        "fn identity(value:i32)i32{ret value;} fn main()void{var value:fn(i32)i32=identity;value++;}",
        "unsafe fn main()void{var value:*u8=0;value++;}",
    ] {
        assert_error(source, "S06");
    }
}

#[test]
fn numeric_mutations_reject_handle_arithmetic_assignment() {
    for source in [
        "struct Owner{value:i32,} fn main()void{var a=Owner{value:1};var b=Owner{value:2};a+=b;}",
        "enum Choice{One,} fn main()void{var value:Choice=Choice.One;value+=Choice.One;}",
        "fn main()void{var value=[1,2];value*=[3,4];}",
        "fn main()void{var value=true;value+=false;}",
        "fn main()void{var value=\"text\";value+=\"more\";}",
        "fn identity(value:i32)i32{ret value;} fn main()void{var a:fn(i32)i32=identity;var b:fn(i32)i32=identity;a+=b;}",
        "unsafe fn main()void{var value:*u8=0;value*=2;}",
        "unsafe fn main()void{var value:*u8=0;value+=1.5;}",
    ] {
        assert_error(source, "S06");
    }
}

#[test]
fn numeric_mutations_fail_closed_when_constructor_types_are_missing() {
    for source in [
        "enum Choice{One,} fn main()void{var value=Choice.One;value++;}",
        "enum Choice{One,} fn main()void{var value=Choice.One;value+=Choice.One;}",
        "enum Choice{One,} fn main()void{var value=1;value+=Choice.One;}",
        "enum Choice{One,} fn helper[T](unused:T)void{var value=Choice.One;value++;}",
    ] {
        assert_error(source, "S14");
    }
}

#[test]
fn numeric_mutations_accept_numbers_aliases_and_loaded_reference_operands() {
    for source in [
        "fn main()void{var value:i32=1;value++;--value;value+=2;value-=1;value*=3;value/=2;value%=2;}",
        "fn main()void{var value:f64=1.5;value++;--value;value+=2.5;value-=1.0;value*=3.0;value/=2.0;value%=2.0;}",
        "type Scalar=f32; fn bump(value:Scalar)Scalar{value++;value+=1.0;ret value;}",
        "fn main()void{var offset=2;var reference=&offset;var value=1;value+=reference;}",
        "fn main()void{var offset=2;var reference=&offset!;var value=1;value+=reference;}",
        "unsafe fn main()void{var pointer:*u8=0;pointer+=4;pointer-=2;}",
        "fn main()void{var value=\"left\"+\"right\";}",
    ] {
        assert_valid(source);
    }
}

#[test]
fn numeric_mutations_keep_directional_numeric_compatibility() {
    assert_valid("fn main()void{var value:f64=1.5;value+=2;}");
    assert_error("fn main()void{var value:i32=1;value+=2.5;}", "S01");
}

#[test]
fn numeric_mutation_obligations_validate_every_concrete_instantiation() {
    let definitions = "fn bump[T](value:T)T{value++;ret value;} fn add[T](value:T,offset:T)T{value+=offset;ret value;}";
    assert_valid(&format!(
        "{definitions} fn main()void{{bump[i32](1);bump[f64](1.5);add[i32](1,2);add[f64](1.5,2.5);}}"
    ));
    for calls in [
        "bump[i32](1);bump[bool](true);",
        "bump[bool](true);bump[i32](1);",
        "add[i32](1,2);add[bool](true,false);",
        "add[str](\"a\",\"b\");add[f64](1.5,2.5);",
        "unsafe{bump[*u8](0);}",
    ] {
        assert_error(&format!("{definitions} fn main()void{{{calls}}}"), "S06");
    }
}

#[test]
fn numeric_mutation_obligations_follow_transitive_generic_calls() {
    let definitions =
        "fn bump[T](value:T)T{value++;ret value;} fn forward[U](value:U)U{ret bump[U](value);}";
    assert_valid(&format!(
        "{definitions} fn main()void{{forward[i32](1);forward[f64](1.5);}}"
    ));
    assert_error(
        &format!("{definitions} fn main()void{{forward[i32](1);forward[bool](true);}}"),
        "S06",
    );
}

#[test]
fn numeric_mutation_binders_shadow_global_aliases() {
    assert_valid(
        "type T=bool; fn bump[T](value:T)T{value++;ret value;} fn main()void{bump[i32](1);}",
    );
    assert_error(
        "type T=i32; fn bump[T](value:T)T{value++;ret value;} fn main()void{bump[bool](true);}",
        "S06",
    );
}

#[test]
fn numeric_mutation_obligations_use_renamed_impl_binders() {
    let definitions =
        "struct Cell[T]{value:T,} impl Cell[Item]{fn bump(self:&Cell[Item]!)void{self.value++;}}";
    assert_valid(&format!(
        "{definitions} fn main()void{{var cell:Cell[i32]=Cell{{value:1}};cell.bump();}}"
    ));
    assert_error(
        &format!(
            "{definitions} fn main()void{{var cell:Cell[bool]=Cell{{value:true}};cell.bump();}}"
        ),
        "S06",
    );
}

#[test]
fn numeric_mutation_obligations_reset_when_the_analyzer_is_reused() {
    let mut analyzer = Analyzer::new();
    let invalid =
        parse("fn bump[T](value:T)T{value++;ret value;} fn main()void{bump[bool](true);}");
    assert!(
        analyzer
            .analyze_program(&invalid)
            .errors
            .iter()
            .any(|error| error.code == "S06")
    );
    let valid = parse("fn bump[T](value:T)T{ret value;} fn main()void{bump[bool](true);}");
    let report = analyzer.analyze_program(&valid);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
}

#[test]
fn numeric_mutations_infer_borrowed_builtin_array_elements() {
    assert_valid("fn update(values:&[f64;2]!)f64{values[0]++;values[1]+=values[0];ret values[1];}");
    assert_error("fn update(values:&[bool;2]!)void{values[0]++;}", "S06");
    assert_error("fn update(values:&[i32;2]!)void{values[2]++;}", "S06");
}

#[test]
fn numeric_mutations_validate_target_and_rhs_parameter_slots() {
    let definitions = "fn target[Marker,T](value:T,offset:i32)T{value+=offset;ret value;}fn rhs[Marker,U](value:i32,offset:U)i32{value+=offset;ret value;}";
    assert_valid(&format!(
        "{definitions}fn main()void{{target[bool,i32](1,2);rhs[bool,i64](1,2);}}"
    ));
    assert_error(
        &format!("{definitions}fn main()void{{rhs[bool,f64](1,2.5);}}"),
        "S01",
    );
    assert_error(
        &format!("{definitions}fn main()void{{rhs[bool,bool](1,true);}}"),
        "S06",
    );
    assert_error(
        &format!("{definitions}fn main()void{{target[bool,bool](true,1);}}"),
        "S06",
    );
}

#[test]
fn numeric_mutation_borrowed_builtin_reads_are_typed_and_bytes_remain_immutable() {
    assert_valid(
        "fn read(...values:i32)i32{var r=&values;ret r[0];}fn main()void{var b=b\"AB\";var r=&b;var value:u8=r[0];}",
    );
    assert_error("fn main()void{var b=b\"AB\";var r=&b!;r[0]++;}", "S07");
}

#[test]
fn numeric_mutation_generic_target_loads_known_reference_rhs() {
    let report = analyze(
        "fn add[T](value:T,offset:&i32)T{value+=offset;ret value;} fn main()void{var n=2;add[i32](1,&n);}",
    );
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(
        report
            .annotated_exprs
            .iter()
            .any(|annotation| annotation.auto_deref
                && annotation.ty.as_ref().is_some_and(Analyzer::is_integer))
    );
}
