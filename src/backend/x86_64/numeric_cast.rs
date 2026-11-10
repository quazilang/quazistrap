// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Numeric `as` conversions between the integer and floating families.
//!
//! VM slots hold floating values as f64 (f32 values are exactly representable
//! there), so every conversion reads and writes one 64-bit slot in place.
//! Integer to float rounds to nearest-even, directly to the target precision.
//! Float to integer truncates toward zero and saturates: NaN becomes 0 and
//! values beyond the target range become its minimum or maximum (the Rust
//! `as` contract, D-021).

use iced_x86::IcedError;
use iced_x86::code_asm::*;

use crate::backend::BackendError;
use crate::bytecode::instruction::numeric_cast as id;

/// Emit the conversion `intrinsic` for the value in `slot`.
pub(super) fn emit(
    asm: &mut CodeAssembler,
    slot: AsmMemoryOperand,
    intrinsic: u16,
) -> Result<(), BackendError> {
    emit_inner(asm, slot, intrinsic).map_err(|error| BackendError(error.to_string()))
}

fn emit_inner(
    asm: &mut CodeAssembler,
    slot: AsmMemoryOperand,
    intrinsic: u16,
) -> Result<(), IcedError> {
    match intrinsic {
        id::SIGNED_TO_F64 => {
            asm.cvtsi2sd(xmm0, slot)?;
            asm.movsd_2(slot, xmm0)
        }
        id::SIGNED_TO_F32 => {
            asm.cvtsi2ss(xmm0, slot)?;
            asm.cvtss2sd(xmm0, xmm0)?;
            asm.movsd_2(slot, xmm0)
        }
        id::UNSIGNED_TO_F64 | id::UNSIGNED_TO_F32 => {
            unsigned_to_float(asm, slot, intrinsic == id::UNSIGNED_TO_F32)
        }
        id::F64_TO_F32 => {
            asm.cvtsd2ss(xmm0, slot)?;
            asm.cvtss2sd(xmm0, xmm0)?;
            asm.movsd_2(slot, xmm0)
        }
        id::F64_TO_I64 => float_to_signed(asm, slot, 64),
        id::F64_TO_I32 => float_to_signed(asm, slot, 32),
        id::F64_TO_I16 => float_to_signed(asm, slot, 16),
        id::F64_TO_I8 => float_to_signed(asm, slot, 8),
        id::F64_TO_U64 => float_to_unsigned(asm, slot, 64),
        id::F64_TO_U32 => float_to_unsigned(asm, slot, 32),
        id::F64_TO_U16 => float_to_unsigned(asm, slot, 16),
        id::F64_TO_U8 => float_to_unsigned(asm, slot, 8),
        _ => unreachable!("validated numeric cast intrinsic {intrinsic}"),
    }
}

/// u64 has no SSE conversion: a value with the top bit set is halved,
/// keeping the lost bit sticky so the final doubling rounds correctly.
fn unsigned_to_float(
    asm: &mut CodeAssembler,
    slot: AsmMemoryOperand,
    single: bool,
) -> Result<(), IcedError> {
    let mut large = asm.create_label();
    let mut done = asm.create_label();
    asm.mov(rax, slot)?;
    asm.test(rax, rax)?;
    asm.js(large)?;
    convert(asm, rax, single)?;
    asm.jmp(done)?;
    asm.set_label(&mut large)?;
    asm.mov(rcx, rax)?;
    asm.shr(rcx, 1)?;
    asm.and(eax, 1)?;
    asm.or(rcx, rax)?;
    convert(asm, rcx, single)?;
    if single {
        asm.addss(xmm0, xmm0)?;
    } else {
        asm.addsd(xmm0, xmm0)?;
    }
    asm.set_label(&mut done)?;
    if single {
        asm.cvtss2sd(xmm0, xmm0)?;
    }
    asm.movsd_2(slot, xmm0)
}

fn convert(asm: &mut CodeAssembler, value: AsmRegister64, single: bool) -> Result<(), IcedError> {
    if single {
        asm.cvtsi2ss(xmm0, value)
    } else {
        asm.cvtsi2sd(xmm0, value)
    }
}

fn float_to_signed(
    asm: &mut CodeAssembler,
    slot: AsmMemoryOperand,
    bits: u32,
) -> Result<(), IcedError> {
    let limit = 2f64.powi(bits as i32 - 1);
    let maximum = (1i64 << (bits - 1)).wrapping_sub(1) as u64;
    let minimum = (-(1i128 << (bits - 1))) as i64 as u64;
    saturating(asm, slot, -limit, limit, minimum, maximum, false)
}

fn float_to_unsigned(
    asm: &mut CodeAssembler,
    slot: AsmMemoryOperand,
    bits: u32,
) -> Result<(), IcedError> {
    let maximum = if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    saturating(
        asm,
        slot,
        0.0,
        2f64.powi(bits as i32),
        0,
        maximum,
        bits == 64,
    )
}

/// `x <= low` yields `minimum`, `x >= high` yields `maximum`, NaN yields 0,
/// and every other value truncates toward zero (all such values fit).
fn saturating(
    asm: &mut CodeAssembler,
    slot: AsmMemoryOperand,
    low: f64,
    high: f64,
    minimum: u64,
    maximum: u64,
    full_unsigned: bool,
) -> Result<(), IcedError> {
    let mut nan = asm.create_label();
    let mut below = asm.create_label();
    let mut above = asm.create_label();
    let mut store = asm.create_label();
    asm.movsd_2(xmm0, slot)?;
    asm.ucomisd(xmm0, xmm0)?;
    asm.jp(nan)?;
    asm.mov(rax, high.to_bits())?;
    asm.movq(xmm1, rax)?;
    asm.ucomisd(xmm0, xmm1)?;
    asm.jae(above)?;
    asm.mov(rax, low.to_bits())?;
    asm.movq(xmm1, rax)?;
    asm.ucomisd(xmm0, xmm1)?;
    asm.jbe(below)?;
    if full_unsigned {
        // [2^63, 2^64) does not fit cvttsd2si: convert x - 2^63 and set the
        // top bit back.
        let mut small = asm.create_label();
        let sign = 2f64.powi(63);
        asm.mov(rax, sign.to_bits())?;
        asm.movq(xmm1, rax)?;
        asm.ucomisd(xmm0, xmm1)?;
        asm.jb(small)?;
        asm.subsd(xmm0, xmm1)?;
        asm.cvttsd2si(rax, xmm0)?;
        asm.btc(rax, 63)?;
        asm.jmp(store)?;
        asm.set_label(&mut small)?;
    }
    asm.cvttsd2si(rax, xmm0)?;
    asm.jmp(store)?;
    asm.set_label(&mut nan)?;
    asm.xor(eax, eax)?;
    asm.jmp(store)?;
    asm.set_label(&mut below)?;
    asm.mov(rax, minimum)?;
    asm.jmp(store)?;
    asm.set_label(&mut above)?;
    asm.mov(rax, maximum)?;
    asm.set_label(&mut store)?;
    asm.mov(slot, rax)
}

#[cfg(all(test, target_arch = "x86_64", target_os = "linux"))]
mod tests {
    use super::*;
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

    /// The conversion compiled as `fn(u64) -> u64` over one stack slot.
    struct Conversion {
        address: *mut c_void,
        length: usize,
    }

    impl Conversion {
        fn new(intrinsic: u16) -> Self {
            let mut asm = CodeAssembler::new(64).unwrap();
            asm.push(rbp).unwrap();
            asm.mov(rbp, rsp).unwrap();
            asm.sub(rsp, 16).unwrap();
            asm.mov(qword_ptr(rbp - 8), rdi).unwrap();
            emit(&mut asm, qword_ptr(rbp - 8), intrinsic).unwrap();
            asm.mov(rax, qword_ptr(rbp - 8)).unwrap();
            asm.leave().unwrap();
            asm.ret().unwrap();
            let bytes = asm.assemble(0).unwrap();
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

        fn call(&self, value: u64) -> u64 {
            let function: unsafe extern "C" fn(u64) -> u64 =
                unsafe { std::mem::transmute(self.address) };
            unsafe { function(value) }
        }
    }

    impl Drop for Conversion {
        fn drop(&mut self) {
            assert_eq!(unsafe { munmap(self.address, self.length) }, 0);
        }
    }

    const SPECIAL: [f64; 18] = [
        0.0,
        -0.0,
        0.5,
        -0.5,
        0.99,
        -1.0,
        -1.5,
        127.9,
        128.0,
        -128.9,
        -129.0,
        255.5,
        65535.9,
        4294967295.5,
        9.223372036854775e18,
        1.8446744073709552e19,
        f64::INFINITY,
        f64::NAN,
    ];

    fn floats() -> Vec<f64> {
        let mut values = Vec::new();
        for value in SPECIAL {
            values.push(value);
            values.push(-value);
        }
        values.extend([
            f64::NEG_INFINITY,
            f64::MAX,
            f64::MIN_POSITIVE,
            -9.223372036854777e18,
            9.223372036854774e18,
            2147483647.9,
            -2147483648.9,
            2147483648.0,
        ]);
        values
    }

    #[test]
    fn float_to_integer_truncates_and_saturates_like_rust() {
        let cases: [(u16, fn(f64) -> u64); 8] = [
            (id::F64_TO_I64, |x| x as i64 as u64),
            (id::F64_TO_I32, |x| x as i32 as i64 as u64),
            (id::F64_TO_I16, |x| x as i16 as i64 as u64),
            (id::F64_TO_I8, |x| x as i8 as i64 as u64),
            (id::F64_TO_U64, |x| x as u64),
            (id::F64_TO_U32, |x| x as u32 as u64),
            (id::F64_TO_U16, |x| x as u16 as u64),
            (id::F64_TO_U8, |x| x as u8 as u64),
        ];
        for (intrinsic, expected) in cases {
            let conversion = Conversion::new(intrinsic);
            for value in floats() {
                assert_eq!(
                    conversion.call(value.to_bits()),
                    expected(value),
                    "intrinsic {intrinsic} of {value:e}"
                );
            }
        }
    }

    #[test]
    fn integer_to_float_rounds_to_nearest_at_the_target_precision() {
        let integers: [u64; 12] = [
            0,
            1,
            u64::MAX,
            1 << 63,
            (1 << 63) - 1,
            (1 << 53) + 1,
            (1 << 54) + 3,
            (1 << 24) + 1,
            (1 << 63) + (1 << 11) + 1,
            (1 << 63) + (1 << 10),
            0x8000_0000_0000_0401,
            12345,
        ];
        let signed_f64 = Conversion::new(id::SIGNED_TO_F64);
        let unsigned_f64 = Conversion::new(id::UNSIGNED_TO_F64);
        let signed_f32 = Conversion::new(id::SIGNED_TO_F32);
        let unsigned_f32 = Conversion::new(id::UNSIGNED_TO_F32);
        for value in integers {
            for value in [value, value.wrapping_neg()] {
                let signed = value as i64;
                assert_eq!(f64::from_bits(signed_f64.call(value)), signed as f64);
                assert_eq!(f64::from_bits(unsigned_f64.call(value)), value as f64);
                assert_eq!(f64::from_bits(signed_f32.call(value)), signed as f32 as f64);
                assert_eq!(
                    f64::from_bits(unsigned_f32.call(value)),
                    value as f32 as f64
                );
            }
        }
    }

    #[test]
    fn f64_to_f32_rounds_and_keeps_special_values() {
        let conversion = Conversion::new(id::F64_TO_F32);
        for value in floats().into_iter().chain([0.1, 1e40, -1e-50, 1.0 / 3.0]) {
            let actual = f64::from_bits(conversion.call(value.to_bits()));
            let expected = value as f32 as f64;
            assert!(
                actual.to_bits() == expected.to_bits() || (actual.is_nan() && expected.is_nan()),
                "{value:e}: {actual:e} != {expected:e}"
            );
        }
    }
}
