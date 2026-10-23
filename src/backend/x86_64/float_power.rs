// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Self-contained typed floating power. Integer exponents retain all their
//! bits; fractional exponents use the x87 logarithm/exponential instructions.
//! The latter is approximate, rather than a correctly-rounded libm contract.

use iced_x86::IcedError;
use iced_x86::code_asm::*;

use crate::backend::BackendError;
use crate::bytecode::instruction::{
    POW_BASE_FLOAT_FLAG, POW_BASE_UNSIGNED_FLAG, POW_EXP_FLOAT_FLAG, POW_EXP_UNSIGNED_FLAG,
    valid_float_power_flags,
};

const ONE: u64 = 1.0_f64.to_bits();
const INFINITY: u64 = f64::INFINITY.to_bits();
const SIGN: u64 = 1 << 63;

pub(super) fn emit(
    asm: &mut CodeAssembler,
    destination: AsmMemoryOperand,
    base: AsmMemoryOperand,
    exponent: AsmMemoryOperand,
    flags: u8,
) -> Result<(), BackendError> {
    if !valid_float_power_flags(flags) {
        return Err(BackendError(
            "Pow requires typed floating operand metadata; rebuild this artifact from source"
                .into(),
        ));
    }
    emit_inner(asm, destination, base, exponent, flags)
        .map_err(|error| BackendError(error.to_string()))
}

fn emit_inner(
    asm: &mut CodeAssembler,
    destination: AsmMemoryOperand,
    base: AsmMemoryOperand,
    exponent: AsmMemoryOperand,
    flags: u8,
) -> Result<(), IcedError> {
    // Inputs and destination address rbp-relative Quazi slots. Reserve private
    // scratch without any external call or reliance on a platform red zone.
    asm.sub(rsp, 32)?;
    if flags & POW_BASE_FLOAT_FLAG != 0 {
        asm.movq(xmm0, base)?;
    } else {
        asm.mov(rax, base)?;
        if flags & POW_BASE_UNSIGNED_FLAG != 0 {
            let mut ordinary = asm.create_label();
            let mut converted = asm.create_label();
            asm.test(rax, rax)?;
            asm.jns(ordinary)?;
            // A sticky low bit preserves rounding when converting the upper
            // half of u64 through an exactly doubled signed conversion.
            asm.mov(rcx, rax)?;
            asm.and(ecx, 1)?;
            asm.shr(rax, 1u32)?;
            asm.or(rax, rcx)?;
            asm.cvtsi2sd(xmm0, rax)?;
            asm.addsd(xmm0, xmm0)?;
            asm.jmp(converted)?;
            asm.set_label(&mut ordinary)?;
            asm.cvtsi2sd(xmm0, rax)?;
            asm.set_label(&mut converted)?;
        } else {
            asm.cvtsi2sd(xmm0, rax)?;
        }
    }
    if flags & POW_EXP_FLOAT_FLAG == 0 {
        emit_integer_exponent(asm, exponent, flags)?;
        asm.movq(destination, xmm1)?;
    } else {
        emit_float_exponent(asm, exponent)?;
        asm.mov(destination, rax)?;
    }
    asm.add(rsp, 32)?;
    Ok(())
}

fn emit_integer_exponent(
    asm: &mut CodeAssembler,
    exponent: AsmMemoryOperand,
    flags: u8,
) -> Result<(), IcedError> {
    let mut magnitude_ready = asm.create_label();
    let mut reduce = asm.create_label();
    let mut shift = asm.create_label();
    let mut done = asm.create_label();
    asm.mov(rcx, exponent)?;
    asm.mov(rax, ONE)?;
    asm.movq(xmm1, rax)?;
    if flags & POW_EXP_UNSIGNED_FLAG == 0 {
        asm.test(rcx, rcx)?;
        asm.jns(magnitude_ready)?;
        // Negating MIN leaves its correct unsigned magnitude. Convert the
        // base to its reciprocal first, preserving e.g. 2.0 ** -1074.
        asm.neg(rcx)?;
        asm.movapd(xmm2, xmm1)?;
        asm.divsd(xmm2, xmm0)?;
        asm.movapd(xmm0, xmm2)?;
    }
    asm.set_label(&mut magnitude_ready)?;
    asm.test(rcx, rcx)?;
    asm.jz(done)?;
    asm.set_label(&mut reduce)?;
    asm.test(cl, 1i32)?;
    asm.jz(shift)?;
    asm.mulsd(xmm1, xmm0)?;
    asm.set_label(&mut shift)?;
    asm.shr(rcx, 1u32)?;
    asm.jz(done)?;
    asm.mulsd(xmm0, xmm0)?;
    asm.jmp(reduce)?;
    asm.set_label(&mut done)?;
    Ok(())
}

fn emit_float_exponent(
    asm: &mut CodeAssembler,
    exponent: AsmMemoryOperand,
) -> Result<(), IcedError> {
    let mut invalid = asm.create_label();
    let mut one = asm.create_label();
    let mut signed_one = asm.create_label();
    let mut zero = asm.create_label();
    let mut infinity = asm.create_label();
    let mut infinite_exponent = asm.create_label();
    let mut smaller_than_one = asm.create_label();
    let mut not_integral = asm.create_label();
    let mut classified = asm.create_label();
    let mut zero_base = asm.create_label();
    let mut infinite_base = asm.create_label();
    let mut overflow = asm.create_label();
    let mut underflow = asm.create_label();
    let mut done = asm.create_label();

    asm.movq(rax, xmm0)?;
    asm.mov(qword_ptr(rsp), rax)?;
    asm.mov(rcx, exponent)?;
    asm.mov(qword_ptr(rsp + 8), rcx)?;
    asm.mov(r8, rax)?;
    asm.btr(r8, 63u32)?;
    asm.mov(r9, rcx)?;
    asm.btr(r9, 63u32)?;
    asm.xor(r10d, r10d)?; // Result sign, set only for a negative base and odd integer exponent.

    // Conventional power identities precede NaN handling: NaN**0 and 1**NaN
    // are one. All nonfinite classifications use integer bits before x87.
    asm.test(r9, r9)?;
    asm.jz(one)?;
    asm.mov(rdx, ONE)?;
    asm.cmp(rax, rdx)?;
    asm.je(one)?;
    asm.mov(rdx, INFINITY)?;
    asm.cmp(r9, rdx)?;
    asm.ja(invalid)?;
    asm.cmp(r8, rdx)?;
    asm.ja(invalid)?;
    asm.cmp(r9, rdx)?;
    asm.je(infinite_exponent)?;

    asm.test(rax, rax)?;
    asm.jns(classified)?;
    // A representable f64 integer above 2**52 is necessarily even. At or
    // below that threshold inspect its actual mantissa, without an i64 cast.
    asm.mov(rdx, r9)?;
    asm.shr(rdx, 52u32)?;
    asm.sub(edx, 1023)?;
    asm.cmp(edx, 0i32)?;
    asm.jl(not_integral)?;
    asm.cmp(edx, 52i32)?;
    asm.jg(classified)?;
    asm.mov(ecx, 52i32)?;
    asm.sub(ecx, edx)?;
    asm.mov(rdx, 1u64)?;
    asm.shl(rdx, cl)?;
    asm.dec(rdx)?;
    asm.test(r9, rdx)?;
    asm.jnz(not_integral)?;
    asm.mov(rdx, r9)?;
    asm.shr(rdx, cl)?;
    asm.test(dl, 1i32)?;
    asm.jz(classified)?;
    asm.mov(r10, SIGN)?;
    asm.jmp(classified)?;

    asm.set_label(&mut not_integral)?;
    asm.test(r8, r8)?;
    asm.jz(classified)?; // Negative zero has positive result for fractional exponents.
    asm.mov(rdx, INFINITY)?;
    asm.cmp(r8, rdx)?;
    asm.je(classified)?; // Likewise negative infinity.
    asm.jmp(invalid)?;

    asm.set_label(&mut classified)?;
    asm.test(r8, r8)?;
    asm.jz(zero_base)?;
    asm.mov(rdx, INFINITY)?;
    asm.cmp(r8, rdx)?;
    asm.je(infinite_base)?;
    asm.mov(rdx, ONE)?;
    asm.cmp(r8, rdx)?;
    asm.je(signed_one)?;

    asm.mov(qword_ptr(rsp), r8)?;
    asm.fld(qword_ptr(rsp + 8))?; // y
    asm.fld(qword_ptr(rsp))?; // |x|, y
    asm.fyl2x()?; // t = y * log2(|x|)
    // Even the largest f64 y times log2(f64 x) fits extended precision.
    // Bound t before splitting, so F2XM1 never receives infinities/out-of-range values.
    asm.mov(rax, 1024.0_f64.to_bits())?;
    asm.mov(qword_ptr(rsp + 16), rax)?;
    asm.fld(qword_ptr(rsp + 16))?;
    asm.fcomip(st0, st1)?;
    asm.jbe(overflow)?;
    asm.mov(rax, (-1075.0_f64).to_bits())?;
    asm.mov(qword_ptr(rsp + 16), rax)?;
    asm.fld(qword_ptr(rsp + 16))?;
    asm.fcomip(st0, st1)?;
    asm.jae(underflow)?;

    asm.fld(st0)?; // t, t
    asm.frndint()?; // n, t
    asm.fxch(st0, st1)?; // t, n
    asm.fsub_2(st0, st1)?; // fraction in [-1, 1], n
    asm.f2xm1()?;
    asm.fld1()?;
    asm.faddp(st1, st0)?;
    asm.fscale()?;
    asm.fstp(qword_ptr(rsp + 16))?;
    asm.fstp(st0)?;
    asm.mov(rax, qword_ptr(rsp + 16))?;
    asm.or(rax, r10)?;
    asm.jmp(done)?;

    asm.set_label(&mut overflow)?;
    asm.fstp(st0)?;
    asm.jmp(infinity)?;
    asm.set_label(&mut underflow)?;
    asm.fstp(st0)?;
    asm.jmp(zero)?;

    asm.set_label(&mut infinite_exponent)?;
    asm.mov(rdx, ONE)?;
    asm.cmp(r8, rdx)?;
    asm.je(one)?;
    asm.jb(smaller_than_one)?;
    asm.test(rcx, rcx)?;
    asm.js(zero)?;
    asm.jmp(infinity)?;
    asm.set_label(&mut smaller_than_one)?;
    asm.test(rcx, rcx)?;
    asm.js(infinity)?;
    asm.jmp(zero)?;

    asm.set_label(&mut zero_base)?;
    asm.cmp(qword_ptr(rsp + 8), 0i32)?;
    asm.js(infinity)?;
    asm.jmp(zero)?;
    asm.set_label(&mut infinite_base)?;
    asm.cmp(qword_ptr(rsp + 8), 0i32)?;
    asm.js(zero)?;
    asm.jmp(infinity)?;
    asm.set_label(&mut invalid)?;
    asm.mov(rax, f64::NAN.to_bits())?;
    asm.jmp(done)?;
    asm.set_label(&mut one)?;
    asm.xor(r10d, r10d)?;
    asm.set_label(&mut signed_one)?;
    asm.mov(rax, ONE)?;
    asm.or(rax, r10)?;
    asm.jmp(done)?;
    asm.set_label(&mut infinity)?;
    asm.mov(rax, INFINITY)?;
    asm.or(rax, r10)?;
    asm.jmp(done)?;
    asm.set_label(&mut zero)?;
    asm.mov(rax, r10)?;
    asm.set_label(&mut done)?;
    Ok(())
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
mod tests {
    use super::*;
    use crate::bytecode::instruction::FLOAT_FLAG;
    use std::ffi::c_void;

    unsafe extern "C" {
        fn mmap(
            address: *mut c_void,
            length: usize,
            protection: i32,
            flags: i32,
            fd: i32,
            offset: i64,
        ) -> *mut c_void;
        fn mprotect(address: *mut c_void, length: usize, protection: i32) -> i32;
        fn munmap(address: *mut c_void, length: usize) -> i32;
    }

    struct Executable {
        address: *mut c_void,
        length: usize,
    }

    impl Executable {
        fn new(flags: u8) -> Self {
            let mut asm = CodeAssembler::new(64).unwrap();
            asm.push(rbp).unwrap();
            asm.mov(rbp, rsp).unwrap();
            asm.sub(rsp, 32).unwrap();
            asm.mov(qword_ptr(rbp - 8), rdi).unwrap();
            asm.mov(qword_ptr(rbp - 16), rsi).unwrap();
            emit(
                &mut asm,
                qword_ptr(rbp - 24),
                qword_ptr(rbp - 8),
                qword_ptr(rbp - 16),
                flags,
            )
            .unwrap();
            asm.mov(rax, qword_ptr(rbp - 24)).unwrap();
            asm.leave().unwrap();
            asm.ret().unwrap();
            let bytes = asm.assemble(0).unwrap();
            // mmap starts writable, then becomes executable/read-only before
            // invoking it. This tests emitted instructions without a linker or
            // source constant folding masking the backend result.
            let address = unsafe { mmap(std::ptr::null_mut(), bytes.len(), 3, 0x22, -1, 0) };
            assert_ne!(address as isize, -1, "mmap failed");
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), address.cast::<u8>(), bytes.len())
            };
            assert_eq!(unsafe { mprotect(address, bytes.len(), 5) }, 0);
            Self {
                address,
                length: bytes.len(),
            }
        }

        fn call(&self, base: u64, exponent: u64) -> f64 {
            let function: unsafe extern "C" fn(u64, u64) -> u64 =
                unsafe { std::mem::transmute(self.address) };
            f64::from_bits(unsafe { function(base, exponent) })
        }
    }

    impl Drop for Executable {
        fn drop(&mut self) {
            assert_eq!(unsafe { munmap(self.address, self.length) }, 0);
        }
    }

    fn assert_same(actual: f64, expected: f64, context: &str) {
        if expected.is_nan() {
            assert!(actual.is_nan(), "{context}: {actual:?} != NaN");
        } else {
            assert_eq!(
                actual.to_bits(),
                expected.to_bits(),
                "{context}: {actual:?} != {expected:?}"
            );
        }
    }

    #[test]
    fn integer_exponent_preserves_magnitude_parity_and_reciprocal_subnormals() {
        let signed = Executable::new(FLOAT_FLAG | POW_BASE_FLOAT_FLAG);
        for (base, exponent, expected) in [
            (2.0_f64, -2_i64, 0.25_f64),
            (2.0, -1074, f64::from_bits(1)),
            (-1.0, i64::MIN, 1.0),
            (-1.0, i64::MAX, -1.0),
            (-1.0, 9_007_199_254_740_993, -1.0),
            (f64::NAN, 0, 1.0),
            (-0.0, -3, f64::NEG_INFINITY),
            (-0.0, 3, -0.0),
            (f64::NEG_INFINITY, -3, -0.0),
            (f64::NEG_INFINITY, 2, f64::INFINITY),
        ] {
            assert_same(
                signed.call(base.to_bits(), exponent as u64),
                expected,
                &format!("{base:?} ** {exponent}"),
            );
        }
        let unsigned = Executable::new(FLOAT_FLAG | POW_BASE_FLOAT_FLAG | POW_EXP_UNSIGNED_FLAG);
        assert_same(
            unsigned.call((-1.0_f64).to_bits(), u64::MAX),
            -1.0,
            "unsigned MAX stays odd",
        );
        assert_same(
            unsigned.call((-1.0_f64).to_bits(), 1_u64 << 63),
            1.0,
            "unsigned high bit is positive and even",
        );
    }

    #[test]
    fn float_exponent_special_values_and_unsigned_base_conversion() {
        let floating = Executable::new(FLOAT_FLAG | POW_BASE_FLOAT_FLAG | POW_EXP_FLOAT_FLAG);
        let values = [
            f64::NAN,
            f64::NEG_INFINITY,
            -2.0,
            -1.0,
            -0.0,
            0.0,
            0.5,
            1.0,
            2.0,
            f64::INFINITY,
        ];
        let exponents = [
            f64::NAN,
            f64::NEG_INFINITY,
            -3.0,
            -2.0,
            -0.5,
            -0.0,
            0.0,
            0.5,
            1.0,
            2.0,
            3.0,
            f64::INFINITY,
        ];
        for base in values {
            for exponent in exponents {
                let actual = floating.call(base.to_bits(), exponent.to_bits());
                let expected = base.powf(exponent);
                // Finite fractional results are hardware transcendental
                // approximations; the selected finite identities are exact.
                if expected.is_finite() && expected != 0.0 && exponent.fract() != 0.0 {
                    assert!(
                        (actual - expected).abs() <= expected.abs() * 2e-14,
                        "{base:?} ** {exponent:?}: {actual:?} != {expected:?}"
                    );
                } else {
                    assert_same(actual, expected, &format!("{base:?} ** {exponent:?}"));
                }
            }
        }
        for exponent in [
            9_007_199_254_740_991.0_f64,
            9_007_199_254_740_992.0,
            4_503_599_627_370_495.0,
            4_503_599_627_370_496.0,
        ] {
            assert_same(
                floating.call((-1.0_f64).to_bits(), exponent.to_bits()),
                (-1.0_f64).powf(exponent),
                "floating exact parity",
            );
        }
        let unsigned = Executable::new(FLOAT_FLAG | POW_BASE_UNSIGNED_FLAG | POW_EXP_FLOAT_FLAG);
        for base in [0, 1, (1_u64 << 53) + 1, 1_u64 << 63, u64::MAX] {
            assert_same(
                unsigned.call(base, 1.0_f64.to_bits()),
                base as f64,
                "unsigned conversion before float power",
            );
        }
    }

    #[test]
    fn float_exponent_approximates_finite_values_and_balances_x87_state() {
        let floating = Executable::new(FLOAT_FLAG | POW_BASE_FLOAT_FLAG | POW_EXP_FLOAT_FLAG);
        let state = || {
            let mut control = 0_u16;
            let status: u16;
            unsafe {
                std::arch::asm!("fnstcw [{address}]", address = in(reg) &mut control, options(nostack, preserves_flags));
                std::arch::asm!("fnstsw ax", out("ax") status, options(nostack, preserves_flags));
            }
            (control, (status >> 11) & 7)
        };
        let before = state();
        for iteration in 0..64 {
            for (base, exponent) in [
                (1.0001_f64, 10000.25_f64),
                (0.25, -2.5),
                (7.0, 0.125),
                (2.0, -1074.0),
                (2.0, -1075.0),
                (2.0, 1024.0),
                (-2.0, 3.0),
                (-2.0, 0.5),
            ] {
                let actual = floating.call(base.to_bits(), exponent.to_bits());
                let expected = base.powf(exponent);
                if expected.is_finite() && expected != 0.0 {
                    assert!(
                        (actual - expected).abs() <= expected.abs() * 2e-13,
                        "iteration {iteration}: {base} ** {exponent}: {actual:?} != {expected:?}"
                    );
                } else {
                    assert_same(actual, expected, "nonfinite/underflow");
                }
            }
        }
        // Exercise a broad finite range while keeping the expected result
        // normal, so relative error is meaningful and not hidden by overflow.
        let mut random = 0x9e37_79b9_7f4a_7c15_u64;
        for _ in 0..768 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let base = 2.0_f64.powf((random % 1_000_001) as f64 / 1000.0 - 500.0);
            let exponent = ((random >> 32) % 3601) as f64 / 1000.0 - 1.8;
            let actual = floating.call(base.to_bits(), exponent.to_bits());
            let expected = base.powf(exponent);
            assert!(
                (actual - expected).abs() <= expected.abs() * 4e-13,
                "finite sample {base:?} ** {exponent:?}: {actual:?} != {expected:?}"
            );
        }
        assert_eq!(
            state(),
            before,
            "power must preserve x87 control word and stack TOP"
        );
    }
}
