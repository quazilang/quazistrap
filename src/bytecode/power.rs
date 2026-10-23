// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Exact checked integer power expressed in portable ordinary QZI instructions.

use super::*;
use crate::bytecode::instruction::{
    POW_BASE_FLOAT_FLAG, POW_BASE_UNSIGNED_FLAG, POW_EXP_FLOAT_FLAG, POW_EXP_UNSIGNED_FLAG,
};

fn integer_shape(ty: &TypeKind) -> Option<(u32, bool)> {
    Some(match ty {
        TypeKind::Int8 => (8, true),
        TypeKind::Int16 => (16, true),
        TypeKind::Int32 => (32, true),
        TypeKind::Int64 | TypeKind::Isize => (64, true),
        TypeKind::Uint8 => (8, false),
        TypeKind::Uint16 => (16, false),
        TypeKind::Uint32 => (32, false),
        TypeKind::Uint64 | TypeKind::Usize => (64, false),
        _ => return None,
    })
}

fn floating(ty: &TypeKind) -> bool {
    matches!(
        ty,
        TypeKind::Float16 | TypeKind::Float32 | TypeKind::Float64
    )
}

impl FnCompiler<'_> {
    fn power_constant(&mut self, value: u64) -> u8 {
        let register = self.alloc_reg();
        let index = self.chunk.add_constant(ConstPoolEntry::Int(value as i64));
        self.chunk.emit(ri16(Opcode::MovConst, register, index));
        register
    }

    /// Magnitudes are unsigned, including the magnitude of signed MIN. Check
    /// before multiplication so even u64 products cannot wrap before detection.
    fn checked_power_multiply(&mut self, dst: u8, left: u8, right: u8, limit: u8, span: Span) {
        let zero = self.chunk.emit(ri16(Opcode::Jz, right, 0));
        let threshold = self.alloc_reg();
        let mut divide = rrr(Opcode::Div, threshold, limit, right);
        divide.flags = UNSIGNED_FLAG;
        self.chunk.emit(divide);
        self.chunk.emit(rrr(Opcode::Cmp, 0, left, threshold));
        let mut fits = ri16(Opcode::Jle, 0, 0);
        fits.flags = UNSIGNED_FLAG;
        let fits = self.chunk.emit(fits);
        self.emit_language_panic(span, "integer power overflow");
        self.chunk.patch_jump(fits, self.chunk.len() as u16);
        self.chunk.patch_jump(zero, self.chunk.len() as u16);
        self.chunk.emit(rrr(Opcode::Mul, dst, left, right));
    }

    pub(super) fn emit_power(
        &mut self,
        dst: u8,
        base: u8,
        exponent: u8,
        left: &Expr,
        right: &Expr,
        span: Span,
    ) {
        let base_ty = self.type_of_expr(left);
        let exponent_ty = self.type_of_expr(right);
        let symbolic = |ty: &Option<TypeKind>| {
            matches!(ty,
            Some(TypeKind::Named { name, type_args }) if type_args.is_empty() && self.bound_names.contains(name))
        };
        if symbolic(&base_ty) || symbolic(&exponent_ty) {
            // Symbolic helper bodies are not numeric implementations. Concrete
            // dependency closure must select their checked specializations.
            self.chunk.emit(rrr(Opcode::Trap, 0, 0, 0));
            self.chunk.emit(ri16(Opcode::MovI, dst, 0));
            return;
        }
        let (Some(base_ty), Some(exponent_ty)) = (base_ty, exponent_ty) else {
            self.codegen_error
                .get_or_insert_with(|| "power operands lack concrete type evidence".into());
            return;
        };
        if floating(&base_ty) || floating(&exponent_ty) {
            let mut instruction = rrr(Opcode::Pow, dst, base, exponent);
            instruction.flags = FLOAT_FLAG;
            for (ty, float_flag, unsigned_flag) in [
                (&base_ty, POW_BASE_FLOAT_FLAG, POW_BASE_UNSIGNED_FLAG),
                (&exponent_ty, POW_EXP_FLOAT_FLAG, POW_EXP_UNSIGNED_FLAG),
            ] {
                if floating(ty) {
                    instruction.flags |= float_flag;
                } else if let Some((_, signed)) = integer_shape(ty) {
                    if !signed {
                        instruction.flags |= unsigned_flag;
                    }
                } else {
                    self.codegen_error
                        .get_or_insert_with(|| "power operand is not a concrete number".into());
                    return;
                }
            }
            self.chunk.emit(instruction);
            return;
        }
        let (Some((width, signed)), Some((_, exponent_signed))) =
            (integer_shape(&base_ty), integer_shape(&exponent_ty))
        else {
            self.codegen_error
                .get_or_insert_with(|| "integer power lacks concrete integer types".into());
            return;
        };
        let zero = self.power_constant(0);
        let one = self.power_constant(1);
        if exponent_signed {
            self.chunk.emit(rrr(Opcode::Cmp, 0, exponent, zero));
            let nonnegative = self.chunk.emit(ri16(Opcode::Jge, 0, 0));
            self.emit_language_panic(span, "integer power exponent must be nonnegative");
            self.chunk.patch_jump(nonnegative, self.chunk.len() as u16);
        }
        let remaining = self.alloc_reg();
        self.chunk.emit(rrr(Opcode::Mov, remaining, exponent, 0));
        let factor = self.alloc_reg();
        self.chunk.emit(rrr(Opcode::Mov, factor, base, 0));
        let negative = self.alloc_reg();
        self.chunk.emit(ri16(Opcode::MovI, negative, 0));
        let maximum = if signed {
            (1_u64 << (width - 1)) - 1
        } else {
            u64::MAX >> (64 - width)
        };
        let limit = self.power_constant(maximum);
        if signed {
            self.chunk.emit(rrr(Opcode::Cmp, 0, base, zero));
            let positive_base = self.chunk.emit(ri16(Opcode::Jge, 0, 0));
            self.chunk.emit(rrr(Opcode::Neg, factor, factor, 0));
            self.chunk.emit(rrr(Opcode::And, negative, exponent, one));
            let positive_result = self.chunk.emit(ri16(Opcode::Jz, negative, 0));
            let minimum_magnitude = self.power_constant(1_u64 << (width - 1));
            self.chunk
                .emit(rrr(Opcode::Mov, limit, minimum_magnitude, 0));
            self.chunk
                .patch_jump(positive_result, self.chunk.len() as u16);
            self.chunk
                .patch_jump(positive_base, self.chunk.len() as u16);
        }
        self.chunk.emit(ri16(Opcode::MovI, dst, 1));
        let loop_top = self.chunk.len() as u16;
        let done_zero = self.chunk.emit(ri16(Opcode::Jz, remaining, 0));
        let odd = self.alloc_reg();
        self.chunk.emit(rrr(Opcode::And, odd, remaining, one));
        let even = self.chunk.emit(ri16(Opcode::Jz, odd, 0));
        self.checked_power_multiply(dst, dst, factor, limit, span);
        self.chunk.patch_jump(even, self.chunk.len() as u16);
        self.chunk.emit(rrr(Opcode::Shr, remaining, remaining, one));
        let done_shift = self.chunk.emit(ri16(Opcode::Jz, remaining, 0));
        self.checked_power_multiply(factor, factor, factor, limit, span);
        self.chunk.emit(ri16(Opcode::Jmp, 0, loop_top));
        self.chunk.patch_jump(done_zero, self.chunk.len() as u16);
        self.chunk.patch_jump(done_shift, self.chunk.len() as u16);
        let positive = self.chunk.emit(ri16(Opcode::Jz, negative, 0));
        self.chunk.emit(rrr(Opcode::Neg, dst, dst, 0));
        self.chunk.patch_jump(positive, self.chunk.len() as u16);
    }
}
