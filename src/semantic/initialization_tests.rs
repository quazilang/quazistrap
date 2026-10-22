use super::{Analyzer, SemanticReport};
use crate::lexer::Lexer;
use crate::parser::Parser;

fn analyze(source: &str) -> SemanticReport {
    let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
    Analyzer::new().analyze_program(&program)
}

fn valid(source: &str) {
    let report = analyze(source);
    assert!(report.errors.is_empty(), "{source}\n{:?}", report.errors);
}

fn uninitialized(source: &str, name: &str) {
    let report = analyze(source);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.code == "S02" && error.message.contains(&format!("'{name}'"))),
        "{source}\n{:?}",
        report.errors
    );
}

#[test]
fn branches_do_not_initialize_siblings_or_implicit_else() {
    for body in [
        "if flag { value=1; } ret value;",
        "if flag { value=1; } else { ret value; } ret 0;",
        "if flag { value=1; } else if value>0 { ret 1; } ret 0;",
        "if flag { value=1; } else if !flag { ret value; } ret 0;",
    ] {
        uninitialized(
            &format!("fn test(flag:bool)i32 {{ var value:i32; {body} }}"),
            "value",
        );
    }
}

#[test]
fn initialization_joins_only_normal_successors() {
    for body in [
        "if flag { value=1; } else { value=2; } ret value;",
        "if flag { ret 0; } else { value=2; } ret value;",
        "if flag { unsafe { ret 0; } } else { value=2; } ret value;",
        "if flag { value=1; } else if ((value=2)>0) {} else { value=3; } ret value;",
    ] {
        valid(&format!(
            "fn test(flag:bool)i32 {{ var value:i32; {body} }}"
        ));
    }
}

#[test]
fn skipped_logical_operands_do_not_initialize_following_reads() {
    for expression in [
        "flag && (value=true)",
        "flag || (value=true)",
        "false && (value=true)",
        "true || (value=true)",
    ] {
        uninitialized(
            &format!("fn test(flag:bool)bool {{ var value:bool; {expression}; ret value; }}"),
            "value",
        );
    }
}

#[test]
fn logical_truth_paths_retain_their_initialization() {
    for body in [
        "if flag && (value=5) { ret value; } ret 0;",
        "if flag || (value=0) { ret 1; } else { ret value; }",
        "if !(flag || (value=0)) { ret value; } ret 1;",
        "if ((flag && (value=5)) as bool) { ret value; } ret 0;",
    ] {
        valid(&format!(
            "fn test(flag:bool)i32 {{ var value:i32; {body} }}"
        ));
    }
    valid("fn test()bool { var value:bool; true && (value=true); ret value; }");
    valid("fn test()bool { var value:bool; false || (value=true); ret value; }");
}

#[test]
fn loop_body_and_update_do_not_initialize_skipped_paths() {
    for body in [
        "for flag { value=1; break; } ret value;",
        "for var index=0; index<1; value=1 { ret value; } ret 0;",
        "for { if flag { break; } value=1; break; } ret value;",
        "for var index=0; index<1; index=value { if flag { continue; } value=1; } ret 0;",
        "for index:0..0 { value=1; } ret value;",
    ] {
        uninitialized(
            &format!("fn test(flag:bool)i32 {{ var value:i32; {body} }}"),
            "value",
        );
    }
}

#[test]
fn loop_exit_and_continue_inputs_follow_execution_order() {
    for body in [
        "for { value=1; break; } ret value;",
        "for { if flag { value=1; break; } value=2; break; } ret value;",
        "for value=5; false; {} ret value;",
        "for (value=0) {} ret value;",
        "for var index=0; index<1; index=value { value=1; continue; } ret 0;",
        "for { for { break; } value=1; break; } ret value;",
        "for { value=1; unsafe { break; } value=2; } ret value;",
    ] {
        valid(&format!(
            "fn test(flag:bool)i32 {{ var value:i32; {body} }}"
        ));
    }
}

#[test]
fn match_arms_and_guard_mismatches_do_not_share_initialization() {
    for body in [
        "match flag { true=>value=1, false=>0, }; ret value;",
        "ret match flag { true=>value=1, false=>value, };",
        "ret match flag { true if (value=1)>0=>0, false=>value, _=>0, };",
        "match flag { value=>0, }; ret value;",
    ] {
        uninitialized(
            &format!("fn test(flag:bool)i32 {{ var value:i32; {body} }}"),
            "value",
        );
    }
    uninitialized(
        r#"fn test()i32 {var value:i32;match "a\0b" {"a"=>0,_=>value=7,};ret value;}"#,
        "value",
    );
}

#[test]
fn exhaustive_matches_scrutinees_and_guard_paths_keep_initialization() {
    for body in [
        "match flag { true=>value=1, false=>value=2, }; ret value;",
        "match (value=5) { 5=>0, _=>1, }; ret value;",
        "ret match flag { _ if (value=5)=>value, _=>value, };",
        "ret match flag { true if (value=1)>0=>0, true=>value, false=>0, };",
    ] {
        valid(&format!(
            "fn test(flag:bool)i32 {{ var value:i32; {body} }}"
        ));
    }
    valid(
        "enum Choice { First, Second } fn test(choice:Choice)i32 { var value:i32; match choice { First=>value=1, Second=>value=2, }; ret value; }",
    );
    valid(r#"fn test()i32 {var value:i32;match "a\0b" {"a"=>value=7,_=>0,};ret value;}"#);
}

#[test]
fn deferred_closure_assignments_do_not_initialize_outer_storage() {
    let report = analyze("fn test()i32 { var value:i32; var action=|| value=1; ret value; }");
    assert!(report.errors.iter().any(|error| error.code == "S07"));
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.code == "S02" && error.message.contains("'value'"))
    );
}

#[test]
fn callable_values_require_initialized_storage() {
    uninitialized(
        "fn test()i32 { var action:fn()i32; ret action(); }",
        "action",
    );
    uninitialized(
        "@repr(C) type Callback=fn()i32; unsafe fn test()i32 { var action:Callback; ret action(); }",
        "action",
    );
}

#[test]
fn value_matches_require_a_result_on_every_path() {
    for source in [
        "fn test(flag:bool)i32 { ret match flag { true=>1, }; }",
        "fn test(value:i32)i32 { ret match value { 1=>1, }; }",
        "fn test(value:f64)i32 { ret match value { 1.0=>1, }; }",
        "fn test(value:str)i32 { ret match value { \"one\"=>1, }; }",
    ] {
        let report = analyze(source);
        assert!(
            report.errors.iter().any(|error| error.code == "S09"),
            "{source}\n{:?}",
            report.errors
        );
    }
    valid("fn observe()void {} fn test(value:i32)void { match value { 1=>observe(), }; }");
    valid("fn test()i32 { ret match -0.0 { 0.0=>1, }; }");
    valid("fn test()i32 { ret match 1 { true=>7, }; }");
}

#[test]
fn literal_patterns_require_matching_runtime_representations() {
    for source in [
        r#"fn test()i32 {ret match 1 {"one"=>0,_=>1,};}"#,
        "fn test()i32 {ret match 0 {0.0=>0,_=>1,};}",
        "fn test()i32 {ret match 0.0 {0=>0,_=>1,};}",
    ] {
        let report = analyze(source);
        assert!(
            report.errors.iter().any(|error| error.code == "S01"),
            "{source}\n{:?}",
            report.errors
        );
    }
}

#[test]
fn named_arguments_use_the_declared_slot_schedule_for_initialization() {
    let function = "fn select(first:i32,second:i32)i32 {ret first;}";
    uninitialized(
        &format!(
            "{function} fn test()i32 {{var value:i32;ret select(second=(value=7),first=value+1);}}"
        ),
        "value",
    );
    valid(&format!(
        "{function} fn test()i32 {{var value:i32;ret select(second=value+1,first=(value=7));}}"
    ));
    let method =
        "struct Reader{} impl Reader {fn select(self:&Reader,first:i32,second:i32)i32{ret first;}}";
    uninitialized(
        &format!(
            "{method} fn test()i32 {{var reader=Reader{{}};var value:i32;ret reader.select(second=(value=7),first=value+1);}}"
        ),
        "value",
    );
    valid(&format!(
        "{method} fn test()i32 {{var reader=Reader{{}};var value:i32;ret reader.select(second=value+1,first=(value=7));}}"
    ));
}

#[test]
fn declaration_identity_and_analyzer_reset_preserve_independent_facts() {
    uninitialized(
        "fn test()i32 { var value:i32; unsafe { var value=1; } ret value; }",
        "value",
    );
    let source = "fn test(flag:bool)i32 { var value:i32; if flag { value=1; } ret value; }";
    let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
    let mut analyzer = Analyzer::new();
    for _ in 0..2 {
        assert!(
            analyzer
                .analyze_program(&program)
                .errors
                .iter()
                .any(|error| error.code == "S02")
        );
    }
}

#[test]
fn boolean_match_coverage_uses_truth_classes_not_canonical_bits() {
    valid(
        "fn test(flag:bool)i32 {var value:i32;match flag{true=>value=1,false=>value=2,};ret value;}",
    );
    uninitialized(
        "fn test(flag:bool)i32 {var value:i32;ret match flag{1=>0,0=>value=7,_=>value,};}",
        "value",
    );
    uninitialized(
        "fn test()i32 {const flag:bool=2;var value:i32;match flag{false=>value=7,_=>0,};ret value;}",
        "value",
    );
    valid(
        "fn test()i32 {const flag:bool=2;var value:i32;match flag{true=>value=7,_=>0,};ret value;}",
    );
}

#[test]
fn nested_refutable_patterns_require_actual_payload_representation() {
    for source in [
        "enum Wrap{Some(i32),None,}fn test()i32{var value:Wrap=Wrap.Some(1);ret match value{Some(\"one\")=>0,_=>1,};}",
        "enum Wrap{Some(i32),None,}fn test()i32{var value=Wrap.Some(1);ret match value{Some(\"one\")=>0,_=>1,};}",
        "enum Wrap[T]{Some(T),None,}fn test()i32{var value:Wrap[i32]=Wrap.Some(1);ret match value{Some(\"one\")=>0,_=>1,};}",
    ] {
        let report = analyze(source);
        assert!(
            report
                .errors
                .iter()
                .any(|error| matches!(error.code, "S01" | "S14")),
            "{source}\n{:?}",
            report.errors
        );
    }
    valid(
        "enum Wrap[T]{Some(T),None,}fn test(value:Wrap[i32])i32{ret match value{Some(1)=>7,_=>0,};}",
    );
    valid(
        "enum Pair[T]{Both(i32,T),}fn test(value:Pair[i32])i32{ret match value{Both(_,second)=>second,};}",
    );
}

#[test]
fn enum_pattern_discriminants_follow_the_scrutinee_declaration() {
    let report = analyze(
        "enum First{Some(i32),None,}enum Second{None,Some(i32),}fn first(value:First)i32{ret match value{Some(1)=>7,_=>0,};}fn second(value:Second)i32{ret match value{Some(1)=>7,_=>0,};}",
    );
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let tags: std::collections::BTreeSet<_> = report.match_pattern_tags.values().copied().collect();
    assert_eq!(tags, [0, 1].into_iter().collect());
    let report = analyze(
        "enum First{Some(i32),None,}fn test()i32{var value=First.Some(1);ret match value{Some(_)=>7,_=>0,};}",
    );
    assert!(report.errors.iter().any(|error| error.code == "S14"));
    valid(
        "enum T{Atom(i32),}enum Holder[X]{Value(X),}fn test(value:Option[Holder[T]])i32{ret match value{Some(Holder.Value(T.Atom(1)))=>7,_=>0,};}",
    );
}

#[test]
fn eager_operands_follow_the_emitted_schedule() {
    uninitialized(
        "struct Pair{first:i32,second:i32,}fn test()i32{var value:i32;var pair=Pair{second:(value=7),first:value+1};ret pair.first;}",
        "value",
    );
    uninitialized(
        "fn helper(value:i32)i32{ret value;}fn get(value:i32)fn(i32)i32{ret helper;}fn test()i32{var value:i32;ret get(value)((value=7));}",
        "value",
    );
    valid(
        "struct Holder{field:i32,}fn make(value:i32)Holder{ret Holder{field:value};}fn test()i32{var value:i32;make((value=7)).field=value;ret value;}",
    );
    let report = analyze(
        "struct Pair{first:i32,second:i32,}fn test()i32{var value:i32;var pair=Pair{first:0,second:0,second:(value=7)};ret value;}",
    );
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.code == "S07" && error.message.contains("duplicate field"))
    );
}

#[test]
fn known_absorbing_results_do_not_erase_executed_operands() {
    for expression in ["(value=7)*0", "(value=7)||true", "((value=7)>0)&&false"] {
        let source = format!("fn test()i32{{var value:i32;{expression};ret value;}}");
        let report = analyze(&source);
        assert!(report.errors.is_empty(), "{source}\n{:?}", report.errors);
        let annotation = report
            .annotated_exprs
            .iter()
            .find(|annotation| &source[annotation.span.start..annotation.span.end] == expression)
            .expect("root expression annotation");
        assert!(
            annotation.const_value.is_none(),
            "must evaluate {expression}"
        );
    }
    uninitialized(
        "fn test(input:f64)i32{var value:i32;match 0.0*input{0.0=>value=7,_=>0,};ret value;}",
        "value",
    );
}
