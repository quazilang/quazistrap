// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use super::*;
use crate::{lexer::Lexer, parser::Parser};

fn analyze(source: &str) -> SemanticReport {
    let program = Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
    Analyzer::new().analyze_program(&program)
}

fn valid(source: &str) -> SemanticReport {
    let report = analyze(source);
    assert!(report.errors.is_empty(), "{source}: {:?}", report.errors);
    report
}

fn invalid(source: &str, message: &str) {
    let report = analyze(source);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains(message)),
        "{source}: expected {message}, got {:?}",
        report.errors
    );
}

#[test]
fn power_rejects_nonnumeric_operands_and_specializations() {
    for source in [
        "fn main()void{true**false;}",
        "fn main()void{2**\"text\";}",
        "fn main()void{[1,2]**2;}",
        "unsafe fn main()void{var p:*u8=0;p**2;}",
        "struct Owner{value:i32,} fn main()void{Owner{value:2}**2;}",
        "fn power[T](a:T,b:T)T{ret a**b;} fn main()void{power[bool](true,false);}",
        "struct Owner{value:i32,} fn power[T](a:T)T{ret a**2;} fn main()void{power[Owner](Owner{value:2});}",
        "fn power[T](a:T,b:T)T{ret a**b;} fn wrap[U](a:U,b:U)U{ret power[U](a,b);} fn main()void{wrap[bool](true,false);}",
        "fn power[T](a:&T,b:T)T{ret a**b;}",
    ] {
        invalid(source, "power requires numeric operands");
    }
}

#[test]
fn power_checks_known_integer_negative_exponents_and_overflow() {
    for source in [
        "fn main()void{2**-2;}",
        "fn main()void{const exponent=-2;2**exponent;}",
        "fn main()void{2**(1-3);}",
        "fn power[T](a:T)T{ret a**-2;} fn main()void{power[i32](2);}",
    ] {
        invalid(source, "nonnegative exponent");
    }
    for source in [
        "fn main()void{(2 as i8)**7;}",
        "fn main()void{(2 as u8)**8;}",
        "fn main()void{(2 as i64)**63;}",
        "fn main()void{(2 as u64)**64;}",
    ] {
        invalid(source, "integer power overflows");
    }
}

#[test]
fn power_preserves_runtime_checks_and_float_negative_exponents() {
    for source in [
        "fn main()void{var n=-2;2**n;}",
        "fn power(n:i32)i32{ret 2**n;}",
        "fn main()void{2.0**-2;2**-2.0;2**0;}",
        "fn main()void{(-2 as i8)**7;(2 as u8)**7;(-128 as i8)**1;}",
        "fn power[T](a:T,b:T)T{ret a**b;} fn main()void{power[i32](2,3);power[f64](2.0,-2.0);}",
        "fn power[T](a:T)T{ret a**-2;} fn main()void{power[f64](2.0);}",
        "type Scalar=i64;fn main()void{var a:Scalar=2;var b=3;var r=&a;r**b;}",
        "fn power[T](a:T,b:&i32)T{ret a**b;} fn main()void{var b=3;power[i32](2,&b);}",
    ] {
        valid(source);
    }
}

#[test]
fn power_floating_overload_and_generic_result_family_are_checked() {
    for source in [
        "fn power(a:i32,b:f64)f64{ret a**b;}",
        "fn power(a:f32,b:i32)f64{ret a**b;}",
        "fn power[T](a:T,b:f64)f64{ret a**b;} fn main()void{power[i32](2,0.5);}",
        "fn power[T](a:f64,b:T)f64{ret a**b;} fn main()void{power[i32](2.0,-2);}",
    ] {
        valid(source);
    }
    invalid(
        "fn power[T,U](a:T,b:U)T{ret a**b;} fn main()void{power[i32,f64](2,0.5);}",
        "provisional integer result",
    );
}

#[test]
fn power_exact_integer_constant_evaluator_respects_all_result_widths() {
    use super::power::checked_integer_power;
    for (ty, positive, negative) in [
        (TypeKind::Int8, 6, 7),
        (TypeKind::Int16, 14, 15),
        (TypeKind::Int32, 30, 31),
        (TypeKind::Int64, 62, 63),
        (TypeKind::Isize, 62, 63),
    ] {
        assert_eq!(
            checked_integer_power(2, positive, &ty),
            Some(1_i64 << positive)
        );
        assert_eq!(checked_integer_power(2, negative, &ty), None);
        assert_eq!(
            checked_integer_power(-2, negative, &ty),
            Some((1_u64 << negative) as i64 * if negative == 63 { 1 } else { -1 })
        );
    }
    for (ty, bits) in [
        (TypeKind::Uint8, 8),
        (TypeKind::Uint16, 16),
        (TypeKind::Uint32, 32),
        (TypeKind::Uint64, 64),
        (TypeKind::Usize, 64),
    ] {
        assert_eq!(
            checked_integer_power(2, bits - 1, &ty),
            Some((1_u64 << (bits - 1)) as i64)
        );
        assert_eq!(checked_integer_power(2, bits, &ty), None);
    }
    assert_eq!(checked_integer_power(-1, 1, &TypeKind::Uint64), Some(-1));
    assert_eq!(
        checked_integer_power(i64::MIN, 1, &TypeKind::Int64),
        Some(i64::MIN)
    );
    assert_eq!(checked_integer_power(0, 0, &TypeKind::Int8), Some(1));
    assert_eq!(
        checked_integer_power(-1, u64::MAX, &TypeKind::Int8),
        Some(-1)
    );
}

#[test]
fn power_integer_capable_templates_retain_hidden_panic_dependency() {
    for source in [
        "fn calc[T](a:T,b:T)T{ret a**b;}fn main()void{calc[i32](2,3);}",
        "fn calc(a:i32,b:i32)i32{ret a**b;}",
    ] {
        let report = valid(source);
        for target in ["panic.panic", "panic"] {
            assert!(
                report
                    .dependency_graph
                    .edges
                    .iter()
                    .any(|edge| edge.from == "calc" && edge.to == target)
            );
        }
    }
}
