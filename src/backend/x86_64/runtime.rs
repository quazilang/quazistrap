// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Small libc-free Linux runtime routines emitted directly as x86-64 code.

use std::collections::HashSet;

use iced_x86::code_asm::*;

pub struct RuntimeFunction {
    pub name: &'static str,
    pub bytes: Vec<u8>,
}

pub fn generate_required(symbols: &HashSet<String>) -> Result<Vec<RuntimeFunction>, String> {
    let mut functions = Vec::new();
    for name in [
        "malloc", "free", "calloc", "realloc", "memcpy", "memmove", "memset", "memcmp", "strcpy",
        "strcat",
    ] {
        if !symbols.contains(name) {
            continue;
        }
        let bytes = match name {
            "malloc" => generate_malloc(),
            "free" => generate_free(),
            "calloc" => generate_calloc(),
            "realloc" => generate_realloc(),
            "memcpy" => generate_memcpy(),
            "memmove" => generate_memmove(),
            "memset" => generate_memset(),
            "memcmp" => generate_memcmp(),
            "strcpy" => generate_strcpy(),
            "strcat" => generate_strcat(),
            _ => unreachable!(),
        }?;
        functions.push(RuntimeFunction { name, bytes });
    }
    Ok(functions)
}

fn assembler() -> Result<CodeAssembler, String> {
    CodeAssembler::new(64).map_err(|error| error.to_string())
}

fn finish(mut assembler: CodeAssembler) -> Result<Vec<u8>, String> {
    assembler
        .assemble(0)
        .map_err(|error| format!("cannot assemble embedded runtime: {error}"))
}

fn generate_malloc() -> Result<Vec<u8>, String> {
    let mut asm = assembler()?;
    let mut nonzero = asm.create_label();
    let mut overflow = asm.create_label();
    let mut failed = asm.create_label();
    asm.test(rdi, rdi).unwrap();
    asm.jne(nonzero).unwrap();
    asm.mov(rdi, 1i64).unwrap();
    asm.set_label(&mut nonzero).unwrap();
    asm.push(rdi).unwrap();
    asm.add(rdi, 16i32).unwrap();
    asm.jo(overflow).unwrap();
    asm.push(rdi).unwrap();
    emit_mmap(&mut asm);
    asm.pop(rdx).unwrap();
    asm.pop(rcx).unwrap();
    asm.cmp(rax, -4095i32).unwrap();
    asm.jae(failed).unwrap();
    asm.mov(qword_ptr(rax), rdx).unwrap();
    asm.mov(qword_ptr(rax + 8i32), rcx).unwrap();
    asm.add(rax, 16i32).unwrap();
    asm.ret().unwrap();
    asm.set_label(&mut overflow).unwrap();
    asm.pop(rax).unwrap();
    asm.set_label(&mut failed).unwrap();
    asm.xor(eax, eax).unwrap();
    asm.ret().unwrap();
    finish(asm)
}

fn generate_free() -> Result<Vec<u8>, String> {
    let mut asm = assembler()?;
    let mut done = asm.create_label();
    asm.test(rdi, rdi).unwrap();
    asm.je(done).unwrap();
    asm.sub(rdi, 16i32).unwrap();
    asm.mov(rsi, qword_ptr(rdi)).unwrap();
    asm.mov(eax, 11i32).unwrap();
    asm.syscall().unwrap();
    asm.set_label(&mut done).unwrap();
    asm.xor(eax, eax).unwrap();
    asm.ret().unwrap();
    finish(asm)
}

fn generate_calloc() -> Result<Vec<u8>, String> {
    let mut asm = assembler()?;
    let mut nonzero = asm.create_label();
    let mut overflow_after_push = asm.create_label();
    let mut failed = asm.create_label();
    asm.mov(rax, rdi).unwrap();
    asm.mul(rsi).unwrap();
    asm.test(rdx, rdx).unwrap();
    asm.jne(failed).unwrap();
    asm.test(rax, rax).unwrap();
    asm.jne(nonzero).unwrap();
    asm.mov(eax, 1i32).unwrap();
    asm.set_label(&mut nonzero).unwrap();
    asm.push(rax).unwrap();
    asm.add(rax, 16i32).unwrap();
    asm.jo(overflow_after_push).unwrap();
    asm.push(rax).unwrap();
    asm.mov(rdi, rax).unwrap();
    emit_mmap(&mut asm);
    asm.pop(rdx).unwrap();
    asm.pop(rcx).unwrap();
    asm.cmp(rax, -4095i32).unwrap();
    asm.jae(failed).unwrap();
    asm.mov(qword_ptr(rax), rdx).unwrap();
    asm.mov(qword_ptr(rax + 8i32), rcx).unwrap();
    asm.add(rax, 16i32).unwrap();
    asm.mov(r8, rax).unwrap();
    asm.mov(rdi, rax).unwrap();
    asm.xor(eax, eax).unwrap();
    asm.rep().stosb().unwrap();
    asm.mov(rax, r8).unwrap();
    asm.ret().unwrap();
    asm.set_label(&mut overflow_after_push).unwrap();
    asm.pop(rax).unwrap();
    asm.set_label(&mut failed).unwrap();
    asm.xor(eax, eax).unwrap();
    asm.ret().unwrap();
    finish(asm)
}

fn generate_realloc() -> Result<Vec<u8>, String> {
    let mut asm = assembler()?;
    let mut allocate = asm.create_label();
    let mut release = asm.create_label();
    let mut allocate_nonzero = asm.create_label();
    let mut overflow_one = asm.create_label();
    let mut overflow_two = asm.create_label();
    let mut failed = asm.create_label();
    let mut copy_size_ready = asm.create_label();

    asm.test(rdi, rdi).unwrap();
    asm.je(allocate).unwrap();
    asm.test(rsi, rsi).unwrap();
    asm.je(release).unwrap();
    asm.push(rdi).unwrap();
    asm.push(rsi).unwrap();
    asm.mov(rax, rsi).unwrap();
    asm.add(rax, 16i32).unwrap();
    asm.jo(overflow_two).unwrap();
    asm.push(rax).unwrap();
    asm.mov(rdi, rax).unwrap();
    emit_mmap(&mut asm);
    asm.pop(rdx).unwrap();
    asm.pop(r9).unwrap();
    asm.pop(rsi).unwrap();
    asm.cmp(rax, -4095i32).unwrap();
    asm.jae(failed).unwrap();
    asm.mov(qword_ptr(rax), rdx).unwrap();
    asm.mov(qword_ptr(rax + 8i32), r9).unwrap();
    asm.lea(r8, qword_ptr(rax + 16i32)).unwrap();
    asm.mov(rcx, qword_ptr(rsi - 8i32)).unwrap();
    asm.cmp(rcx, r9).unwrap();
    asm.jbe(copy_size_ready).unwrap();
    asm.mov(rcx, r9).unwrap();
    asm.set_label(&mut copy_size_ready).unwrap();
    asm.push(r8).unwrap();
    asm.push(rsi).unwrap();
    asm.mov(rdi, r8).unwrap();
    asm.cld().unwrap();
    asm.rep().movsb().unwrap();
    asm.pop(rdi).unwrap();
    asm.sub(rdi, 16i32).unwrap();
    asm.mov(rsi, qword_ptr(rdi)).unwrap();
    asm.mov(eax, 11i32).unwrap();
    asm.syscall().unwrap();
    asm.pop(rax).unwrap();
    asm.ret().unwrap();

    asm.set_label(&mut release).unwrap();
    asm.sub(rdi, 16i32).unwrap();
    asm.mov(rsi, qword_ptr(rdi)).unwrap();
    asm.mov(eax, 11i32).unwrap();
    asm.syscall().unwrap();
    asm.xor(eax, eax).unwrap();
    asm.ret().unwrap();

    asm.set_label(&mut allocate).unwrap();
    asm.mov(rdi, rsi).unwrap();
    asm.test(rdi, rdi).unwrap();
    asm.jne(allocate_nonzero).unwrap();
    asm.mov(rdi, 1i64).unwrap();
    asm.set_label(&mut allocate_nonzero).unwrap();
    asm.push(rdi).unwrap();
    asm.add(rdi, 16i32).unwrap();
    asm.jo(overflow_one).unwrap();
    asm.push(rdi).unwrap();
    emit_mmap(&mut asm);
    asm.pop(rdx).unwrap();
    asm.pop(rcx).unwrap();
    asm.cmp(rax, -4095i32).unwrap();
    asm.jae(failed).unwrap();
    asm.mov(qword_ptr(rax), rdx).unwrap();
    asm.mov(qword_ptr(rax + 8i32), rcx).unwrap();
    asm.add(rax, 16i32).unwrap();
    asm.ret().unwrap();

    asm.set_label(&mut overflow_two).unwrap();
    asm.pop(rax).unwrap();
    asm.pop(rax).unwrap();
    asm.jmp(failed).unwrap();
    asm.set_label(&mut overflow_one).unwrap();
    asm.pop(rax).unwrap();
    asm.set_label(&mut failed).unwrap();
    asm.xor(eax, eax).unwrap();
    asm.ret().unwrap();
    finish(asm)
}

fn generate_memcpy() -> Result<Vec<u8>, String> {
    let mut asm = assembler()?;
    asm.mov(rax, rdi).unwrap();
    asm.mov(rcx, rdx).unwrap();
    asm.cld().unwrap();
    asm.rep().movsb().unwrap();
    asm.ret().unwrap();
    finish(asm)
}

fn generate_memmove() -> Result<Vec<u8>, String> {
    let mut asm = assembler()?;
    let mut forward = asm.create_label();
    let mut done = asm.create_label();
    asm.mov(rax, rdi).unwrap();
    asm.test(rdx, rdx).unwrap();
    asm.je(done).unwrap();
    asm.cmp(rdi, rsi).unwrap();
    asm.jbe(forward).unwrap();
    asm.lea(r8, qword_ptr(rsi + rdx)).unwrap();
    asm.cmp(rdi, r8).unwrap();
    asm.jae(forward).unwrap();
    asm.lea(rsi, qword_ptr(rsi + rdx - 1i32)).unwrap();
    asm.lea(rdi, qword_ptr(rdi + rdx - 1i32)).unwrap();
    asm.mov(rcx, rdx).unwrap();
    asm.std().unwrap();
    asm.rep().movsb().unwrap();
    asm.cld().unwrap();
    asm.jmp(done).unwrap();
    asm.set_label(&mut forward).unwrap();
    asm.mov(rcx, rdx).unwrap();
    asm.cld().unwrap();
    asm.rep().movsb().unwrap();
    asm.set_label(&mut done).unwrap();
    asm.ret().unwrap();
    finish(asm)
}

fn generate_memset() -> Result<Vec<u8>, String> {
    let mut asm = assembler()?;
    asm.mov(r8, rdi).unwrap();
    asm.mov(eax, esi).unwrap();
    asm.mov(rcx, rdx).unwrap();
    asm.cld().unwrap();
    asm.rep().stosb().unwrap();
    asm.mov(rax, r8).unwrap();
    asm.ret().unwrap();
    finish(asm)
}

fn generate_memcmp() -> Result<Vec<u8>, String> {
    let mut asm = assembler()?;
    let mut loop_label = asm.create_label();
    let mut different = asm.create_label();
    let mut equal = asm.create_label();
    asm.test(rdx, rdx).unwrap();
    asm.je(equal).unwrap();
    asm.set_label(&mut loop_label).unwrap();
    asm.movzx(eax, byte_ptr(rdi)).unwrap();
    asm.movzx(ecx, byte_ptr(rsi)).unwrap();
    asm.cmp(eax, ecx).unwrap();
    asm.jne(different).unwrap();
    asm.inc(rdi).unwrap();
    asm.inc(rsi).unwrap();
    asm.dec(rdx).unwrap();
    asm.jne(loop_label).unwrap();
    asm.set_label(&mut equal).unwrap();
    asm.xor(eax, eax).unwrap();
    asm.ret().unwrap();
    asm.set_label(&mut different).unwrap();
    asm.sub(eax, ecx).unwrap();
    asm.ret().unwrap();
    finish(asm)
}

fn generate_strcpy() -> Result<Vec<u8>, String> {
    let mut asm = assembler()?;
    let mut copy = asm.create_label();
    asm.mov(rax, rdi).unwrap();
    asm.set_label(&mut copy).unwrap();
    asm.mov(cl, byte_ptr(rsi)).unwrap();
    asm.mov(byte_ptr(rdi), cl).unwrap();
    asm.inc(rsi).unwrap();
    asm.inc(rdi).unwrap();
    asm.test(cl, cl).unwrap();
    asm.jne(copy).unwrap();
    asm.ret().unwrap();
    finish(asm)
}

fn generate_strcat() -> Result<Vec<u8>, String> {
    let mut asm = assembler()?;
    let mut scan = asm.create_label();
    let mut copy = asm.create_label();
    asm.mov(rax, rdi).unwrap();
    asm.set_label(&mut scan).unwrap();
    asm.cmp(byte_ptr(rdi), 0i32).unwrap();
    asm.je(copy).unwrap();
    asm.inc(rdi).unwrap();
    asm.jmp(scan).unwrap();
    asm.set_label(&mut copy).unwrap();
    asm.mov(cl, byte_ptr(rsi)).unwrap();
    asm.mov(byte_ptr(rdi), cl).unwrap();
    asm.inc(rsi).unwrap();
    asm.inc(rdi).unwrap();
    asm.test(cl, cl).unwrap();
    asm.jne(copy).unwrap();
    asm.ret().unwrap();
    finish(asm)
}

fn emit_mmap(asm: &mut CodeAssembler) {
    // Input: rdi = requested mapping size.
    asm.mov(rsi, rdi).unwrap();
    asm.xor(edi, edi).unwrap(); // preferred address = NULL
    asm.mov(rdx, 3i64).unwrap(); // PROT_READ | PROT_WRITE
    asm.mov(r10, 0x22i64).unwrap(); // MAP_PRIVATE | MAP_ANONYMOUS
    asm.mov(r8, -1i64).unwrap();
    asm.xor(r9d, r9d).unwrap();
    asm.mov(eax, 9i32).unwrap();
    asm.syscall().unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_only_requested_runtime_functions() {
        let required = HashSet::from(["malloc".to_string(), "memcpy".to_string()]);
        let functions = generate_required(&required).expect("runtime generation");
        let names: Vec<_> = functions.iter().map(|function| function.name).collect();
        assert_eq!(names, ["malloc", "memcpy"]);
        assert!(functions.iter().all(|function| !function.bytes.is_empty()));
    }
}
