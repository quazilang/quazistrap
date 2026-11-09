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
        "malloc",
        "free",
        "calloc",
        "realloc",
        "memcpy",
        "memmove",
        "memset",
        "memcmp",
        "strcpy",
        "strcat",
        "__quazi_thread_spawn",
        "__quazi_thread_join",
        "__quazi_sigreturn",
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
            "__quazi_thread_spawn" => generate_thread_spawn(),
            "__quazi_thread_join" => generate_thread_join(),
            "__quazi_sigreturn" => generate_sigreturn(),
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

// ── Threads (Linux, no libc) ─────────────────────────────────────────────────
//
// Each thread owns one mmap'd region: a PROT_NONE guard page at the bottom,
// the stack, and a control block in the last 64 bytes. The thread handle is
// the control block address:
//   +0  u32 kernel thread id (CLONE_PARENT_SETTID; cleared and futex-woken by
//       the kernel when the thread exits, via CLONE_CHILD_CLEARTID)
//   +8  region base (for munmap)
// The child starts with rsp at [callback, argument] below the control block.

const THREAD_REGION_SIZE: i32 = 8 * 1024 * 1024;
const THREAD_CONTROL_OFFSET: i32 = THREAD_REGION_SIZE - 64;
/// CLONE_VM | FS | FILES | SIGHAND | THREAD | SYSVSEM | PARENT_SETTID | CHILD_CLEARTID
const THREAD_CLONE_FLAGS: i64 = 0x100 | 0x200 | 0x400 | 0x800 | 0x10000 | 0x40000 | 0x100000 | 0x200000;

/// `__quazi_thread_spawn(callback: fn(*u8) *u8, arg: *u8) usize`: start a
/// thread running `callback(arg)`; returns its handle, or 0 on failure.
fn generate_thread_spawn() -> Result<Vec<u8>, String> {
    let mut asm = assembler()?;
    let mut map_failed = asm.create_label();
    let mut clone_failed = asm.create_label();
    let mut child = asm.create_label();
    asm.push(rbx).unwrap();
    asm.push(r12).unwrap();
    asm.push(r13).unwrap();
    asm.mov(r12, rdi).unwrap(); // callback
    asm.mov(r13, rsi).unwrap(); // argument
    // mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_STACK, -1, 0)
    asm.xor(edi, edi).unwrap();
    asm.mov(esi, THREAD_REGION_SIZE).unwrap();
    asm.mov(edx, 3i32).unwrap();
    asm.mov(r10d, 0x20022i32).unwrap();
    asm.mov(r8, -1i64).unwrap();
    asm.xor(r9d, r9d).unwrap();
    asm.mov(eax, 9i32).unwrap();
    asm.syscall().unwrap();
    asm.cmp(rax, -4095i32).unwrap();
    asm.jae(map_failed).unwrap();
    asm.mov(rbx, rax).unwrap(); // region base
    // Guard page: mprotect(base, 4096, PROT_NONE). Best effort.
    asm.mov(rdi, rbx).unwrap();
    asm.mov(esi, 4096i32).unwrap();
    asm.xor(edx, edx).unwrap();
    asm.mov(eax, 10i32).unwrap();
    asm.syscall().unwrap();
    // Control block and the child's initial stack.
    asm.lea(rcx, qword_ptr(rbx + THREAD_CONTROL_OFFSET)).unwrap();
    asm.mov(qword_ptr(rcx), 0i32).unwrap();
    asm.mov(qword_ptr(rcx + 8i32), rbx).unwrap();
    asm.mov(qword_ptr(rcx - 16i32), r12).unwrap();
    asm.mov(qword_ptr(rcx - 8i32), r13).unwrap();
    // clone(flags, child_stack, &tid, &tid, 0)
    asm.mov(rdi, THREAD_CLONE_FLAGS).unwrap();
    asm.lea(rsi, qword_ptr(rcx - 16i32)).unwrap();
    asm.mov(rdx, rcx).unwrap();
    asm.mov(r10, rcx).unwrap();
    asm.xor(r8d, r8d).unwrap();
    asm.mov(eax, 56i32).unwrap();
    asm.syscall().unwrap();
    asm.test(rax, rax).unwrap();
    asm.je(child).unwrap();
    asm.js(clone_failed).unwrap();
    asm.lea(rax, qword_ptr(rbx + THREAD_CONTROL_OFFSET)).unwrap();
    asm.pop(r13).unwrap();
    asm.pop(r12).unwrap();
    asm.pop(rbx).unwrap();
    asm.ret().unwrap();

    asm.set_label(&mut clone_failed).unwrap();
    asm.mov(rdi, rbx).unwrap();
    asm.mov(esi, THREAD_REGION_SIZE).unwrap();
    asm.mov(eax, 11i32).unwrap(); // munmap
    asm.syscall().unwrap();
    asm.set_label(&mut map_failed).unwrap();
    asm.xor(eax, eax).unwrap();
    asm.pop(r13).unwrap();
    asm.pop(r12).unwrap();
    asm.pop(rbx).unwrap();
    asm.ret().unwrap();

    // Child: rsp is 16-byte aligned at [callback, argument].
    asm.set_label(&mut child).unwrap();
    asm.xor(ebp, ebp).unwrap();
    asm.mov(rdi, qword_ptr(rsp + 8i32)).unwrap();
    asm.mov(rax, qword_ptr(rsp)).unwrap();
    asm.call(rax).unwrap();
    // exit(0) ends only this thread; the kernel then clears the tid word.
    asm.xor(edi, edi).unwrap();
    asm.mov(eax, 60i32).unwrap();
    asm.syscall().unwrap();
    asm.ud2().unwrap();
    finish(asm)
}

/// `__quazi_thread_join(handle: usize)`: wait until the thread has exited,
/// then release its stack. A zero handle is a no-op.
fn generate_thread_join() -> Result<Vec<u8>, String> {
    let mut asm = assembler()?;
    let mut done = asm.create_label();
    let mut wait = asm.create_label();
    let mut exited = asm.create_label();
    asm.test(rdi, rdi).unwrap();
    asm.je(done).unwrap();
    asm.push(rbx).unwrap();
    asm.mov(rbx, rdi).unwrap();
    asm.set_label(&mut wait).unwrap();
    asm.mov(edx, dword_ptr(rbx)).unwrap();
    asm.test(edx, edx).unwrap();
    asm.je(exited).unwrap();
    // futex(&tid, FUTEX_WAIT, tid, NULL): returns at once if tid changed.
    asm.mov(rdi, rbx).unwrap();
    asm.xor(esi, esi).unwrap();
    asm.xor(r10d, r10d).unwrap();
    asm.xor(r8d, r8d).unwrap();
    asm.xor(r9d, r9d).unwrap();
    asm.mov(eax, 202i32).unwrap();
    asm.syscall().unwrap();
    asm.jmp(wait).unwrap();
    asm.set_label(&mut exited).unwrap();
    asm.mov(rdi, qword_ptr(rbx + 8i32)).unwrap();
    asm.mov(esi, THREAD_REGION_SIZE).unwrap();
    asm.mov(eax, 11i32).unwrap(); // munmap
    asm.syscall().unwrap();
    asm.pop(rbx).unwrap();
    asm.set_label(&mut done).unwrap();
    asm.ret().unwrap();
    finish(asm)
}

/// `__quazi_sigreturn`: signal-handler restorer (`rt_sigreturn`), installed
/// by `quazi.signal.install`.
fn generate_sigreturn() -> Result<Vec<u8>, String> {
    let mut asm = assembler()?;
    asm.mov(eax, 15i32).unwrap();
    asm.syscall().unwrap();
    asm.ud2().unwrap();
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
