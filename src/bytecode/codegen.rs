// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::HashMap;

use crate::parser::ast::*;
use crate::semantic::{ConstValue, SemanticReport};
use super::instruction::{ri16, rrr};
use super::{Chunk, ConstPoolEntry, Opcode};

// ── Public entry point ────────────────────────────────────────────────────────

pub struct Codegen<'a> {
    report: &'a SemanticReport,
    fn_index: HashMap<String, u16>,
    const_map: HashMap<(usize, usize), ConstValue>,
}

impl<'a> Codegen<'a> {
    pub fn new(report: &'a SemanticReport) -> Self {
        let mut const_map = HashMap::new();
        for ann in &report.annotated_exprs {
            if let Some(cv) = &ann.const_value {
                const_map.insert((ann.span.start, ann.span.end), cv.clone());
            }
        }
        Self { report, fn_index: HashMap::new(), const_map }
    }

    pub fn compile_program(&mut self, program: &Program) -> Vec<Chunk> {
        // Pass 1: assign each function a table index.
        let mut idx = 0u16;
        for item in &program.items {
            if let ItemKind::Fn { name, .. } = &item.node {
                self.fn_index.insert(name.clone(), idx);
                idx += 1;
            }
        }

        // Pass 2: compile each function body.
        let mut chunks = Vec::new();
        for item in &program.items {
            if let ItemKind::Fn { name, params, body, .. } = &item.node {
                chunks.push(self.compile_fn(name, params, body));
            }
        }
        chunks
    }

    fn compile_fn(
        &self,
        name: &str,
        params: &[(String, Type)],
        body: &Block,
    ) -> Chunk {
        let mut fc = FnCompiler::new(name, &self.fn_index, &self.const_map);
        for (param_name, _) in params {
            fc.bind(param_name.clone());
        }
        fc.compile_block(body);
        // Guarantee every path ends with Ret.
        if fc.chunk.code.last().map(|i| i.opcode) != Some(Opcode::Ret as u8) {
            fc.chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
        }
        fc.chunk
    }
}

// ── Per-function compiler ─────────────────────────────────────────────────────

struct FnCompiler<'a> {
    chunk: Chunk,
    regs: HashMap<String, u8>,
    next_reg: u8,
    fn_index: &'a HashMap<String, u16>,
    const_map: &'a HashMap<(usize, usize), ConstValue>,
}

impl<'a> FnCompiler<'a> {
    fn new(
        name: &str,
        fn_index: &'a HashMap<String, u16>,
        const_map: &'a HashMap<(usize, usize), ConstValue>,
    ) -> Self {
        Self {
            chunk: Chunk::new(name),
            regs: HashMap::new(),
            next_reg: 0,
            fn_index,
            const_map,
        }
    }

    fn alloc_reg(&mut self) -> u8 {
        let r = self.next_reg;
        self.next_reg = self.next_reg.wrapping_add(1);
        r
    }

    fn bind(&mut self, name: String) -> u8 {
        let r = self.alloc_reg();
        self.regs.insert(name, r);
        r
    }

    fn reg_of(&self, name: &str) -> u8 {
        self.regs.get(name).copied().unwrap_or(0)
    }

    // ── Block / statement ─────────────────────────────────────────────────────

    fn compile_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            if self.compile_stmt(stmt) {
                break;
            }
        }
    }

    /// Returns true if the statement guarantees exit (return).
    fn compile_stmt(&mut self, stmt: &Stmt) -> bool {
        match &stmt.node {
            StmtKind::Var { name, value, .. } => {
                let dst = self.bind(name.clone());
                if let Some(expr) = value {
                    let src = self.compile_expr(expr);
                    if src != dst {
                        self.chunk.emit(rrr(Opcode::Mov, dst, src, 0));
                    }
                }
                false
            }
            StmtKind::Const { name, value, .. } => {
                let dst = self.bind(name.clone());
                let src = self.compile_expr(value);
                if src != dst {
                    self.chunk.emit(rrr(Opcode::Mov, dst, src, 0));
                }
                false
            }
            StmtKind::Return(expr) => {
                if let Some(expr) = expr {
                    let src = self.compile_expr(expr);
                    // Convention: return value in r0.
                    if src != 0 {
                        self.chunk.emit(rrr(Opcode::Mov, 0, src, 0));
                    }
                }
                self.chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
                true
            }
            StmtKind::ExprStmt(expr) => {
                self.compile_expr(expr);
                false
            }
            StmtKind::If { condition, then_block, else_block } => {
                // Emit condition + jump-if-false past the then block.
                let jump_else = self.compile_condition_jump(condition, true);

                self.compile_block(then_block);

                if let Some(else_block) = else_block {
                    let jump_end = self.chunk.emit(ri16(Opcode::Jmp, 0, 0));
                    self.chunk.patch_jump(jump_else, self.chunk.len() as u16);
                    self.compile_block(else_block);
                    self.chunk.patch_jump(jump_end, self.chunk.len() as u16);
                } else {
                    self.chunk.patch_jump(jump_else, self.chunk.len() as u16);
                }
                false
            }
            StmtKind::While { condition, body } => {
                let loop_top = self.chunk.len() as u16;
                let jump_exit = self.compile_condition_jump(condition, true);
                self.compile_block(body);
                self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                self.chunk.patch_jump(jump_exit, self.chunk.len() as u16);
                false
            }
        }
    }

    // ── Condition helpers ─────────────────────────────────────────────────────

    /// Emit instructions for a boolean condition and a conditional jump.
    /// `jump_if_false = true` means jump when condition is false (used for if/while).
    /// Returns the index of the emitted jump instruction (caller must patch).
    fn compile_condition_jump(&mut self, expr: &Expr, jump_if_false: bool) -> usize {
        match &expr.node {
            ExprKind::Binary { left, op, right }
                if is_comparison(op) =>
            {
                let r1 = self.compile_expr(left);
                let r2 = self.compile_expr(right);
                self.chunk.emit(rrr(Opcode::Cmp, 0, r1, r2));
                let jop = if jump_if_false { negate_cmp(op) } else { direct_cmp(op) };
                self.chunk.emit(ri16(jop, 0, 0))
            }
            ExprKind::Group(inner) => self.compile_condition_jump(inner, jump_if_false),
            _ => {
                let r = self.compile_expr(expr);
                let jop = if jump_if_false { Opcode::Jz } else { Opcode::Jnz };
                // ri16 layout: ops[0]=register, ops[1..2]=target (patched later)
                self.chunk.emit(ri16(jop, r, 0))
            }
        }
    }

    // ── Expression ───────────────────────────────────────────────────────────

    fn compile_expr(&mut self, expr: &Expr) -> u8 {
        // Const-fold: if the semantic pass computed a known value for a non-trivial
        // expression, emit it directly instead of computing it at runtime.
        // Skip Ident (value already in a register) and Literal (emits directly below).
        let key = (expr.span.start, expr.span.end);
        if !matches!(expr.node, ExprKind::Ident(_) | ExprKind::Literal(_)) {
            if let Some(cv) = self.const_map.get(&key).cloned() {
                return self.emit_const_value(cv);
            }
        }

        match &expr.node {
            ExprKind::Literal(lit) => self.emit_literal(lit),

            ExprKind::Ident(name) => self.reg_of(name),

            ExprKind::Group(inner) => self.compile_expr(inner),

            ExprKind::Unary { op, expr: inner } => {
                let src = self.compile_expr(inner);
                let dst = self.alloc_reg();
                let opcode = match op {
                    UnaryOpKind::Neg => Opcode::Neg,
                    UnaryOpKind::Not => Opcode::Not,
                };
                self.chunk.emit(rrr(opcode, dst, src, 0));
                dst
            }

            // Short-circuit logical ops — lazy right evaluation.
            ExprKind::Binary { left, op: BinOpKind::AndAnd, right } => {
                let r1 = self.compile_expr(left);
                let dst = self.alloc_reg();
                let false_idx = self.chunk.emit(ri16(Opcode::Jz, r1, 0));
                let r2 = self.compile_expr(right);
                self.chunk.emit(rrr(Opcode::Mov, dst, r2, 0));
                let end_idx = self.chunk.emit(ri16(Opcode::Jmp, 0, 0));
                let false_tgt = self.chunk.len() as u16;
                self.chunk.patch_jump(false_idx, false_tgt);
                self.chunk.emit(ri16(Opcode::MovI, dst, 0));
                self.chunk.patch_jump(end_idx, self.chunk.len() as u16);
                dst
            }
            ExprKind::Binary { left, op: BinOpKind::OrOr, right } => {
                let r1 = self.compile_expr(left);
                let dst = self.alloc_reg();
                let true_idx = self.chunk.emit(ri16(Opcode::Jnz, r1, 0));
                let r2 = self.compile_expr(right);
                self.chunk.emit(rrr(Opcode::Mov, dst, r2, 0));
                let end_idx = self.chunk.emit(ri16(Opcode::Jmp, 0, 0));
                let true_tgt = self.chunk.len() as u16;
                self.chunk.patch_jump(true_idx, true_tgt);
                self.chunk.emit(ri16(Opcode::MovI, dst, 1));
                self.chunk.patch_jump(end_idx, self.chunk.len() as u16);
                dst
            }

            ExprKind::Binary { left, op, right } => {
                let r1 = self.compile_expr(left);
                let r2 = self.compile_expr(right);
                let dst = self.alloc_reg();
                match op {
                    BinOpKind::Add => { self.chunk.emit(rrr(Opcode::Add, dst, r1, r2)); }
                    BinOpKind::Sub => { self.chunk.emit(rrr(Opcode::Sub, dst, r1, r2)); }
                    BinOpKind::Mul => { self.chunk.emit(rrr(Opcode::Mul, dst, r1, r2)); }
                    BinOpKind::Div => { self.chunk.emit(rrr(Opcode::Div, dst, r1, r2)); }
                    BinOpKind::Mod => { self.chunk.emit(rrr(Opcode::Mod, dst, r1, r2)); }
                    // Comparisons: materialize bool result into dst.
                    _ if is_comparison(op) => {
                        self.chunk.emit(rrr(Opcode::Cmp, 0, r1, r2));
                        self.chunk.emit(ri16(Opcode::MovI, dst, 1));
                        let skip = self.chunk.emit(ri16(direct_cmp(op), 0, 0));
                        self.chunk.emit(ri16(Opcode::MovI, dst, 0));
                        self.chunk.patch_jump(skip, self.chunk.len() as u16);
                    }
                    BinOpKind::AndAnd | BinOpKind::OrOr => unreachable!(),
                    _ => { self.chunk.emit(rrr(Opcode::Add, dst, r1, r2)); } // fallback
                }
                dst
            }

            ExprKind::Assign { target, value } => {
                let src = self.compile_expr(value);
                if let ExprKind::Ident(name) = &target.node {
                    let dst = self.reg_of(name);
                    if dst != src {
                        self.chunk.emit(rrr(Opcode::Mov, dst, src, 0));
                    }
                    dst
                } else {
                    src
                }
            }

            ExprKind::CompoundAssign { target, op, value } => {
                let src = self.compile_expr(value);
                if let ExprKind::Ident(name) = &target.node {
                    let dst = self.reg_of(name);
                    let opcode = match op {
                        CompoundAssignOp::Add => Opcode::Add,
                        CompoundAssignOp::Sub => Opcode::Sub,
                        CompoundAssignOp::Mul => Opcode::Mul,
                        CompoundAssignOp::Div => Opcode::Div,
                        CompoundAssignOp::Mod => Opcode::Mod,
                    };
                    self.chunk.emit(rrr(opcode, dst, dst, src));
                    dst
                } else {
                    src
                }
            }

            ExprKind::IncDec { expr: inner, op, .. } => {
                if let ExprKind::Ident(name) = &inner.node {
                    let r = self.reg_of(name);
                    let opcode = match op {
                        IncDecOp::Inc => Opcode::Inc,
                        IncDecOp::Dec => Opcode::Dec,
                    };
                    self.chunk.emit(rrr(opcode, r, r, 0));
                    r
                } else {
                    0
                }
            }

            ExprKind::Call { callee, args, .. } => {
                // Push args (they go into fresh regs; calling convention TBD by backend).
                for arg in args {
                    self.compile_expr(arg);
                }
                if let ExprKind::Ident(name) = &callee.node {
                    if let Some(&idx) = self.fn_index.get(name.as_str()) {
                        self.chunk.emit(ri16(Opcode::CallIdx, 0, idx));
                    }
                }
                // Return value in r0 by convention.
                0
            }

            ExprKind::MethodCall { object, args, .. } => {
                let obj = self.compile_expr(object);
                for arg in args {
                    self.compile_expr(arg);
                }
                let vtbl = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::VtblLoad, vtbl, obj, 0));
                self.chunk.emit(rrr(Opcode::CallReg, vtbl, 0, 0));
                0
            }

            ExprKind::Field { object, .. } => {
                let obj = self.compile_expr(object);
                let dst = self.alloc_reg();
                // Field offset resolved by AOT backend — 0 is a placeholder.
                self.chunk.emit(rrr(Opcode::FieldLoad, dst, obj, 0));
                dst
            }

            ExprKind::Match { scrutinee, arms } => {
                let scr = self.compile_expr(scrutinee);
                let dst = self.alloc_reg();
                let mut end_jumps = Vec::new();

                for arm in arms {
                    match &arm.pattern.node {
                        PatternKind::Wildcard => {
                            let val = self.compile_expr(&arm.expr);
                            if val != dst {
                                self.chunk.emit(rrr(Opcode::Mov, dst, val, 0));
                            }
                        }
                        PatternKind::Variant { .. } => {
                            // Load discriminant at offset 0, compare to arm tag.
                            let disc = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::FieldLoad, disc, scr, 0));
                            let tag = self.alloc_reg();
                            // Tag values resolved by AOT backend — 0 placeholder.
                            self.chunk.emit(ri16(Opcode::MovI, tag, 0));
                            self.chunk.emit(rrr(Opcode::Cmp, 0, disc, tag));
                            let skip = self.chunk.emit(ri16(Opcode::Jne, 0, 0));
                            let val = self.compile_expr(&arm.expr);
                            if val != dst {
                                self.chunk.emit(rrr(Opcode::Mov, dst, val, 0));
                            }
                            end_jumps.push(self.chunk.emit(ri16(Opcode::Jmp, 0, 0)));
                            self.chunk.patch_jump(skip, self.chunk.len() as u16);
                        }
                    }
                }
                let end = self.chunk.len() as u16;
                for j in end_jumps {
                    self.chunk.patch_jump(j, end);
                }
                dst
            }
        }
    }

    // ── Literal / const-value emitters ────────────────────────────────────────

    fn emit_literal(&mut self, lit: &Literal) -> u8 {
        let dst = self.alloc_reg();
        match lit {
            Literal::Int(n) if *n >= 0 && *n <= 0xFFFF => {
                self.chunk.emit(ri16(Opcode::MovI, dst, *n as u16));
            }
            Literal::Int(n) => {
                let idx = self.chunk.add_constant(ConstPoolEntry::Int(*n));
                self.chunk.emit(ri16(Opcode::MovConst, dst, idx));
            }
            Literal::Float(f) => {
                let idx = self.chunk.add_constant(ConstPoolEntry::Float(*f));
                self.chunk.emit(ri16(Opcode::MovConst, dst, idx));
            }
            Literal::String(s) => {
                let idx = self.chunk.add_constant(ConstPoolEntry::Str(s.clone()));
                self.chunk.emit(ri16(Opcode::MovConst, dst, idx));
            }
            Literal::Bool(b) => {
                self.chunk.emit(ri16(Opcode::MovI, dst, *b as u16));
            }
        }
        dst
    }

    fn emit_const_value(&mut self, cv: ConstValue) -> u8 {
        let dst = self.alloc_reg();
        match cv {
            ConstValue::Int(n) if n >= 0 && n <= 0xFFFF => {
                self.chunk.emit(ri16(Opcode::MovI, dst, n as u16));
            }
            ConstValue::Int(n) => {
                let idx = self.chunk.add_constant(ConstPoolEntry::Int(n));
                self.chunk.emit(ri16(Opcode::MovConst, dst, idx));
            }
            ConstValue::Float(f) => {
                let idx = self.chunk.add_constant(ConstPoolEntry::Float(f));
                self.chunk.emit(ri16(Opcode::MovConst, dst, idx));
            }
            ConstValue::String(s) => {
                let idx = self.chunk.add_constant(ConstPoolEntry::Str(s));
                self.chunk.emit(ri16(Opcode::MovConst, dst, idx));
            }
            ConstValue::Bool(b) => {
                self.chunk.emit(ri16(Opcode::MovI, dst, b as u16));
            }
        }
        dst
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn is_comparison(op: &BinOpKind) -> bool {
    matches!(
        op,
        BinOpKind::Lt
            | BinOpKind::LtEq
            | BinOpKind::Gt
            | BinOpKind::GtEq
            | BinOpKind::EqEq
            | BinOpKind::NotEq
    )
}

/// Conditional jump opcode that fires when the comparison is TRUE.
fn direct_cmp(op: &BinOpKind) -> Opcode {
    match op {
        BinOpKind::Lt    => Opcode::Jl,
        BinOpKind::LtEq  => Opcode::Jle,
        BinOpKind::Gt    => Opcode::Jg,
        BinOpKind::GtEq  => Opcode::Jge,
        BinOpKind::EqEq  => Opcode::Je,
        BinOpKind::NotEq => Opcode::Jne,
        _                => Opcode::Jnz,
    }
}

/// Conditional jump opcode that fires when the comparison is FALSE (negated).
fn negate_cmp(op: &BinOpKind) -> Opcode {
    match op {
        BinOpKind::Lt    => Opcode::Jge,
        BinOpKind::LtEq  => Opcode::Jg,
        BinOpKind::Gt    => Opcode::Jle,
        BinOpKind::GtEq  => Opcode::Jl,
        BinOpKind::EqEq  => Opcode::Jne,
        BinOpKind::NotEq => Opcode::Je,
        _                => Opcode::Jz,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::semantic::Analyzer;

    fn compile(src: &str) -> Vec<Chunk> {
        let tokens = Lexer::new(src).tokenize();
        let program = Parser::new(tokens).parse().expect("parse failed");
        let report = Analyzer::new().analyze_program(&program);
        assert!(report.errors.is_empty(), "semantic errors: {:?}", report.errors);
        Codegen::new(&report).compile_program(&program)
    }

    #[test]
    fn simple_add_function_emits_add_and_ret() {
        let chunks = compile("fn add(a: int32, b: int32) int32 { return a + b; }");
        assert_eq!(chunks.len(), 1);
        let chunk = &chunks[0];
        assert_eq!(chunk.name, "add");
        assert!(
            chunk.code.iter().any(|i| i.opcode == Opcode::Add as u8),
            "expected Add instruction"
        );
        assert_eq!(chunk.code.last().unwrap().opcode, Opcode::Ret as u8);
    }

    #[test]
    fn const_fold_reduces_instruction_count() {
        // Without folding: MovI, MovI, Add, Mov, Ret = 5
        // With folding:    MovI(3), Mov, Ret = 3
        let chunks = compile("fn foo() int32 { const x: int32 = 1 + 2; return x; }");
        assert_eq!(chunks.len(), 1);
        let count = chunks[0].code.len();
        assert!(
            count <= 3,
            "expected ≤3 instructions (const-folded), got {}",
            count
        );
        // Must not contain Add — the folded path emits only MovI
        assert!(
            !chunks[0].code.iter().any(|i| i.opcode == Opcode::Add as u8),
            "Add should be eliminated by const folding"
        );
    }

    #[test]
    fn while_loop_jump_points_back_to_condition() {
        let chunks = compile(
            r#"fn countdown(x: int32) void {
                while (x > 0) { x = x + 1; }
            }"#,
        );
        assert_eq!(chunks.len(), 1);
        let code = &chunks[0].code;

        // Find the trailing Jmp (back-edge of loop).
        let back_jmp = code
            .iter()
            .rposition(|i| i.opcode == Opcode::Jmp as u8)
            .expect("expected Jmp back-edge");

        // Its target must be instruction 0 (loop_top = 0).
        let (_, target) = code[back_jmp].ri16();
        assert_eq!(target, 0, "back-edge Jmp must target instruction 0 (loop top)");
    }

    #[test]
    fn if_else_jump_targets_are_patched() {
        let chunks = compile(
            r#"fn sign(x: int32) int32 {
                if (x > 0) { return 1; } else { return 0; }
            }"#,
        );
        let code = &chunks[0].code;
        let len = code.len() as u16;

        // All jump targets must be valid instruction indices.
        for instr in code {
            let op = instr.opcode;
            let is_jump = matches!(
                Opcode::from_u8(op),
                Some(
                    Opcode::Jmp | Opcode::Je | Opcode::Jne | Opcode::Jl | Opcode::Jle |
                    Opcode::Jg | Opcode::Jge | Opcode::Jz | Opcode::Jnz
                )
            );
            if is_jump {
                let (_, target) = instr.ri16();
                assert!(
                    target <= len,
                    "jump target {} out of bounds (chunk has {} instructions)",
                    target, len
                );
            }
        }
    }

    #[test]
    fn function_call_emits_call_idx() {
        let chunks = compile(
            r#"fn helper(x: int32) int32 { return x; }
               fn main() void { helper(1); }"#,
        );
        let main_chunk = chunks.iter().find(|c| c.name == "main").expect("no main chunk");
        assert!(
            main_chunk.code.iter().any(|i| i.opcode == Opcode::CallIdx as u8),
            "expected CallIdx in main"
        );
    }

    #[test]
    fn ret_always_last_in_every_chunk() {
        let chunks = compile(
            r#"fn a() void {}
               fn b(x: int32) int32 { return x; }"#,
        );
        for chunk in &chunks {
            assert_eq!(
                chunk.code.last().map(|i| i.opcode),
                Some(Opcode::Ret as u8),
                "chunk '{}' does not end with Ret",
                chunk.name
            );
        }
    }

    #[test]
    fn compound_assign_emits_arithmetic_op_in_place() {
        let chunks = compile(
            r#"fn inc(x: int32) int32 {
                x += 1;
                return x;
            }"#,
        );
        assert!(
            chunks[0].code.iter().any(|i| i.opcode == Opcode::Add as u8),
            "x += 1 should emit Add"
        );
    }

    #[test]
    fn inc_dec_emits_inc_dec_opcode() {
        let chunks = compile(
            r#"fn bump(x: int32) int32 {
                x++;
                return x;
            }"#,
        );
        assert!(
            chunks[0].code.iter().any(|i| i.opcode == Opcode::Inc as u8),
            "x++ should emit Inc"
        );
    }

    #[test]
    fn large_int_goes_to_constant_pool() {
        let chunks = compile("fn big() int32 { return 100000; }");
        assert!(!chunks[0].constants.is_empty(), "100000 should be in constant pool");
        assert!(
            chunks[0].code.iter().any(|i| i.opcode == Opcode::MovConst as u8),
            "expected MovConst for large literal"
        );
    }

    #[test]
    fn string_literal_goes_to_constant_pool() {
        let chunks = compile(r#"fn greeting() str { return "hello"; }"#);
        assert!(
            matches!(chunks[0].constants.first(), Some(ConstPoolEntry::Str(s)) if s == "hello"),
            "string literal should be in constant pool"
        );
    }

    #[test]
    fn to_bytes_produces_six_bytes_per_instruction() {
        let chunks = compile("fn f(a: int32, b: int32) int32 { return a + b; }");
        let bytes = chunks[0].to_bytes();
        assert_eq!(bytes.len(), chunks[0].code.len() * 6);
    }
}
