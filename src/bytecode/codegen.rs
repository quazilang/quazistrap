<<<<<<< Updated upstream
// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::{HashMap, HashSet};

use super::instruction::{mem_lea, mem_load, mem_store, ri16, rrr};
use super::{Chunk, ConstPoolEntry, Opcode};
use crate::parser::ast::*;
use crate::semantic::{ConstValue, DependencyKind, SemanticReport};

// ── Public entry point ────────────────────────────────────────────────────────

pub struct Codegen<'a> {
    report: &'a SemanticReport,
    fn_index: HashMap<String, u16>,
    const_map: HashMap<(usize, usize), ConstValue>,
    type_map: HashMap<(usize, usize), TypeKind>,
    import_names: HashSet<String>,
}

impl<'a> Codegen<'a> {
    pub fn new(report: &'a SemanticReport) -> Self {
        let mut const_map = HashMap::new();
        let mut type_map = HashMap::new();
        for ann in &report.annotated_exprs {
            let key = (ann.span.start, ann.span.end);
            if let Some(cv) = &ann.const_value {
                const_map.insert(key, cv.clone());
            }
            if let Some(ty) = &ann.ty {
                type_map.insert(key, ty.clone());
            }
        }
        let mut import_names = HashSet::new();
        for entry in &report.symbol_table.entries {
            if entry.symbol.is_import {
                import_names.insert(entry.name.clone());
            }
        }
        Self {
            report,
            fn_index: HashMap::new(),
            const_map,
            type_map,
            import_names,
        }
    }

    pub fn compile_program(&mut self, program: &Program) -> Vec<Chunk> {
        // Compute the set of functions reachable from main via the call graph.
        // Library mode (no main) compiles everything.
        let has_main = program
            .items
            .iter()
            .any(|item| matches!(&item.node, ItemKind::Fn { name, .. } if name == "main"));

        let reachable: Option<std::collections::HashSet<String>> = if has_main {
            let mut set = std::collections::HashSet::new();
            set.insert("main".to_string());
            let mut queue = vec!["main".to_string()];
            while let Some(fn_name) = queue.pop() {
                for edge in &self.report.dependency_graph.edges {
                    if edge.kind == DependencyKind::Call && edge.from == fn_name {
                        if set.insert(edge.to.clone()) {
                            queue.push(edge.to.clone());
                        }
                    }
                }
            }
            Some(set)
        } else {
            None
        };

        let is_live =
            |name: &str| -> bool { reachable.as_ref().map_or(true, |r| r.contains(name)) };

        // Pass 1: assign each live function a table index.
        let mut idx = 0u16;
        for item in &program.items {
            if let ItemKind::Fn { name, .. } = &item.node {
                if is_live(name) {
                    self.fn_index.insert(name.clone(), idx);
                    idx += 1;
                }
            }
        }

        // Pass 2: compile each live function body.
        let mut chunks = Vec::new();
        for item in &program.items {
            if let ItemKind::Fn {
                name,
                params,
                body,
                attributes,
                ..
            } = &item.node
            {
                if is_live(name) {
                    if let Some(chunk) = self.compile_fn(
                        name,
                        params,
                        body.as_ref().map(|b| b as &Block),
                        attributes,
                    ) {
                        chunks.push(chunk);
                    }
                }
            }
        }

        // Post-pass: inline small functions that are inline candidates.
        let inline_set: std::collections::HashSet<String> = self
            .report
            .inline_candidates
            .iter()
            .map(|c| c.name.clone())
            .collect();

        if !inline_set.is_empty() {
            // Build a snapshot of callee chunks before mutating.
            let callee_map: std::collections::HashMap<String, Chunk> = chunks
                .iter()
                .filter(|c| inline_set.contains(&c.name))
                .map(|c| (c.name.clone(), c.clone()))
                .collect();

            // Build reverse index: fn_idx -> name
            let idx_to_name: std::collections::HashMap<u16, String> =
                self.fn_index.iter().map(|(k, &v)| (v, k.clone())).collect();

            for chunk in &mut chunks {
                let mut i = 0;
                while i < chunk.code.len() {
                    let instr = chunk.code[i];
                    if instr.opcode != Opcode::CallIdx as u8 {
                        i += 1;
                        continue;
                    }

                    let (dst, fn_idx) = instr.ri16();
                    let Some(callee_name) = idx_to_name.get(&fn_idx) else {
                        i += 1;
                        continue;
                    };
                    let Some(callee) = callee_map.get(callee_name) else {
                        i += 1;
                        continue;
                    };

                    // Collect preceding CallArg instructions.
                    let arg_count = callee.param_count;
                    if i < arg_count {
                        i += 1;
                        continue;
                    }
                    let call_start = i - arg_count;
                    let all_callargs = chunk.code[call_start..i]
                        .iter()
                        .all(|ins| ins.opcode == Opcode::CallArg as u8);
                    if !all_callargs {
                        i += 1;
                        continue;
                    }

                    let arg_regs: Vec<u8> = chunk.code[call_start..i]
                        .iter()
                        .map(|ins| ins.ops[0])
                        .collect();

                    // Remap and inline.
                    let base = chunk.reg_count;
                    let remap = |r: u8| base.wrapping_add(r);

                    let mut inlined: Vec<crate::bytecode::instruction::Instruction> = Vec::new();

                    // Copy args into callee's param registers (base+0, base+1, ...).
                    for (k, &arg) in arg_regs.iter().enumerate() {
                        let param_reg = remap(k as u8);
                        if param_reg != arg {
                            inlined.push(rrr(Opcode::Mov, param_reg, arg, 0));
                        }
                    }

                    // Remap callee body, drop final Ret.
                    let body: &[_] = if callee
                        .code
                        .last()
                        .map(|x| x.opcode == Opcode::Ret as u8)
                        .unwrap_or(false)
                    {
                        &callee.code[..callee.code.len() - 1]
                    } else {
                        &callee.code[..]
                    };

                    for &ins in body {
                        let mut r = ins;
                        remap_instr_regs(&mut r, remap);
                        inlined.push(r);
                    }

                    // Move return value (base+0 = remapped r0) to dst.
                    let ret_reg = remap(0);
                    if ret_reg != dst {
                        inlined.push(rrr(Opcode::Mov, dst, ret_reg, 0));
                    }

                    // Replace the CallArg* + CallIdx range.
                    chunk.code.splice(call_start..=i, inlined);
                    chunk.reg_count = base.wrapping_add(callee.reg_count);
                    // Adjust i: we removed (arg_count + 1) instrs, restart from call_start.
                    i = call_start;
                }
            }
        }

        chunks
    }

    fn compile_fn(
        &self,
        name: &str,
        params: &[crate::parser::ast::Param],
        body: Option<&Block>,
        attributes: &[crate::parser::ast::Attribute],
    ) -> Option<Chunk> {
        // @intrinsic: emit a platform-neutral Intrinsic instruction.
        if let Some(attr) = attributes.iter().find(|a| a.name == "intrinsic") {
            return Some(self.compile_intrinsic_fn(name, params, attr));
        }

        // @syscall: emit a single Syscall instruction instead of the function body.
        if let Some(attr) = attributes.iter().find(|a| a.name == "syscall") {
            return Some(self.compile_syscall_fn(name, params, attr));
        }

        // @api: emit a single CallExt instruction instead of the function body.
        if let Some(attr) = attributes.iter().find(|a| a.name == "api") {
            return Some(self.compile_api_fn(name, params, attr));
        }

        // Bodyless declaration — no code to emit; linker must resolve calls.
        let body = body?;

        let mut fc = FnCompiler::new(
            name,
            params.len(),
            &self.fn_index,
            &self.const_map,
            &self.import_names,
        );
        for p in params {
            fc.bind(p.name.clone());
        }
        fc.compile_block(body);
        // Guarantee every path ends with Ret.
        if fc.chunk.code.last().map(|i| i.opcode) != Some(Opcode::Ret as u8) {
            fc.chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
        }
        fc.chunk.reg_count = fc.next_reg;
        Some(fc.chunk)
    }

    fn compile_syscall_fn(
        &self,
        name: &str,
        params: &[crate::parser::ast::Param],
        attr: &crate::parser::ast::Attribute,
    ) -> Chunk {
        use crate::parser::ast::{AttrArg, AttrVal};
        let mut chunk = Chunk::with_params(name, params.len());
        // Store name or raw number in const pool — arch-neutral VBC.
        let entry = match attr.args.first() {
            Some(AttrArg::Positional(AttrVal::Int(n))) => ConstPoolEntry::Int(*n),
            Some(AttrArg::Positional(AttrVal::Str(s))) => ConstPoolEntry::Str(s.clone()),
            _ => ConstPoolEntry::Str(String::new()),
        };
        let idx = chunk.add_constant(entry);
        let arg_count = params.len() as u8;
        let mut instr = ri16(Opcode::Syscall, 0, idx);
        instr.flags = arg_count;
        chunk.emit(instr);
        chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
        chunk
    }

    fn compile_intrinsic_fn(
        &self,
        name: &str,
        params: &[crate::parser::ast::Param],
        attr: &crate::parser::ast::Attribute,
    ) -> Chunk {
        let mut chunk = Chunk::with_params(name, params.len());
        let id = intrinsic_id(attr);
        let arg_count = params.len() as u8;
        let mut instr = ri16(Opcode::Intrinsic, 0, id);
        instr.flags = arg_count;
        chunk.emit(instr);
        chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
        chunk
    }

    fn compile_api_fn(
        &self,
        name: &str,
        params: &[crate::parser::ast::Param],
        attr: &crate::parser::ast::Attribute,
    ) -> Chunk {
        let mut chunk = Chunk::with_params(name, params.len());
        let symbol = api_symbol(attr);
        let sym_idx = chunk.add_constant(ConstPoolEntry::Str(symbol));
        // Params are in r0..r(n-1) by calling convention.
        // Emit CallExt: dst=r0, sym_idx, flags=arg_count.
        let arg_count = params.len() as u8;
        let mut instr = ri16(Opcode::CallExt, 0, sym_idx);
        instr.flags = arg_count;
        chunk.emit(instr);
        chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
        chunk
    }
}

// ── Per-function compiler ─────────────────────────────────────────────────────

struct FnCompiler<'a> {
    chunk: Chunk,
    regs: HashMap<String, u8>,
    next_reg: u8,
    fn_index: &'a HashMap<String, u16>,
    const_map: &'a HashMap<(usize, usize), ConstValue>,
    // type_map: &'a HashMap<(usize, usize), TypeKind>,
    import_names: &'a HashSet<String>,
}

impl<'a> FnCompiler<'a> {
    fn new(
        name: &str,
        param_count: usize,
        fn_index: &'a HashMap<String, u16>,
        const_map: &'a HashMap<(usize, usize), ConstValue>,
        // type_map: &'a HashMap<(usize, usize), TypeKind>,
        import_names: &'a HashSet<String>,
    ) -> Self {
        Self {
            chunk: Chunk::with_params(name, param_count),
            regs: HashMap::new(),
            next_reg: 0,
            fn_index,
            const_map,
            // type_map,
            import_names,
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

    fn compile_block(&mut self, block: &Block) -> bool {
        for stmt in &block.stmts {
            if self.compile_stmt(stmt) {
                return true;
            }
        }
        false
    }

    /// Returns true if the statement guarantees exit (return).
    fn compile_stmt(&mut self, stmt: &Stmt) -> bool {
        match &stmt.node {
            StmtKind::Var { name, value, .. } => {
                if let Some(expr) = value {
                    let src = self.compile_expr(expr);
                    self.regs.insert(name.clone(), src);
                } else {
                    self.bind(name.clone());
                }
                false
            }
            StmtKind::Const { name, value, .. } => {
                let src = self.compile_expr(value);
                self.regs.insert(name.clone(), src);
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
            StmtKind::CfgBlock { body, condition } => {
                if cfg_condition_matches(condition) {
                    self.compile_block(body)
                } else {
                    false
                }
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                // Constant-condition elimination: skip the dead branch entirely.
                let cond_key = (condition.span.start, condition.span.end);
                if let Some(ConstValue::Bool(b)) = self.const_map.get(&cond_key).cloned() {
                    return if b {
                        self.compile_block(then_block)
                    } else if let Some(eb) = else_block {
                        self.compile_block(eb)
                    } else {
                        false
                    };
                }

                // Emit condition + jump-if-false past the then block.
                let jump_else = self.compile_condition_jump(condition, true);
                let then_returns = self.compile_block(then_block);

                if let Some(else_block) = else_block {
                    let jump_end = self.chunk.emit(ri16(Opcode::Jmp, 0, 0));
                    self.chunk.patch_jump(jump_else, self.chunk.len() as u16);
                    let else_returns = self.compile_block(else_block);
                    self.chunk.patch_jump(jump_end, self.chunk.len() as u16);
                    then_returns && else_returns
                } else {
                    self.chunk.patch_jump(jump_else, self.chunk.len() as u16);
                    false
                }
            }
            StmtKind::For { kind, body } => {
                match kind {
                    ForLoop::Cond { condition: None } => {
                        // Infinite loop.
                        let loop_top = self.chunk.len() as u16;
                        self.compile_block(body);
                        self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                    }
                    ForLoop::Cond {
                        condition: Some(condition),
                    } => {
                        // While-like loop.
                        let cond_key = (condition.span.start, condition.span.end);
                        if let Some(ConstValue::Bool(false)) =
                            self.const_map.get(&cond_key).cloned()
                        {
                            return false;
                        }
                        let loop_top = self.chunk.len() as u16;
                        let jump_exit = self.compile_condition_jump(condition, true);
                        self.compile_block(body);
                        self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                        self.chunk.patch_jump(jump_exit, self.chunk.len() as u16);
                    }
                    ForLoop::CStyle {
                        init,
                        condition,
                        update,
                    } => {
                        if let Some(init_stmt) = init {
                            self.compile_stmt(init_stmt);
                        }
                        let loop_top = self.chunk.len() as u16;
                        let jump_exit = if let Some(cond) = condition {
                            Some(self.compile_condition_jump(cond, true))
                        } else {
                            None
                        };
                        self.compile_block(body);
                        if let Some(upd) = update {
                            self.compile_expr(upd);
                        }
                        self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                        if let Some(je) = jump_exit {
                            self.chunk.patch_jump(je, self.chunk.len() as u16);
                        }
                    }
                    ForLoop::Each { vars, iter } => match iter {
                        ForIter::Range { start, end } => {
                            let loop_var = vars.first().map(|s| s.as_str()).unwrap_or("_");
                            let r_i = self.bind(loop_var.to_string());
                            let r_start = self.compile_expr(start);
                            if r_start != r_i {
                                self.chunk.emit(rrr(Opcode::Mov, r_i, r_start, 0));
                            }
                            let r_end = self.compile_expr(end);
                            let loop_top = self.chunk.len() as u16;
                            self.chunk.emit(rrr(Opcode::Cmp, 0, r_i, r_end));
                            let jump_exit = self.chunk.emit(ri16(Opcode::Jge, 0, 0));
                            self.compile_block(body);
                            self.chunk.emit(rrr(Opcode::Inc, r_i, r_i, 0));
                            self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                            self.chunk.patch_jump(jump_exit, self.chunk.len() as u16);
                        }
                        ForIter::Iter(expr) => {
                            let r_iter = self.compile_expr(expr);
                            for var in vars.iter() {
                                self.bind(var.clone());
                            }
                            let loop_top = self.chunk.len() as u16;
                            let jump_exit = self.chunk.emit(ri16(Opcode::Jz, r_iter, 0));
                            self.compile_block(body);
                            self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                            self.chunk.patch_jump(jump_exit, self.chunk.len() as u16);
                        }
                    },
                }
                false
            }
            StmtKind::UnsafeBlock { body } => {
                self.compile_block(body);
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
            ExprKind::Binary { left, op, right } if is_comparison(op) => {
                let r1 = self.compile_expr(left);
                let r2 = self.compile_expr(right);
                self.chunk.emit(rrr(Opcode::Cmp, 0, r1, r2));
                let jop = if jump_if_false {
                    negate_cmp(op)
                } else {
                    direct_cmp(op)
                };
                self.chunk.emit(ri16(jop, 0, 0))
            }
            ExprKind::Group(inner) => self.compile_condition_jump(inner, jump_if_false),
            _ => {
                let r = self.compile_expr(expr);
                let jop = if jump_if_false {
                    Opcode::Jz
                } else {
                    Opcode::Jnz
                };
                // ri16 layout: ops[0]=register, ops[1..2]=target (patched later)
                self.chunk.emit(ri16(jop, r, 0))
            }
        }
    }

    fn emit_call_by_name(&mut self, name: &str, arg_regs: &[u8], dst: u8) {
        if let Some(&idx) = self.fn_index.get(name) {
            for &r in arg_regs {
                self.chunk.emit(rrr(Opcode::CallArg, r, 0, 0));
            }
            // ops[0] = dst reg for return value; ops[1..2] = fn index
            self.chunk.emit(ri16(Opcode::CallIdx, dst, idx));
        }
    }

    fn is_module_import_receiver(&self, object: &Expr) -> bool {
        let Some((base, _)) = extract_field_chain(object) else {
            return false;
        };
        if self.regs.contains_key(&base) {
            return false;
        }
        self.import_names.contains(&base)
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

            ExprKind::Unary { op, expr: inner } => match op {
                UnaryOpKind::Ref => {
                    let src = self.compile_expr(inner);
                    let dst = self.alloc_reg();
                    self.chunk.emit(mem_lea(src, dst, 0));
                    dst
                }
                UnaryOpKind::Deref => {
                    let ptr = self.compile_expr(inner);
                    let dst = self.alloc_reg();
                    self.chunk.emit(mem_load(ptr, dst, 0));
                    dst
                }
                UnaryOpKind::Neg => {
                    let src = self.compile_expr(inner);
                    let dst = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Neg, dst, src, 0));
                    dst
                }
                UnaryOpKind::Not => {
                    let src = self.compile_expr(inner);
                    let dst = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Not, dst, src, 0));
                    dst
                }
            },

            // Short-circuit logical ops — lazy right evaluation.
            ExprKind::Binary {
                left,
                op: BinOpKind::AndAnd,
                right,
            } => {
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
            ExprKind::Binary {
                left,
                op: BinOpKind::OrOr,
                right,
            } => {
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
                    BinOpKind::Add => {
                        self.chunk.emit(rrr(Opcode::Add, dst, r1, r2));
                    }
                    BinOpKind::Sub => {
                        self.chunk.emit(rrr(Opcode::Sub, dst, r1, r2));
                    }
                    BinOpKind::Mul => {
                        self.chunk.emit(rrr(Opcode::Mul, dst, r1, r2));
                    }
                    BinOpKind::Div => {
                        self.chunk.emit(rrr(Opcode::Div, dst, r1, r2));
                    }
                    BinOpKind::Mod => {
                        self.chunk.emit(rrr(Opcode::Mod, dst, r1, r2));
                    }
                    // Comparisons: materialize bool result into dst.
                    _ if is_comparison(op) => {
                        self.chunk.emit(rrr(Opcode::Cmp, 0, r1, r2));
                        self.chunk.emit(ri16(Opcode::MovI, dst, 1));
                        let skip = self.chunk.emit(ri16(direct_cmp(op), 0, 0));
                        self.chunk.emit(ri16(Opcode::MovI, dst, 0));
                        self.chunk.patch_jump(skip, self.chunk.len() as u16);
                    }
                    BinOpKind::Pow => {
                        self.chunk.emit(rrr(Opcode::Pow, dst, r1, r2));
                    }
                    BinOpKind::AndAnd | BinOpKind::OrOr => unreachable!(),
                    _ => {
                        self.chunk.emit(rrr(Opcode::Add, dst, r1, r2));
                    } // fallback
                }
                dst
            }

            ExprKind::Assign { target, value } => {
                let src = self.compile_expr(value);
                match &target.node {
                    ExprKind::Ident(name) => {
                        let dst = self.reg_of(name);
                        if dst != src {
                            self.chunk.emit(rrr(Opcode::Mov, dst, src, 0));
                        }
                        dst
                    }
                    ExprKind::Unary {
                        op: UnaryOpKind::Deref,
                        expr: ptr_expr,
                    } => {
                        let ptr = self.compile_expr(ptr_expr);
                        self.chunk.emit(mem_store(ptr, src, 0));
                        src
                    }
                    _ => src,
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

            ExprKind::IncDec {
                expr: inner, op, ..
            } => {
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
                let arg_regs: Vec<u8> = args.iter().map(|a| self.compile_expr(a)).collect();
                let dst = self.alloc_reg();
                if let ExprKind::Ident(name) = &callee.node {
                    self.emit_call_by_name(name, &arg_regs, dst);
                }
                dst
            }

            ExprKind::MethodCall {
                object,
                method,
                type_args,
                args,
            } => {
                if self.is_module_import_receiver(object) {
                    let arg_regs: Vec<u8> = args.iter().map(|a| self.compile_expr(a)).collect();
                    let dst = self.alloc_reg();
                    self.emit_call_by_name(method, &arg_regs, dst);
                    return dst;
                }
                let obj = self.compile_expr(object);
                // str.len() — known built-in, emit StrLen directly.
                if method == "len" && args.is_empty() {
                    let dst = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::StrLen, dst, obj, 0));
                    return dst;
                }
                if (method == "to_string" || method == "to_str") && args.is_empty() {
                    let dst = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::PrimToStr, dst, obj, 0));
                    return dst;
                }
                if (method == "as_string" || method == "as_str") && args.is_empty() {
                    let dst = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::StrAsStr, dst, obj, 0));
                    return dst;
                }
                if method == "parse" {
                    let dst = self.alloc_reg();
                    let op = match type_args.first().map(|t| &t.node) {
                        Some(TypeKind::Float32) | Some(TypeKind::Float64) => Opcode::StrToFloat,
                        _ => Opcode::StrToInt,
                    };
                    self.chunk.emit(rrr(op, dst, obj, 0));
                    return dst;
                }
                // General vtable dispatch.
                let arg_regs: Vec<u8> = args.iter().map(|a| self.compile_expr(a)).collect();
                for &r in &arg_regs {
                    self.chunk.emit(rrr(Opcode::CallArg, r, 0, 0));
                }
                let vtbl = self.alloc_reg();
                let dst = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::VtblLoad, vtbl, obj, 0));
                self.chunk.emit(rrr(Opcode::CallReg, dst, vtbl, 0));
                dst
            }

            ExprKind::Field { object, .. } => {
                let obj = self.compile_expr(object);
                let dst = self.alloc_reg();
                // Field offset resolved by AOT backend — 0 is a placeholder.
                self.chunk.emit(rrr(Opcode::FieldLoad, dst, obj, 0));
                dst
            }

            ExprKind::ArrayLit(elems) => {
                let n = elems.len() as u8;
                let base = self.next_reg;
                self.next_reg = self.next_reg.wrapping_add(n);
                for (i, elem) in elems.iter().enumerate() {
                    let val = self.compile_expr(elem);
                    let dst = base + i as u8;
                    if val != dst {
                        self.chunk.emit(rrr(Opcode::Mov, dst, val, 0));
                    }
                }
                base
            }

            ExprKind::Index { object, index } => {
                let base = self.compile_expr(object);
                if let ExprKind::Literal(Literal::Int(n)) = &index.node {
                    if *n >= 0 {
                        return base + *n as u8;
                    }
                }
                // Dynamic index: Lea + scale + Sub + Load
                let idx = self.compile_expr(index);
                let ptr = self.alloc_reg();
                self.chunk.emit(mem_lea(base, ptr, 0));
                let eight = self.alloc_reg();
                self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                let offset = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::Mul, offset, idx, eight));
                self.chunk.emit(rrr(Opcode::Sub, ptr, ptr, offset));
                let dst = self.alloc_reg();
                self.chunk.emit(mem_load(ptr, dst, 0));
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

fn remap_instr_regs(
    instr: &mut crate::bytecode::instruction::Instruction,
    remap: impl Fn(u8) -> u8,
) {
    use crate::bytecode::opcode::Opcode;
    let Some(op) = Opcode::from_u8(instr.opcode) else {
        return;
    };
    match op {
        // No-op / no regs
        Opcode::Nop
        | Opcode::Ret
        | Opcode::MemFence
        | Opcode::Jmp
        | Opcode::Je
        | Opcode::Jne
        | Opcode::Jg
        | Opcode::Jge
        | Opcode::Jl
        | Opcode::Jle
        | Opcode::Ja
        | Opcode::Jb => {}

        // RI16 — ops[0]=dst only
        Opcode::MovI
        | Opcode::MovConst
        | Opcode::CallIdx
        | Opcode::CallExt
        | Opcode::Syscall
        | Opcode::New
        | Opcode::NewObj => {
            instr.ops[0] = remap(instr.ops[0]);
        }

        // Jz/Jnz: ops[0] may be a register (or 0 for flag-only)
        Opcode::Jz | Opcode::Jnz => {
            if instr.ops[0] != 0 {
                instr.ops[0] = remap(instr.ops[0]);
            }
        }

        // CallArg / Drop — single reg
        Opcode::CallArg | Opcode::Drop => {
            instr.ops[0] = remap(instr.ops[0]);
        }

        // MEM — ops[0]=val/dst, ops[1]=base
        Opcode::Load | Opcode::Store | Opcode::Lea => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
        }

        // Pow — RRR
        Opcode::Pow => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
            instr.ops[2] = remap(instr.ops[2]);
        }

        // RRR and all others — remap ops[0], ops[1], ops[2]
        _ => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
            instr.ops[2] = remap(instr.ops[2]);
        }
    }
}

fn extract_field_chain(expr: &Expr) -> Option<(String, Vec<String>)> {
    match &expr.node {
        ExprKind::Ident(name) => Some((name.clone(), vec![])),
        ExprKind::Field { object, name } => {
            let (base, mut path) = extract_field_chain(object)?;
            path.push(name.clone());
            Some((base, path))
        }
        _ => None,
    }
}

fn intrinsic_id(attr: &crate::parser::ast::Attribute) -> u16 {
    let name = attr
        .args
        .first()
        .and_then(|a| match a {
            crate::parser::ast::AttrArg::Positional(crate::parser::ast::AttrVal::Str(s)) => {
                Some(s.as_str())
            }
            _ => None,
        })
        .unwrap_or("");
    match name {
        "void.write" => 0,
        "void.read" => 1,
        "void.exit" => 2,
        "void.malloc" => 3,
        "void.free" => 4,
        "void.realloc" => 5,
        "void.memcpy" => 6,
        "void.memset" => 7,
        "void.memmove" => 8,
        "void.memcmp" => 9,
        "void.strlen" => 10,
        "void.stderr_write" => 11,
        "void.sleep_ms" => 12,
        "void.getenv" => 13,
        _ => 0,
    }
}

fn api_symbol(attr: &crate::parser::ast::Attribute) -> String {
    attr.args
        .first()
        .and_then(|a| match a {
            crate::parser::ast::AttrArg::Positional(crate::parser::ast::AttrVal::Str(s)) => {
                Some(s.clone())
            }
            _ => None,
        })
        .unwrap_or_default()
}

fn cfg_condition_matches(attr: &crate::parser::ast::Attribute) -> bool {
    use crate::parser::ast::{AttrArg, AttrVal};
    for arg in &attr.args {
        match arg {
            AttrArg::KeyValue(key, AttrVal::Str(val)) if key == "target_os" => {
                return val.as_str() == std::env::consts::OS;
            }
            _ => {}
        }
    }
    true // unknown condition — include unconditionally
}

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
        BinOpKind::Lt => Opcode::Jl,
        BinOpKind::LtEq => Opcode::Jle,
        BinOpKind::Gt => Opcode::Jg,
        BinOpKind::GtEq => Opcode::Jge,
        BinOpKind::EqEq => Opcode::Je,
        BinOpKind::NotEq => Opcode::Jne,
        _ => Opcode::Jnz,
    }
}

/// Conditional jump opcode that fires when the comparison is FALSE (negated).
fn negate_cmp(op: &BinOpKind) -> Opcode {
    match op {
        BinOpKind::Lt => Opcode::Jge,
        BinOpKind::LtEq => Opcode::Jg,
        BinOpKind::Gt => Opcode::Jle,
        BinOpKind::GtEq => Opcode::Jl,
        BinOpKind::EqEq => Opcode::Jne,
        BinOpKind::NotEq => Opcode::Je,
        _ => Opcode::Jz,
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
        assert!(
            report.errors.is_empty(),
            "semantic errors: {:?}",
            report.errors
        );
        Codegen::new(&report).compile_program(&program)
    }

    #[test]
    fn simple_add_function_emits_add_and_ret() {
        let chunks = compile("fn add(a: i32, b: i32) i32 { ret a + b; }");
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
        let chunks = compile("fn foo() i32 { const x: i32 = 1 + 2; ret x; }");
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
            r#"fn countdown(x: i32) void {
                for x > 0 { x = x + 1; }
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
        assert_eq!(
            target, 0,
            "back-edge Jmp must target instruction 0 (loop top)"
        );
    }

    #[test]
    fn if_else_jump_targets_are_patched() {
        let chunks = compile(
            r#"fn sign(x: i32) i32 {
                if (x > 0) { ret 1; } else { ret 0; }
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
                    Opcode::Jmp
                        | Opcode::Je
                        | Opcode::Jne
                        | Opcode::Jl
                        | Opcode::Jle
                        | Opcode::Jg
                        | Opcode::Jge
                        | Opcode::Jz
                        | Opcode::Jnz
                )
            );
            if is_jump {
                let (_, target) = instr.ri16();
                assert!(
                    target <= len,
                    "jump target {} out of bounds (chunk has {} instructions)",
                    target,
                    len
                );
            }
        }
    }

    #[test]
    fn function_call_emits_call_idx() {
        // Use a function with more than 2 statements so it won't be inlined.
        let chunks = compile(
            r#"fn helper(x: i32) i32 { const a: i32 = 1; const b: i32 = 2; ret x; }
               fn main() void { helper(1); }"#,
        );
        let main_chunk = chunks
            .iter()
            .find(|c| c.name == "main")
            .expect("no main chunk");
        assert!(
            main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::CallIdx as u8),
            "expected CallIdx in main"
        );
    }

    #[test]
    fn ret_always_last_in_every_chunk() {
        let chunks = compile(
            r#"fn a() void {}
               fn b(x: i32) i32 { ret x; }"#,
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
            r#"fn inc(x: i32) i32 {
                x += 1;
                ret x;
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
            r#"fn bump(x: i32) i32 {
                x++;
                ret x;
            }"#,
        );
        assert!(
            chunks[0].code.iter().any(|i| i.opcode == Opcode::Inc as u8),
            "x++ should emit Inc"
        );
    }

    #[test]
    fn large_int_goes_to_constant_pool() {
        let chunks = compile("fn big() i32 { ret 100000; }");
        assert!(
            !chunks[0].constants.is_empty(),
            "100000 should be in constant pool"
        );
        assert!(
            chunks[0]
                .code
                .iter()
                .any(|i| i.opcode == Opcode::MovConst as u8),
            "expected MovConst for large literal"
        );
    }

    #[test]
    fn string_literal_goes_to_constant_pool() {
        let chunks = compile(r#"fn greeting() str { ret "hello"; }"#);
        assert!(
            matches!(chunks[0].constants.first(), Some(ConstPoolEntry::Str(s)) if s == "hello"),
            "string literal should be in constant pool"
        );
    }

    #[test]
    fn to_bytes_produces_six_bytes_per_instruction() {
        let chunks = compile("fn f(a: i32, b: i32) i32 { ret a + b; }");
        let bytes = chunks[0].to_bytes();
        assert_eq!(bytes.len(), chunks[0].code.len() * 6);
    }

    #[test]
    fn negative_const_value_goes_to_constant_pool() {
        // const-folded 0 - 1 produces ConstValue::Int(-1) which must use MovConst not MovI
        let chunks = compile("fn neg() i32 { const x: i32 = 0 - 1; ret x; }");
        assert!(
            chunks[0]
                .code
                .iter()
                .any(|i| i.opcode == Opcode::MovConst as u8),
            "negative constant should be in constant pool"
        );
    }

    #[test]
    fn syscall_attribute_emits_syscall_opcode() {
        let chunks =
            compile(r#"@syscall("write") fn write(fd: i32, buf: str, len: usize) isize { }"#);
        assert_eq!(chunks[0].name, "write");
        assert!(
            chunks[0]
                .code
                .iter()
                .any(|i| i.opcode == Opcode::Syscall as u8),
            "expected Syscall instruction for @syscall fn"
        );
        assert_eq!(chunks[0].code.last().unwrap().opcode, Opcode::Ret as u8);
    }

    #[test]
    fn syscall_attribute_accepts_numeric_id() {
        let chunks = compile(r#"@syscall(60) fn exit(code: i32) isize { }"#);
        let instr = chunks[0]
            .code
            .iter()
            .find(|i| i.opcode == Opcode::Syscall as u8)
            .expect("expected Syscall instruction");
        // Numeric syscall id is stored in the const pool, ri16 gives the pool index.
        let (_, idx) = instr.ri16();
        assert!(
            matches!(
                chunks[0].constants.get(idx as usize),
                Some(ConstPoolEntry::Int(60))
            ),
            "expected Int(60) in const pool at index {idx}"
        );
        assert_eq!(instr.flags, 1);
    }

    #[test]
    fn api_attribute_emits_call_ext_opcode() {
        let chunks = compile(
            r#"@api("WriteFile") fn win_write(h: usize, buf: str, len: usize, out: usize, ovl: usize) usize { }"#,
        );
        let chunk = &chunks[0];
        assert!(
            chunk.code.iter().any(|i| i.opcode == Opcode::CallExt as u8),
            "expected CallExt instruction for @api fn"
        );
        assert!(
            chunk
                .constants
                .iter()
                .any(|c| matches!(c, ConstPoolEntry::Str(s) if s == "WriteFile")),
            "expected WriteFile in constant pool"
        );
    }

    #[test]
    fn str_len_emits_strlen_opcode() {
        let chunks = compile(r#"fn f(s: str) any { ret s.len(); }"#);
        assert!(
            chunks[0]
                .code
                .iter()
                .any(|i| i.opcode == Opcode::StrLen as u8),
            "s.len() should emit StrLen"
        );
    }

    #[test]
    fn method_call_emits_call_arg_and_vtbl_load() {
        let chunks = compile(
            r#"fn main() void {
                   var s: any = 0;
                   s.doSomething(1);
               }"#,
        );
        let main_chunk = chunks.iter().find(|c| c.name == "main").unwrap();
        assert!(
            main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::CallArg as u8),
            "method call with args should emit CallArg"
        );
        assert!(
            main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::VtblLoad as u8),
            "method call should emit VtblLoad"
        );
    }

    #[test]
    fn tree_shaking_omits_unreachable_function() {
        // dead_fn is called only by zombie_fn; zombie_fn is never called by main.
        // Tree-shaking should exclude both from the output chunks.
        let chunks = compile(
            r#"fn dead_fn(x: i32) i32 { const a: i32 = 1; const b: i32 = 2; ret x; }
               fn zombie_fn() void { dead_fn(1); }
               fn main() void { ret; }"#,
        );
        assert!(
            !chunks.iter().any(|c| c.name == "dead_fn"),
            "dead_fn should be tree-shaken"
        );
        assert!(
            !chunks.iter().any(|c| c.name == "zombie_fn"),
            "zombie_fn should be tree-shaken"
        );
        assert!(
            chunks.iter().any(|c| c.name == "main"),
            "main must be present"
        );
    }

    #[test]
    fn const_true_if_skips_else_branch() {
        // `1 == 1` const-folds to Bool(true) — else branch must not be compiled.
        let chunks = compile(
            r#"fn f() i32 {
                if (1 == 1) { ret 1; } else { ret 2; }
            }"#,
        );
        let code = &chunks[0].code;
        // With const-condition elimination, no conditional jump instruction.
        let has_conditional_jump = code.iter().any(|i| {
            matches!(
                Opcode::from_u8(i.opcode),
                Some(
                    Opcode::Je
                        | Opcode::Jne
                        | Opcode::Jl
                        | Opcode::Jle
                        | Opcode::Jg
                        | Opcode::Jge
                        | Opcode::Jz
                        | Opcode::Jnz
                )
            )
        });
        assert!(
            !has_conditional_jump,
            "const-true if should not emit a conditional jump"
        );
        // Dead else branch must not emit MovI(2).
        let has_movi_2 = code.iter().any(|i| {
            let (_, imm) = i.ri16();
            i.opcode == Opcode::MovI as u8 && imm == 2
        });
        assert!(
            !has_movi_2,
            "const-true if should not emit MovI(2) from dead else branch"
        );
    }

    #[test]
    fn const_false_if_skips_then_branch() {
        // `0 == 1` const-folds to Bool(false) — then branch must not be compiled.
        let chunks = compile(
            r#"fn f() i32 {
                if (0 == 1) { ret 99; } else { ret 7; }
            }"#,
        );
        let code = &chunks[0].code;
        let has_movi_99 = code.iter().any(|i| {
            let (_, imm) = i.ri16();
            i.opcode == Opcode::MovI as u8 && imm == 99
        });
        assert!(
            !has_movi_99,
            "const-false if should not emit MovI(99) from dead then branch"
        );
    }

    #[test]
    fn const_false_while_emits_no_loop_instructions() {
        // `0 == 1` const-folds to Bool(false) — while body must be skipped entirely.
        let chunks = compile(
            r#"fn f() void {
                for 0 == 1 { var x: i32 = 1; }
            }"#,
        );
        let code = &chunks[0].code;
        // No loop-back Jmp should exist.
        assert!(
            !code.iter().any(|i| i.opcode == Opcode::Jmp as u8),
            "for(0==1) should emit no Jmp"
        );
    }
}
=======
// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use super::instruction::{mem_lea, mem_load, mem_store, ri16, rrr};
use super::{Chunk, ConstPoolEntry, Opcode};
use crate::parser::ast::*;
use crate::semantic::{ConstValue, DependencyKind, SemanticReport};

// Enum heap layout invariants:
//   - discriminant (tag) is stored at ENUM_DISCRIM_OFFSET (8 bytes)
//   - variant payloads start at ENUM_PAYLOAD_OFFSET (8 bytes each)
const ENUM_DISCRIM_OFFSET: u8 = 0;
const ENUM_PAYLOAD_OFFSET: u8 = 8;

/// Compute the allocation size (in bytes) for an enum variant with `payload_count` payloads.
/// The layout is: discriminant (8 bytes) + payload_count * 8 bytes per payload,
/// with a minimum of 16 bytes.
fn enum_variant_alloc_size(payload_count: usize) -> u16 {
    ((payload_count + 1) * 8).max(16) as u16
}

// ── Public entry point ────────────────────────────────────────────────────────

pub struct Codegen<'a> {
    report: &'a SemanticReport,
    fn_index: HashMap<String, u16>,
    const_map: HashMap<(usize, usize), ConstValue>,
    type_map: HashMap<(usize, usize), TypeKind>,
    import_names: HashSet<String>,
    /// Maps variadic function name → number of fixed (non-variadic) params.
    variadic_fn_info: HashMap<String, usize>,
    /// Functions/methods marked @format: compiler pre-formats args at call sites.
    format_fns: HashSet<String>,
    /// Variadic @intrinsic functions: call with coerced args directly (no pre-format step).
    variadic_intrinsic_fns: HashSet<String>,
}

impl<'a> Codegen<'a> {
    pub fn new(report: &'a SemanticReport) -> Self {
        let mut const_map = HashMap::new();
        let mut type_map = HashMap::new();
        for ann in &report.annotated_exprs {
            let key = (ann.span.start, ann.span.end);
            if let Some(cv) = &ann.const_value {
                const_map.insert(key, cv.clone());
            }
            if let Some(ty) = &ann.ty {
                type_map.insert(key, ty.clone());
            }
        }
        let mut import_names = HashSet::new();
        for entry in &report.symbol_table.entries {
            if entry.symbol.is_import {
                import_names.insert(entry.name.clone());
            }
        }
        let mut variadic_fn_info = HashMap::new();
        for entry in &report.symbol_table.entries {
            if entry.symbol.variadic {
                let fixed = entry.symbol.params.len().saturating_sub(1);
                variadic_fn_info.insert(entry.name.clone(), fixed);
            }
        }
        Self {
            report,
            fn_index: HashMap::new(),
            const_map,
            type_map,
            import_names,
            variadic_fn_info,
            format_fns: HashSet::new(),
            variadic_intrinsic_fns: HashSet::new(),
        }
    }

    pub fn compile_program(&mut self, program: &Program) -> Vec<Chunk> {
        // Pre-pass: collect @format, variadic @intrinsic, and @panic_handler names.
        let mut user_panic_handler: Option<String> = None;
        for item in &program.items {
            // Skip @cfg-disabled items.
            if let ItemKind::Fn { attributes, .. } = &item.node {
                if !item_cfg_active(attributes) {
                    continue;
                }
            }
            match &item.node {
                ItemKind::Fn { name, attributes, params, .. } => {
                    if attributes.iter().any(|a| a.name == "format") {
                        self.format_fns.insert(name.clone());
                    }
                    if attributes.iter().any(|a| a.name == "intrinsic")
                        && params.last().map(|p| p.variadic).unwrap_or(false)
                    {
                        self.variadic_intrinsic_fns.insert(name.clone());
                    }
                    if attributes.iter().any(|a| a.name == "panic_handler") {
                        user_panic_handler = Some(name.clone());
                    }
                }
                ItemKind::Impl { for_ty, methods, .. } => {
                    let type_name = type_kind_base_name(&for_ty.node);
                    for method in methods {
                        if let ItemKind::Fn { name, attributes, .. } = &method.node {
                            if attributes.iter().any(|a| a.name == "format") {
                                self.format_fns.insert(format!("{}.{}", type_name, name));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Compute the set of functions reachable from main via the call graph.
        // Library mode (no main) compiles everything.
        let has_main = program
            .items
            .iter()
            .any(|item| matches!(&item.node, ItemKind::Fn { name, .. } if name == "main"));

        let reachable: Option<std::collections::HashSet<String>> = if has_main {
            let mut set = std::collections::HashSet::new();
            set.insert("main".to_string());
            let mut queue = vec!["main".to_string()];
            while let Some(fn_name) = queue.pop() {
                for edge in &self.report.dependency_graph.edges {
                    if edge.kind == DependencyKind::Call && edge.from == fn_name {
                        if set.insert(edge.to.clone()) {
                            queue.push(edge.to.clone());
                        }
                    }
                }
            }
            // @panic_handler: the user's function is compiled under __void_panic_handler but
            // its own body's Call edges are indexed by the original function name. Seed BFS
            // from the original name so its dependencies are included.
            if set.contains("__void_panic_handler") {
                if let Some(ph_name) = &user_panic_handler {
                    if set.insert(ph_name.clone()) {
                        let mut q2 = vec![ph_name.clone()];
                        while let Some(fn_name) = q2.pop() {
                            for edge in &self.report.dependency_graph.edges {
                                if edge.kind == DependencyKind::Call && edge.from == fn_name {
                                    if set.insert(edge.to.clone()) {
                                        q2.push(edge.to.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Some(set)
        } else {
            None
        };

        let is_live =
            |name: &str| -> bool { reachable.as_ref().map_or(true, |r| r.contains(name)) };

        // Pass 1: assign each live function a table index.
        let mut idx = 0u16;
        for item in &program.items {
            if let ItemKind::Fn { name, attributes, .. } = &item.node {
                let is_ph = attributes.iter().any(|a| a.name == "panic_handler");
                // When user has @panic_handler, skip the stdlib default.
                if name == "__void_panic_handler" && user_panic_handler.is_some() {
                    continue;
                }
                // @panic_handler fn is registered under the handler slot name.
                let index_name = if is_ph { "__void_panic_handler" } else { name.as_str() };
                if is_live(index_name) || is_ph {
                    self.fn_index.insert(index_name.to_string(), idx);
                    idx += 1;
                }
            }
        }
        // Index live impl methods as "TypeName.method_name".
        for item in &program.items {
            if let ItemKind::Impl { for_ty, methods, .. } = &item.node {
                let type_name = type_kind_base_name(&for_ty.node);
                for method in methods {
                    if let ItemKind::Fn { name, .. } = &method.node {
                        let mangled = format!("{}.{}", type_name, name);
                        if is_live(&mangled) {
                            self.fn_index.insert(mangled, idx);
                            idx += 1;
                        }
                    }
                }
            }
        }

        // Index monomorphized specializations.
        for mono in &self.report.monomorphizations {
            let mono_name = &mono.mangled_name;
            // Only add if the specialized name is reachable.
            if is_live(mono_name) && !self.fn_index.contains_key(mono_name) {
                self.fn_index.insert(mono_name.clone(), idx);
                idx += 1;
            }
        }

        // Pass 2: compile each live function body.
        let mut chunks = Vec::new();
        let mut next_closure_idx = 0u16;
        for item in &program.items {
            if let ItemKind::Fn {
                name,
                params,
                body,
                attributes,
                ..
                            } = &item.node
            {
                if !item_cfg_active(attributes) {
                    continue;
                }
                let is_ph = attributes.iter().any(|a| a.name == "panic_handler");
                // Skip stdlib default handler when user has their own.
                if name == "__void_panic_handler" && user_panic_handler.is_some() {
                    continue;
                }
                // @panic_handler fn is compiled under the handler slot name.
                let compile_name = if is_ph { "__void_panic_handler" } else { name.as_str() };
                if is_live(compile_name) || is_ph {
                    if let Some(chunk) = self.compile_fn(
                        compile_name,
                        params,
                        body.as_ref().map(|b| b as &Block),
                        attributes,
                        &mut chunks,
                        &mut next_closure_idx,
                    ) {
                        chunks.push(chunk);
                    }
                }
            }
        }
        // Compile live impl methods.
        for item in &program.items {
            if let ItemKind::Impl { for_ty, methods, .. } = &item.node {
                let type_name = type_kind_base_name(&for_ty.node);
                for method in methods {
                    if let ItemKind::Fn {
                        name,
                        params,
                        body,
                        attributes,
                        ..
                    } = &method.node
                    {
                        if !item_cfg_active(attributes) {
                            continue;
                        }
                        let mangled = format!("{}.{}", type_name, name);
                        if is_live(&mangled) {
                            if let Some(chunk) = self.compile_fn(
                                &mangled,
                                params,
                                body.as_ref().map(|b| b as &Block),
                                attributes,
                                &mut chunks,
                                &mut next_closure_idx,
                            ) {
                                chunks.push(chunk);
                            }
                        }
                    }
                }
            }
        }

        // Compile monomorphized specializations.
        for mono in &self.report.monomorphizations {
            let mono_name = &mono.mangled_name;
            if !self.fn_index.contains_key(mono_name) {
                continue;
            }
            // Already compiled as a regular item? Skip duplicate.
            if chunks.iter().any(|c| c.name == *mono_name) {
                continue;
            }
            // Find the original generic function item.
            let original = program.items.iter().find(|item| {
                matches!(&item.node, ItemKind::Fn { name, .. } if name == &mono.fn_name)
            });
            if let Some(Item {
                node: ItemKind::Fn { params, body, attributes, generic_params, .. },
                ..
            }) = original
            {
                let subst: HashMap<String, TypeKind> = generic_params.iter()
                    .zip(mono.type_args.iter())
                    .map(|(p, t)| (p.clone(), t.clone()))
                    .collect();
                if let Some(chunk) = self.compile_fn_with_subst(
                    mono_name,
                    params,
                    body.as_ref().map(|b| b as &Block),
                    attributes,
                    &mut chunks,
                    &mut next_closure_idx,
                    subst,
                ) {
                    chunks.push(chunk);
                }
            }
        }

        // Post-pass: inline small functions that are inline candidates.
        let inline_set: std::collections::HashSet<String> = self
            .report
            .inline_candidates
            .iter()
            .map(|c| c.name.clone())
            .collect();

        // Variadic intrinsics must always be inlined so the caller's actual arg count
        // propagates into the Intrinsic instruction's flags field.  They have no AST
        // body so the semantic pass never adds them to inline_candidates; we include
        // them here unconditionally.
        let has_variadic_intrinsics = chunks.iter().any(|c| c.variadic && c.intrinsic);

        if !inline_set.is_empty() || has_variadic_intrinsics {
            // Build a snapshot of callee chunks before mutating.
            let callee_map: std::collections::HashMap<String, Chunk> = chunks
                .iter()
                .filter(|c| inline_set.contains(&c.name) || (c.variadic && c.intrinsic))
                .map(|c| (c.name.clone(), c.clone()))
                .collect();

            // Build reverse index: fn_idx -> name
            let idx_to_name: std::collections::HashMap<u16, String> =
                self.fn_index.iter().map(|(k, &v)| (v, k.clone())).collect();

            for chunk in &mut chunks {
                let mut i = 0;
                while i < chunk.code.len() {
                    let instr = chunk.code[i];
                    if instr.opcode != Opcode::CallIdx as u8 {
                        i += 1;
                        continue;
                    }

                    let (dst, fn_idx) = instr.ri16();
                    let Some(callee_name) = idx_to_name.get(&fn_idx) else {
                        i += 1;
                        continue;
                    };
                    let Some(callee) = callee_map.get(callee_name) else {
                        i += 1;
                        continue;
                    };

                    // Skip inlining if callee contains jump instructions — jump targets are
                    // callee-relative offsets and the inline pass does not remap them.
                    let has_jumps = callee.code.iter().any(|ins| {
                        let op = ins.opcode;
                        op == Opcode::Jmp as u8
                            || op == Opcode::Je as u8
                            || op == Opcode::Jne as u8
                            || op == Opcode::Jz as u8
                            || op == Opcode::Jnz as u8
                    });
                    if has_jumps {
                        i += 1;
                        continue;
                    }

                    // Collect preceding CallArg instructions.
                    let (call_start, arg_regs) = if callee.variadic {
                        // Variadic: scan backward for all consecutive CallArgs.
                        let mut start = i;
                        while start > 0
                            && chunk.code[start - 1].opcode == Opcode::CallArg as u8
                        {
                            start -= 1;
                        }
                        let min_args = callee.param_count.saturating_sub(1);
                        if i - start < min_args {
                            i += 1;
                            continue;
                        }
                        let regs: Vec<u8> =
                            chunk.code[start..i].iter().map(|ins| ins.ops[0]).collect();
                        (start, regs)
                    } else {
                        let arg_count = callee.param_count;
                        if i < arg_count {
                            i += 1;
                            continue;
                        }
                        let start = i - arg_count;
                        let all_callargs = chunk.code[start..i]
                            .iter()
                            .all(|ins| ins.opcode == Opcode::CallArg as u8);
                        if !all_callargs {
                            i += 1;
                            continue;
                        }
                        let regs: Vec<u8> =
                            chunk.code[start..i].iter().map(|ins| ins.ops[0]).collect();
                        (start, regs)
                    };

                    // Merge callee's constant pool into caller's, recording index offset.
                    let const_base = chunk.constants.len() as u16;
                    for entry in &callee.constants {
                        chunk.constants.push(entry.clone());
                    }

                    // Remap and inline.
                    let base = chunk.reg_count;
                    let remap = |r: u8| base.wrapping_add(r);

                    let mut inlined: Vec<crate::bytecode::instruction::Instruction> = Vec::new();

                    // Copy args into callee's param registers (base+0, base+1, ...).
                    for (k, &arg) in arg_regs.iter().enumerate() {
                        let param_reg = remap(k as u8);
                        if param_reg != arg {
                            inlined.push(rrr(Opcode::Mov, param_reg, arg, 0));
                        }
                    }

                    // Remap callee body, drop final Ret.
                    let body: &[_] = if callee
                        .code
                        .last()
                        .map(|x| x.opcode == Opcode::Ret as u8)
                        .unwrap_or(false)
                    {
                        &callee.code[..callee.code.len() - 1]
                    } else {
                        &callee.code[..]
                    };

                    for &ins in body {
                        let mut r = ins;
                        remap_instr_regs(&mut r, remap);
                        // Remap MovConst constant pool index.
                        if r.opcode == Opcode::MovConst as u8 {
                            let old_idx = u16::from_le_bytes([r.ops[1], r.ops[2]]);
                            let new_idx = old_idx.wrapping_add(const_base);
                            let bytes = new_idx.to_le_bytes();
                            r.ops[1] = bytes[0];
                            r.ops[2] = bytes[1];
                        }
                        // For variadic intrinsics, update flags to the actual call-site
                        // arg count (flags was fixed at declaration param_count).
                        if r.opcode == Opcode::Intrinsic as u8 && callee.variadic {
                            r.flags = arg_regs.len() as u8;
                        }
                        inlined.push(r);
                    }

                    // Move return value (base+0 = remapped r0) to dst.
                    let ret_reg = remap(0);
                    if ret_reg != dst {
                        inlined.push(rrr(Opcode::Mov, dst, ret_reg, 0));
                    }

                    // Replace the CallArg* + CallIdx range.
                    let old_len = i - call_start + 1;
                    let new_len = inlined.len();
                    chunk.code.splice(call_start..=i, inlined);

                    // After the splice, any absolute jump targets that pointed past the
                    // replaced range must be adjusted by the size delta.  Targets inside
                    // the old range (the callarg/callidx instructions themselves) should
                    // never be jump destinations, so we leave them as-is.
                    let delta = new_len as isize - old_len as isize;
                    if delta != 0 {
                        let splice_end = call_start + old_len;
                        for instr in chunk.code.iter_mut() {
                            let is_jump = matches!(
                                instr.opcode,
                                x if x == Opcode::Jmp as u8
                                    || x == Opcode::Je as u8
                                    || x == Opcode::Jne as u8
                                    || x == Opcode::Jg as u8
                                    || x == Opcode::Jge as u8
                                    || x == Opcode::Jl as u8
                                    || x == Opcode::Jle as u8
                                    || x == Opcode::Ja as u8
                                    || x == Opcode::Jb as u8
                                    || x == Opcode::Jz as u8
                                    || x == Opcode::Jnz as u8
                            );
                            if is_jump {
                                let target =
                                    u16::from_le_bytes([instr.ops[1], instr.ops[2]]) as isize;
                                if target >= splice_end as isize {
                                    let new_target =
                                        (target + delta).clamp(0, u16::MAX as isize) as u16;
                                    let [lo, hi] = new_target.to_le_bytes();
                                    instr.ops[1] = lo;
                                    instr.ops[2] = hi;
                                }
                            }
                        }
                    }

                    // For variadic callees the actual slot count is arg_regs.len()
                    // (may exceed callee.reg_count which was fixed at declaration time).
                    let needed = callee.reg_count.max(arg_regs.len() as u8);
                    chunk.reg_count = base.wrapping_add(needed);
                    // Adjust i: we removed (arg_count + 1) instrs, restart from call_start.
                    i = call_start;
                }
            }
        }

        // Variadic intrinsic chunks were fully inlined at every call site above.
        // Replace their bodies with a single Ret so the encoder emits a tiny stub
        // (~20 bytes) instead of the full expanded implementation (~300+ bytes).
        for chunk in &mut chunks {
            if chunk.variadic && chunk.intrinsic {
                chunk.code = vec![rrr(Opcode::Ret, 0, 0, 0)];
                chunk.constants.clear();
                chunk.reg_count = 0;
            }
        }

        chunks
    }

    fn compile_fn(
        &self,
        name: &str,
        params: &[crate::parser::ast::Param],
        body: Option<&Block>,
        attributes: &[crate::parser::ast::Attribute],
        output_chunks: &mut Vec<Chunk>,
        next_closure_idx: &mut u16,
    ) -> Option<Chunk> {
        self.compile_fn_with_subst(name, params, body, attributes, output_chunks, next_closure_idx, HashMap::new())
    }

    fn compile_fn_with_subst(
        &self,
        name: &str,
        params: &[crate::parser::ast::Param],
        body: Option<&Block>,
        attributes: &[crate::parser::ast::Attribute],
        output_chunks: &mut Vec<Chunk>,
        next_closure_idx: &mut u16,
        type_subst: HashMap<String, TypeKind>,
    ) -> Option<Chunk> {
        // @intrinsic: emit a platform-neutral Intrinsic instruction.
        if let Some(attr) = attributes.iter().find(|a| a.name == "intrinsic") {
            return Some(self.compile_intrinsic_fn(name, params, attr));
        }

        // @syscall: emit a single Syscall instruction instead of the function body.
        if let Some(attr) = attributes.iter().find(|a| a.name == "syscall") {
            return Some(self.compile_syscall_fn(name, params, attr));
        }

        // @api: emit a single CallExt instruction instead of the function body.
        if let Some(attr) = attributes.iter().find(|a| a.name == "api") {
            return Some(self.compile_api_fn(name, params, attr));
        }

        // Bodyless declaration — no code to emit; linker must resolve calls.
        let body = body?;

        // Variadic param needs 2 registers: ptr (the param name) + len (__len_<name>).
        let has_variadic = params.last().map(|p| p.variadic).unwrap_or(false);
        let effective_param_count = params.len() + if has_variadic { 1 } else { 0 };
        let mut fc = FnCompiler::new(
            name,
            effective_param_count,
            &self.fn_index,
            &self.const_map,
            &self.type_map,
            &self.import_names,
            &self.report.struct_defs,
            &self.report.struct_sizes,
            &self.report.struct_field_offsets,
            &self.report.trait_impls,
            &self.variadic_fn_info,
            &self.report.enum_defs,
            &self.format_fns,
            &self.variadic_intrinsic_fns,
            &self.report.monomorphizations,
            &self.report.trait_method_slots,
            output_chunks,
            next_closure_idx,
            type_subst,
        );
        for p in params {
            if p.variadic {
                fc.bind(p.name.clone());
                fc.bind(format!("__len_{}", p.name));
            } else {
                fc.bind(p.name.clone());
            }
        }
        fc.compile_block(body);
        // Guarantee every path ends with Ret.
        if fc.chunk.code.last().map(|i| i.opcode) != Some(Opcode::Ret as u8) {
            fc.chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
        }
        fc.chunk.reg_count = fc.next_reg;
        Some(fc.chunk)
    }

    fn compile_syscall_fn(
        &self,
        name: &str,
        params: &[crate::parser::ast::Param],
        attr: &crate::parser::ast::Attribute,
    ) -> Chunk {
        use crate::parser::ast::{AttrArg, AttrVal};
        let mut chunk = Chunk::with_params(name, params.len());
        // Store name or raw number in const pool — arch-neutral VBC.
        let entry = match attr.args.first() {
            Some(AttrArg::Positional(AttrVal::Int(n))) => ConstPoolEntry::Int(*n),
            Some(AttrArg::Positional(AttrVal::Str(s))) => ConstPoolEntry::Str(s.clone()),
            _ => ConstPoolEntry::Str(String::new()),
        };
        let idx = chunk.add_constant(entry);
        let arg_count = params.len() as u8;
        let mut instr = ri16(Opcode::Syscall, 0, idx);
        instr.flags = arg_count;
        chunk.emit(instr);
        chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
        chunk
    }

    fn compile_intrinsic_fn(
        &self,
        name: &str,
        params: &[crate::parser::ast::Param],
        attr: &crate::parser::ast::Attribute,
    ) -> Chunk {
        let mut chunk = Chunk::with_params(name, params.len());
        let instr_name = attr.args.first()
            .and_then(|a| match a {
                crate::parser::ast::AttrArg::Positional(crate::parser::ast::AttrVal::Str(s)) => {
                    Some(s.as_str())
                }
                _ => None,
            })
            .unwrap_or("");
        // Intrinsics with dedicated opcodes (not routed through Intrinsic case_id).
        {
            static INTRINSIC_OPCODE_MAP: LazyLock<HashMap<&'static str, Opcode>> = LazyLock::new(|| {
                let mut m = HashMap::new();
                m.insert("void.array.store", Opcode::ArrayStore);
                m.insert("void.array.load", Opcode::ArrayLoad);
                m
            });
            if let Some(&op) = INTRINSIC_OPCODE_MAP.get(instr_name) {
                // RRR: ops[0]=val/dst, ops[1]=base_ptr, ops[2]=idx
                chunk.emit(rrr(op, 0, 0, 0));
                chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
                chunk.reg_count = params.len() as u8;
                return chunk;
            }
        }
        let id = intrinsic_id(attr);
        let arg_count = params.len() as u8;
        let mut instr = ri16(Opcode::Intrinsic, 0, id);
        instr.flags = arg_count;
        chunk.emit(instr);
        chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
        chunk.intrinsic = true;
        chunk.reg_count = arg_count; // ensure frame covers all param slots
        chunk.variadic = params.last().map(|p| p.variadic).unwrap_or(false);
        chunk
    }

    fn compile_api_fn(
        &self,
        name: &str,
        params: &[crate::parser::ast::Param],
        attr: &crate::parser::ast::Attribute,
    ) -> Chunk {
        let mut chunk = Chunk::with_params(name, params.len());
        let symbol = api_symbol(attr);
        let sym_idx = chunk.add_constant(ConstPoolEntry::Str(symbol));
        // Params are in r0..r(n-1) by calling convention.
        // Emit CallExt: dst=r0, sym_idx, flags=arg_count.
        let arg_count = params.len() as u8;
        let mut instr = ri16(Opcode::CallExt, 0, sym_idx);
        instr.flags = arg_count;
        chunk.emit(instr);
        chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
        chunk
    }
}

// ── Per-function compiler ─────────────────────────────────────────────────────

struct FnCompiler<'a> {
    chunk: Chunk,
    regs: HashMap<String, u8>,
    next_reg: u8,
    fn_index: &'a HashMap<String, u16>,
    const_map: &'a HashMap<(usize, usize), ConstValue>,
    type_map: &'a HashMap<(usize, usize), TypeKind>,
    import_names: &'a HashSet<String>,
    struct_defs: &'a HashMap<String, Vec<(String, TypeKind)>>,
    struct_sizes: &'a HashMap<String, usize>,
    struct_field_offsets: &'a HashMap<String, Vec<(String, usize)>>,
    trait_impls: &'a HashMap<String, std::collections::HashSet<String>>,
    /// Maps variadic function name → number of fixed (non-variadic) params.
    variadic_fn_info: &'a HashMap<String, usize>,
    /// Enum variant tags: enum name → variant name → discriminant.
    enum_defs: &'a HashMap<String, HashMap<String, usize>>,
    /// Functions/methods with @format: pre-format args at call sites.
    format_fns: &'a HashSet<String>,
    /// Variadic @intrinsic functions: coerce args and call directly.
    variadic_intrinsic_fns: &'a HashSet<String>,
    /// Monomorphization info: used to resolve mangled names for generic calls.
    monomorphizations: &'a [crate::semantic::MonomorphizationInfo],
    /// Vtable method slot order per trait: trait name → ordered method names.
    trait_method_slots: &'a HashMap<String, Vec<String>>,
    /// Output chunks accumulator — closure chunks are pushed here.
    output_chunks: &'a mut Vec<Chunk>,
    /// Counter for generating unique closure names.
    next_closure_idx: &'a mut u16,
    /// Tracks which local variable registers hold closure environment struct pointers.
    /// Used at call sites to dispatch through the env-wrapper convention.
    closure_env_regs: HashSet<u8>,
    /// Type substitution map for monomorphized functions: generic param name → concrete type.
    type_subst: HashMap<String, TypeKind>,
}

impl<'a> FnCompiler<'a> {
    fn new(
        name: &str,
        param_count: usize,
        fn_index: &'a HashMap<String, u16>,
        const_map: &'a HashMap<(usize, usize), ConstValue>,
        type_map: &'a HashMap<(usize, usize), TypeKind>,
        import_names: &'a HashSet<String>,
        struct_defs: &'a HashMap<String, Vec<(String, TypeKind)>>,
        struct_sizes: &'a HashMap<String, usize>,
        struct_field_offsets: &'a HashMap<String, Vec<(String, usize)>>,
        trait_impls: &'a HashMap<String, std::collections::HashSet<String>>,
        variadic_fn_info: &'a HashMap<String, usize>,
        enum_defs: &'a HashMap<String, HashMap<String, usize>>,
        format_fns: &'a HashSet<String>,
        variadic_intrinsic_fns: &'a HashSet<String>,
        monomorphizations: &'a [crate::semantic::MonomorphizationInfo],
        trait_method_slots: &'a HashMap<String, Vec<String>>,
        output_chunks: &'a mut Vec<Chunk>,
        next_closure_idx: &'a mut u16,
        type_subst: HashMap<String, TypeKind>,
    ) -> Self {
        Self {
            chunk: Chunk::with_params(name, param_count),
            regs: HashMap::new(),
            next_reg: 0,
            fn_index,
            const_map,
            type_map,
            import_names,
            struct_defs,
            struct_sizes,
            struct_field_offsets,
            trait_impls,
            variadic_fn_info,
            enum_defs,
            format_fns,
            variadic_intrinsic_fns,
            monomorphizations,
            trait_method_slots,
            output_chunks,
            next_closure_idx,
            closure_env_regs: HashSet::new(),
            type_subst,
        }
    }

    /// Resolve a type through the monomorphization substitution map.
    /// Generic param references `T` → concrete type; all others pass through.
    fn resolve_type(&self, ty: &TypeKind) -> TypeKind {
        match ty {
            TypeKind::Named { name, type_args } if type_args.is_empty() => {
                self.type_subst.get(name).cloned().unwrap_or_else(|| ty.clone())
            }
            TypeKind::Named { name, type_args } => {
                let resolved_args: Vec<Type> = type_args.iter()
                    .map(|a| Spanned::new(self.resolve_type(&a.node), a.span))
                    .collect();
                TypeKind::Named { name: name.clone(), type_args: resolved_args }
            }
            TypeKind::Ref { inner } => TypeKind::Ref {
                inner: Box::new(Spanned::new(self.resolve_type(&inner.node), inner.span)),
            },
            TypeKind::RawPtr { inner } => TypeKind::RawPtr {
                inner: Box::new(Spanned::new(self.resolve_type(&inner.node), inner.span)),
            },
            TypeKind::Array { elem_ty, len } => TypeKind::Array {
                elem_ty: Box::new(Spanned::new(self.resolve_type(&elem_ty.node), elem_ty.span)),
                len: len.clone(),
            },
            TypeKind::Slice { elem_ty } => TypeKind::Slice {
                elem_ty: Box::new(Spanned::new(self.resolve_type(&elem_ty.node), elem_ty.span)),
            },
            _ => ty.clone(),
        }
    }

    /// Look up the type for a span from the type_map, resolving generic params through
    /// the monomorphization substitution map.
    fn type_of_span(&self, key: (usize, usize)) -> Option<TypeKind> {
        self.type_map.get(&key).map(|ty| self.resolve_type(ty))
    }

    /// Look up the monomorphized name for a generic function call.
    fn resolve_monomorphized_name(&self, fn_name: &str, type_args: &[Type]) -> Option<String> {
        let type_kinds: Vec<TypeKind> = type_args.iter().map(|t| t.node.clone()).collect();
        self.monomorphizations.iter()
            .find(|m| m.fn_name == fn_name && types_equal_slice(&m.type_args, &type_kinds))
            .map(|m| m.mangled_name.clone())
    }

    fn enum_ctor_tag(&self, name: &str) -> Option<usize> {
        for variants in self.enum_defs.values() {
            if let Some(&tag) = variants.get(name) {
                return Some(tag);
            }
        }
        None
    }

    fn variant_tag(&self, enum_name: Option<&str>, variant: &str) -> usize {
        if let Some(ename) = enum_name {
            if let Some(variants) = self.enum_defs.get(ename) {
                if let Some(&tag) = variants.get(variant) {
                    return tag;
                }
            }
        }
        for variants in self.enum_defs.values() {
            if let Some(&tag) = variants.get(variant) {
                return tag;
            }
        }
        0
    }

    fn field_offset_by_name(&self, struct_name: &str, field_name: &str) -> u8 {
        if let Some(offsets) = self.struct_field_offsets.get(struct_name) {
            for (fname, offset) in offsets {
                if fname == field_name {
                    return *offset as u8;
                }
            }
        }
        0
    }

    /// Scan the body of an `ExprKind::Closure` for identifiers that reference
    /// outer-scope local variables. Returns the deduplicated list of capture names.
    fn capture_ident_names(&self, body: &Expr, closure_params: &[String]) -> Vec<String> {
        let mut names = Vec::new();
        collect_idents(body, &mut names);
        names.sort();
        names.dedup();
        names.retain(|n| self.regs.contains_key(n) && !closure_params.contains(n));
        names
    }

    fn field_offset(&self, object: &Expr, field_name: &str) -> u8 {
        let key = (object.span.start, object.span.end);
        if let Some(TypeKind::Named { name: struct_name, .. }) = self.type_of_span(key) {
            if let Some(offsets) = self.struct_field_offsets.get(&struct_name) {
                for (fname, offset) in offsets {
                    if fname == field_name {
                        return *offset as u8;
                    }
                }
            }
        }
        0
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

    /// Emit an indirect call through a register, dispatching via env-wrapper convention
    /// when `callee_reg` is a closure environment struct pointer.
    fn emit_indirect_call(&mut self, dst: u8, callee_reg: u8, arg_regs: &[u8]) {
        if self.closure_env_regs.contains(&callee_reg) {
            // Closure with captures: callee_reg points to env struct {fn_ptr, captures...}.
            // Load fn_ptr from env[0] and pass env ptr as hidden first arg.
            let fn_ptr_reg = self.alloc_reg();
            self.chunk.emit(rrr(Opcode::FieldLoad, fn_ptr_reg, callee_reg, ENUM_DISCRIM_OFFSET));
            self.chunk.emit(rrr(Opcode::CallArg, callee_reg, 0, 0));
            for &r in arg_regs {
                self.chunk.emit(rrr(Opcode::CallArg, r, 0, 0));
            }
            self.chunk.emit(rrr(Opcode::CallReg, dst, fn_ptr_reg, 0));
        } else {
            for &r in arg_regs {
                self.chunk.emit(rrr(Opcode::CallArg, r, 0, 0));
            }
            self.chunk.emit(rrr(Opcode::CallReg, dst, callee_reg, 0));
        }
    }

    // ── Block / statement ──

    fn compile_block(&mut self, block: &Block) -> bool {
        for stmt in &block.stmts {
            if self.compile_stmt(stmt) {
                return true;
            }
        }
        false
    }

    /// Returns true if the statement guarantees exit (return).
    fn compile_stmt(&mut self, stmt: &Stmt) -> bool {
        match &stmt.node {
            StmtKind::Var { name, value, .. } => {
                if let Some(expr) = value {
                    let src = self.compile_expr(expr);
                    self.regs.insert(name.clone(), src);
                } else {
                    self.bind(name.clone());
                }
                false
            }
            StmtKind::Const { name, value, .. } => {
                let src = self.compile_expr(value);
                self.regs.insert(name.clone(), src);
                false
            }
            StmtKind::Return(expr) => {
                if let Some(expr) = expr {
                    let src = self.compile_expr(expr);
                    // Convention: return value in r0.
                    if src != 0 {
                        self.chunk.emit(rrr(Opcode::Mov, 0, src, 0));
                    }
                } else {
                    // void return — zero r0 so the exit code is 0.
                    self.chunk.emit(ri16(Opcode::MovI, 0, 0));
                }
                self.chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
                true
            }
            StmtKind::ExprStmt(expr) => {
                self.compile_expr(expr);
                false
            }
            StmtKind::CfgBlock { body, condition } => {
                if cfg_condition_matches(condition) {
                    self.compile_block(body)
                } else {
                    false
                }
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                // Constant-condition elimination: skip the dead branch entirely.
                let cond_key = (condition.span.start, condition.span.end);
                if let Some(ConstValue::Bool(b)) = self.const_map.get(&cond_key).cloned() {
                    return if b {
                        self.compile_block(then_block)
                    } else if let Some(eb) = else_block {
                        self.compile_block(eb)
                    } else {
                        false
                    };
                }

                // Emit condition + jump-if-false past the then block.
                let jump_else = self.compile_condition_jump(condition, true);
                let then_returns = self.compile_block(then_block);

                if let Some(else_block) = else_block {
                    let jump_end = self.chunk.emit(ri16(Opcode::Jmp, 0, 0));
                    self.chunk.patch_jump(jump_else, self.chunk.len() as u16);
                    let else_returns = self.compile_block(else_block);
                    self.chunk.patch_jump(jump_end, self.chunk.len() as u16);
                    then_returns && else_returns
                } else {
                    self.chunk.patch_jump(jump_else, self.chunk.len() as u16);
                    false
                }
            }
            StmtKind::For { kind, body } => {
                match kind {
                    ForLoop::Cond { condition: None } => {
                        // Infinite loop.
                        let loop_top = self.chunk.len() as u16;
                        self.compile_block(body);
                        self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                    }
                    ForLoop::Cond {
                        condition: Some(condition),
                    } => {
                        // While-like loop.
                        let cond_key = (condition.span.start, condition.span.end);
                        if let Some(ConstValue::Bool(false)) =
                            self.const_map.get(&cond_key).cloned()
                        {
                            return false;
                        }
                        let loop_top = self.chunk.len() as u16;
                        let jump_exit = self.compile_condition_jump(condition, true);
                        self.compile_block(body);
                        self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                        self.chunk.patch_jump(jump_exit, self.chunk.len() as u16);
                    }
                    ForLoop::CStyle {
                        init,
                        condition,
                        update,
                    } => {
                        if let Some(init_stmt) = init {
                            self.compile_stmt(init_stmt);
                        }
                        let loop_top = self.chunk.len() as u16;
                        let jump_exit = if let Some(cond) = condition {
                            Some(self.compile_condition_jump(cond, true))
                        } else {
                            None
                        };
                        self.compile_block(body);
                        if let Some(upd) = update {
                            self.compile_expr(upd);
                        }
                        self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                        if let Some(je) = jump_exit {
                            self.chunk.patch_jump(je, self.chunk.len() as u16);
                        }
                    }
                    ForLoop::Each { vars, iter } => match iter {
                        ForIter::Range { start, end } => {
                            let loop_var = vars.first().map(|s| s.as_str()).unwrap_or("_");
                            let r_i = self.bind(loop_var.to_string());
                            let r_start = self.compile_expr(start);
                            if r_start != r_i {
                                self.chunk.emit(rrr(Opcode::Mov, r_i, r_start, 0));
                            }
                            let r_end = self.compile_expr(end);
                            let loop_top = self.chunk.len() as u16;
                            self.chunk.emit(rrr(Opcode::Cmp, 0, r_i, r_end));
                            let jump_exit = self.chunk.emit(ri16(Opcode::Jge, 0, 0));
                            self.compile_block(body);
                            self.chunk.emit(rrr(Opcode::Inc, r_i, r_i, 0));
                            self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                            self.chunk.patch_jump(jump_exit, self.chunk.len() as u16);
                        }
                        ForIter::Iter(expr) => {
                            let iter_key = (expr.span.start, expr.span.end);
                            if matches!(
                                self.type_of_span(iter_key),
                                Some(TypeKind::Slice { .. })
                            ) {
                                // Slice iteration: ptr register + __len_<name> register.
                                let ptr = self.compile_expr(expr);
                                let len_reg = if let ExprKind::Ident(vname) = &expr.node {
                                    self.regs
                                        .get(&format!("__len_{}", vname))
                                        .copied()
                                        .unwrap_or(ptr + 1)
                                } else {
                                    ptr + 1
                                };
                                // Bind loop variables: (idx) or (val) or (idx, val).
                                let (r_counter, r_val_opt) = match vars.as_slice() {
                                    [] => (self.alloc_reg(), None),
                                    [v] => (self.alloc_reg(), Some(self.bind(v.clone()))),
                                    [i, v, ..] => {
                                        let ri = self.bind(i.clone());
                                        let rv = self.bind(v.clone());
                                        (ri, Some(rv))
                                    }
                                };
                                self.chunk.emit(ri16(Opcode::MovI, r_counter, 0));
                                let loop_top = self.chunk.len() as u16;
                                self.chunk.emit(rrr(Opcode::Cmp, 0, r_counter, len_reg));
                                let jump_exit = self.chunk.emit(ri16(Opcode::Jge, 0, 0));
                                // Load element at ptr - counter*8.
                                if let Some(r_val) = r_val_opt {
                                    let eight = self.alloc_reg();
                                    self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                                    let offset = self.alloc_reg();
                                    self.chunk.emit(rrr(Opcode::Mul, offset, r_counter, eight));
                                    let addr = self.alloc_reg();
                                    self.chunk.emit(rrr(Opcode::Sub, addr, ptr, offset));
                                    self.chunk.emit(mem_load(addr, r_val, 0));
                                }
                                self.compile_block(body);
                                self.chunk.emit(rrr(Opcode::Inc, r_counter, r_counter, 0));
                                self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                                self.chunk.patch_jump(jump_exit, self.chunk.len() as u16);
                            } else {
                                // Non-slice iterator (placeholder — not yet fully implemented).
                                let r_iter = self.compile_expr(expr);
                                for var in vars.iter() {
                                    self.bind(var.clone());
                                }
                                let loop_top = self.chunk.len() as u16;
                                let jump_exit = self.chunk.emit(ri16(Opcode::Jz, r_iter, 0));
                                self.compile_block(body);
                                self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
                                self.chunk.patch_jump(jump_exit, self.chunk.len() as u16);
                            }
                        }
                    },
                }
                false
            }
            StmtKind::UnsafeBlock { body } => {
                self.compile_block(body);
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
            ExprKind::Binary { left, op, right } if is_comparison(op) => {
                let r1 = self.compile_expr(left);
                let r2 = self.compile_expr(right);
                self.chunk.emit(rrr(Opcode::Cmp, 0, r1, r2));
                let jop = if jump_if_false {
                    negate_cmp(op)
                } else {
                    direct_cmp(op)
                };
                self.chunk.emit(ri16(jop, 0, 0))
            }
            ExprKind::Group(inner) => self.compile_condition_jump(inner, jump_if_false),
            _ => {
                let r = self.compile_expr(expr);
                let jop = if jump_if_false {
                    Opcode::Jz
                } else {
                    Opcode::Jnz
                };
                // ri16 layout: ops[0]=register, ops[1..2]=target (patched later)
                self.chunk.emit(ri16(jop, r, 0))
            }
        }
    }

    fn emit_call_by_name(&mut self, name: &str, arg_regs: &[u8], dst: u8) {
        if let Some(&idx) = self.fn_index.get(name) {
            for &r in arg_regs {
                self.chunk.emit(rrr(Opcode::CallArg, r, 0, 0));
            }
            // ops[0] = dst reg for return value; ops[1..2] = fn index
            self.chunk.emit(ri16(Opcode::CallIdx, dst, idx));
        }
    }

    fn is_module_import_receiver(&self, object: &Expr) -> bool {
        let Some((base, _)) = extract_field_chain(object) else {
            return false;
        };
        if self.regs.contains_key(&base) {
            return false;
        }
        self.import_names.contains(&base)
    }

    // ── Expression ───────────────────────────────────────────────────────────

    /// Uses PrimToStr with a type tag in ops[2]: 0=int, 1=float, 2=bool.
    /// For str/any types returns reg unchanged.
    fn coerce_to_display_str(&mut self, reg: u8, span: crate::parser::ast::Span) -> u8 {
        let key = (span.start, span.end);
        let type_tag: Option<u8> = match self.type_of_span(key) {
            Some(
                TypeKind::Int8
                | TypeKind::Int16
                | TypeKind::Int32
                | TypeKind::Int64
                | TypeKind::Uint8
                | TypeKind::Uint16
                | TypeKind::Uint32
                | TypeKind::Uint64
                | TypeKind::Isize
                | TypeKind::Usize,
            ) => Some(0), // int
            Some(TypeKind::Float32 | TypeKind::Float64) => Some(1), // float
            Some(TypeKind::Bool) => Some(2), // bool
            Some(TypeKind::Str) | Some(TypeKind::Ref { .. }) => None, // str/&str — already a pointer
            // Named/Ptr/Slice/etc.: pass as-is (will print address; caller should format explicitly)
            Some(TypeKind::Named { .. } | TypeKind::RawPtr { .. } | TypeKind::Slice { .. }) => None,
            // Any (unresolved generic T from unwrap/ok/etc.) defaults to int to avoid
            // treating a raw integer register as a string pointer in the format engine.
            Some(TypeKind::Any) | None | Some(_) => Some(0),
        };
        if let Some(tag) = type_tag {
            let dst = self.alloc_reg();
            let instr = rrr(Opcode::PrimToStr, dst, reg, tag);
            self.chunk.emit(instr);
            dst
        } else {
            reg
        }
    }

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

            ExprKind::Ident(name) => {
                // Zero-arg enum variant used as a value (e.g. `None`).
                // Only if not bound as a local variable.
                if !self.regs.contains_key(name.as_str()) {
                    if let Some(tag) = self.enum_ctor_tag(name) {
                        let dst = self.alloc_reg();
                        let ptr = self.alloc_reg();
                        self.chunk.emit(ri16(Opcode::New, ptr, enum_variant_alloc_size(0)));
                        let tag_reg = self.alloc_reg();
                        self.chunk.emit(ri16(Opcode::MovI, tag_reg, tag as u16));
                        self.chunk.emit(rrr(Opcode::FieldStore, tag_reg, ptr, ENUM_DISCRIM_OFFSET));
                        self.chunk.emit(rrr(Opcode::Mov, dst, ptr, 0));
                        return dst;
                    }
                    // Function name used as a value → load its address.
                    if self.fn_index.contains_key(name.as_str()) {
                        let dst = self.alloc_reg();
                        let idx = self.chunk.add_constant(ConstPoolEntry::FnAddr(name.clone()));
                        self.chunk.emit(ri16(Opcode::MovConst, dst, idx));
                        return dst;
                    }
                }
                self.reg_of(name)
            }

            ExprKind::Group(inner) => self.compile_expr(inner),

            ExprKind::Unary { op, expr: inner } => match op {
                UnaryOpKind::Ref => {
                    let src = self.compile_expr(inner);
                    let dst = self.alloc_reg();
                    self.chunk.emit(mem_lea(src, dst, 0));
                    dst
                }
                UnaryOpKind::Deref => {
                    let ptr = self.compile_expr(inner);
                    let dst = self.alloc_reg();
                    self.chunk.emit(mem_load(ptr, dst, 0));
                    dst
                }
                UnaryOpKind::Neg => {
                    let src = self.compile_expr(inner);
                    let dst = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Neg, dst, src, 0));
                    dst
                }
                UnaryOpKind::Not => {
                    let src = self.compile_expr(inner);
                    let dst = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Not, dst, src, 0));
                    dst
                }
            },

            // Short-circuit logical ops — lazy right evaluation.
            ExprKind::Binary {
                left,
                op: BinOpKind::AndAnd,
                right,
            } => {
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
            ExprKind::Binary {
                left,
                op: BinOpKind::OrOr,
                right,
            } => {
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
                    BinOpKind::Add => {
                        self.chunk.emit(rrr(Opcode::Add, dst, r1, r2));
                    }
                    BinOpKind::Sub => {
                        self.chunk.emit(rrr(Opcode::Sub, dst, r1, r2));
                    }
                    BinOpKind::Mul => {
                        self.chunk.emit(rrr(Opcode::Mul, dst, r1, r2));
                    }
                    BinOpKind::Div => {
                        self.chunk.emit(rrr(Opcode::Div, dst, r1, r2));
                    }
                    BinOpKind::Mod => {
                        self.chunk.emit(rrr(Opcode::Mod, dst, r1, r2));
                    }
                    // Comparisons: materialize bool result into dst.
                    _ if is_comparison(op) => {
                        self.chunk.emit(rrr(Opcode::Cmp, 0, r1, r2));
                        self.chunk.emit(ri16(Opcode::MovI, dst, 1));
                        let skip = self.chunk.emit(ri16(direct_cmp(op), 0, 0));
                        self.chunk.emit(ri16(Opcode::MovI, dst, 0));
                        self.chunk.patch_jump(skip, self.chunk.len() as u16);
                    }
                    BinOpKind::Pow => {
                        self.chunk.emit(rrr(Opcode::Pow, dst, r1, r2));
                    }
                    BinOpKind::AndAnd | BinOpKind::OrOr => unreachable!(),
                    _ => {
                        self.chunk.emit(rrr(Opcode::Add, dst, r1, r2));
                    } // fallback
                }
                dst
            }

            ExprKind::Assign { target, value } => {
                let src = self.compile_expr(value);
                match &target.node {
                    ExprKind::Ident(name) => {
                        let dst = self.reg_of(name);
                        if dst != src {
                            self.chunk.emit(rrr(Opcode::Mov, dst, src, 0));
                        }
                        dst
                    }
                    ExprKind::Unary {
                        op: UnaryOpKind::Deref,
                        expr: ptr_expr,
                    } => {
                        let ptr = self.compile_expr(ptr_expr);
                        self.chunk.emit(mem_store(ptr, src, 0));
                        src
                    }
                    ExprKind::Field {
                        object,
                        name: field_name,
                    } => {
                        let byte_offset = self.field_offset(object, field_name);
                        let obj = self.compile_expr(object);
                        self.chunk.emit(rrr(Opcode::FieldStore, src, obj, byte_offset));
                        src
                    }
                    _ => src,
                }
            }
            ExprKind::StructInit { name, fields } => {
                // Get field order from struct_defs
                let field_order: Vec<String> = if let Some(defs) = self.struct_defs.get(name) {
                    defs.iter().map(|(fn_, _)| fn_.clone()).collect()
                } else {
                    fields.iter().map(|(fn_, _)| fn_.clone()).collect()
                };
                let struct_size = self.struct_sizes.get(name).copied().unwrap_or_else(|| field_order.len() * 8);
                let dst = self.alloc_reg();
                self.chunk.emit(ri16(Opcode::New, dst, struct_size as u16));

                // Compile and store each field in declaration order using computed offsets
                for field_name in &field_order {
                    if let Some((_, fval)) = fields.iter().find(|(fn_, _)| fn_ == field_name) {
                        let val = self.compile_expr(fval);
                        let off = self.field_offset_by_name(name, field_name);
                        self.chunk.emit(rrr(Opcode::FieldStore, val, dst, off));
                    }
                }
                dst
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

            ExprKind::IncDec {
                expr: inner, op, ..
            } => {
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

            ExprKind::Call { callee, type_args, args, .. } => {
                let dst = self.alloc_reg();
                if let ExprKind::Ident(name) = &callee.node {
                    // Monomorphized generic function: resolve to mangled name.
                    if !type_args.is_empty() {
                        if let Some(mono_name) = self.resolve_monomorphized_name(name, type_args) {
                            let arg_regs: Vec<u8> =
                                args.iter().map(|a| self.compile_expr(a)).collect();
                            self.emit_call_by_name(&mono_name, &arg_regs, dst);
                            return dst;
                        }
                    }
                    // @format dispatch: pre-format args at call sites.
                    if self.format_fns.contains(name.as_str()) && args.len() > 1 {
                        let template_reg = self.compile_expr(&args[0]);
                        let mut coerced_var: Vec<u8> = Vec::new();
                        for arg in &args[1..] {
                            let reg = self.compile_expr(arg);
                            coerced_var.push(self.coerce_to_display_str(reg, arg.span));
                        }
                        // format(template, ..args: str) — variadic: pack var args into slice.
                        let fmt_dst = self.alloc_reg();
                        let (r_ptr, r_len) = if coerced_var.is_empty() {
                            let rp = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, rp, 0));
                            let rl = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, rl, 0));
                            (rp, rl)
                        } else {
                            let first_slot = self.next_reg;
                            for &r in &coerced_var {
                                let slot = self.alloc_reg();
                                if r != slot {
                                    self.chunk.emit(rrr(Opcode::Mov, slot, r, 0));
                                }
                            }
                            let rp = self.alloc_reg();
                            self.chunk.emit(mem_lea(first_slot, rp, 0));
                            let rl = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, rl, coerced_var.len() as u16));
                            (rp, rl)
                        };
                        self.emit_call_by_name("format", &[template_reg, r_ptr, r_len], fmt_dst);
                        // Call the actual @format function with the single formatted string.
                        let mut call_args = vec![fmt_dst];
                        if self.variadic_fn_info.contains_key(name.as_str()) {
                            let rp = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, rp, 0));
                            let rl = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, rl, 0));
                            call_args.push(rp);
                            call_args.push(rl);
                        }
                        self.emit_call_by_name(name, &call_args, dst);
                        return dst;
                    }
                    if let Some(&fixed_count) = self.variadic_fn_info.get(name.as_str()) {
                        // Variadic call: compile fixed args, pack variadic args into
                        // consecutive stack slots, pass (ptr, len) as hidden trailing args.
                        let fixed_regs: Vec<u8> = args[..fixed_count.min(args.len())]
                            .iter()
                            .map(|a| self.compile_expr(a))
                            .collect();
                        let var_args = &args[fixed_count.min(args.len())..];
                        let (r_ptr, r_len) = if var_args.is_empty() {
                            // Zero variadic args: pass null ptr + count 0.
                            let rp = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, rp, 0));
                            let rl = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, rl, 0));
                            (rp, rl)
                        } else {
                            // Compile each variadic arg (may land in non-consecutive regs).
                            let var_regs: Vec<u8> =
                                var_args.iter().map(|a| self.compile_expr(a)).collect();
                            // Copy into fresh consecutive slots so Lea gives a contiguous block.
                            let first_slot = self.next_reg;
                            for (i, &r) in var_regs.iter().enumerate() {
                                let slot = self.alloc_reg(); // = first_slot + i
                                if r != slot {
                                    self.chunk.emit(rrr(Opcode::Mov, slot, r, 0));
                                }
                                let _ = i; // suppress unused warning
                            }
                            // Lea first_slot → pointer to its stack slot on this frame.
                            let rp = self.alloc_reg();
                            self.chunk.emit(mem_lea(first_slot, rp, 0));
                            let rl = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, rl, var_regs.len() as u16));
                            (rp, rl)
                        };
                        let mut all_regs = fixed_regs;
                        all_regs.push(r_ptr);
                        all_regs.push(r_len);
                        self.emit_call_by_name(name, &all_regs, dst);
                    } else if let Some(tag) = self.enum_ctor_tag(name) {
                        // Enum variant constructor: allocate heap struct, store discriminant + payloads.
                        let payload_regs: Vec<u8> =
                            args.iter().map(|a| self.compile_expr(a)).collect();
                        let ptr = self.alloc_reg();
                        self.chunk.emit(ri16(Opcode::New, ptr, enum_variant_alloc_size(payload_regs.len())));
                        let tag_reg = self.alloc_reg();
                        self.chunk.emit(ri16(Opcode::MovI, tag_reg, tag as u16));
                        self.chunk.emit(rrr(Opcode::FieldStore, tag_reg, ptr, ENUM_DISCRIM_OFFSET));
                        for (i, &payload) in payload_regs.iter().enumerate() {
                            let off = ENUM_PAYLOAD_OFFSET + (i as u8 * 8);
                            self.chunk.emit(rrr(Opcode::FieldStore, payload, ptr, off));
                        }
                        if dst != ptr {
                            self.chunk.emit(rrr(Opcode::Mov, dst, ptr, 0));
                        }
                        return dst;
                    } else if self.regs.contains_key(name.as_str()) {
                        // Local variable — fn pointer or closure env pointer.
                        let fn_reg = self.compile_expr(callee);
                        let arg_regs: Vec<u8> =
                            args.iter().map(|a| self.compile_expr(a)).collect();
                        self.emit_indirect_call(dst, fn_reg, &arg_regs);
                    } else {
                        let arg_regs: Vec<u8> =
                            args.iter().map(|a| self.compile_expr(a)).collect();
                        self.emit_call_by_name(name, &arg_regs, dst);
                    }
                } else {
                    // Indirect call: callee is an expression (variable, closure, etc.)
                    let fn_reg = self.compile_expr(callee);
                    let arg_regs: Vec<u8> =
                        args.iter().map(|a| self.compile_expr(a)).collect();
                    self.emit_indirect_call(dst, fn_reg, &arg_regs);
                }
                dst
            }

            ExprKind::MethodCall {
                object,
                method,
                type_args,
                args,
            } => {
                if self.is_module_import_receiver(object) {
                    let dst = self.alloc_reg();
                    let is_fmt_fn = self.format_fns.contains(method.as_str());
                    let is_variadic_intrinsic = self.variadic_intrinsic_fns.contains(method.as_str());
                    if (is_fmt_fn || is_variadic_intrinsic) && args.len() > 1 {
                        let template_reg = self.compile_expr(&args[0]);
                        let mut coerced = vec![template_reg];
                        for arg in &args[1..] {
                            let reg = self.compile_expr(arg);
                            let coerced_reg = self.coerce_to_display_str(reg, arg.span);
                            coerced.push(coerced_reg);
                        }
                        if is_variadic_intrinsic {
                            // Variadic intrinsic (e.g. format): call directly with all coerced args.
                            self.emit_call_by_name(method, &coerced, dst);
                        } else {
                            // @format fn: pre-format, then call with single formatted string.
                            let fmt_dst = self.alloc_reg();
                            self.emit_call_by_name("format", &coerced, fmt_dst);
                            self.emit_call_by_name(method, &[fmt_dst], dst);
                        }
                    } else {
                        let arg_regs: Vec<u8> = args.iter().map(|a| self.compile_expr(a)).collect();
                        self.emit_call_by_name(method, &arg_regs, dst);
                    }
                    return dst;
                }

                // Static type-namespace call: `String.from(...)`, `Box.new(...)`, etc.
                // Object is a known struct name used as a namespace, not a value instance.
                if let ExprKind::Ident(type_name) = &object.node {
                    let is_type_ns = self.struct_defs.contains_key(type_name.as_str())
                        && !self.regs.contains_key(type_name.as_str());
                    if is_type_ns {
                        let mangled = format!("{}.{}", type_name, method);
                        if self.fn_index.contains_key(&mangled) {
                            let arg_regs: Vec<u8> =
                                args.iter().map(|a| self.compile_expr(a)).collect();
                            let dst = self.alloc_reg();
                            self.emit_call_by_name(&mangled, &arg_regs, dst);
                            return dst;
                        }
                    }
                }

                let obj = self.compile_expr(object);
                let key = (object.span.start, object.span.end);

                // Resolve the receiver type through monomorphization substitution,
                // so generic param `T` resolves to the concrete type (e.g., Int32).
                let receiver_ty = self.type_of_span(key);

                // Static dispatch: Named type with a known impl method takes priority
                // over built-in method dispatch so that user impls can override any name.
                if let Some(TypeKind::Named { name: type_name, .. }) = receiver_ty {
                    // Special case: Type.from([e0, e1, ...]) → Type.new() + Type.push each element
                    if method == "from" {
                        if let Some(arg) = args.first() {
                            if let ExprKind::ArrayLit(elems) = &arg.node {
                                let elems = elems.clone();
                                let dst = self.alloc_reg();
                                self.emit_call_by_name(&format!("{}.new", type_name), &[obj], dst);
                                for elem in &elems {
                                    let val = self.compile_expr(elem);
                                    let new_dst = self.alloc_reg();
                                    self.emit_call_by_name(
                                        &format!("{}.push", type_name),
                                        &[dst, val],
                                        new_dst,
                                    );
                                    self.chunk.emit(rrr(Opcode::Mov, dst, new_dst, 0));
                                }
                                return dst;
                            }
                        }
                    }

                    // @format instance methods: pre-format args, call method with single string.
                    {
                        let mangled_check = format!("{}.{}", type_name, method);
                        if self.format_fns.contains(&mangled_check) && !args.is_empty() {
                            let template_reg = self.compile_expr(&args[0]);
                            let fmt_reg = if args.len() > 1 {
                                let mut coerced = vec![template_reg];
                                for arg in &args[1..] {
                                    let r = self.compile_expr(arg);
                                    let cr = self.coerce_to_display_str(r, arg.span);
                                    coerced.push(cr);
                                }
                                let fd = self.alloc_reg();
                                self.emit_call_by_name("format", &coerced, fd);
                                fd
                            } else {
                                template_reg
                            };
                            let dst = self.alloc_reg();
                            self.emit_call_by_name(&mangled_check, &[obj, fmt_reg], dst);
                            return dst;
                        }
                    }

                    let mangled = format!("{}.{}", type_name, method);
                    if self.fn_index.contains_key(&mangled) {
                        let arg_regs: Vec<u8> =
                            args.iter().map(|a| self.compile_expr(a)).collect();
                        let dst = self.alloc_reg();
                        let mut all_args = vec![obj];
                        all_args.extend_from_slice(&arg_regs);
                        self.emit_call_by_name(&mangled, &all_args, dst);
                        return dst;
                    }
                }

                // Built-in methods for primitive and known types.
                let resolved_receiver = self.type_of_span(key);
                if let Some(pm) = resolve_primitive_method(
                    method, args, resolved_receiver.as_ref(),
                ) {
                    match pm {
                        PrimitiveMethod::Len => {
                            // Slice: .len() returns the hidden __len register.
                            if matches!(self.type_map.get(&key), Some(TypeKind::Slice { .. })) {
                                let len_reg = if let ExprKind::Ident(vname) = &object.node {
                                    self.regs
                                        .get(&format!("__len_{}", vname))
                                        .copied()
                                        .unwrap_or(obj + 1)
                                } else {
                                    obj + 1
                                };
                                let dst = self.alloc_reg();
                                self.chunk.emit(rrr(Opcode::Mov, dst, len_reg, 0));
                                return dst;
                            }
                            // str / &str
                            let dst = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::StrLen, dst, obj, 0));
                            return dst;
                        }
                        PrimitiveMethod::PrimToStr { tag } => {
                            let dst = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::PrimToStr, dst, obj, tag));
                            return dst;
                        }
                        PrimitiveMethod::StrToString => {
                            let dst = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::StrAsStr, dst, obj, 0));
                            return dst;
                        }
                        PrimitiveMethod::BoolToString => {
                            let dst = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::PrimToStr, dst, obj, 2));
                            return dst;
                        }
                        PrimitiveMethod::PrimToString { intrinsic_id } => {
                            let dst = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::Mov, dst, obj, 0));
                            let mut instr =
                                ri16(Opcode::Intrinsic, dst, intrinsic_id);
                            instr.flags = 1;
                            self.chunk.emit(instr);
                            return dst;
                        }
                        PrimitiveMethod::AsStr => {
                            let dst = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::StrAsStr, dst, obj, 0));
                            return dst;
                        }
                        PrimitiveMethod::Parse { is_float } => {
                            let dst = self.alloc_reg();
                            let op = if is_float || matches!(
                                type_args.first().map(|t| &t.node),
                                Some(TypeKind::Float32 | TypeKind::Float64)
                            ) {
                                Opcode::StrToFloat
                            } else {
                                Opcode::StrToInt
                            };
                            self.chunk.emit(rrr(op, dst, obj, 0));
                            return dst;
                        }
                    }
                }
                // General vtable dispatch (fallback for dynamic/polymorphic calls).
                // Determine method slot from trait_method_slots.
                let method_slot: u8 = self.type_of_span(key)
                    .and_then(|ty| {
                        let type_name = match &ty {
                            TypeKind::Named { name, .. } => Some(name.clone()),
                            _ => None,
                        }?;
                        // Find a trait implemented by this type that defines the method.
                        let traits = self.trait_impls.get(type_name.as_str())?;
                        for trait_name in traits {
                            if let Some(slots) = self.trait_method_slots.get(trait_name) {
                                if let Some(idx) = slots.iter().position(|m| m == method) {
                                    return Some(idx as u8);
                                }
                            }
                        }
                        None
                    })
                    .unwrap_or(0);
                let arg_regs: Vec<u8> = args.iter().map(|a| self.compile_expr(a)).collect();
                for &r in &arg_regs {
                    self.chunk.emit(rrr(Opcode::CallArg, r, 0, 0));
                }
                let vtbl = self.alloc_reg();
                let dst = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::VtblLoad, vtbl, obj, method_slot));
                self.chunk.emit(rrr(Opcode::CallReg, dst, vtbl, 0));
                dst
            }

            ExprKind::Field { object, name } => {
                let byte_offset = self.field_offset(object, name);
                let obj = self.compile_expr(object);
                let dst = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::FieldLoad, dst, obj, byte_offset));
                dst
            }

            ExprKind::ArrayLit(elems) => {
                let n = elems.len() as u8;
                let base = self.next_reg;
                self.next_reg = self.next_reg.wrapping_add(n);
                for (i, elem) in elems.iter().enumerate() {
                    let val = self.compile_expr(elem);
                    let dst = base + i as u8;
                    if val != dst {
                        self.chunk.emit(rrr(Opcode::Mov, dst, val, 0));
                    }
                }
                base
            }

            ExprKind::Index { object, indices } => {
                // Named type that implements the Index trait → dispatch to Type.index.
                // Checks trait_impls registry — no accidental dispatch from any "index" method.
                let key = (object.span.start, object.span.end);
                if let Some(TypeKind::Named { name: type_name, .. }) =
                    self.type_of_span(key)
                {
                    let implements_index = self
                        .trait_impls
                        .get(type_name.as_str())
                        .map(|ts| ts.contains("Index"))
                        .unwrap_or(false);
                    if implements_index {
                        let mangled = format!("{}.index", type_name);
                        if self.fn_index.contains_key(&mangled) {
                            let obj = self.compile_expr(object);
                            let idx_regs: Vec<u8> =
                                indices.iter().map(|i| self.compile_expr(i)).collect();
                            let dst = self.alloc_reg();
                            let mut all_args = vec![obj];
                            all_args.extend_from_slice(&idx_regs);
                            self.emit_call_by_name(&mangled, &all_args, dst);
                            return dst;
                        }
                    }
                }
                let index = indices.first().expect("index expr must have at least one index");
                let obj_key = (object.span.start, object.span.end);
                // Slice (variadic param): ptr register holds caller's stack address.
                if matches!(self.type_of_span(obj_key), Some(TypeKind::Slice { .. })) {
                    let ptr = self.compile_expr(object);
                    let idx = self.compile_expr(index);
                    let eight = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                    let offset = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Mul, offset, idx, eight));
                    let addr = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::Sub, addr, ptr, offset));
                    let dst = self.alloc_reg();
                    self.chunk.emit(mem_load(addr, dst, 0));
                    return dst;
                }
                // Fallback: raw static-array register arithmetic (single index only).
                let base = self.compile_expr(object);
                if let ExprKind::Literal(Literal::Int(n)) = &index.node {
                    if *n >= 0 {
                        return base + *n as u8;
                    }
                }
                // Dynamic index: Lea + scale + Sub + Load
                let idx = self.compile_expr(index);
                let ptr = self.alloc_reg();
                self.chunk.emit(mem_lea(base, ptr, 0));
                let eight = self.alloc_reg();
                self.chunk.emit(ri16(Opcode::MovI, eight, 8));
                let offset = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::Mul, offset, idx, eight));
                self.chunk.emit(rrr(Opcode::Sub, ptr, ptr, offset));
                let dst = self.alloc_reg();
                self.chunk.emit(mem_load(ptr, dst, 0));
                dst
            }

            ExprKind::Match { scrutinee, arms } => {
                let scr = self.compile_expr(scrutinee);
                let dst = self.alloc_reg();
                let mut end_jumps = Vec::new();
                // Jumps from failed guards to the next arm.
                let mut guard_fail_jumps: Vec<usize> = Vec::new();
                // Track whether the last arm was a wildcard with no guard (full coverage).
                #[allow(unused_assignments)]
                let mut _last_arm_is_full_default = false;

                for arm in arms {
                    // Patch all pending guard-fail jumps to the start of this arm
                    // (before pattern matching so the next arm gets a chance).
                    for j in &guard_fail_jumps {
                        self.chunk.patch_jump(*j, self.chunk.len() as u16);
                    }
                    guard_fail_jumps.clear();

                    // For variant arms: check discriminant and skip on mismatch.
                    if let PatternKind::Variant { enum_name, variant, bindings } = &arm.pattern.node {
                        let tag = self.variant_tag(enum_name.as_deref(), variant);
                        let disc = self.alloc_reg();
                        self.chunk.emit(rrr(Opcode::FieldLoad, disc, scr, ENUM_DISCRIM_OFFSET));
                        let tag_reg = self.alloc_reg();
                        self.chunk.emit(ri16(Opcode::MovI, tag_reg, tag as u16));
                        self.chunk.emit(rrr(Opcode::Cmp, 0, disc, tag_reg));
                        let skip = self.chunk.emit(ri16(Opcode::Jne, 0, 0));
                        // Extract bound variables from payload slots.
                        for (i, binding) in bindings.iter().enumerate() {
                            if binding != "_" {
                                let bound_reg = self.bind(binding.clone());
                                let off = ENUM_PAYLOAD_OFFSET + (i as u8 * 8);
                                self.chunk.emit(rrr(Opcode::FieldLoad, bound_reg, scr, off));
                            }
                        }
                        // If this arm has a guard: compile guard and jump to next arm on false.
                        if let Some(guard) = &arm.guard {
                            let guard_val = self.compile_expr(guard);
                            self.chunk.emit(rrr(Opcode::Cmp, 0, guard_val, 0));
                            guard_fail_jumps.push(self.chunk.emit(ri16(Opcode::Je, 0, 0)));
                        }
                        let val = self.compile_expr(&arm.expr);
                        if val != dst {
                            self.chunk.emit(rrr(Opcode::Mov, dst, val, 0));
                        }
                        end_jumps.push(self.chunk.emit(ri16(Opcode::Jmp, 0, 0)));
                        self.chunk.patch_jump(skip, self.chunk.len() as u16);
                    } else {
                        // Wildcard arm.
                        // If guard present, compile it and skip body on false.
                        if let Some(guard) = &arm.guard {
                            let guard_val = self.compile_expr(guard);
                            self.chunk.emit(rrr(Opcode::Cmp, 0, guard_val, 0));
                            guard_fail_jumps.push(self.chunk.emit(ri16(Opcode::Je, 0, 0)));
                            // Note: after the guard-fail jump, the arm body still executes if guard passes.
                        }
                        let val = self.compile_expr(&arm.expr);
                        if val != dst {
                            self.chunk.emit(rrr(Opcode::Mov, dst, val, 0));
                        }
                        // If no guard, this is a full default — no need to jump to next arm.
                        if arm.guard.is_none() {
                            _last_arm_is_full_default = true;
                        }
                        end_jumps.push(self.chunk.emit(ri16(Opcode::Jmp, 0, 0)));
                    }
                }
                // If the last arm had a guard that failed, we fall through here.
                // Since there are no more arms, patch remaining guard-fail jumps to the end.
                let end = self.chunk.len() as u16;
                for j in guard_fail_jumps {
                    self.chunk.patch_jump(j, end);
                }
                for j in end_jumps {
                    self.chunk.patch_jump(j, end);
                }
                dst
            }

            ExprKind::Try { expr: inner } => {
                let scr = self.compile_expr(inner);
                let key = (inner.span.start, inner.span.end);
                let enum_name = match self.type_of_span(key) {
                    Some(TypeKind::Named { name, .. }) if name == "Option" => "Option",
                    _ => "Result",
                };
                let (success_variant, failure_variant) = match enum_name {
                    "Option" => ("Some", "None"),
                    _ => ("Ok", "Err"),
                };
                let success_tag = *self.enum_defs.get(enum_name)
                    .and_then(|v| v.get(success_variant))
                    .expect("? operator: success variant not found in enum_defs");
                let failure_tag = *self.enum_defs.get(enum_name)
                    .and_then(|v| v.get(failure_variant))
                    .expect("? operator: failure variant not found in enum_defs");

                // Load discriminant and compare against success tag.
                let disc = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::FieldLoad, disc, scr, ENUM_DISCRIM_OFFSET));
                let tag_ok = self.alloc_reg();
                self.chunk.emit(ri16(Opcode::MovI, tag_ok, success_tag as u16));
                self.chunk.emit(rrr(Opcode::Cmp, 0, disc, tag_ok));
                let jne = self.chunk.emit(ri16(Opcode::Jne, 0, 0));

                // Success path: extract first payload.
                let val = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::FieldLoad, val, scr, ENUM_PAYLOAD_OFFSET));
                let jmp_end = self.chunk.emit(ri16(Opcode::Jmp, 0, 0));

                // Failure path: build failure variant and early-return.
                self.chunk.patch_jump(jne, self.chunk.len() as u16);
                if enum_name == "Option" {
                    // Return None — zero-arg variant.
                    let none_ptr = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::New, none_ptr, enum_variant_alloc_size(0)));
                    let tag_fail = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::MovI, tag_fail, failure_tag as u16));
                    self.chunk.emit(rrr(Opcode::FieldStore, tag_fail, none_ptr, ENUM_DISCRIM_OFFSET));
                    if none_ptr != 0 {
                        self.chunk.emit(rrr(Opcode::Mov, 0, none_ptr, 0));
                    }
                } else {
                    // Return Err(payload) — copy scrutinee payload into new Err object.
                    let err_payload = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::FieldLoad, err_payload, scr, ENUM_PAYLOAD_OFFSET));
                    let err_ptr = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::New, err_ptr, enum_variant_alloc_size(1)));
                    let tag_fail = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::MovI, tag_fail, failure_tag as u16));
                    self.chunk.emit(rrr(Opcode::FieldStore, tag_fail, err_ptr, ENUM_DISCRIM_OFFSET));
                    self.chunk.emit(rrr(Opcode::FieldStore, err_payload, err_ptr, ENUM_PAYLOAD_OFFSET));
                    if err_ptr != 0 {
                        self.chunk.emit(rrr(Opcode::Mov, 0, err_ptr, 0));
                    }
                }
                self.chunk.emit(rrr(Opcode::Ret, 0, 0, 0));

                self.chunk.patch_jump(jmp_end, self.chunk.len() as u16);
                val
            }

            ExprKind::Closure { params, body } => {
                let val = *self.next_closure_idx;
                *self.next_closure_idx = val.wrapping_add(1);
                let anon_name = format!("__void_closure_{}", val);

                let captures = self.capture_ident_names(body, params);

                // Use a temporary output buffer for the sub-compiler
                // so we avoid borrowing conflicts with self.output_chunks.
                let mut temp_chunks = Vec::new();
                let mut temp_idx = 0u16;
                let anon_param_count = if captures.is_empty() {
                    params.len()
                } else {
                    params.len() + 1 // hidden env ptr as first param
                };
                let mut anon = FnCompiler::new(
                    &anon_name,
                    anon_param_count,
                    self.fn_index,
                    self.const_map,
                    self.type_map,
                    self.import_names,
                    self.struct_defs,
                    self.struct_sizes,
                    self.struct_field_offsets,
                    self.trait_impls,
                    self.variadic_fn_info,
                    self.enum_defs,
                    self.format_fns,
                    self.variadic_intrinsic_fns,
                    self.monomorphizations,
                    self.trait_method_slots,
                    &mut temp_chunks,
                    &mut temp_idx,
                    self.type_subst.clone(),
                );
                if captures.is_empty() {
                    // No-capture: params mapped as-is (r0, r1, ...)
                    for p in params {
                        anon.bind(p.clone());
                    }
                } else {
                    // With captures: r0 = env ptr, user params start at r1.
                    // Load each captured variable from the env struct.
                    for (i, cap_name) in captures.iter().enumerate() {
                        let cap_reg = anon.alloc_reg();
                        let off = ENUM_PAYLOAD_OFFSET + (i as u8 * 8);
                        anon.chunk.emit(rrr(Opcode::FieldLoad, cap_reg, 0, off));
                        anon.regs.insert(cap_name.clone(), cap_reg);
                    }
                    // Bind user params starting at r1.
                    for (i, p) in params.iter().enumerate() {
                        anon.regs.insert(p.clone(), (i + 1) as u8);
                    }
                }
                let body_reg = anon.compile_expr(body);
                if body_reg != 0 {
                    anon.chunk.emit(rrr(Opcode::Mov, 0, body_reg, 0));
                }
                anon.chunk.emit(rrr(Opcode::Ret, 0, 0, 0));
                anon.chunk.reg_count = anon.next_reg;

                // Push the closure chunk and any nested closures into
                // the parent's output queue.
                self.output_chunks.push(anon.chunk);
                self.output_chunks.extend(temp_chunks);

                if captures.is_empty() {
                    // No-capture: just load the function address (existing behavior).
                    let dst = self.alloc_reg();
                    let cidx = self.chunk.add_constant(ConstPoolEntry::FnAddr(anon_name));
                    self.chunk.emit(ri16(Opcode::MovConst, dst, cidx));
                    dst
                } else {
                    // With captures: allocate an environment struct, store fn ptr + captures.
                    let env_ptr = self.alloc_reg();
                    let env_size = ((captures.len() + 1) * 8).max(16) as u16;
                    self.chunk.emit(ri16(Opcode::New, env_ptr, env_size));

                    let fn_addr_reg = self.alloc_reg();
                    let cidx = self.chunk.add_constant(ConstPoolEntry::FnAddr(anon_name));
                    self.chunk.emit(ri16(Opcode::MovConst, fn_addr_reg, cidx));
                    self.chunk.emit(rrr(Opcode::FieldStore, fn_addr_reg, env_ptr, ENUM_DISCRIM_OFFSET));

                    for (i, cap_name) in captures.iter().enumerate() {
                        let cap_reg = self.reg_of(cap_name);
                        let off = ENUM_PAYLOAD_OFFSET + (i as u8 * 8);
                        self.chunk.emit(rrr(Opcode::FieldStore, cap_reg, env_ptr, off));
                    }

                    self.closure_env_regs.insert(env_ptr);
                    env_ptr
                }
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

fn remap_instr_regs(
    instr: &mut crate::bytecode::instruction::Instruction,
    remap: impl Fn(u8) -> u8,
) {
    use crate::bytecode::opcode::Opcode;
    let Some(op) = Opcode::from_u8(instr.opcode) else {
        return;
    };
    match op {
        // No-op / no regs
        Opcode::Nop
        | Opcode::Ret
        | Opcode::MemFence
        | Opcode::Jmp
        | Opcode::Je
        | Opcode::Jne
        | Opcode::Jg
        | Opcode::Jge
        | Opcode::Jl
        | Opcode::Jle
        | Opcode::Ja
        | Opcode::Jb => {}

        // RI16 — ops[0]=dst only; ops[1..2] are an immediate (id/index), not registers
        Opcode::MovI
        | Opcode::MovConst
        | Opcode::CallIdx
        | Opcode::CallExt
        | Opcode::Syscall
        | Opcode::Intrinsic
        | Opcode::New
        | Opcode::NewObj => {
            instr.ops[0] = remap(instr.ops[0]);
        }

        // Jz/Jnz: ops[0] may be a register (or 0 for flag-only)
        Opcode::Jz | Opcode::Jnz => {
            if instr.ops[0] != 0 {
                instr.ops[0] = remap(instr.ops[0]);
            }
        }

        // CallArg / Drop — single reg
        Opcode::CallArg | Opcode::Drop => {
            instr.ops[0] = remap(instr.ops[0]);
        }

        // MEM — ops[0]=val/dst, ops[1]=base
        Opcode::Load | Opcode::Store | Opcode::Lea => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
        }

        // FieldLoad/FieldStore — ops[2] is a byte offset, NOT a register.
        Opcode::FieldLoad | Opcode::FieldStore => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
            // ops[2] = byte_off — leave unchanged
        }

        // VtblLoad — ops[2] is a method slot index, NOT a register.
        Opcode::VtblLoad => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
            // ops[2] = method_slot — leave unchanged
        }

        // PrimToStr — ops[2] is a type tag, NOT a register.
        Opcode::PrimToStr => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
            // ops[2] = type tag — leave unchanged
        }

        // StrLen, StrToInt, StrToFloat, StrAsStr, StrConcat — ops[2] unused or not a reg.
        Opcode::StrLen | Opcode::StrToInt | Opcode::StrToFloat | Opcode::StrAsStr => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
            // ops[2] unused — leave unchanged
        }

        // Pow — RRR
        Opcode::Pow => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
            instr.ops[2] = remap(instr.ops[2]);
        }

        // RRR and all others — remap ops[0], ops[1], ops[2]
        _ => {
            instr.ops[0] = remap(instr.ops[0]);
            instr.ops[1] = remap(instr.ops[1]);
            instr.ops[2] = remap(instr.ops[2]);
        }
    }
}

/// Describes a resolved primitive method operation for codegen emission.
enum PrimitiveMethod {
    /// .len() on &str or slice — emit StrLen or copy hidden __len register
    Len,
    /// .to_str() on primitives — emit PrimToStr with the given type tag
    PrimToStr { tag: u8 },
    /// .to_string() on &str — identity view via StrAsStr
    StrToString,
    /// .to_string() on bool — PrimToStr with tag=2
    BoolToString,
    /// .to_string() on int/float — Intrinsic with given id
    PrimToString { intrinsic_id: u16 },
    /// .as_str() / .as_string() — StrAsStr identity view
    AsStr,
    /// .parse[T]() — true=float, false=int
    Parse { is_float: bool },
}

/// Maps receiver types to their `PrimToStr` tag value.
fn prim_to_str_tag(receiver_type: Option<&TypeKind>) -> u8 {
    match receiver_type {
        Some(TypeKind::Float32 | TypeKind::Float64) => 1,
        Some(TypeKind::Bool) => 2,
        _ => 0,
    }
}

/// Maps receiver types to the `PrimToString` intrinsic ID.
fn prim_to_string_intrinsic_id(receiver_type: Option<&TypeKind>) -> u16 {
    match receiver_type {
        Some(TypeKind::Float32 | TypeKind::Float64) => 16,
        _ => 15,
    }
}

/// Resolve a method call on a primitive/built-in receiver to a `PrimitiveMethod` operation.
/// Returns `None` if the method is not a known built-in (falls through to vtable dispatch).
fn resolve_primitive_method(
    method: &str,
    args: &[Expr],
    receiver_ty: Option<&TypeKind>,
) -> Option<PrimitiveMethod> {
    match method {
        "len" if args.is_empty() => Some(PrimitiveMethod::Len),
        "to_str" if args.is_empty() => {
            Some(PrimitiveMethod::PrimToStr { tag: prim_to_str_tag(receiver_ty) })
        }
        "to_string" if args.is_empty() => {
            match receiver_ty {
                Some(TypeKind::Str) | Some(TypeKind::Ref { .. }) | None => {
                    Some(PrimitiveMethod::StrToString)
                }
                Some(TypeKind::Bool) => Some(PrimitiveMethod::BoolToString),
                _ => Some(PrimitiveMethod::PrimToString {
                    intrinsic_id: prim_to_string_intrinsic_id(receiver_ty),
                }),
            }
        }
        "as_string" | "as_str" if args.is_empty() => Some(PrimitiveMethod::AsStr),
        "parse" => Some(PrimitiveMethod::Parse { is_float: false }),
        _ => None,
    }
}

fn extract_field_chain(expr: &Expr) -> Option<(String, Vec<String>)> {
    match &expr.node {
        ExprKind::Ident(name) => Some((name.clone(), vec![])),
        ExprKind::Field { object, name } => {
            let (base, mut path) = extract_field_chain(object)?;
            path.push(name.clone());
            Some((base, path))
        }
        _ => None,
    }
}

/// Maps `@intrinsic("void.X")` attribute strings to case IDs for the `Intrinsic` opcode.
fn intrinsic_id(attr: &crate::parser::ast::Attribute) -> u16 {
    static INTRINSIC_MAP: LazyLock<HashMap<&'static str, u16>> = LazyLock::new(|| {
        let mut m = HashMap::new();
        m.insert("void.write", 0);
        m.insert("void.read", 1);
        m.insert("void.exit", 2);
        m.insert("void.malloc", 3);
        m.insert("void.free", 4);
        m.insert("void.realloc", 5);
        m.insert("void.memcpy", 6);
        m.insert("void.memset", 7);
        m.insert("void.memmove", 8);
        m.insert("void.memcmp", 9);
        m.insert("void.strlen", 10);
        m.insert("void.stderr_write", 11);
        m.insert("void.sleep_ms", 12);
        m.insert("void.getenv", 13);
        m.insert("void.str_concat", 14);
        m.insert("void.int_to_str", 15);
        m.insert("void.float_to_str", 16);
        // Threading: malloc+pthread_create/CreateThread; pthread_join+free/WaitForSingleObject
        m.insert("void.thread.spawn", 18);
        m.insert("void.thread.join", 19);
        // Net: only the ops that need sockaddr_in construction (accept uses 0 directly now)
        m.insert("void.net.bind_tcp", 20);
        m.insert("void.net.connect_tcp", 21);
        // String primitives needed to implement format in void
        m.insert("void.str.byte_at", 23);
        m.insert("void.str.from_byte", 24);
        m
    });
    let name = attr
        .args
        .first()
        .and_then(|a| match a {
            crate::parser::ast::AttrArg::Positional(crate::parser::ast::AttrVal::Str(s)) => {
                Some(s.as_str())
            }
            _ => None,
        })
        .unwrap_or("");
    INTRINSIC_MAP.get(name).copied().unwrap_or(0)
}

fn api_symbol(attr: &crate::parser::ast::Attribute) -> String {
    attr.args
        .first()
        .and_then(|a| match a {
            crate::parser::ast::AttrArg::Positional(crate::parser::ast::AttrVal::Str(s)) => {
                Some(s.clone())
            }
            _ => None,
        })
        .unwrap_or_default()
}

fn cfg_condition_matches(attr: &crate::parser::ast::Attribute) -> bool {
    use crate::parser::ast::{AttrArg, AttrVal};
    for arg in &attr.args {
        match arg {
            AttrArg::KeyValue(key, AttrVal::Str(val)) if key == "target_os" => {
                return val.as_str() == std::env::consts::OS;
            }
            AttrArg::KeyValue(key, AttrVal::Str(val)) if key == "target_arch" => {
                return val.as_str() == std::env::consts::ARCH;
            }
            AttrArg::KeyValue(key, AttrVal::Str(val)) if key == "target_abi" => {
                #[cfg(target_os = "windows")]
                let host_abi = "win64";
                #[cfg(not(target_os = "windows"))]
                let host_abi = "sysv";
                return val.as_str() == host_abi;
            }
            _ => {}
        }
    }
    true // unknown condition — include unconditionally
}

/// Check whether an item's @cfg attributes (if any) evaluate to true on this host.
fn item_cfg_active(attributes: &[crate::parser::ast::Attribute]) -> bool {
    for attr in attributes {
        if attr.name == "cfg" {
            if !cfg_condition_matches(attr) {
                return false;
            }
        }
    }
    true
}

/// Recursively collect all `ExprKind::Ident` names from an expression tree.
fn collect_idents(expr: &Expr, names: &mut Vec<String>) {
    match &expr.node {
        ExprKind::Ident(name) => names.push(name.clone()),
        ExprKind::Literal(_) => {}
        ExprKind::Group(inner) => collect_idents(inner, names),
        ExprKind::Unary { expr: inner, .. } => collect_idents(inner, names),
        ExprKind::Binary { left, right, .. } => {
            collect_idents(left, names);
            collect_idents(right, names);
        }
        ExprKind::Assign { target, value, .. } => {
            collect_idents(target, names);
            collect_idents(value, names);
        }
        ExprKind::Call { callee, args, .. } => {
            collect_idents(callee, names);
            for a in args { collect_idents(a, names); }
        }
        ExprKind::Field { object, .. } => collect_idents(object, names),
        ExprKind::StructInit { fields, .. } => {
            for (_, f) in fields { collect_idents(f, names); }
        }
        ExprKind::MethodCall { object, args, .. } => {
            collect_idents(object, names);
            for a in args { collect_idents(a, names); }
        }
        ExprKind::Match { scrutinee, arms } => {
            collect_idents(scrutinee, names);
            for arm in arms {
                collect_idents(&arm.expr, names);
                if let Some(g) = &arm.guard { collect_idents(g, names); }
            }
        }
        ExprKind::CompoundAssign { target, value, .. } => {
            collect_idents(target, names);
            collect_idents(value, names);
        }
        ExprKind::IncDec { expr: inner, .. } => collect_idents(inner, names),
        ExprKind::ArrayLit(elems) => {
            for e in elems { collect_idents(e, names); }
        }
        ExprKind::Index { object, indices } => {
            collect_idents(object, names);
            for i in indices { collect_idents(i, names); }
        }
        ExprKind::Try { expr: inner } => collect_idents(inner, names),
        ExprKind::Closure { body, .. } => collect_idents(body, names),
    }
}

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
        BinOpKind::Lt => Opcode::Jl,
        BinOpKind::LtEq => Opcode::Jle,
        BinOpKind::Gt => Opcode::Jg,
        BinOpKind::GtEq => Opcode::Jge,
        BinOpKind::EqEq => Opcode::Je,
        BinOpKind::NotEq => Opcode::Jne,
        _ => Opcode::Jnz,
    }
}

/// Conditional jump opcode that fires when the comparison is FALSE (negated).
fn negate_cmp(op: &BinOpKind) -> Opcode {
    match op {
        BinOpKind::Lt => Opcode::Jge,
        BinOpKind::LtEq => Opcode::Jg,
        BinOpKind::Gt => Opcode::Jle,
        BinOpKind::GtEq => Opcode::Jl,
        BinOpKind::EqEq => Opcode::Jne,
        BinOpKind::NotEq => Opcode::Je,
        _ => Opcode::Jz,
    }
}

fn type_kind_base_name(ty: &TypeKind) -> String {
    match ty {
        TypeKind::Named { name, .. } => name.clone(),
        TypeKind::Int8 => "i8".to_string(),
        TypeKind::Int16 => "i16".to_string(),
        TypeKind::Int32 => "i32".to_string(),
        TypeKind::Int64 => "i64".to_string(),
        TypeKind::Uint8 => "u8".to_string(),
        TypeKind::Uint16 => "u16".to_string(),
        TypeKind::Uint32 => "u32".to_string(),
        TypeKind::Uint64 => "u64".to_string(),
        TypeKind::Isize => "isize".to_string(),
        TypeKind::Usize => "usize".to_string(),
        TypeKind::Float16 => "f16".to_string(),
        TypeKind::Float32 => "f32".to_string(),
        TypeKind::Float64 => "f64".to_string(),
        TypeKind::Bool => "bool".to_string(),
        TypeKind::Str => "str".to_string(),
        TypeKind::Ref { inner } => type_kind_base_name(&inner.node),
        TypeKind::RawPtr { inner } => type_kind_base_name(&inner.node),
        other => format!("{}", other),
    }
}

/// Structural equality comparison for TypeKind (no PartialEq derive available).
fn types_equal(a: &TypeKind, b: &TypeKind) -> bool {
    use TypeKind::*;
    match (a, b) {
        (Int8, Int8) | (Int16, Int16) | (Int32, Int32) | (Int64, Int64) => true,
        (Uint8, Uint8) | (Uint16, Uint16) | (Uint32, Uint32) | (Uint64, Uint64) => true,
        (Isize, Isize) | (Usize, Usize) => true,
        (Float16, Float16) | (Float32, Float32) | (Float64, Float64) => true,
        (Bool, Bool) | (Str, Str) | (Void, Void) | (Any, Any) | (Never, Never) => true,
        (Named { name: n1, type_args: a1 }, Named { name: n2, type_args: a2 }) => {
            n1 == n2 && a1.len() == a2.len() && a1.iter().zip(a2).all(|(t1, t2)| types_equal(&t1.node, &t2.node))
        }
        (Ref { inner: i1 }, Ref { inner: i2 }) => types_equal(&i1.node, &i2.node),
        (RawPtr { inner: i1 }, RawPtr { inner: i2 }) => types_equal(&i1.node, &i2.node),
        (Array { elem_ty: e1, len: l1 }, Array { elem_ty: e2, len: l2 }) => {
            l1 == l2 && types_equal(&e1.node, &e2.node)
        }
        (Slice { elem_ty: e1 }, Slice { elem_ty: e2 }) => types_equal(&e1.node, &e2.node),
        _ => false,
    }
}

fn types_equal_slice(a: &[TypeKind], b: &[TypeKind]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(t1, t2)| types_equal(t1, t2))
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
        assert!(
            report.errors.is_empty(),
            "semantic errors: {:?}",
            report.errors
        );
        Codegen::new(&report).compile_program(&program)
    }

    #[test]
    fn simple_add_function_emits_add_and_ret() {
        let chunks = compile("fn add(a: i32, b: i32) i32 { ret a + b; }");
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
        let chunks = compile("fn foo() i32 { const x: i32 = 1 + 2; ret x; }");
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
            r#"fn countdown(x: i32) void {
                for x > 0 { x = x + 1; }
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
        assert_eq!(
            target, 0,
            "back-edge Jmp must target instruction 0 (loop top)"
        );
    }

    #[test]
    fn if_else_jump_targets_are_patched() {
        let chunks = compile(
            r#"fn sign(x: i32) i32 {
                if (x > 0) { ret 1; } else { ret 0; }
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
                    Opcode::Jmp
                        | Opcode::Je
                        | Opcode::Jne
                        | Opcode::Jl
                        | Opcode::Jle
                        | Opcode::Jg
                        | Opcode::Jge
                        | Opcode::Jz
                        | Opcode::Jnz
                )
            );
            if is_jump {
                let (_, target) = instr.ri16();
                assert!(
                    target <= len,
                    "jump target {} out of bounds (chunk has {} instructions)",
                    target,
                    len
                );
            }
        }
    }

    #[test]
    fn function_call_emits_call_idx() {
        // Use a function with more than 2 statements so it won't be inlined.
        let chunks = compile(
            r#"fn helper(x: i32) i32 { const a: i32 = 1; const b: i32 = 2; ret x; }
               fn main() void { helper(1); }"#,
        );
        let main_chunk = chunks
            .iter()
            .find(|c| c.name == "main")
            .expect("no main chunk");
        assert!(
            main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::CallIdx as u8),
            "expected CallIdx in main"
        );
    }

    #[test]
    fn ret_always_last_in_every_chunk() {
        let chunks = compile(
            r#"fn a() void {}
               fn b(x: i32) i32 { ret x; }"#,
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
    fn generic_function_produces_monomorphized_chunks() {
        let chunks = compile(
            r#"fn id[T](x: T) T { ret x; }
               fn main() void { id[i32](5); }"#,
        );
        // Should have chunks: id<i32> and main
        let id_i32 = "id<i32>";
        assert!(
            chunks.iter().any(|c| c.name == id_i32),
            "expected monomorphized chunk {}",
            id_i32
        );
        let main_chunk = chunks.iter().find(|c| c.name == "main");
        assert!(main_chunk.is_some(), "expected main chunk");
        // main should call the monomorphized function
        assert!(
            main_chunk.unwrap().code.iter().any(|i| i.opcode == Opcode::CallIdx as u8),
            "expected CallIdx in main"
        );
    }

    #[test]
    fn monomorphized_primitive_method_uses_concrete_type() {
        let chunks = compile(
            r#"fn show[T](x: T) void {
                var s = x.to_string();
            }
            fn main() void {
                show[i32](1);
                show[f64](2);
            }"#,
        );
        // show<i32> should use Intrinsic(15) for int to_string
        let show_i32 = chunks.iter().find(|c| c.name == "show<i32>").unwrap();
        let has_int_intrinsic = show_i32.code.iter().any(|i| {
            i.opcode == Opcode::Intrinsic as u8 && i.ri16().1 == 15
        });
        assert!(
            has_int_intrinsic,
            "show<i32> should use Intrinsic(15) for int to_string"
        );
        // show<f64> should use Intrinsic(16) for float to_string
        let show_f64 = chunks.iter().find(|c| c.name == "show<f64>").unwrap();
        let has_float_intrinsic = show_f64.code.iter().any(|i| {
            i.opcode == Opcode::Intrinsic as u8 && i.ri16().1 == 16
        });
        assert!(
            has_float_intrinsic,
            "show<f64> should use Intrinsic(16) for float to_string"
        );
    }

    #[test]
    fn compound_assign_emits_arithmetic_op_in_place() {
        let chunks = compile(
            r#"fn inc(x: i32) i32 {
                x += 1;
                ret x;
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
            r#"fn bump(x: i32) i32 {
                x++;
                ret x;
            }"#,
        );
        assert!(
            chunks[0].code.iter().any(|i| i.opcode == Opcode::Inc as u8),
            "x++ should emit Inc"
        );
    }

    #[test]
    fn large_int_goes_to_constant_pool() {
        let chunks = compile("fn big() i32 { ret 100000; }");
        assert!(
            !chunks[0].constants.is_empty(),
            "100000 should be in constant pool"
        );
        assert!(
            chunks[0]
                .code
                .iter()
                .any(|i| i.opcode == Opcode::MovConst as u8),
            "expected MovConst for large literal"
        );
    }

    #[test]
    fn string_literal_goes_to_constant_pool() {
        let chunks = compile(r#"fn greeting() str { ret "hello"; }"#);
        assert!(
            matches!(chunks[0].constants.first(), Some(ConstPoolEntry::Str(s)) if s == "hello"),
            "string literal should be in constant pool"
        );
    }

    #[test]
    fn to_bytes_produces_six_bytes_per_instruction() {
        let chunks = compile("fn f(a: i32, b: i32) i32 { ret a + b; }");
        let bytes = chunks[0].to_bytes();
        assert_eq!(bytes.len(), chunks[0].code.len() * 6);
    }

    #[test]
    fn negative_const_value_goes_to_constant_pool() {
        // const-folded 0 - 1 produces ConstValue::Int(-1) which must use MovConst not MovI
        let chunks = compile("fn neg() i32 { const x: i32 = 0 - 1; ret x; }");
        assert!(
            chunks[0]
                .code
                .iter()
                .any(|i| i.opcode == Opcode::MovConst as u8),
            "negative constant should be in constant pool"
        );
    }

    #[test]
    fn syscall_attribute_emits_syscall_opcode() {
        let chunks =
            compile(r#"@syscall("write") fn write(fd: i32, buf: str, len: usize) isize { }"#);
        assert_eq!(chunks[0].name, "write");
        assert!(
            chunks[0]
                .code
                .iter()
                .any(|i| i.opcode == Opcode::Syscall as u8),
            "expected Syscall instruction for @syscall fn"
        );
        assert_eq!(chunks[0].code.last().unwrap().opcode, Opcode::Ret as u8);
    }

    #[test]
    fn syscall_attribute_accepts_numeric_id() {
        let chunks = compile(r#"@syscall(60) fn exit(code: i32) isize { }"#);
        let instr = chunks[0]
            .code
            .iter()
            .find(|i| i.opcode == Opcode::Syscall as u8)
            .expect("expected Syscall instruction");
        // Numeric syscall id is stored in the const pool, ri16 gives the pool index.
        let (_, idx) = instr.ri16();
        assert!(
            matches!(
                chunks[0].constants.get(idx as usize),
                Some(ConstPoolEntry::Int(60))
            ),
            "expected Int(60) in const pool at index {idx}"
        );
        assert_eq!(instr.flags, 1);
    }

    #[test]
    fn api_attribute_emits_call_ext_opcode() {
        let chunks = compile(
            r#"@api("WriteFile") fn win_write(h: usize, buf: str, len: usize, out: usize, ovl: usize) usize { }"#,
        );
        let chunk = &chunks[0];
        assert!(
            chunk.code.iter().any(|i| i.opcode == Opcode::CallExt as u8),
            "expected CallExt instruction for @api fn"
        );
        assert!(
            chunk
                .constants
                .iter()
                .any(|c| matches!(c, ConstPoolEntry::Str(s) if s == "WriteFile")),
            "expected WriteFile in constant pool"
        );
    }

    #[test]
    fn str_len_emits_strlen_opcode() {
        let chunks = compile(r#"fn f(s: str) any { ret s.len(); }"#);
        assert!(
            chunks[0]
                .code
                .iter()
                .any(|i| i.opcode == Opcode::StrLen as u8),
            "s.len() should emit StrLen"
        );
    }

    #[test]
    fn method_call_emits_call_arg_and_vtbl_load() {
        let chunks = compile(
            r#"fn main() void {
                   var s: any = 0;
                   s.doSomething(1);
               }"#,
        );
        let main_chunk = chunks.iter().find(|c| c.name == "main").unwrap();
        assert!(
            main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::CallArg as u8),
            "method call with args should emit CallArg"
        );
        assert!(
            main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::VtblLoad as u8),
            "method call should emit VtblLoad"
        );
    }

    #[test]
    fn tree_shaking_omits_unreachable_function() {
        // dead_fn is called only by zombie_fn; zombie_fn is never called by main.
        // Tree-shaking should exclude both from the output chunks.
        let chunks = compile(
            r#"fn dead_fn(x: i32) i32 { const a: i32 = 1; const b: i32 = 2; ret x; }
               fn zombie_fn() void { dead_fn(1); }
               fn main() void { ret; }"#,
        );
        assert!(
            !chunks.iter().any(|c| c.name == "dead_fn"),
            "dead_fn should be tree-shaken"
        );
        assert!(
            !chunks.iter().any(|c| c.name == "zombie_fn"),
            "zombie_fn should be tree-shaken"
        );
        assert!(
            chunks.iter().any(|c| c.name == "main"),
            "main must be present"
        );
    }

    #[test]
    fn const_true_if_skips_else_branch() {
        // `1 == 1` const-folds to Bool(true) — else branch must not be compiled.
        let chunks = compile(
            r#"fn f() i32 {
                if (1 == 1) { ret 1; } else { ret 2; }
            }"#,
        );
        let code = &chunks[0].code;
        // With const-condition elimination, no conditional jump instruction.
        let has_conditional_jump = code.iter().any(|i| {
            matches!(
                Opcode::from_u8(i.opcode),
                Some(
                    Opcode::Je
                        | Opcode::Jne
                        | Opcode::Jl
                        | Opcode::Jle
                        | Opcode::Jg
                        | Opcode::Jge
                        | Opcode::Jz
                        | Opcode::Jnz
                )
            )
        });
        assert!(
            !has_conditional_jump,
            "const-true if should not emit a conditional jump"
        );
        // Dead else branch must not emit MovI(2).
        let has_movi_2 = code.iter().any(|i| {
            let (_, imm) = i.ri16();
            i.opcode == Opcode::MovI as u8 && imm == 2
        });
        assert!(
            !has_movi_2,
            "const-true if should not emit MovI(2) from dead else branch"
        );
    }

    #[test]
    fn const_false_if_skips_then_branch() {
        // `0 == 1` const-folds to Bool(false) — then branch must not be compiled.
        let chunks = compile(
            r#"fn f() i32 {
                if (0 == 1) { ret 99; } else { ret 7; }
            }"#,
        );
        let code = &chunks[0].code;
        let has_movi_99 = code.iter().any(|i| {
            let (_, imm) = i.ri16();
            i.opcode == Opcode::MovI as u8 && imm == 99
        });
        assert!(
            !has_movi_99,
            "const-false if should not emit MovI(99) from dead then branch"
        );
    }

    #[test]
    fn const_false_while_emits_no_loop_instructions() {
        // `0 == 1` const-folds to Bool(false) — while body must be skipped entirely.
        let chunks = compile(
            r#"fn f() void {
                for 0 == 1 { var x: i32 = 1; }
            }"#,
        );
        let code = &chunks[0].code;
        // No loop-back Jmp should exist.
        assert!(
            !code.iter().any(|i| i.opcode == Opcode::Jmp as u8),
            "for(0==1) should emit no Jmp"
        );
    }

    #[test]
    fn impl_method_is_compiled_with_mangled_name() {
        let chunks = compile(
            r#"struct Point { x: i32, y: i32, }
               impl Point {
                   fn get_x(self: Point) i32 { ret self.x; }
               }
               fn main() void {
                   var p: Point = Point { x: 1, y: 0 };
                   p.get_x();
                   ret;
               }"#,
        );
        assert!(
            chunks.iter().any(|c| c.name == "Point.get_x"),
            "impl method should be compiled as 'Point.get_x', got: {:?}",
            chunks.iter().map(|c| &c.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn impl_method_call_emits_call_idx_not_vtbl() {
        // Static dispatch: impl method call must NOT use VtblLoad/CallReg (dynamic dispatch).
        // After inlining the method body is expanded in-place — CallIdx may be absent — but
        // dynamic dispatch instructions must never appear for a Known Named type.
        let chunks = compile(
            r#"struct Counter { val: i32, }
               impl Counter {
                   fn get(self: Counter) i32 { ret self.val; }
               }
               fn main() void {
                   var c: Counter = Counter { val: 0 };
                   var n: i32 = c.get();
               }"#,
        );
        let main_chunk = chunks.iter().find(|c| c.name == "main").unwrap();
        assert!(
            !main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::VtblLoad as u8),
            "impl method call on Named type should NOT emit VtblLoad"
        );
        assert!(
            !main_chunk
                .code
                .iter()
                .any(|i| i.opcode == Opcode::CallReg as u8),
            "impl method call on Named type should NOT emit CallReg (dynamic dispatch)"
        );
        // Verify Counter.get was compiled as a named chunk (static identity exists)
        assert!(
            chunks.iter().any(|c| c.name == "Counter.get"),
            "Counter.get must be compiled as its own chunk"
        );
    }

    #[test]
    fn trait_impl_method_is_compiled_with_mangled_name() {
        let chunks = compile(
            r#"trait Display { fn to_str(self: Num) str; }
               struct Num { val: i32, }
               impl Display for Num {
                   fn to_str(self: Num) str { ret "num"; }
               }
               fn main() void {
                   var n: Num = Num { val: 42 };
                   n.to_str();
                   ret;
               }"#,
        );
        assert!(
            chunks.iter().any(|c| c.name == "Num.to_str"),
            "trait impl method should be compiled as 'Num.to_str'"
        );
    }

    #[test]
    fn impl_method_with_args_passes_receiver_first() {
        // Verify that an impl method with (self + explicit args) is compiled correctly.
        // The method may be inlined, but dynamic dispatch must never be used for Named types.
        // We verify: (1) no VtblLoad, (2) Acc.add has param_count == 2 (receiver + n).
        let chunks = compile(
            r#"struct Acc { sum: i32, }
               impl Acc {
                   fn add(self: Acc, n: i32) i32 { ret self.sum + n; }
               }
               fn main() void {
                   var a: Acc = Acc { sum: 0 };
                   var r: i32 = a.add(5);
               }"#,
        );
        let main_chunk = chunks.iter().find(|c| c.name == "main").unwrap();
        assert!(
            !main_chunk.code.iter().any(|i| i.opcode == Opcode::VtblLoad as u8),
            "a.add(5) on Named type should NOT use VtblLoad"
        );
        let add_chunk = chunks.iter().find(|c| c.name == "Acc.add").unwrap();
        assert_eq!(
            add_chunk.param_count, 2,
            "Acc.add must have param_count == 2 (self + n)"
        );
    }

    #[test]
    fn inherent_impl_parses_and_compiles() {
        // impl Type {} without 'for' keyword (inherent impl)
        let chunks = compile(
            r#"struct Box { val: i32, }
               impl Box {
                   fn get_val(self: Box) i32 { ret self.val; }
               }
               fn main() void {
                   var b: Box = Box { val: 1 };
                   b.get_val();
                   ret;
               }"#,
        );
        assert!(chunks.iter().any(|c| c.name == "Box.get_val"));
    }

    #[test]
    fn closure_produces_anonymous_chunk() {
        let chunks = compile(
            r#"fn main() void {
                var f = || 42;
                var x: i32 = f();
            }"#,
        );
        assert!(
            chunks.iter().any(|c| c.name == "__void_closure_0"),
            "expected anonymous closure chunk"
        );
        let closure_chunk = chunks.iter().find(|c| c.name == "__void_closure_0").unwrap();
        assert_eq!(
            closure_chunk.code.last().unwrap().opcode,
            Opcode::Ret as u8,
            "closure chunk must end with Ret"
        );
        assert_eq!(closure_chunk.param_count, 0, "|| 42 has 0 params");
        let main_chunk = chunks.iter().find(|c| c.name == "main").unwrap();
        assert!(
            main_chunk.code.iter().any(|i| i.opcode == Opcode::MovConst as u8),
            "expected MovConst for FnAddr in main"
        );
        assert!(
            main_chunk.code.iter().any(|i| i.opcode == Opcode::CallReg as u8),
            "expected CallReg for indirect call in main"
        );
    }

    #[test]
    fn closure_capture_allocates_env_struct() {
        let chunks = compile(
            r#"fn main() void {
                var a: i32 = 1;
                var f = || a;
                var r: i32 = f();
            }"#,
        );
        let closure_chunk = chunks.iter().find(|c| c.name == "__void_closure_0").unwrap();
        // With one capture, param_count = 0 user params + 1 hidden env ptr = 1
        assert_eq!(closure_chunk.param_count, 1, "closure should have hidden env ptr param");
        // The closure should load from the env struct (FieldLoad)
        assert!(
            closure_chunk.code.iter().any(|i| i.opcode == Opcode::FieldLoad as u8),
            "expected FieldLoad for capture access in closure"
        );
        let main_chunk = chunks.iter().find(|c| c.name == "main").unwrap();
        // Main should allocate the env struct
        assert!(
            main_chunk.code.iter().any(|i| i.opcode == Opcode::New as u8),
            "expected New for env struct allocation"
        );
        // Main should pass env ptr as hidden first arg (CallArg before fn_ptr load)
        assert!(
            main_chunk.code.iter().any(|i| i.opcode == Opcode::CallArg as u8),
            "expected CallArg for hidden env ptr"
        );
    }
}
>>>>>>> Stashed changes
