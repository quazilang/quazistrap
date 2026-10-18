// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use super::*;
use crate::{lexer::Lexer, parser::Parser};

const MEMORY: &str = r#"
@intrinsic("quazi.malloc") unsafe fn allocate(n: usize) *u8;
@intrinsic("quazi.memcpy") unsafe fn copy(dst: *u8, src: *u8, n: usize) *u8;
@intrinsic("quazi.memmove") unsafe fn relocate(dst: *u8, src: *u8, n: usize) *u8;
@intrinsic("quazi.str.as_ptr") unsafe fn text_bytes(text: str) *u8;
@intrinsic("quazi.str.from_ptr") unsafe fn text_view(pointer: *u8) str;
unsafe fn new_storage() *u8 { ret allocate(8); }
unsafe fn nested_storage() *u8 { ret new_storage(); }
"#;

fn dependencies(body: &str) -> Vec<usize> {
    let source = format!("{MEMORY}\n{body}");
    let program = Parser::new(Lexer::new(&source).tokenize()).parse().unwrap();
    let mut analyzer = Analyzer::new();
    let report = analyzer.analyze_program(&program);
    assert!(report.errors.is_empty(), "{source}\n{:?}", report.errors);
    analyzer.result_retention["wrapper"]
        .iter()
        .copied()
        .collect()
}

#[test]
fn copied_view_dependencies_survive_direct_and_helper_allocations() {
    for allocation in ["allocate(8)", "new_storage()", "nested_storage()"] {
        assert_eq!(
            dependencies(&format!(
                r#"
unsafe fn wrapper(view: str) str {{
    var source = allocate(8);
    *(source as *str) = view;
    var pointer = {allocation};
    copy(pointer, source, 8);
    ret *(pointer as *str);
}}
"#
            )),
            [0],
            "{allocation}"
        );
    }
}

#[test]
fn temporary_copy_destinations_preserve_the_stored_view() {
    assert_eq!(
        dependencies(
            r#"
unsafe fn wrapper(view: str) str {
    var source = allocate(8);
    *(source as *str) = view;
    ret *(copy(new_storage(), source, 8) as *str);
}
"#
        ),
        [0]
    );
}

#[test]
fn aggregate_returned_storage_preserves_later_writes() {
    assert_eq!(
        dependencies(
            r#"
struct Buffer { pointer: *u8 }
unsafe fn make_buffer() Buffer { ret Buffer { pointer: allocate(8) }; }
unsafe fn wrapper(view: str) str {
    var source = allocate(8);
    *(source as *str) = view;
    var buffer = make_buffer();
    copy(buffer.pointer, source, 8);
    ret *(buffer.pointer as *str);
}
"#
        ),
        [0]
    );
}

#[test]
fn loaded_pointer_targets_preserve_writes_to_original_storage() {
    assert_eq!(
        dependencies(
            r#"
unsafe fn wrapper(view: str) str {
    var source = allocate(8);
    *(source as *str) = view;
    var destination = allocate(8);
    var slot = allocate(8);
    *(slot as **u8) = destination;
    var loaded = *(slot as **u8);
    copy(loaded, source, 8);
    ret *(destination as *str);
}
"#
        ),
        [0]
    );
}

#[test]
fn writes_through_unknown_aliases_reach_other_pointer_parameters() {
    assert_eq!(
        dependencies(
            r#"
unsafe fn wrapper(left: *str, right: *str, view: str) str {
    *left = view;
    ret *right;
}
"#
        ),
        [1, 2]
    );
}

#[test]
fn copies_through_unknown_aliases_reach_other_pointer_parameters() {
    assert_eq!(
        dependencies(
            r#"
unsafe fn wrapper(left: *u8, right: *u8, view: str) str {
    var source = allocate(8);
    *(source as *str) = view;
    copy(left, source, 8);
    ret *(right as *str);
}
"#
        ),
        [1, 2]
    );
}

#[test]
fn independent_known_allocations_keep_their_own_view_dependencies() {
    assert_eq!(
        dependencies(
            r#"
unsafe fn wrapper(first: str, second: str) str {
    var left = allocate(8);
    var right = allocate(8);
    *(left as *str) = first;
    *(right as *str) = second;
    ret *(left as *str);
}
"#
        ),
        [0]
    );
}

#[test]
fn copying_utf8_bytes_does_not_copy_a_view_handle() {
    assert!(
        dependencies(
            r#"
unsafe fn wrapper(view: str) str {
    var pointer = allocate(8);
    copy(pointer, text_bytes(view), 8);
    ret text_view(pointer);
}
"#
        )
        .is_empty()
    );
}

#[test]
fn branch_and_loop_allocations_preserve_later_writes() {
    for setup in [
        "var pointer: *u8 = 0; if flag { var local = allocate(8); pointer = local; } else { pointer = allocate(8); }",
        "var pointer = allocate(8); for flag { var local = allocate(8); pointer = local; }",
    ] {
        assert_eq!(
            dependencies(&format!(
                r#"
unsafe fn wrapper(view: str, flag: bool) str {{
    {setup}
    *(pointer as *str) = view;
    ret *(pointer as *str);
}}
"#
            )),
            [0],
            "{setup}"
        );
    }
}

#[test]
fn opaque_pointer_returns_preserve_subsequent_stores() {
    assert_eq!(
        dependencies(
            r#"
@api("make_storage") unsafe fn external_storage() *u8;
unsafe fn wrapper(view: str) str {
    var pointer = external_storage();
    *(pointer as *str) = view;
    ret *(pointer as *str);
}
"#
        ),
        [0]
    );
}

#[test]
fn known_writes_are_visible_to_unknown_alias_reads() {
    assert_eq!(
        dependencies(
            r#"
unsafe fn wrapper(pointer: *str, view: str) str {
    var destination = allocate(8);
    *(destination as *str) = view;
    ret *pointer;
}
"#
        ),
        [0, 1]
    );
}

#[test]
fn unknown_writes_are_visible_to_known_allocation_reads() {
    assert_eq!(
        dependencies(
            r#"
unsafe fn wrapper(pointer: *str, view: str) str {
    var destination = allocate(8);
    *pointer = view;
    ret *(destination as *str);
}
"#
        ),
        [1]
    );
}

#[test]
fn static_view_results_do_not_observe_unrelated_raw_writes() {
    assert!(
        dependencies(
            r#"
struct View { text: str }
fn static_view(unused: str) View { ret View { text: "static" }; }
unsafe fn wrapper(view: str) View {
    var unrelated = allocate(8);
    *(unrelated as *str) = view;
    ret static_view(view);
}
"#
        )
        .is_empty()
    );
}

#[test]
fn memory_copy_slots_follow_declared_names() {
    assert_eq!(
        dependencies(
            r#"
unsafe fn wrapper(view: str) str {
    var source = allocate(8);
    *(source as *str) = view;
    var destination = allocate(8);
    relocate(n = 8, src = source, dst = destination);
    ret *(destination as *str);
}
"#
        ),
        [0]
    );
}

#[test]
fn source_pointer_helpers_keep_copied_dependencies() {
    assert_eq!(
        dependencies(
            r#"
unsafe fn forward(pointer: *u8) *u8 { ret pointer; }
unsafe fn store(pointer: *str, view: str) void { *pointer = view; }
unsafe fn wrapper(view: str) str {
    var pointer = allocate(8);
    store(pointer, view);
    ret *(forward(pointer) as *str);
}
"#
        ),
        [0]
    );
}

#[test]
fn unsupported_reconstruction_and_projected_addresses_stay_rejected() {
    for (source, code) in [
        (
            "unsafe fn probe(pointer: *u8) void { var bits = pointer as usize; }",
            "S06",
        ),
        (
            "unsafe fn probe(bits: usize) void { var pointer = bits as *u8; }",
            "S06",
        ),
        (
            "unsafe fn probe(pointer: *str) void { var reference = &*pointer!; }",
            "S14",
        ),
    ] {
        let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        let report = Analyzer::new().analyze_program(&program);
        assert!(
            report.errors.iter().any(|error| error.code == code),
            "{source}: {:?}",
            report.errors
        );
    }
}

#[test]
fn implicit_reference_projection_loads_stored_pointer_identity() {
    for setup in ["struct Holder { pointer: *u8 }", ""] {
        let projection = if setup.is_empty() {
            "var holder = [target]; var reference = &holder; var pointer: *u8 = reference[0];"
        } else {
            "var holder = Holder { pointer: target }; var reference = &holder; var pointer: *u8 = reference.pointer;"
        };
        assert_eq!(
            dependencies(&format!(
                r#"
{setup}
unsafe fn wrapper(view: str) str {{
    var target = allocate(8);
    {projection}
    *(pointer as *str) = view;
    ret *(target as *str);
}}
"#
            )),
            [0],
            "{projection}"
        );
    }
}

#[test]
fn directly_addressed_view_slots_keep_their_owner_dependency() {
    assert_eq!(
        dependencies(
            r#"
@intrinsic("quazi.memcpy") unsafe fn slot_copy(destination: *u8, source: *str, length: usize) *u8;
unsafe fn wrapper(view: str) str {
    var destination = new_storage();
    slot_copy(length = 8, source = &view, destination = destination);
    ret *(destination as *str);
}
"#
        ),
        [0]
    );
}

#[test]
fn iteration_over_reference_arrays_loads_pointer_values() {
    assert_eq!(
        dependencies(
            r#"
unsafe fn wrapper(view: str) str {
    var target = allocate(8);
    var pointers = [target];
    for pointer : &pointers { *(pointer as *str) = view; }
    ret *(target as *str);
}
"#
        ),
        [0]
    );
}

#[test]
fn raw_string_pointer_round_trips_keep_known_stored_contents() {
    assert_eq!(
        dependencies(
            r#"
unsafe fn wrapper(view: str) str {
    var source = allocate(8);
    *(source as *str) = view;
    var destination = allocate(8);
    copy(destination, text_bytes(text_view(source)), 8);
    ret *(destination as *str);
}
"#
        ),
        [0]
    );
}

#[test]
fn memset_weak_updates_do_not_erase_view_dependencies() {
    assert_eq!(
        dependencies(
            r#"
@intrinsic("quazi.memset") unsafe fn clear(destination: *u8, byte: u8, length: usize) *u8;
unsafe fn wrapper(view: str) str {
    var destination = allocate(8);
    *(destination as *str) = view;
    clear(destination, 0 as u8, 8);
    ret *(destination as *str);
}
"#
        ),
        [0]
    );
}

#[test]
fn byte_pointer_local_aliases_and_named_intrinsics_remain_independent() {
    assert!(
        dependencies(
            r#"
unsafe fn wrapper(view: str) str {
    var byte_pointer = text_bytes(text = view);
    var alias = byte_pointer;
    var destination = allocate(n = 8);
    copy(n = 8, dst = destination, src = alias);
    ret text_view(pointer = destination);
}
"#
        )
        .is_empty()
    );
}

#[test]
fn memory_facts_and_intrinsic_registry_reset_between_analyses() {
    let program = Parser::new(Lexer::new(MEMORY).tokenize()).parse().unwrap();
    let mut analyzer = Analyzer::new();
    assert!(analyzer.analyze_program(&program).errors.is_empty());
    assert!(!analyzer.retention_intrinsics.is_empty());
    let empty = Parser::new(Lexer::new("").tokenize()).parse().unwrap();
    assert!(
        analyzer
            .analyze_program(&empty)
            .source_result_retention
            .is_empty()
    );
    assert!(analyzer.retention_intrinsics.is_empty());
    assert!(analyzer.result_retention.is_empty());
}
