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

/// The block that starts evaluating the expression at `text`: an opaque
/// region containing it or its result identity.
fn expression_block(graph: &OwnershipControlFlow, source: &str, text: &str) -> usize {
    let start = source.find(text).unwrap();
    graph
        .blocks
        .iter()
        .position(|block| {
            block.events.iter().any(|event| match event {
                OwnershipFlowEvent::Evaluate { spans, .. } => spans[0].start == start,
                OwnershipFlowEvent::ExpressionResult { span, .. } => span.start == start,
                _ => false,
            })
        })
        .unwrap_or_else(|| panic!("missing expression {text}"))
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
    let update = expression_block(graph, source, "i++");
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
fn source_control_flow_invokes_direct_calls_in_declared_slot_order() {
    let source = r#"
fn step(value: Option[i32]) Option[i32] { ret value; }
fn later() i32 { ret 2; }
fn combine(first: i32, second: i32) i32 { ret first + second; }
fn sequence(value: Option[i32]) Option[i32] {
    const sum = combine(second = later(), first = step(value)?);
    ret Some(sum);
}
fn defer() void {
    const work: fn() i32 = || later();
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = |name| {
        report.ownership_call_graph.iter().find(|callable| callable.id.callable == name)
            .unwrap().control_flow.as_deref().unwrap()
    };
    let invoke = |graph: &OwnershipControlFlow, needle| {
        let start = source.rfind(needle).unwrap();
        graph.blocks.iter().position(|block| block.events.iter().any(
            |event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == start),
        ))
    };

    let sequence = graph("sequence");
    let step = invoke(sequence, "step(value)?").unwrap();
    let combine = invoke(sequence, "combine(second").unwrap();
    let later = sequence.blocks.iter().position(|block| block.events.iter().any(
        |event| matches!(event, OwnershipFlowEvent::Invoke { span }
            if span.start == source.find("later(), first").unwrap()),
    )).unwrap();
    let (success, failure) = sequence.blocks.iter().find_map(|block| match block.terminator {
        OwnershipFlowTerminator::Branch {
            condition: OwnershipBranchCondition::TrySuccess { .. },
            then_target,
            else_target,
        } => Some((then_target, else_target)),
        _ => None,
    }).unwrap();
    assert!(reachable(sequence, step).contains(&failure));
    assert!(reachable(sequence, success).contains(&later));
    assert!(reachable(sequence, later).contains(&combine));
    assert!(!reachable(sequence, later).contains(&step));
    assert!(!reachable(sequence, failure).contains(&later));
    assert!(!reachable(sequence, failure).contains(&combine));

    let deferred = graph("defer");
    assert!(invoke(deferred, "later();").is_none());
    assert!(invoke(&deferred.deferred[0].graph, "later();").is_some());
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
    let inner_update = expression_block(graph, source, "inner++");
    let outer_update = expression_block(graph, source, "outer++");
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
        let failure = match block.terminator {
            OwnershipFlowTerminator::Opaque {
                early_return: Some(failure),
                ..
            } => failure,
            OwnershipFlowTerminator::Opaque {
                normal,
                early_return: None,
            } => match graph.blocks[normal].terminator {
                OwnershipFlowTerminator::Branch {
                    condition: OwnershipBranchCondition::TrySuccess { .. },
                    else_target,
                    ..
                } => else_target,
                _ => continue,
            },
            _ => continue,
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

fn evaluation_block(graph: &OwnershipControlFlow, source: &str, text: &str) -> usize {
    let start = source.find(text).unwrap();
    graph
        .blocks
        .iter()
        .position(|block| {
            block.events.iter().any(|event| {
                matches!(event, OwnershipFlowEvent::Evaluate { spans, .. }
                    if spans.iter().any(|span| span.start == start))
            })
        })
        .unwrap_or_else(|| panic!("missing evaluation {text}"))
}

#[test]
fn source_control_flow_logical_skip_excludes_rhs_and_its_try_exit() {
    for (operator, right_on) in [("&&", true), ("||", false)] {
        let source =
            format!("fn main() void {{ var value = left() {operator} right()?; done(); }}");
        let maps = graphs(&source);
        let graph = &maps["main"];
        graph.validate().unwrap();
        let left = source.find("left()").unwrap();
        let (then_target, else_target) = graph
            .blocks
            .iter()
            .find_map(|block| match block.terminator {
                OwnershipFlowTerminator::Branch {
                    condition: OwnershipBranchCondition::Expression(span),
                    then_target,
                    else_target,
                } if span.start == left => Some((then_target, else_target)),
                _ => None,
            })
            .unwrap();
        let (rhs, skip) = if right_on {
            (then_target, else_target)
        } else {
            (else_target, then_target)
        };
        let skip_path = reachable(graph, skip);
        let rhs_path = reachable(graph, rhs);
        let rhs_evaluation = evaluation_block(graph, &source, "right()");
        assert!(!skip_path.contains(&rhs_evaluation));
        assert!(rhs_path.contains(&rhs_evaluation));
        let failures = graph
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(id, block)| {
                matches!(
                    block.terminator,
                    OwnershipFlowTerminator::Return {
                        kind: OwnershipReturnKind::TryFailure,
                        ..
                    }
                )
                .then_some(id)
            })
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1);
        assert!(!skip_path.contains(&failures[0]));
        assert!(rhs_path.contains(&failures[0]));
        assert!(skip_path.iter().any(|id| graph.blocks[*id].events.iter().any(|event| {
            matches!(event, OwnershipFlowEvent::ExpressionResult { span, value: OwnershipExpressionResult::Boolean(value) }
                if span.start == left && *value == !right_on)
        })));
        assert!(rhs_path.iter().any(|id| graph.blocks[*id].events.iter().any(|event| {
            matches!(event, OwnershipFlowEvent::ExpressionResult { span, value: OwnershipExpressionResult::Truth(_) } if span.start == left)
        })));
    }
}

#[test]
fn source_control_flow_eager_failure_skips_later_operand_and_preserves_unary_result() {
    let source = "fn main() void { var value = -(first()? + later()); done(); }";
    let maps = graphs(source);
    let graph = &maps["main"];
    let (success, failure) = graph
        .blocks
        .iter()
        .find_map(|block| match block.terminator {
            OwnershipFlowTerminator::Branch {
                condition: OwnershipBranchCondition::TrySuccess { operand, .. },
                then_target,
                else_target,
            } if operand.start == source.find("first()").unwrap() => {
                Some((then_target, else_target))
            }
            _ => None,
        })
        .unwrap();
    let later = evaluation_block(graph, source, "later()");
    let done = statement_block(graph, source, "done()");
    assert!(reachable(graph, success).contains(&later));
    assert!(reachable(graph, success).contains(&done));
    assert!(!reachable(graph, failure).contains(&later));
    assert!(!reachable(graph, failure).contains(&done));
    assert!(
        graph
            .blocks
            .iter()
            .flat_map(|block| &block.events)
            .any(|event| matches!(
                event,
                OwnershipFlowEvent::ExpressionResult {
                    value: OwnershipExpressionResult::Unary {
                        op: UnaryOpKind::Neg,
                        ..
                    },
                    ..
                }
            ))
    );
    assert!(
        graph
            .blocks
            .iter()
            .flat_map(|block| &block.events)
            .any(|event| matches!(
                event,
                OwnershipFlowEvent::ExpressionResult {
                    value: OwnershipExpressionResult::Binary {
                        op: BinOpKind::Add,
                        ..
                    },
                    ..
                }
            ))
    );
    assert!(!graph.blocks.iter().any(|block| matches!(
        block.terminator,
        OwnershipFlowTerminator::Opaque {
            early_return: Some(_),
            ..
        }
    )));
}

#[test]
fn source_control_flow_wrappers_keep_distinct_result_identities() {
    let source =
        "fn main() void { if !(left() && right()?) { body(); } var value = (first()? as i32); }";
    let maps = graphs(source);
    let graph = &maps["main"];
    let not_start = source.find('!').unwrap();
    assert!(graph.blocks.iter().any(|block| matches!(block.terminator,
        OwnershipFlowTerminator::Branch { condition: OwnershipBranchCondition::Expression(span), .. } if span.start == not_start)));
    assert!(graph.blocks.iter().flat_map(|block| &block.events).any(|event| matches!(event,
        OwnershipFlowEvent::ExpressionResult { span, value: OwnershipExpressionResult::Unary { op: UnaryOpKind::Not, .. } } if span.start == not_start)));
    assert!(
        graph
            .blocks
            .iter()
            .flat_map(|block| &block.events)
            .any(|event| matches!(
                event,
                OwnershipFlowEvent::ExpressionResult {
                    value: OwnershipExpressionResult::Cast(_),
                    ..
                }
            ))
    );
    assert_eq!(
        graph
            .blocks
            .iter()
            .filter(|block| matches!(
                block.terminator,
                OwnershipFlowTerminator::Return {
                    kind: OwnershipReturnKind::TryFailure,
                    ..
                }
            ))
            .count(),
        2
    );
}

#[test]
fn source_control_flow_opaque_context_and_deferred_controls_stay_separate() {
    let source =
        "fn main() void { consume(left() && right()?); var closure = || left() || right()?; }";
    let maps = graphs(source);
    let graph = &maps["main"];
    assert_eq!(graph.deferred.len(), 1);
    assert!(
        !graph
            .blocks
            .iter()
            .any(|block| matches!(block.terminator, OwnershipFlowTerminator::Branch { .. }))
    );
    assert_eq!(
        graph
            .blocks
            .iter()
            .filter(|block| matches!(
                block.terminator,
                OwnershipFlowTerminator::Return {
                    kind: OwnershipReturnKind::TryFailure,
                    ..
                }
            ))
            .count(),
        1
    );
    let deferred = &graph.deferred[0].graph;
    assert!(deferred.blocks.iter().any(|block| matches!(
        block.terminator,
        OwnershipFlowTerminator::Branch {
            condition: OwnershipBranchCondition::TrySuccess { .. },
            ..
        }
    )));
    assert_eq!(
        deferred
            .blocks
            .iter()
            .filter(|block| matches!(
                block.terminator,
                OwnershipFlowTerminator::Return {
                    kind: OwnershipReturnKind::TryFailure,
                    ..
                }
            ))
            .count(),
        1
    );
}

#[test]
fn source_control_flow_refinement_is_iterative_bounded_and_cancellable() {
    let source = format!(
        "fn main() void {{ var value = {}; }}",
        std::iter::repeat_n("1", 200).collect::<Vec<_>>().join("+")
    );
    let maps = graphs(&source);
    let graph = &maps["main"];
    assert_eq!(
        graph
            .blocks
            .iter()
            .flat_map(|block| &block.events)
            .filter(|event| matches!(
                event,
                OwnershipFlowEvent::ExpressionResult {
                    value: OwnershipExpressionResult::Binary { .. },
                    ..
                }
            ))
            .count(),
        199
    );
    let program = parse(&source);
    let mut analyzer = Analyzer::new();
    let limited = analyzer
        .record_ownership_control_flow_with_limits(
            &program,
            &mut || Ok(()),
            FlowLimits {
                blocks: 10,
                ..FlowLimits::default()
            },
        )
        .unwrap();
    assert!(limited.is_empty());
    assert!(analyzer.errors.iter().any(|error| error.code == "S14"));
    let mut polls = 0;
    let cancelled = analyzer.record_ownership_control_flow(&program, &mut || {
        polls += 1;
        if polls == 100 {
            Err(crate::cancel::Cancelled)
        } else {
            Ok(())
        }
    });
    assert!(matches!(cancelled, Err(crate::cancel::Cancelled)));
    assert_eq!(polls, 100);
}

#[test]
fn source_control_flow_named_assignment_completes_its_value_first() {
    let source = r#"
struct Pair { left: i32, right: i32 }
fn produce(value: Option[i32]) Option[i32] { ret value; }
fn later() i32 { ret 2; }
fn assign(value: Option[i32]) Option[i32] {
    var total = 0;
    (total) = produce(value)? + later();
    var pair = Pair { left: 0, right: 0 };
    pair.left = later();
    ret Some(total + pair.left);
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "assign")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let block_with = |matches: &dyn Fn(&OwnershipFlowEvent) -> bool| {
        graph
            .blocks
            .iter()
            .position(|block| block.events.iter().any(matches))
    };
    let target = source.find("(total) =").unwrap();
    let store = block_with(&|event| {
        matches!(event, OwnershipFlowEvent::ExpressionResult {
            value: OwnershipExpressionResult::NamedAssignment { target: span, .. }, ..
        } if span.start == target)
    })
    .expect("named assignment result");
    let later = source.find("later();").unwrap();
    let invoke_later = block_with(
        &|event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == later),
    )
    .expect("the assigned value's calls are scheduled");
    let failure = graph
        .blocks
        .iter()
        .find_map(|block| match block.terminator {
            OwnershipFlowTerminator::Branch {
                condition: OwnershipBranchCondition::TrySuccess { .. },
                else_target,
                ..
            } => Some(else_target),
            _ => None,
        })
        .unwrap();
    assert!(reachable(graph, invoke_later).contains(&store));
    assert!(!reachable(graph, failure).contains(&store));
    assert!(!reachable(graph, store).contains(&invoke_later));

    // A field target is a place update: its value is scheduled too.
    let field_value = source.rfind("later();").unwrap();
    assert!(
        block_with(&|event| {
            matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == field_value)
        })
        .is_some()
    );
}

#[test]
fn source_control_flow_named_compound_assignment_reads_target_before_value() {
    let source = r#"
fn produce(value: Option[i32]) Option[i32] { ret value; }
fn later() i32 { ret 2; }
fn accumulate(value: Option[i32]) Option[i32] {
    var total = 1;
    total += produce(value)? * later();
    ret Some(total);
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "accumulate")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let block_with = |matches: &dyn Fn(&OwnershipFlowEvent) -> bool| {
        graph
            .blocks
            .iter()
            .position(|block| block.events.iter().any(matches))
            .unwrap()
    };
    let read = block_with(&|event| {
        matches!(
            event,
            OwnershipFlowEvent::ExpressionResult {
                value: OwnershipExpressionResult::NamedTargetRead(_),
                ..
            }
        )
    });
    let store = block_with(&|event| {
        matches!(
            event,
            OwnershipFlowEvent::ExpressionResult {
                value: OwnershipExpressionResult::NamedCompoundAssignment {
                    op: CompoundAssignOp::Add,
                    ..
                },
                ..
            }
        )
    });
    let produce = source.find("produce(value)?").unwrap();
    let invoke_produce = block_with(
        &|event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == produce),
    );
    let failure = graph
        .blocks
        .iter()
        .find_map(|block| match block.terminator {
            OwnershipFlowTerminator::Branch {
                condition: OwnershipBranchCondition::TrySuccess { .. },
                else_target,
                ..
            } => Some(else_target),
            _ => None,
        })
        .unwrap();
    assert!(reachable(graph, read).contains(&invoke_produce));
    assert!(reachable(graph, invoke_produce).contains(&store));
    assert!(!reachable(graph, invoke_produce).contains(&read));
    assert!(!reachable(graph, failure).contains(&store));
}

#[test]
fn source_control_flow_named_increments_have_result_identities() {
    let source = "fn main() void { var count = 0; count++; --count; for var i = 0; i < 3; i++ { after(); } }";
    let maps = graphs(source);
    let graph = maps.get("main").unwrap();
    let increments: Vec<(usize, bool)> = graph
        .blocks
        .iter()
        .flat_map(|block| block.events.iter())
        .filter_map(|event| match event {
            OwnershipFlowEvent::ExpressionResult {
                value: OwnershipExpressionResult::NamedIncDec { target, prefix, .. },
                ..
            } => Some((target.start, *prefix)),
            _ => None,
        })
        .collect();
    for (needle, prefix) in [("count++", false), ("count;", true), ("i++", false)] {
        let start = source.find(needle).unwrap();
        assert!(
            increments.contains(&(start, prefix)),
            "{needle}: {increments:?}"
        );
    }
    graph.validate().unwrap();
}

#[test]
fn source_control_flow_invokes_resolved_methods_after_receiver_and_slots() {
    let source = r#"
struct Counter { value: i32 }
impl Counter {
    fn add(self: &Counter, first: i32, second: i32) i32 { ret self.value + first + second; }
}
fn make() Counter { ret Counter { value: 1 }; }
fn step(value: Option[i32]) Option[i32] { ret value; }
fn later() i32 { ret 2; }
fn sum(value: Option[i32]) Option[i32] {
    const counter = make();
    const total = counter.add(second = later(), first = step(value)?);
    ret Some(total);
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "sum")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let invoke = |needle: &str| {
        let start = source.find(needle).unwrap();
        graph
            .blocks
            .iter()
            .position(|block| {
                block.events.iter().any(|event| {
                    matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == start)
                })
            })
            .unwrap_or_else(|| panic!("missing invoke {needle}"))
    };
    let add = invoke("counter.add(");
    let step = invoke("step(value)?");
    let later = invoke("later(), first");
    let failure = graph
        .blocks
        .iter()
        .find_map(|block| match block.terminator {
            OwnershipFlowTerminator::Branch {
                condition: OwnershipBranchCondition::TrySuccess { .. },
                else_target,
                ..
            } => Some(else_target),
            _ => None,
        })
        .unwrap();
    // Declaration slots order the actuals: `first` before `second`.
    assert!(reachable(graph, step).contains(&later));
    assert!(!reachable(graph, later).contains(&step));
    assert!(reachable(graph, later).contains(&add));
    assert!(!reachable(graph, failure).contains(&add));
}

#[test]
fn source_control_flow_invokes_associated_functions_without_a_namespace_operand() {
    let source = r#"
struct Counter { value: i32 }
impl Counter {
    fn create(first: i32, second: i32) Counter { ret Counter { value: first + second }; }
    fn get(self: &Counter) i32 { ret self.value; }
}
fn step(value: Option[i32]) Option[i32] { ret value; }
fn later() i32 { ret 2; }
fn build(value: Option[i32]) Option[i32] {
    const counter = Counter.create(second = later(), first = step(value)?);
    ret Some(Counter.get(&counter));
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "build")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let invoke = |needle: &str| {
        let start = source.find(needle).unwrap();
        graph.blocks.iter().position(|block| {
            block.events.iter().any(
                |event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == start),
            )
        })
    };
    let create = invoke("Counter.create(").expect("associated call is scheduled");
    let step = invoke("step(value)?").unwrap();
    let later = invoke("later(), first").unwrap();
    assert!(reachable(graph, step).contains(&later));
    assert!(reachable(graph, later).contains(&create));
    assert!(!reachable(graph, later).contains(&step));
    // A receiver passed explicitly through the type name keeps its own lowering.
    assert!(invoke("Counter.get(").is_none());
}

#[test]
fn source_control_flow_place_updates_evaluate_address_operands_before_value() {
    let source = r#"
struct Pair { left: i32, right: i32 }
fn slot(value: Option[usize]) Option[usize] { ret value; }
fn later() i32 { ret 2; }
fn store(value: Option[usize]) Option[usize] {
    var values: [i32; 3] = [0, 0, 0];
    values[slot(value)?] = later();
    var pair = Pair { left: 0, right: 0 };
    pair.left += later();
    pair.right++;
    ret Some(0);
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "store")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let block_with = |matches: &dyn Fn(&OwnershipFlowEvent) -> bool| {
        graph
            .blocks
            .iter()
            .position(|block| block.events.iter().any(matches))
    };
    let invoke = |needle: &str| {
        let start = source.find(needle).unwrap();
        block_with(
            &|event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == start),
        )
        .unwrap_or_else(|| panic!("missing invoke {needle}"))
    };
    let update = |needle: &str| {
        let start = source.find(needle).unwrap();
        block_with(&|event| {
            matches!(event, OwnershipFlowEvent::ExpressionResult {
                value: OwnershipExpressionResult::PlaceUpdate { target, .. }, ..
            } if target.start == start)
        })
        .unwrap_or_else(|| panic!("missing place update {needle}"))
    };
    let index = invoke("slot(value)?");
    let value = invoke("later();\n    var pair");
    let indexed = update("values[slot");
    let failure = graph
        .blocks
        .iter()
        .find_map(|block| match block.terminator {
            OwnershipFlowTerminator::Branch {
                condition: OwnershipBranchCondition::TrySuccess { .. },
                else_target,
                ..
            } => Some(else_target),
            _ => None,
        })
        .unwrap();
    assert!(reachable(graph, index).contains(&value));
    assert!(!reachable(graph, value).contains(&index));
    assert!(reachable(graph, value).contains(&indexed));
    assert!(!reachable(graph, failure).contains(&value));

    let compound_value = invoke("later();\n    pair.right");
    let compound = update("pair.left");
    assert!(reachable(graph, compound_value).contains(&compound));
    // A compound update reads its place before evaluating the value.
    let left = source.find("pair.left").unwrap();
    let read = block_with(&|event| {
        matches!(event, OwnershipFlowEvent::ExpressionResult {
            value: OwnershipExpressionResult::PlaceTargetRead(span), ..
        } if span.start == left)
    })
    .expect("compound place read");
    assert!(reachable(graph, read).contains(&compound_value));
    assert!(!reachable(graph, compound_value).contains(&read));
    assert!(reachable(graph, compound).contains(&update("pair.right")));
}

#[test]
fn source_control_flow_index_reads_evaluate_object_then_index() {
    let source = r#"
fn values() [i32; 3] { ret [1, 2, 3]; }
fn position(value: Option[usize]) Option[usize] { ret value; }
fn read(value: Option[usize]) Option[i32] {
    const item = values()[position(value)?];
    ret Some(item);
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "read")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let block_with = |matches: &dyn Fn(&OwnershipFlowEvent) -> bool| {
        graph
            .blocks
            .iter()
            .position(|block| block.events.iter().any(matches))
            .unwrap()
    };
    let at = |needle: &str| source.find(needle).unwrap();
    let object = block_with(
        &|event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == at("values()[")),
    );
    let index = block_with(
        &|event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == at("position(value)?")),
    );
    let read = block_with(&|event| {
        matches!(
            event,
            OwnershipFlowEvent::ExpressionResult {
                value: OwnershipExpressionResult::Index { .. },
                ..
            }
        )
    });
    assert!(reachable(graph, object).contains(&index));
    assert!(!reachable(graph, index).contains(&object));
    assert!(reachable(graph, index).contains(&read));
}

#[test]
fn source_control_flow_field_reads_evaluate_expression_objects() {
    let source = r#"
struct Pair { left: i32, right: i32 }
fn make(value: Option[i32]) Option[Pair] {
    ret Some(Pair { left: value?, right: 0 });
}
fn sum(value: Option[i32]) Option[i32] {
    const pair = Pair { left: 1, right: 2 };
    const total = make(value)?.left + pair.right;
    ret Some(total);
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "sum")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let fields: Vec<&str> = graph
        .blocks
        .iter()
        .flat_map(|block| block.events.iter())
        .filter_map(|event| match event {
            OwnershipFlowEvent::ExpressionResult {
                value: OwnershipExpressionResult::Field { field, .. },
                ..
            } => Some(field.as_str()),
            _ => None,
        })
        .collect();
    // `make(value)?.left` evaluates its object; `pair.right` is a name chain.
    assert_eq!(fields, ["left"]);
    let make = source.find("make(value)?").unwrap();
    assert!(graph.blocks.iter().any(|block| {
        block
            .events
            .iter()
            .any(|event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == make))
    }));
    graph.validate().unwrap();
}

#[test]
fn source_control_flow_variant_constructors_schedule_payloads() {
    let source = r#"
fn produce(value: Option[i32]) Option[i32] { ret value; }
fn later() i32 { ret 2; }
fn wrap(value: Option[i32]) Option[i32] {
    const choice: Result[i32, i32] = Ok(later());
    ret Some(produce(value)? + later());
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "wrap")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let constructors: Vec<(usize, &str)> = graph
        .blocks
        .iter()
        .enumerate()
        .flat_map(|(id, block)| block.events.iter().map(move |event| (id, event)))
        .filter_map(|(id, event)| match event {
            OwnershipFlowEvent::ExpressionResult {
                value: OwnershipExpressionResult::VariantConstructor { variant },
                ..
            } => Some((id, variant.as_str())),
            _ => None,
        })
        .collect();
    let names: BTreeSet<&str> = constructors.iter().map(|(_, name)| *name).collect();
    assert_eq!(names, BTreeSet::from(["Ok", "Some"]));
    let some = constructors
        .iter()
        .find(|(_, name)| *name == "Some")
        .unwrap()
        .0;
    let produce = source.find("produce(value)?").unwrap();
    let invoke = graph
        .blocks
        .iter()
        .position(|block| {
            block
                .events
                .iter()
                .any(|event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == produce))
        })
        .unwrap();
    assert!(reachable(graph, invoke).contains(&some));
}

#[test]
fn source_control_flow_struct_initializers_follow_declaration_order() {
    let source = r#"
struct Pair { left: i32, right: i32 }
fn produce(value: Option[i32]) Option[i32] { ret value; }
fn later() i32 { ret 2; }
fn build(value: Option[i32]) Option[i32] {
    const pair = Pair { right: later(), left: produce(value)? };
    ret Some(pair.left);
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "build")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let invoke = |needle: &str| {
        let start = source.find(needle).unwrap();
        graph
            .blocks
            .iter()
            .position(|block| {
                block.events.iter().any(
                    |event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == start),
                )
            })
            .unwrap()
    };
    let built = graph
        .blocks
        .iter()
        .position(|block| {
            block.events.iter().any(|event| {
                matches!(
                    event,
                    OwnershipFlowEvent::ExpressionResult {
                        value: OwnershipExpressionResult::StructInit { .. },
                        ..
                    }
                )
            })
        })
        .unwrap();
    // `left` is declared first, so its value runs before `right`'s.
    let left = invoke("produce(value)?");
    let right = invoke("later(), left");
    assert!(reachable(graph, left).contains(&right));
    assert!(!reachable(graph, right).contains(&left));
    assert!(reachable(graph, right).contains(&built));
}

#[test]
fn source_control_flow_array_literals_evaluate_elements_in_order() {
    let source = r#"
fn produce(value: Option[i32]) Option[i32] { ret value; }
fn later() i32 { ret 2; }
fn gather(value: Option[i32]) Option[i32] {
    const values: [i32; 2] = [produce(value)?, later()];
    ret Some(values[0]);
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "gather")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let position = |matches: &dyn Fn(&OwnershipFlowEvent) -> bool| {
        graph
            .blocks
            .iter()
            .position(|block| block.events.iter().any(matches))
            .unwrap()
    };
    let at = |needle: &str| source.find(needle).unwrap();
    let first = position(
        &|event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == at("produce(value)?")),
    );
    let second = position(
        &|event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == at("later()]")),
    );
    let array = position(&|event| {
        matches!(
            event,
            OwnershipFlowEvent::ExpressionResult {
                value: OwnershipExpressionResult::ArrayLiteral { len: 2 },
                ..
            }
        )
    });
    assert!(reachable(graph, first).contains(&second));
    assert!(!reachable(graph, second).contains(&first));
    assert!(reachable(graph, second).contains(&array));
}

#[test]
fn source_control_flow_match_expressions_branch_per_arm_and_guard() {
    let source = r#"
fn check(value: i32) bool { ret value > 1; }
fn produce(value: Option[i32]) Option[i32] { ret value; }
fn later() i32 { ret 2; }
fn choose(value: Option[i32]) Option[i32] {
    const picked = match value {
        Some(inner) if check(inner) => produce(Some(inner))?,
        Some(inner) => later(),
        None => 0,
    };
    ret Some(picked);
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "choose")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let invoke = |needle: &str| {
        let start = source.find(needle).unwrap();
        graph
            .blocks
            .iter()
            .position(|block| {
                block.events.iter().any(
                    |event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == start),
                )
            })
            .unwrap_or_else(|| panic!("missing invoke {needle}"))
    };
    let arm_tests: Vec<usize> = graph
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| {
            matches!(
                block.terminator,
                OwnershipFlowTerminator::Branch {
                    condition: OwnershipBranchCondition::MatchArm { .. },
                    ..
                }
            )
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(arm_tests.len(), 3);
    let guard = invoke("check(inner)");
    let guarded_body = invoke("produce(Some(inner))?");
    let fallback_body = invoke("later(),");
    let matched = graph
        .blocks
        .iter()
        .position(|block| {
            block.events.iter().any(|event| {
                matches!(
                    event,
                    OwnershipFlowEvent::ExpressionResult {
                        value: OwnershipExpressionResult::Match { arms: 3 },
                        ..
                    }
                )
            })
        })
        .unwrap();
    // A failed guard tries the next arm; arm bodies are alternatives.
    assert!(reachable(graph, guard).contains(&guarded_body));
    assert!(reachable(graph, guard).contains(&fallback_body));
    assert!(!reachable(graph, guarded_body).contains(&fallback_body));
    assert!(reachable(graph, fallback_body).contains(&matched));
    // A value-producing match with no matching arm stops (lowering traps).
    assert!(graph.blocks.iter().any(|block| matches!(
        block.terminator,
        OwnershipFlowTerminator::Stop { .. }
    ) && block.events.is_empty()));
    graph.validate().unwrap();
}

#[test]
fn source_control_flow_invokes_generic_calls_in_declared_slot_order() {
    let source = r#"
fn pick[T](first: T, second: T) T { ret first; }
fn step(value: Option[i32]) Option[i32] { ret value; }
fn later() i32 { ret 2; }
fn choose(value: Option[i32]) Option[i32] {
    const chosen = pick(second = later(), first = step(value)?);
    ret Some(chosen);
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "choose")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let invoke = |needle: &str| {
        let start = source.find(needle).unwrap();
        graph
            .blocks
            .iter()
            .position(|block| {
                block.events.iter().any(
                    |event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == start),
                )
            })
            .unwrap_or_else(|| panic!("missing invoke {needle}"))
    };
    let pick = invoke("pick(second");
    let first = invoke("step(value)?");
    let second = invoke("later(), first");
    assert!(reachable(graph, first).contains(&second));
    assert!(!reachable(graph, second).contains(&first));
    assert!(reachable(graph, second).contains(&pick));
}

#[test]
fn source_control_flow_diverging_and_unsafe_calls_are_scheduled() {
    let source = r#"
fn fail(code: i32) ! { fail(code) }
unsafe fn raw(value: i32) i32 { ret value; }
fn later() i32 { ret 2; }
fn after(value: i32) void {}
fn check(flag: bool) i32 {
    var value: i32 = 0;
    unsafe { value = raw(later()); }
    if flag { fail(later()); }
    after(value);
    ret value;
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "check")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let invoke = |needle: &str| {
        let start = source.find(needle).unwrap();
        graph
            .blocks
            .iter()
            .position(|block| {
                block.events.iter().any(
                    |event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == start),
                )
            })
            .unwrap_or_else(|| panic!("missing invoke {needle}"))
    };
    invoke("raw(later())");
    let fail = invoke("fail(later())");
    // A `!` callee ends its path: nothing after the call is reachable from it.
    assert!(matches!(
        graph.blocks[fail].terminator,
        OwnershipFlowTerminator::Stop { .. }
    ));
    let after = statement_block(graph, source, "after(value)");
    assert!(!reachable(graph, fail).contains(&after));
    graph.validate().unwrap();
}

#[test]
fn source_control_flow_local_function_values_invoke_after_arguments() {
    let source = r#"
fn step(value: Option[i32]) Option[i32] { ret value; }
fn later() i32 { ret 2; }
fn apply(value: Option[i32], callback: fn(i32, i32) i32) Option[i32] {
    const total = callback(step(value)?, later());
    ret Some(total);
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "apply")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let block_with = |matches: &dyn Fn(&OwnershipFlowEvent) -> bool| {
        graph
            .blocks
            .iter()
            .position(|block| block.events.iter().any(matches))
            .unwrap()
    };
    let at = |needle: &str| source.find(needle).unwrap();
    let first = block_with(
        &|event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == at("step(value)?")),
    );
    let second = block_with(
        &|event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == at("later())")),
    );
    let indirect = block_with(
        &|event| matches!(event, OwnershipFlowEvent::IndirectInvoke { span } if span.start == at("callback(step")),
    );
    let failure = graph
        .blocks
        .iter()
        .find_map(|block| match block.terminator {
            OwnershipFlowTerminator::Branch {
                condition: OwnershipBranchCondition::TrySuccess { .. },
                else_target,
                ..
            } => Some(else_target),
            _ => None,
        })
        .unwrap();
    assert!(reachable(graph, first).contains(&second));
    assert!(reachable(graph, second).contains(&indirect));
    assert!(!reachable(graph, failure).contains(&indirect));
}

#[test]
fn source_control_flow_dynamic_methods_invoke_after_receiver_and_arguments() {
    let source = r#"
trait Scaled { fn scale(self: &Self, factor: i32, offset: i32) i32; }
struct Counter { value: i32 }
impl Scaled for Counter {
    fn scale(self: &Counter, factor: i32, offset: i32) i32 { ret self.value * factor + offset; }
}
fn step(value: Option[i32]) Option[i32] { ret value; }
fn later() i32 { ret 2; }
fn measure(value: Option[i32]) Option[i32] {
    var shape: dyn Scaled = Counter { value: 3 };
    const total = shape.scale(step(value)?, later());
    ret Some(total);
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "measure")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let block_with = |matches: &dyn Fn(&OwnershipFlowEvent) -> bool| {
        graph
            .blocks
            .iter()
            .position(|block| block.events.iter().any(matches))
            .unwrap()
    };
    let at = |needle: &str| source.find(needle).unwrap();
    let first = block_with(
        &|event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == at("step(value)?")),
    );
    let second = block_with(
        &|event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == at("later())")),
    );
    let dynamic = block_with(
        &|event| matches!(event, OwnershipFlowEvent::IndirectInvoke { span } if span.start == at("shape.scale")),
    );
    assert!(reachable(graph, first).contains(&second));
    assert!(reachable(graph, second).contains(&dynamic));
    assert!(!reachable(graph, dynamic).contains(&first));
}

#[test]
fn source_control_flow_format_calls_schedule_expanded_arguments() {
    let source = r#"
fn show(template: str, ...args: Display) void {}
fn step(value: Option[i32]) Option[i32] { ret value; }
fn later() i32 { ret 2; }
fn report(value: Option[i32]) Option[i32] {
    show("first {} then {later()}", step(value)?);
    ret Some(1);
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "report")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let block_with = |matches: &dyn Fn(&OwnershipFlowEvent) -> bool| {
        graph
            .blocks
            .iter()
            .position(|block| block.events.iter().any(matches))
            .unwrap()
    };
    let at = |needle: &str| source.find(needle).unwrap();
    let explicit = block_with(
        &|event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == at("step(value)?")),
    );
    // Placeholder expressions are evaluated as regions of their own.
    let placeholder = block_with(
        &|event| matches!(event, OwnershipFlowEvent::Evaluate { spans, .. } if spans[0].start == at("later()}")),
    );
    let shown = block_with(
        &|event| matches!(event, OwnershipFlowEvent::FormatInvoke { span } if span.start == at("show(\"first")),
    );
    let failure = graph
        .blocks
        .iter()
        .find_map(|block| match block.terminator {
            OwnershipFlowTerminator::Branch {
                condition: OwnershipBranchCondition::TrySuccess { .. },
                else_target,
                ..
            } => Some(else_target),
            _ => None,
        })
        .unwrap();
    // Template order: the explicit `{}` argument, then the `{later()}` placeholder.
    assert!(reachable(graph, explicit).contains(&placeholder));
    assert!(!reachable(graph, placeholder).contains(&explicit));
    assert!(reachable(graph, placeholder).contains(&shown));
    assert!(!reachable(graph, failure).contains(&shown));
    graph.validate().unwrap();
}

#[test]
fn source_control_flow_nested_format_calls_expand_in_later_rounds() {
    let source = r#"
fn show(template: str, ...args: Display) void {}
fn count(template: str, ...args: Display) i32 { ret 1; }
fn later() i32 { ret 2; }
fn report() void {
    show("outer {}", count("inner {}", later()));
}
fn main() void {}
"#;
    let report = Analyzer::new().analyze_program(&parse(source));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let graph = report
        .ownership_call_graph
        .iter()
        .find(|callable| callable.id.callable == "report")
        .unwrap()
        .control_flow
        .as_deref()
        .unwrap();
    let block_with = |matches: &dyn Fn(&OwnershipFlowEvent) -> bool| {
        graph
            .blocks
            .iter()
            .position(|block| block.events.iter().any(matches))
            .unwrap()
    };
    let at = |needle: &str| source.find(needle).unwrap();
    let value = block_with(
        &|event| matches!(event, OwnershipFlowEvent::Invoke { span } if span.start == at("later()))")),
    );
    let inner = block_with(
        &|event| matches!(event, OwnershipFlowEvent::FormatInvoke { span } if span.start == at("count(\"inner")),
    );
    let outer = block_with(
        &|event| matches!(event, OwnershipFlowEvent::FormatInvoke { span } if span.start == at("show(\"outer")),
    );
    assert!(reachable(graph, value).contains(&inner));
    assert!(reachable(graph, inner).contains(&outer));
    assert!(!reachable(graph, inner).contains(&value));
}
