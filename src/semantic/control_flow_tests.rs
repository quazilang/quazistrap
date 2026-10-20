// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use super::control_flow::FlowLimits;
use super::*;
use crate::{lexer::Lexer, parser::Parser};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

fn parse(source: &str) -> Program {
    Parser::new(Lexer::new(source).tokenize()).parse().unwrap()
}
fn graphs(source: &str) -> BTreeMap<String, Arc<OwnershipControlFlow>> {
    let program = parse(source);
    let mut analyzer = Analyzer::new();
    let result = analyzer
        .record_ownership_control_flow(&program, &mut || Ok(()))
        .unwrap();
    assert!(analyzer.errors.is_empty(), "{:?}", analyzer.errors);
    result
}
fn reachable(graph: &OwnershipControlFlow, start: usize) -> BTreeSet<usize> {
    let mut result = BTreeSet::new();
    let mut pending = vec![start];
    while let Some(block) = pending.pop() {
        if result.insert(block) {
            pending.extend(graph.blocks[block].terminator.successors());
        }
    }
    result
}
fn statement_block(graph: &OwnershipControlFlow, source: &str, text: &str) -> usize {
    let start = source.find(text).unwrap();
    graph
        .blocks
        .iter()
        .position(|block| {
            block.events.iter().any(
                |event| matches!(event, OwnershipFlowEvent::Statement(span) if span.start == start),
            )
        })
        .unwrap_or_else(|| panic!("missing statement {text}"))
}

#[test]
fn source_control_flow_keeps_leaf_generic_and_terminal_statements() {
    let source =
        "fn leaf() void {} fn generic[T](value: T) void {} fn main() void { ret; after(); }";
    let maps = graphs(source);
    assert_eq!(maps.len(), 3);
    let main = &maps["main"];
    let after = statement_block(main, source, "after()");
    assert!(!reachable(main, main.entry).contains(&after));
    let returns = main
        .blocks
        .iter()
        .filter_map(|block| match &block.terminator {
            OwnershipFlowTerminator::Return { exited_scopes, .. } => Some(exited_scopes),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(returns.len(), 1);
    assert_eq!(returns[0].len(), 1);
}

#[test]
fn source_control_flow_else_if_is_only_on_previous_false_path() {
    let source = "fn main() void { if first() { yes(); } else if second() { middle(); } else { no(); } after(); }";
    let maps = graphs(source);
    let graph = &maps["main"];
    let branches = graph
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(id, block)| match &block.terminator {
            OwnershipFlowTerminator::Branch {
                condition: OwnershipBranchCondition::Expression(span),
                then_target,
                else_target,
            } => Some((span.start, id, *then_target, *else_target)),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let first = branches
        .iter()
        .find(|(start, ..)| *start == source.find("first()").unwrap())
        .unwrap();
    let second = branches
        .iter()
        .find(|(start, ..)| *start == source.find("second()").unwrap())
        .unwrap();
    assert!(!reachable(graph, first.2).contains(&second.1));
    assert!(reachable(graph, first.3).contains(&second.1));
    let after = statement_block(graph, source, "after()");
    assert!(reachable(graph, first.2).contains(&after));
    assert!(reachable(graph, second.2).contains(&after));
    assert!(reachable(graph, second.3).contains(&after));
}

#[test]
fn source_control_flow_c_continue_runs_update_and_break_exits_loop_scope() {
    let source = "fn main() void { for var i = 0; i < 3; i++ { if test() { continue; } unsafe { break; } } after(); }";
    let maps = graphs(source);
    let graph = &maps["main"];
    let update = graph
        .blocks
        .iter()
        .position(|block| {
            block.events.iter().any(|event| {
                matches!(
                    event,
                    OwnershipFlowEvent::Evaluate {
                        kind: OwnershipEvaluationKind::LoopUpdate,
                        ..
                    }
                )
            })
        })
        .unwrap();
    let continue_block = statement_block(graph, source, "continue;");
    let break_block = statement_block(graph, source, "break;");
    let after = statement_block(graph, source, "after()");
    let OwnershipFlowTerminator::Goto {
        target,
        exited_scopes,
    } = &graph.blocks[continue_block].terminator
    else {
        panic!("continue transfer");
    };
    assert_eq!(*target, update);
    assert_eq!(exited_scopes.len(), 2);
    let OwnershipFlowTerminator::Goto {
        target,
        exited_scopes,
    } = &graph.blocks[break_block].terminator
    else {
        panic!("break transfer");
    };
    assert_eq!(*target, after);
    assert_eq!(exited_scopes.len(), 3);
    assert!(
        exited_scopes
            .iter()
            .any(|scope| graph.scopes[*scope].kind == OwnershipFlowScopeKind::Loop)
    );
}

#[test]
fn source_control_flow_returns_exit_nested_scopes_and_never_reach_join() {
    let source = "fn main() void { unsafe { if test() { ret; } else { ret; } after(); } }";
    let maps = graphs(source);
    let graph = &maps["main"];
    let after = statement_block(graph, source, "after()");
    assert!(!reachable(graph, graph.entry).contains(&after));
    for block in &graph.blocks {
        if let OwnershipFlowTerminator::Return { exited_scopes, .. } = &block.terminator {
            assert_eq!(
                exited_scopes,
                &block.scopes.iter().rev().copied().collect::<Vec<_>>()
            );
            assert_eq!(exited_scopes.len(), 3);
        }
    }
}

#[test]
fn source_control_flow_try_and_deferred_closure_have_separate_returns() {
    let source = "fn main() void { var closure = || nested()?; use(outer()?); }";
    let maps = graphs(source);
    let graph = &maps["main"];
    assert_eq!(graph.deferred.len(), 1);
    let early = graph
        .blocks
        .iter()
        .filter(|block| {
            matches!(
                block.terminator,
                OwnershipFlowTerminator::Return {
                    kind: OwnershipReturnKind::TryFailure,
                    ..
                }
            )
        })
        .count();
    assert_eq!(early, 1);
    let declaration = statement_block(graph, source, "var closure");
    assert!(matches!(
        graph.blocks[declaration].terminator,
        OwnershipFlowTerminator::Opaque {
            early_return: None,
            ..
        }
    ));
    let deferred = &graph.deferred[0].graph;
    assert!(deferred.blocks.iter().any(|block| matches!(
        block.terminator,
        OwnershipFlowTerminator::Return {
            kind: OwnershipReturnKind::TryFailure,
            ..
        }
    )));
    assert!(deferred.blocks.iter().any(|block| matches!(
        block.terminator,
        OwnershipFlowTerminator::Return {
            kind: OwnershipReturnKind::ClosureResult,
            ..
        }
    )));
}

#[test]
fn source_control_flow_iteration_evaluates_bounds_once_and_protocol_repeats() {
    let source = "fn main() void { for i : lower()..upper() { continue; } after(); }";
    let maps = graphs(source);
    let graph = &maps["main"];
    let input = graph.blocks.iter().position(|block| block.events.iter().any(|event| matches!(event, OwnershipFlowEvent::Evaluate { kind: OwnershipEvaluationKind::IterationInput, spans } if spans.len() == 2))).unwrap();
    let prepare = graph
        .blocks
        .iter()
        .position(|block| {
            block.events.iter().any(|event| {
                matches!(
                    event,
                    OwnershipFlowEvent::IterationProtocol {
                        stage: OwnershipIterationStage::Prepare,
                        ..
                    }
                )
            })
        })
        .unwrap();
    let next = graph
        .blocks
        .iter()
        .position(|block| {
            block.events.iter().any(|event| {
                matches!(
                    event,
                    OwnershipFlowEvent::IterationProtocol {
                        stage: OwnershipIterationStage::Next,
                        ..
                    }
                )
            })
        })
        .unwrap();
    assert!(!reachable(graph, next).contains(&input));
    assert!(!reachable(graph, next).contains(&prepare));
    let continue_block = statement_block(graph, source, "continue;");
    assert!(
        matches!(graph.blocks[continue_block].terminator, OwnershipFlowTerminator::Goto { target, .. } if target == next)
    );
}

#[test]
fn source_control_flow_disabled_cfg_is_absent() {
    let source = "@cfg(target_os = \"impossible\") fn absent() void {} fn main() void { @cfg(target_os = \"impossible\") { hidden(); } visible(); }";
    let maps = graphs(source);
    assert!(!maps.contains_key("absent"));
    let graph = &maps["main"];
    assert!(!graph.blocks.iter().flat_map(|block| &block.events).any(|event| matches!(event, OwnershipFlowEvent::Statement(span) if span.start == source.find("hidden()").unwrap())));
    assert!(reachable(graph, graph.entry).contains(&statement_block(graph, source, "visible()")));
}

#[test]
fn source_control_flow_limits_are_aggregate_and_publish_no_partial_map() {
    let program = parse("fn first() void {} fn second() void {} fn main() void {}");
    for limits in [
        FlowLimits {
            blocks: 3,
            ..FlowLimits::default()
        },
        FlowLimits {
            edges: 1,
            ..FlowLimits::default()
        },
        FlowLimits {
            nodes: 1,
            ..FlowLimits::default()
        },
        FlowLimits {
            depth: 0,
            ..FlowLimits::default()
        },
    ] {
        let mut analyzer = Analyzer::new();
        let result = analyzer
            .record_ownership_control_flow_with_limits(&program, &mut || Ok(()), limits)
            .unwrap();
        assert!(result.is_empty());
        assert!(analyzer.errors.iter().any(|error| error.code == "S14"));
    }
}

#[test]
fn source_control_flow_cancellation_polls_within_expression_regions() {
    let program = parse("fn main() void { use(1 + 2 + 3 + 4 + 5); }");
    let mut analyzer = Analyzer::new();
    let mut polls = 0;
    let result = analyzer.record_ownership_control_flow(&program, &mut || {
        polls += 1;
        if polls == 12 {
            Err(crate::cancel::Cancelled)
        } else {
            Ok(())
        }
    });
    assert!(matches!(result, Err(crate::cancel::Cancelled)));
    assert_eq!(polls, 12);
}

#[test]
fn source_control_flow_nested_loops_transfer_to_innermost_targets() {
    let source = "fn main() void { for var outer = 0; outer < 3; outer++ { for var inner = 0; inner < 2; inner++ { if test() { continue; } break; } outer_after(); } done(); }";
    let maps = graphs(source);
    let graph = &maps["main"];
    let updates = graph
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(id, block)| {
            block.events.iter().find_map(|event| match event {
                OwnershipFlowEvent::Evaluate {
                    spans,
                    kind: OwnershipEvaluationKind::LoopUpdate,
                } => Some((spans[0].start, id)),
                _ => None,
            })
        })
        .collect::<BTreeMap<_, _>>();
    let inner_update = updates[&source.find("inner++").unwrap()];
    let outer_update = updates[&source.find("outer++").unwrap()];
    let continue_block = statement_block(graph, source, "continue;");
    assert!(
        matches!(graph.blocks[continue_block].terminator, OwnershipFlowTerminator::Goto { target, .. } if target == inner_update && target != outer_update)
    );
    let break_block = statement_block(graph, source, "break;");
    let after = statement_block(graph, source, "outer_after()");
    assert!(
        matches!(graph.blocks[break_block].terminator, OwnershipFlowTerminator::Goto { target, .. } if target == after)
    );
    assert!(reachable(graph, after).contains(&outer_update));
}

#[test]
fn source_control_flow_try_failures_in_loop_regions_exit_entire_callable() {
    let source = "fn main() void { for var i = init()?; condition()?; update()? { body(); } for j : low()?..high()? { body_two(); } }";
    let maps = graphs(source);
    let graph = &maps["main"];
    let mut regions = BTreeSet::new();
    for block in &graph.blocks {
        let OwnershipFlowTerminator::Opaque {
            early_return: Some(failure),
            ..
        } = block.terminator
        else {
            continue;
        };
        let OwnershipFlowTerminator::Return {
            kind: OwnershipReturnKind::TryFailure,
            exited_scopes,
            ..
        } = &graph.blocks[failure].terminator
        else {
            panic!("try failure is a return");
        };
        assert!(graph.blocks[failure].terminator.successors().is_empty());
        assert_eq!(
            exited_scopes,
            &block.scopes.iter().rev().copied().collect::<Vec<_>>()
        );
        assert!(
            exited_scopes
                .iter()
                .any(|scope| graph.scopes[*scope].kind == OwnershipFlowScopeKind::Loop)
        );
        for event in &block.events {
            if let OwnershipFlowEvent::Evaluate { kind, .. } = event {
                regions.insert(format!("{kind:?}"));
            }
        }
    }
    for expected in ["Statement", "Condition", "LoopUpdate", "IterationInput"] {
        assert!(regions.contains(expected));
    }
}

#[test]
fn source_control_flow_validator_rejects_broken_targets_scopes_and_transfers() {
    let maps = graphs("fn main() void { unsafe { consume(); } }");
    let graph = maps["main"].as_ref();
    graph.validate().unwrap();
    let mut invalid = graph.clone();
    invalid.entry = invalid.blocks.len();
    assert!(invalid.validate().unwrap_err().contains("entry"));
    let mut invalid = graph.clone();
    invalid.entry = 1;
    assert!(invalid.validate().unwrap_err().contains("entry"));
    let mut invalid = graph.clone();
    invalid.blocks[invalid.entry].scopes.push(usize::MAX);
    assert!(invalid.validate().is_err());
    let mut invalid = graph.clone();
    invalid.scopes[1].parent = Some(1);
    assert!(invalid.validate().unwrap_err().contains("parent"));
    let mut invalid = graph.clone();
    invalid.blocks[invalid.entry].terminator = OwnershipFlowTerminator::Goto {
        target: usize::MAX,
        exited_scopes: Vec::new(),
    };
    assert!(invalid.validate().unwrap_err().contains("target"));
    let mut invalid = graph.clone();
    let exit = invalid.blocks.iter_mut().find(|block| matches!(&block.terminator, OwnershipFlowTerminator::Goto { exited_scopes, .. } if !exited_scopes.is_empty())).unwrap();
    let OwnershipFlowTerminator::Goto { exited_scopes, .. } = &mut exit.terminator else {
        unreachable!()
    };
    exited_scopes.clear();
    assert!(invalid.validate().unwrap_err().contains("exited scopes"));
    let mut invalid = graph.clone();
    invalid.blocks[invalid.entry].terminator = OwnershipFlowTerminator::Stop {
        exited_scopes: Vec::new(),
    };
    assert!(
        invalid
            .validate()
            .unwrap_err()
            .contains("exit every active scope")
    );
}

#[test]
fn source_control_flow_expression_fanout_and_deferred_work_are_bounded() {
    let source = format!(
        "fn main() void {{ use({}); }}",
        std::iter::repeat_n("1", 100).collect::<Vec<_>>().join(",")
    );
    let program = parse(&source);
    let mut analyzer = Analyzer::new();
    let result = analyzer
        .record_ownership_control_flow_with_limits(
            &program,
            &mut || Ok(()),
            FlowLimits {
                nodes: 40,
                ..FlowLimits::default()
            },
        )
        .unwrap();
    assert!(result.is_empty());
    assert!(analyzer.errors.iter().any(|error| error.code == "S14"));
    let program = parse("fn main() void { var closure = || || || || value; }");
    let mut analyzer = Analyzer::new();
    let result = analyzer
        .record_ownership_control_flow_with_limits(
            &program,
            &mut || Ok(()),
            FlowLimits {
                depth: 2,
                ..FlowLimits::default()
            },
        )
        .unwrap();
    assert!(result.is_empty());
    assert!(analyzer.errors.iter().any(|error| error.code == "S14"));
}

#[test]
fn source_control_flow_opaque_scan_accepts_deep_binary_expression() {
    let source = format!(
        "fn main() void {{ use({}); }}",
        std::iter::repeat_n("1", 200).collect::<Vec<_>>().join("+")
    );
    let maps = graphs(&source);
    maps["main"].validate().unwrap();
    assert!(
        maps["main"]
            .blocks
            .iter()
            .any(|block| block.events.iter().any(|event| matches!(
                event,
                OwnershipFlowEvent::Evaluate {
                    kind: OwnershipEvaluationKind::Statement,
                    ..
                }
            )))
    );
}
