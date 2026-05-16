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
                        let mangled = format!("{}.{}", type_name, name);
                        if is_live(&mangled) {
                            if let Some(chunk) = self.compile_fn(
                                &mangled,
                                params,
                                body.as_ref().map(|b| b as &Block),
                                attributes,
                            ) {
                                chunks.push(chunk);
                            }
                        }
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
            &self.report.trait_impls,
            &self.variadic_fn_info,
            &self.report.enum_defs,
            &self.format_fns,
            &self.variadic_intrinsic_fns,
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
    trait_impls: &'a HashMap<String, std::collections::HashSet<String>>,
    /// Maps variadic function name → number of fixed (non-variadic) params.
    variadic_fn_info: &'a HashMap<String, usize>,
    /// Enum variant tags: enum name → variant name → discriminant.
    enum_defs: &'a HashMap<String, HashMap<String, usize>>,
    /// Functions/methods with @format: pre-format args at call sites.
    format_fns: &'a HashSet<String>,
    /// Variadic @intrinsic functions: coerce args and call directly.
    variadic_intrinsic_fns: &'a HashSet<String>,
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
        trait_impls: &'a HashMap<String, std::collections::HashSet<String>>,
        variadic_fn_info: &'a HashMap<String, usize>,
        enum_defs: &'a HashMap<String, HashMap<String, usize>>,
        format_fns: &'a HashSet<String>,
        variadic_intrinsic_fns: &'a HashSet<String>,
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
            trait_impls,
            variadic_fn_info,
            enum_defs,
            format_fns,
            variadic_intrinsic_fns,
        }
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

    fn field_offset(&self, object: &Expr, field_name: &str) -> u8 {
        let key = (object.span.start, object.span.end);
        if let Some(TypeKind::Named { name: struct_name, .. }) = self.type_map.get(&key) {
            if let Some(fields) = self.struct_defs.get(struct_name) {
                for (i, (fname, _)) in fields.iter().enumerate() {
                    if fname == field_name {
                        return (i * 8) as u8;
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
                                self.type_map.get(&iter_key),
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

    /// Auto-coerce a value in `reg` to a str representation for fmt.format.
    fn prim_to_str_tag(&self, span: crate::parser::ast::Span) -> u8 {
        let key = (span.start, span.end);
        match self.type_map.get(&key) {
            Some(TypeKind::Float32 | TypeKind::Float64) => 1,
            Some(TypeKind::Bool) => 2,
            _ => 0, // int (default) or unknown
        }
    }

    /// Uses PrimToStr with a type tag in ops[2]: 0=int, 1=float, 2=bool.
    /// For str/any types returns reg unchanged.
    fn coerce_to_display_str(&mut self, reg: u8, span: crate::parser::ast::Span) -> u8 {
        let key = (span.start, span.end);
        let type_tag: Option<u8> = match self.type_map.get(&key) {
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
                        self.chunk.emit(ri16(Opcode::New, ptr, 16));
                        let tag_reg = self.alloc_reg();
                        self.chunk.emit(ri16(Opcode::MovI, tag_reg, tag as u16));
                        self.chunk.emit(rrr(Opcode::FieldStore, tag_reg, ptr, 0));
                        self.chunk.emit(rrr(Opcode::Mov, dst, ptr, 0));
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
                let field_order: Vec<(String, usize)> = if let Some(defs) = self.struct_defs.get(name) {
                    defs.iter().enumerate().map(|(i, (fn_, _))| (fn_.clone(), i)).collect()
                } else {
                    fields.iter().enumerate().map(|(i, (fn_, _))| (fn_.clone(), i)).collect()
                };
                let n_fields = field_order.len().max(fields.len());
                let dst = self.alloc_reg();
                // Emit New(dst, n_fields * 8)
                self.chunk.emit(ri16(Opcode::New, dst, (n_fields * 8) as u16));

                // Compile and store each field in declaration order
                for (field_name, i) in &field_order {
                    if let Some((_, fval)) = fields.iter().find(|(fn_, _)| fn_ == field_name) {
                        let val = self.compile_expr(fval);
                        self.chunk.emit(rrr(Opcode::FieldStore, val, dst, (i * 8) as u8));
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

            ExprKind::Call { callee, args, .. } => {
                let dst = self.alloc_reg();
                if let ExprKind::Ident(name) = &callee.node {
                    // @format dispatch: pre-format args at call sites.
                    if self.format_fns.contains(name.as_str()) && args.len() > 1 {
                        let template_reg = self.compile_expr(&args[0]);
                        let mut coerced = vec![template_reg];
                        for arg in &args[1..] {
                            let reg = self.compile_expr(arg);
                            let cr = self.coerce_to_display_str(reg, arg.span);
                            coerced.push(cr);
                        }
                        let fmt_dst = self.alloc_reg();
                        self.emit_call_by_name("format", &coerced, fmt_dst);
                        // Call function with single formatted string.
                        // If variadic, append empty (ptr=0, len=0) hidden args.
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
                        let alloc_size = (((payload_regs.len() + 1) * 8).max(16)) as u16;
                        let ptr = self.alloc_reg();
                        self.chunk.emit(ri16(Opcode::New, ptr, alloc_size));
                        let tag_reg = self.alloc_reg();
                        self.chunk.emit(ri16(Opcode::MovI, tag_reg, tag as u16));
                        self.chunk.emit(rrr(Opcode::FieldStore, tag_reg, ptr, 0));
                        for (i, &payload) in payload_regs.iter().enumerate() {
                            let off = ((i + 1) * 8) as u8;
                            self.chunk.emit(rrr(Opcode::FieldStore, payload, ptr, off));
                        }
                        if dst != ptr {
                            self.chunk.emit(rrr(Opcode::Mov, dst, ptr, 0));
                        }
                        return dst;
                    } else {
                        let arg_regs: Vec<u8> =
                            args.iter().map(|a| self.compile_expr(a)).collect();
                        self.emit_call_by_name(name, &arg_regs, dst);
                    }
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

                // Static dispatch: Named type with a known impl method takes priority
                // over built-in method dispatch so that user impls can override any name.
                if let Some(TypeKind::Named { name: type_name, .. }) =
                    self.type_map.get(&key).cloned()
                {
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
                // Slice (.len() returns the hidden __len register).
                if method == "len" && args.is_empty() {
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
                if method == "to_str" && args.is_empty() {
                    // Borrowed &str view: static buffer, no heap allocation.
                    let dst = self.alloc_reg();
                    let tag = self.prim_to_str_tag(object.span);
                    self.chunk.emit(rrr(Opcode::PrimToStr, dst, obj, tag));
                    return dst;
                }
                if method == "to_string" && args.is_empty() {
                    // Owned String: heap-allocate via int_to_str / float_to_str intrinsics.
                    // Intrinsic reads slot(base) as its first arg and writes result back there.
                    match self.type_map.get(&key).cloned() {
                        Some(TypeKind::Str) | Some(TypeKind::Ref { .. }) | None => {
                            // str/&str → to_string() is identity
                            let dst = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::StrAsStr, dst, obj, 0));
                            return dst;
                        }
                        Some(TypeKind::Bool) => {
                            // bool → static "true"/"false" via PrimToStr tag=2
                            let dst = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::PrimToStr, dst, obj, 2));
                            return dst;
                        }
                        _ => {
                            // int / float → heap malloc via intrinsic id=15/16
                            let intrinsic_id: u16 = match self.type_map.get(&key) {
                                Some(TypeKind::Float32 | TypeKind::Float64) => 16,
                                _ => 15,
                            };
                            // Move value into a fresh reg; Intrinsic uses it as both arg and dst.
                            let dst = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::Mov, dst, obj, 0));
                            let mut instr = crate::bytecode::instruction::ri16(Opcode::Intrinsic, dst, intrinsic_id);
                            instr.flags = 1; // arg_count = 1
                            self.chunk.emit(instr);
                            return dst;
                        }
                    }
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
                // General vtable dispatch (fallback for dynamic/polymorphic calls).
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
                    self.type_map.get(&key).cloned()
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
                if matches!(self.type_map.get(&obj_key), Some(TypeKind::Slice { .. })) {
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

                for arm in arms {
                    match &arm.pattern.node {
                        PatternKind::Wildcard => {
                            let val = self.compile_expr(&arm.expr);
                            if val != dst {
                                self.chunk.emit(rrr(Opcode::Mov, dst, val, 0));
                            }
                        }
                        PatternKind::Variant { enum_name, variant, bindings } => {
                            let tag = self.variant_tag(enum_name.as_deref(), variant);
                            let disc = self.alloc_reg();
                            self.chunk.emit(rrr(Opcode::FieldLoad, disc, scr, 0));
                            let tag_reg = self.alloc_reg();
                            self.chunk.emit(ri16(Opcode::MovI, tag_reg, tag as u16));
                            self.chunk.emit(rrr(Opcode::Cmp, 0, disc, tag_reg));
                            let skip = self.chunk.emit(ri16(Opcode::Jne, 0, 0));
                            // Extract bound variables from payload slots.
                            for (i, binding) in bindings.iter().enumerate() {
                                if binding != "_" {
                                    let bound_reg = self.bind(binding.clone());
                                    let off = ((i + 1) * 8) as u8;
                                    self.chunk.emit(rrr(Opcode::FieldLoad, bound_reg, scr, off));
                                }
                            }
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

            ExprKind::Try { expr: inner } => {
                let scr = self.compile_expr(inner);
                let key = (inner.span.start, inner.span.end);
                let is_option = matches!(
                    self.type_map.get(&key),
                    Some(TypeKind::Named { name, .. }) if name == "Option"
                );

                // Load discriminant (offset 0) and compare against tag 1 (Ok/Some).
                let disc = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::FieldLoad, disc, scr, 0));
                let tag_ok = self.alloc_reg();
                self.chunk.emit(ri16(Opcode::MovI, tag_ok, 1));
                self.chunk.emit(rrr(Opcode::Cmp, 0, disc, tag_ok));
                let jne = self.chunk.emit(ri16(Opcode::Jne, 0, 0));

                // Success path: extract payload at offset 8.
                let val = self.alloc_reg();
                self.chunk.emit(rrr(Opcode::FieldLoad, val, scr, 8));
                let jmp_end = self.chunk.emit(ri16(Opcode::Jmp, 0, 0));

                // Failure path: build Err(payload) or None and early-return.
                self.chunk.patch_jump(jne, self.chunk.len() as u16);
                if is_option {
                    // Return None — tag=0, no payload.
                    let none_ptr = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::New, none_ptr, 16));
                    let tag_zero = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::MovI, tag_zero, 0));
                    self.chunk.emit(rrr(Opcode::FieldStore, tag_zero, none_ptr, 0));
                    if none_ptr != 0 {
                        self.chunk.emit(rrr(Opcode::Mov, 0, none_ptr, 0));
                    }
                } else {
                    // Return Err(payload) — tag=0, payload at offset 8.
                    let err_payload = self.alloc_reg();
                    self.chunk.emit(rrr(Opcode::FieldLoad, err_payload, scr, 8));
                    let err_ptr = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::New, err_ptr, 16));
                    let tag_zero = self.alloc_reg();
                    self.chunk.emit(ri16(Opcode::MovI, tag_zero, 0));
                    self.chunk.emit(rrr(Opcode::FieldStore, tag_zero, err_ptr, 0));
                    self.chunk.emit(rrr(Opcode::FieldStore, err_payload, err_ptr, 8));
                    if err_ptr != 0 {
                        self.chunk.emit(rrr(Opcode::Mov, 0, err_ptr, 0));
                    }
                }
                self.chunk.emit(rrr(Opcode::Ret, 0, 0, 0));

                self.chunk.patch_jump(jmp_end, self.chunk.len() as u16);
                val
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
        "void.str_concat" => 14,
        "void.int_to_str" => 15,
        "void.float_to_str" => 16,
        "void.format" => 17,
        "void.array.store" => 18,
        "void.array.load" => 19,
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
}
