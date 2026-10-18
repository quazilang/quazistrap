// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use super::*;
use crate::{lexer::Lexer, parser::Parser};

fn analyze(source: &str) -> SemanticReport {
    let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
    let report = Analyzer::new().analyze_program(&program);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    report
}

fn callable<'a>(report: &'a SemanticReport, name: &str) -> &'a OwnershipCallable {
    report
        .ownership_call_graph
        .iter()
        .find(|node| node.id.callable == name)
        .unwrap()
}

fn arguments<'a>(
    report: &'a SemanticReport,
    source: &str,
    caller: &str,
    spelling: &str,
) -> &'a OwnershipCallArguments {
    callable(report, caller)
        .calls
        .iter()
        .find(|site| source[site.span.start..].starts_with(spelling))
        .unwrap_or_else(|| {
            panic!(
                "missing call {spelling:?}; actual {:?}",
                callable(report, caller)
                    .calls
                    .iter()
                    .map(|site| &source[site.span.start..site.span.end])
                    .collect::<Vec<_>>()
            )
        })
        .arguments
        .as_ref()
        .expect("source call operand facts")
}

fn place(argument: &OwnershipCallArgument) -> &OwnershipPlace {
    let OwnershipArgumentValue::Place(place) = &argument.value else {
        panic!("expected place: {argument:?}");
    };
    place
}

#[test]
fn argument_places_keep_named_spelling_order_and_variadic_slots() {
    let source = r#"
fn select(first: str, ...rest: str) void {}
fn main() void { var a: str = "a"; var b: str = "b"; var c: str = "c"; select(a, b, c); select(rest = b, first = a); }
"#;
    let report = analyze(source);
    let packed = arguments(&report, source, "main", "select(a, b, c)");
    assert_eq!(
        packed
            .actuals
            .iter()
            .map(|arg| arg.parameter_slot)
            .collect::<Vec<_>>(),
        vec![Some(0), Some(1), Some(1)]
    );
    for (index, arg) in packed.actuals.iter().enumerate() {
        assert_eq!(arg.role, OwnershipArgumentRole::Positional(index));
        assert_eq!(place(arg).root.binding.name, ["a", "b", "c"][index]);
    }
    let named = arguments(&report, source, "main", "select(rest = b, first = a)");
    assert_eq!(
        named.actuals[0].role,
        OwnershipArgumentRole::Named("rest".into())
    );
    assert_eq!(
        named.actuals[1].role,
        OwnershipArgumentRole::Named("first".into())
    );
    assert_eq!(
        named
            .actuals
            .iter()
            .map(|arg| arg.parameter_slot)
            .collect::<Vec<_>>(),
        vec![Some(1), Some(0)]
    );
}

#[test]
fn argument_places_distinguish_legacy_receiver_and_static_namespace() {
    let source = r#"
struct Box { value: i32 }
impl Box {
    fn observe(receiver: Box, count: i32) void {}
    fn create(count: i32) i32 { ret count; }
}
fn main() void { var box = Box { value: 1 }; var count = 2; box.observe(count = count); Box.create(count); }
"#;
    let report = analyze(source);
    let instance = arguments(&report, source, "main", "box.observe(count = count)");
    assert_eq!(instance.actuals.len(), 2);
    assert_eq!(instance.actuals[0].role, OwnershipArgumentRole::Receiver);
    assert_eq!(instance.actuals[0].parameter_slot, Some(0));
    assert_eq!(place(&instance.actuals[0]).root.binding.name, "box");
    assert_eq!(instance.actuals[1].parameter_slot, Some(1));
    let associated = arguments(&report, source, "main", "Box.create(count)");
    assert_eq!(associated.actuals.len(), 1);
    assert_eq!(
        associated.actuals[0].role,
        OwnershipArgumentRole::Positional(0)
    );
    assert_eq!(associated.actuals[0].parameter_slot, Some(0));
}

#[test]
fn argument_places_keep_module_and_import_alias_out_of_actuals() {
    let module = "pub fn consume(value: i32) void {}";
    let main = "import helpers; import helpers.consume as renamed; fn main() void { var x = 7; renamed(x); helpers.consume(value = x); }";
    let source = format!("{module}\n{main}");
    let files = vec![
        SourceFile {
            path: "helpers.qz".into(),
            module_name: Some("helpers".into()),
            start: 0,
            end: module.len(),
            line_start: 1,
        },
        SourceFile {
            path: "main.qz".into(),
            module_name: None,
            start: module.len() + 1,
            end: source.len(),
            line_start: 2,
        },
    ];
    let program =
        Parser::new_with_source_files(Lexer::new(&source).tokenize(), &source, files.clone())
            .parse()
            .unwrap();
    let mut analyzer = Analyzer::new();
    analyzer.set_source_files(files);
    analyzer.set_namespaced_paths(HashSet::from(["helpers.qz".into()]));
    let report = analyzer.analyze_program(&program);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    for spelling in ["renamed(x)", "helpers.consume(value = x)"] {
        let args = arguments(&report, &source, "main", spelling);
        assert_eq!(args.actuals.len(), 1);
        assert_eq!(args.actuals[0].parameter_slot, Some(0));
        assert_eq!(place(&args.actuals[0]).root.binding.name, "x");
    }
}

#[test]
fn argument_places_distinguish_shadowed_and_pattern_declarations() {
    let source = r#"
enum Pair { Values(i32, i32) }
fn observe(x: i32) i32 { ret x; }
fn main() void {
    var x = 1; observe(x);
    if true { var x = 2; observe(x); }
    var pair: Pair = Pair.Values(3, 4);
    match pair { Values(left, right) => observe(left) + observe(right), };
}
"#;
    let report = analyze(source);
    let observed = callable(&report, "main")
        .calls
        .iter()
        .filter(|site| source[site.span.start..site.span.end].starts_with("observe("))
        .map(|site| place(&site.arguments.as_ref().unwrap().actuals[0]))
        .collect::<Vec<_>>();
    assert_eq!(observed.len(), 4);
    assert_eq!(observed[0].root.binding.name, "x");
    assert_eq!(observed[1].root.binding.name, "x");
    assert_ne!(observed[0].root.binding.span, observed[1].root.binding.span);
    assert_eq!(observed[2].root.binding.name, "left");
    assert_eq!(observed[3].root.binding.name, "right");
    assert_eq!(observed[2].root.binding.span, observed[3].root.binding.span);
    assert_ne!(observed[2].root.binding, observed[3].root.binding);
}

#[test]
fn argument_places_label_deferred_closure_parameters() {
    let source = r#"
fn observe(x: i32) i32 { ret x; }
fn main() void { var closure: fn(i32, i32) i32 = |left, right| observe(left) + observe(right); closure(1, 2); }
"#;
    let report = analyze(source);
    let left = arguments(&report, source, "main", "observe(left)");
    let right = arguments(&report, source, "main", "observe(right)");
    assert_eq!(left.deferred_closures.len(), 1);
    assert_eq!(left.deferred_closures, right.deferred_closures);
    let left = &place(&left.actuals[0]).root;
    let right = &place(&right.actuals[0]).root;
    assert_eq!(left.storage, OwnershipPlaceStorage::ClosureParameter);
    assert_eq!(right.storage, OwnershipPlaceStorage::ClosureParameter);
    assert_eq!(left.binding.span, right.binding.span);
    assert_ne!(left.binding, right.binding);
    assert!(
        arguments(&report, source, "main", "closure(1, 2)")
            .deferred_closures
            .is_empty()
    );
}

#[test]
fn argument_places_substitute_caller_binders_before_same_named_aliases() {
    let source = r#"
type T = i64;
fn observe[U](value: U) void {}
fn caller[T](value: T) void { observe[T](value); }
fn main() void { caller[i32](7); }
"#;
    let report = analyze(source);
    let args = arguments(&report, source, "caller", "observe[T](value)");
    assert!(matches!(args.actuals[0].ty, Some(TypeKind::Int32)));
    let root = &place(&args.actuals[0]).root;
    assert!(matches!(root.ty, Some(TypeKind::Int32)));
    assert_eq!(root.storage, OwnershipPlaceStorage::Parameter(0));
}

#[test]
fn argument_places_keep_calls_casts_and_overloaded_indexes_conservative() {
    let source = r#"
trait Index { fn index(self: &Box, index: i32) i32; }
struct Box { value: i32 }
impl Index for Box { fn index(self: &Box, index: i32) i32 { ret self.value; } }
fn fresh() i32 { ret 1; }
fn observe(x: i32) void {}
fn main() void { var value = 1; var box = Box { value: 2 }; observe(value as i32); observe(fresh()); observe(box[0]); }
"#;
    let report = analyze(source);
    assert!(matches!(
        arguments(&report, source, "main", "observe(value as i32)").actuals[0].value,
        OwnershipArgumentValue::Unknown
    ));
    assert!(matches!(
        arguments(&report, source, "main", "observe(fresh())").actuals[0].value,
        OwnershipArgumentValue::Rvalue
    ));
    assert!(matches!(
        arguments(&report, source, "main", "observe(box[0])").actuals[0].value,
        OwnershipArgumentValue::Unknown
    ));
}

#[test]
fn argument_places_preserve_grouped_addresses_and_explicit_implicit_reads() {
    let source = r#"
fn shared(value: &i32) void {}
fn copied(value: i32) void {}
fn main() void { var owner = 3; shared((&owner)); var reference = &owner; copied(reference); copied(*reference); }
"#;
    let report = analyze(source);
    let address = &arguments(&report, source, "main", "shared((&owner))").actuals[0];
    assert_eq!(address.formation, OwnershipArgumentFormation::SharedAddress);
    assert_eq!(place(address).root.binding.name, "owner");
    assert!(place(address).projections.is_empty());
    let implicit = &arguments(&report, source, "main", "copied(reference)").actuals[0];
    assert!(implicit.auto_deref);
    assert!(matches!(
        place(implicit).projections.as_slice(),
        [OwnershipProjection::Deref {
            kind: OwnershipDerefKind::Shared,
            implicit: true
        }]
    ));
    assert!(matches!(
        place(implicit).root.ty,
        Some(TypeKind::Ref { .. })
    ));
    let explicit = &arguments(&report, source, "main", "copied(*reference)").actuals[0];
    assert!(!explicit.auto_deref);
    assert!(matches!(
        place(explicit).projections.as_slice(),
        [OwnershipProjection::Deref {
            kind: OwnershipDerefKind::Shared,
            implicit: false
        }]
    ));
}

#[test]
fn argument_places_classify_foreign_global_storage_and_indirect_actuals() {
    let source = r#"
@api("counter") var external: i32;
fn observe(x: i32) void {}
fn indirect(callback: fn(i32) void, value: i32) void { callback(value); }
fn main() void { unsafe { observe(external); } }
"#;
    let report = analyze(source);
    let global = &place(&arguments(&report, source, "main", "observe(external)").actuals[0]).root;
    assert_eq!(global.binding.name, "external");
    assert_eq!(global.storage, OwnershipPlaceStorage::Global);
    assert!(matches!(global.ty, Some(TypeKind::Int32)));
    let node = callable(&report, "indirect");
    assert!(matches!(node.calls[0].target, OwnershipCallTarget::Opaque));
    let actual = &arguments(&report, source, "indirect", "callback(value)").actuals[0];
    assert_eq!(actual.parameter_slot, None);
    assert_eq!(
        place(actual).root.storage,
        OwnershipPlaceStorage::Parameter(1)
    );
    assert_eq!(place(actual).root.binding.name, "value");
}

#[test]
fn argument_places_keep_namespace_globals_conservative_and_aliases_canonical() {
    let module = "@api(\"counter\") pub var external: i32;";
    let main = "import globals; import globals.external as renamed; fn observe(value: i32) void {} fn main() void { unsafe { observe(globals.external); observe(renamed); } }";
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "quazi_places_globals_{}_{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("globals.qz"), module).unwrap();
    std::fs::write(directory.join("main.qz"), main).unwrap();
    let loaded =
        crate::loader::load_programs_configured(&[directory.join("main.qz")], None, false, &[])
            .unwrap();
    std::fs::remove_dir_all(&directory).unwrap();
    assert!(loaded.parse_error.is_none(), "{:?}", loaded.parse_error);
    let source = loaded.merged_source;
    let mut analyzer = Analyzer::new();
    analyzer.set_source_files(loaded.source_files);
    analyzer.set_namespaced_paths(
        loaded
            .namespaced_paths
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
    );
    let report = analyzer.analyze_program(&loaded.program);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    // Qualified C-global reads currently have no resolved-global annotation.
    // The module namespace must never be reported as their storage owner.
    let qualified = &arguments(&report, &source, "main", "observe(globals.external)").actuals[0];
    assert!(matches!(qualified.value, OwnershipArgumentValue::Unknown));
    let aliased = place(&arguments(&report, &source, "main", "observe(renamed)").actuals[0]);
    assert_eq!(aliased.root.binding.name, "globals.external");
    assert_eq!(aliased.root.storage, OwnershipPlaceStorage::Global);
    assert!(aliased.projections.is_empty());
}

#[test]
fn argument_places_keep_global_aliases_out_of_caller_binders() {
    let source = r#"
type T = i64;
@api("counter") var external: T;
@api("pointer") var pointer: *T;
fn observe(value: i64) void {}
fn caller[T]() void { unsafe { observe(external); observe((external)); observe(*pointer); } }
fn main() void { caller[i32](); }
"#;
    let report = analyze(source);
    for spelling in [
        "observe(external)",
        "observe((external))",
        "observe(*pointer)",
    ] {
        let actual = &arguments(&report, source, "caller", spelling).actuals[0];
        assert_eq!(place(actual).root.storage, OwnershipPlaceStorage::Global);
        assert!(
            matches!(actual.ty, Some(TypeKind::Int64)),
            "{spelling}: {actual:?}"
        );
        if spelling == "observe(*pointer)" {
            assert!(
                matches!(&place(actual).root.ty, Some(TypeKind::RawPtr { inner }) if matches!(inner.node, TypeKind::Int64))
            );
        } else {
            assert!(matches!(place(actual).root.ty, Some(TypeKind::Int64)));
        }
    }
}

#[test]
fn argument_places_do_not_label_projected_temporaries_as_values() {
    let source = r#"
struct Pair { value: i32 }
unsafe fn pointer() *i32 { ret 0; }
fn pair() Pair { ret Pair { value: 7 }; }
fn array() [i32; 2] { ret [1, 2]; }
fn observe(value: i32) void {}
fn main() void { unsafe { observe(*pointer()); } observe(pair().value); observe(array()[0]); }
"#;
    let report = analyze(source);
    for spelling in [
        "observe(*pointer())",
        "observe(pair().value)",
        "observe(array()[0])",
    ] {
        assert!(matches!(
            arguments(&report, source, "main", spelling).actuals[0].value,
            OwnershipArgumentValue::Unknown
        ));
    }
}

#[test]
fn argument_places_cancel_collection_and_reset_with_the_analyzer() {
    let program = Parser::new(
        Lexer::new("fn observe(value: i32) void {} fn main() void { var x = 1; observe(x); }")
            .tokenize(),
    )
    .parse()
    .unwrap();
    let mut analyzer = Analyzer::new();
    assert!(analyzer.analyze_program(&program).errors.is_empty());
    for sites in analyzer.ownership_call_sites.values_mut() {
        for site in sites.values_mut() {
            site.arguments = None;
        }
    }
    let mut polls = 0;
    let result = analyzer.record_ownership_argument_places(&program, &mut || {
        polls += 1;
        if polls == 5 {
            Err(crate::cancel::Cancelled)
        } else {
            Ok(())
        }
    });
    assert_eq!(result, Err(crate::cancel::Cancelled));
    assert!(
        analyzer
            .ownership_call_sites
            .values()
            .flat_map(|sites| sites.values())
            .all(|site| site.arguments.is_none())
    );
    let empty = Parser::new(Lexer::new("").tokenize()).parse().unwrap();
    assert!(
        analyzer
            .analyze_program(&empty)
            .ownership_call_graph
            .is_empty()
    );
    assert!(analyzer.ownership_call_sites.is_empty());
}

#[test]
fn argument_places_keep_exclusive_addresses_and_raw_dereferences() {
    let source = r#"
fn exclusive(value: &i32!) void {}
fn observe(value: i32) void {}
unsafe fn raw(pointer: *i32) void { observe(*pointer); }
fn main() void {
    var owner = 1;
    exclusive(&owner!);
    var reference = &owner!;
    observe(*reference);
}
"#;
    let report = analyze(source);
    let address = &arguments(&report, source, "main", "exclusive(&owner!)").actuals[0];
    assert_eq!(
        address.formation,
        OwnershipArgumentFormation::ExclusiveAddress
    );
    assert_eq!(place(address).root.binding.name, "owner");
    assert!(place(address).projections.is_empty());
    let reference = &arguments(&report, source, "main", "observe(*reference)").actuals[0];
    assert!(matches!(
        place(reference).projections.as_slice(),
        [OwnershipProjection::Deref {
            kind: OwnershipDerefKind::Exclusive,
            implicit: false
        }]
    ));
    let pointer = &arguments(&report, source, "raw", "observe(*pointer)").actuals[0];
    assert_eq!(
        place(pointer).root.storage,
        OwnershipPlaceStorage::Parameter(0)
    );
    assert!(matches!(
        place(pointer).projections.as_slice(),
        [OwnershipProjection::Deref {
            kind: OwnershipDerefKind::Raw,
            implicit: false
        }]
    ));
}
