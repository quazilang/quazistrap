// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

// Entry-point stubs with crash handlers.
//
// Linux layout (fn_offset = start of stub in .text):
//   0:   __quazi_sigreturn        (7 B)   — restorer for rt_sigaction
//   7:   __quazi_crash_handler    (241 B) — SA_SIGINFO handler: signal/addr/RIP hex + backtrace
//   248: __quazi_print_backtrace  (192 B) — RBP chain walk (max 16 frames)
//   440: _start                  (242 B) — sigaction setup, QUAZI_TRACE=1 env check, calls main
//
// Windows layout:
//   0:  __quazi_crash_handler_win (69 B) — unhandled-exception filter: WriteFile + ExitProcess
//   69: mainCRTStartup           (32 B) — SetUnhandledExceptionFilter + call main

use super::relocations::{PendingReloc, RelocKind};

pub struct StartStub {
    pub bytes: Vec<u8>,
    pub relocs: Vec<PendingReloc>,
    /// Internal symbols to define: (name, offset_in_stub, size).
    pub extra_symbols: Vec<(String, usize, usize)>,
    /// Offset of the executable entry point within `bytes`.
    pub start_offset: usize,
}

impl StartStub {
    pub fn generate(fn_offset: usize, no_crash: bool, main_takes_args: bool) -> Self {
        if no_crash {
            Self::generate_minimal_linux(fn_offset, main_takes_args)
        } else {
            Self::generate_full_linux(fn_offset, main_takes_args)
        }
    }

    pub fn generate_windows(fn_offset: usize, no_crash: bool, main_takes_args: bool) -> Self {
        if no_crash {
            Self::generate_minimal_windows(fn_offset, main_takes_args)
        } else {
            Self::generate_full_windows(fn_offset, main_takes_args)
        }
    }

    /// Linux _start stub with SIGSEGV/SIGABRT/SIGFPE/SIGBUS crash handler,
    /// backtrace printer, and QUAZI_TRACE=1 environment detection.
    /// Rewritten with iced-x86 so every relocation offset is exact.
    fn generate_full_linux(fn_offset: usize, main_takes_args: bool) -> Self {
        use iced_x86::code_asm::*;

        let mut asm = CodeAssembler::new(64).expect("asm");
        let fn_start = asm.create_label();
        let mut pending: Vec<(usize, usize, RelocKind, String, i64)> = Vec::new();

        macro_rules! emit {
            ($e:expr) => {
                $e.expect("emit")
            };
        }

        macro_rules! call_ext {
            ($sym:expr, $kind:expr) => {{
                let idx = asm.instructions().len();
                emit!(asm.call(fn_start));
                pending.push((idx, 1, $kind, $sym.into(), -4));
            }};
        }

        macro_rules! lea_rip {
            ($reg:expr, $sym:expr) => {{
                let idx = asm.instructions().len();
                emit!(asm.lea($reg, qword_ptr(fn_start)));
                pending.push((idx, 3, RelocKind::Pc32, $sym.into(), -4));
            }};
        }

        let mut print_bt_label = asm.create_label();

        // ═══════════════════════════════════════════════════════
        // __quazi_sigreturn
        // ═══════════════════════════════════════════════════════
        let sigreturn_idx = asm.instructions().len();
        emit!(asm.mov(rax, 15i64)); // sys_rt_sigreturn
        emit!(asm.syscall());

        // ═══════════════════════════════════════════════════════
        // __quazi_crash_handler  (SA_SIGINFO handler)
        // ═══════════════════════════════════════════════════════
        let crash_handler_idx = asm.instructions().len();

        emit!(asm.push(rbx));
        emit!(asm.push(r12));
        emit!(asm.push(r13));
        emit!(asm.push(r14));
        emit!(asm.push(r15));

        // rdi = sig, rsi = siginfo_t*, rdx = ucontext_t*
        emit!(asm.mov(r12, rdi)); // sig number
        emit!(asm.mov(r13, rsi)); // info pointer
        emit!(asm.mov(r14, rdx)); // ucontext pointer

        // write(2, "== CRASHED ==\n", 14)
        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_crash_header");
        emit!(asm.mov(rdx, 14i64));
        emit!(asm.syscall());

        // ── "fatal: signal 0x" + 8 hex digits ──
        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_fatal_sig");
        emit!(asm.mov(rdx, 16i64));
        emit!(asm.syscall());

        emit!(asm.mov(rbx, r12));
        emit!(asm.mov(ecx, 8i32));
        lea_rip!(r8, "__quazi_hex_buf");
        emit!(asm.add(r8, 7i32));
        let mut h1 = asm.create_label();
        let mut s1 = asm.create_label();
        emit!(asm.set_label(&mut h1));
        emit!(asm.mov(rax, rbx));
        emit!(asm.and(rax, 0x0Fi32));
        emit!(asm.cmp(al, 10i32));
        emit!(asm.jb(s1));
        emit!(asm.add(al, 7i32));
        emit!(asm.set_label(&mut s1));
        emit!(asm.add(al, '0' as i32));
        emit!(asm.mov(byte_ptr(r8), al));
        emit!(asm.dec(r8));
        emit!(asm.shr(rbx, 4i32));
        emit!(asm.dec(ecx));
        emit!(asm.jne(h1));

        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_hex_buf");
        emit!(asm.mov(rdx, 8i64));
        emit!(asm.syscall());

        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_nl");
        emit!(asm.mov(rdx, 1i64));
        emit!(asm.syscall());

        // ── "fault: 0x" + 16 hex digits ──
        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_fault");
        emit!(asm.mov(rdx, 9i64));
        emit!(asm.syscall());

        // si_addr is at offset 0x10 in siginfo_t
        emit!(asm.mov(rbx, qword_ptr(r13 + 0x10)));
        emit!(asm.mov(ecx, 16i32));
        lea_rip!(r8, "__quazi_hex_buf");
        emit!(asm.add(r8, 15i32));
        let mut h2 = asm.create_label();
        let mut s2 = asm.create_label();
        emit!(asm.set_label(&mut h2));
        emit!(asm.mov(rax, rbx));
        emit!(asm.and(rax, 0x0Fi32));
        emit!(asm.cmp(al, 10i32));
        emit!(asm.jb(s2));
        emit!(asm.add(al, 7i32));
        emit!(asm.set_label(&mut s2));
        emit!(asm.add(al, '0' as i32));
        emit!(asm.mov(byte_ptr(r8), al));
        emit!(asm.dec(r8));
        emit!(asm.shr(rbx, 4i32));
        emit!(asm.dec(ecx));
        emit!(asm.jne(h2));

        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_hex_buf");
        emit!(asm.mov(rdx, 16i64));
        emit!(asm.syscall());

        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_nl");
        emit!(asm.mov(rdx, 1i64));
        emit!(asm.syscall());

        // ── "rip: 0x" + 16 hex digits ──
        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_rip");
        emit!(asm.mov(rdx, 7i64));
        emit!(asm.syscall());

        // RIP is at offset 0xA8 in ucontext_t on x86_64 Linux
        emit!(asm.mov(rbx, qword_ptr(r14 + 0xA8)));
        emit!(asm.mov(ecx, 16i32));
        lea_rip!(r8, "__quazi_hex_buf");
        emit!(asm.add(r8, 15i32));
        let mut h3 = asm.create_label();
        let mut s3 = asm.create_label();
        emit!(asm.set_label(&mut h3));
        emit!(asm.mov(rax, rbx));
        emit!(asm.and(rax, 0x0Fi32));
        emit!(asm.cmp(al, 10i32));
        emit!(asm.jb(s3));
        emit!(asm.add(al, 7i32));
        emit!(asm.set_label(&mut s3));
        emit!(asm.add(al, '0' as i32));
        emit!(asm.mov(byte_ptr(r8), al));
        emit!(asm.dec(r8));
        emit!(asm.shr(rbx, 4i32));
        emit!(asm.dec(ecx));
        emit!(asm.jne(h3));

        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_hex_buf");
        emit!(asm.mov(rdx, 16i64));
        emit!(asm.syscall());

        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_nl");
        emit!(asm.mov(rdx, 1i64));
        emit!(asm.syscall());

        // ── Backtrace or hint ──
        let mut bt_done_c = asm.create_label();
        lea_rip!(rax, "__quazi_trace_enabled");
        emit!(asm.cmp(byte_ptr(rax), 0i32));
        emit!(asm.je(bt_done_c));
        call_ext!("__quazi_print_backtrace", RelocKind::Plt32);
        emit!(asm.set_label(&mut bt_done_c));

        // Print hint when backtrace was skipped
        lea_rip!(rax, "__quazi_trace_enabled");
        emit!(asm.cmp(byte_ptr(rax), 0i32));
        let mut skip_hint = asm.create_label();
        emit!(asm.jne(skip_hint));
        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_trace_hint");
        emit!(asm.mov(rdx, 41i64));
        emit!(asm.syscall());
        emit!(asm.set_label(&mut skip_hint));

        // exit_group(101)
        emit!(asm.mov(rdi, 101i64));
        emit!(asm.mov(rax, 231i64));
        emit!(asm.syscall());

        // Epilogue (unreachable)
        emit!(asm.pop(r15));
        emit!(asm.pop(r14));
        emit!(asm.pop(r13));
        emit!(asm.pop(r12));
        emit!(asm.pop(rbx));
        emit!(asm.ret());

        let crash_handler_end_idx = asm.instructions().len();

        // ═══════════════════════════════════════════════════════
        // __quazi_print_backtrace
        // ═══════════════════════════════════════════════════════
        let print_bt_idx = asm.instructions().len();
        emit!(asm.set_label(&mut print_bt_label));

        emit!(asm.push(rbx));
        emit!(asm.push(r12));
        emit!(asm.push(r13));
        emit!(asm.push(r14));
        emit!(asm.push(r15));

        let mut bt_done = asm.create_label();
        lea_rip!(rax, "__quazi_trace_enabled");
        emit!(asm.cmp(byte_ptr(rax), 0i32));
        emit!(asm.je(bt_done));

        // write(2, "stack backtrace:\n", 17)
        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_bt_prefix");
        emit!(asm.mov(rdx, 17i64));
        emit!(asm.syscall());

        emit!(asm.mov(r12, rbp));
        emit!(asm.mov(r13, 16i64));

        let mut bt_loop = asm.create_label();
        emit!(asm.set_label(&mut bt_loop));
        emit!(asm.test(r12, r12));
        emit!(asm.je(bt_done));
        emit!(asm.test(r13, r13));
        emit!(asm.je(bt_done));

        // write(2, "  at 0x", 7)
        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_at_0x");
        emit!(asm.mov(rdx, 7i64));
        emit!(asm.syscall());

        // format [r12+8] as 16 hex digits
        emit!(asm.mov(rbx, qword_ptr(r12 + 8)));
        emit!(asm.mov(ecx, 16i32));
        lea_rip!(r8, "__quazi_hex_buf");
        emit!(asm.add(r8, 15i32));

        let mut hbt = asm.create_label();
        let mut sbt = asm.create_label();
        emit!(asm.set_label(&mut hbt));
        emit!(asm.mov(rax, rbx));
        emit!(asm.and(rax, 0x0Fi32));
        emit!(asm.cmp(al, 10i32));
        emit!(asm.jb(sbt));
        emit!(asm.add(al, 7i32));
        emit!(asm.set_label(&mut sbt));
        emit!(asm.add(al, '0' as i32));
        emit!(asm.mov(byte_ptr(r8), al));
        emit!(asm.dec(r8));
        emit!(asm.shr(rbx, 4i32));
        emit!(asm.dec(ecx));
        emit!(asm.jne(hbt));

        // write(2, hex_buf, 16)
        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_hex_buf");
        emit!(asm.mov(rdx, 16i64));
        emit!(asm.syscall());

        // write(2, "\n", 1)
        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_nl");
        emit!(asm.mov(rdx, 1i64));
        emit!(asm.syscall());

        emit!(asm.mov(r12, qword_ptr(r12)));
        emit!(asm.dec(r13));
        emit!(asm.jmp(bt_loop));

        emit!(asm.set_label(&mut bt_done));
        emit!(asm.pop(r15));
        emit!(asm.pop(r14));
        emit!(asm.pop(r13));
        emit!(asm.pop(r12));
        emit!(asm.pop(rbx));
        emit!(asm.ret());

        let print_bt_end_idx = asm.instructions().len();

        // ═══════════════════════════════════════════════════════
        // _start
        // ═══════════════════════════════════════════════════════
        let startup_idx = asm.instructions().len();

        emit!(asm.xor(ebp, ebp));
        emit!(asm.mov(rbx, rsp)); // preserve the kernel-provided initial stack

        // Allocate 672 bytes on stack (sigaction struct + environ read buffer)
        emit!(asm.sub(rsp, 0x40i32));

        // Zero sa_mask (offset 24) — the rest is overwritten immediately
        emit!(asm.xor(eax, eax));
        emit!(asm.mov(qword_ptr(rsp + 24), rax));

        // sa_sigaction = __quazi_crash_handler
        lea_rip!(rax, "__quazi_crash_handler");
        emit!(asm.mov(qword_ptr(rsp), rax));
        // sa_flags = SA_SIGINFO | SA_RESTORER (0x04000004)
        emit!(asm.mov(dword_ptr(rsp + 8), 0x04000004u32 as i32));
        // sa_restorer = __quazi_sigreturn
        lea_rip!(rax, "__quazi_sigreturn");
        emit!(asm.mov(qword_ptr(rsp + 16), rax));

        // rt_sigaction(signum, &act, NULL, 8)
        emit!(asm.mov(rsi, rsp));
        emit!(asm.xor(rdx, rdx));
        emit!(asm.mov(r10, 8i64));
        emit!(asm.mov(rax, 13i64));

        emit!(asm.mov(rdi, 11i64)); // SIGSEGV
        emit!(asm.syscall());
        emit!(asm.mov(rdi, 6i64)); // SIGABRT
        emit!(asm.syscall());
        emit!(asm.mov(rdi, 8i64)); // SIGFPE
        emit!(asm.syscall());
        emit!(asm.mov(rdi, 7i64)); // SIGBUS
        emit!(asm.syscall());

        // Locate envp directly on the kernel-provided initial stack. This keeps
        // the entry path independent of libc and a dynamic loader.
        let mut no_trace = asm.create_label();
        let mut env_loop = asm.create_label();
        let mut next_env = asm.create_label();
        emit!(asm.mov(rcx, qword_ptr(rbx))); // argc
        emit!(asm.lea(rsi, qword_ptr(rbx + rcx * 8 + 16i32))); // envp
        lea_rip!(rax, "__quazi_envp");
        emit!(asm.mov(qword_ptr(rax), rsi));
        emit!(asm.set_label(&mut env_loop));
        emit!(asm.mov(rax, qword_ptr(rsi)));
        emit!(asm.test(rax, rax));
        emit!(asm.je(no_trace));
        emit!(asm.mov(rdx, 0x5254_5f49_5a41_5551u64)); // "QUAZI_TR"
        emit!(asm.cmp(qword_ptr(rax), rdx));
        emit!(asm.jne(next_env));
        emit!(asm.cmp(dword_ptr(rax + 8i32), 0x3d45_4341u32 as i32)); // "ACE="
        emit!(asm.jne(next_env));
        emit!(asm.cmp(word_ptr(rax + 12i32), 0x0031i32)); // "1\0"
        emit!(asm.jne(next_env));
        lea_rip!(rax, "__quazi_trace_enabled");
        emit!(asm.mov(byte_ptr(rax), 1i32));
        emit!(asm.jmp(no_trace));
        emit!(asm.set_label(&mut next_env));
        emit!(asm.add(rsi, 8i32));
        emit!(asm.jmp(env_loop));
        emit!(asm.set_label(&mut no_trace));
        emit!(asm.add(rsp, 0x40i32));

        // Build `Array[str] args` if `fn main(args: Array[str])` was declared.
        if main_takes_args {
            // At Linux entry: [rsp] = argc, [rsp+8] = argv[0], ...
            emit!(asm.mov(r12, qword_ptr(rsp)));
            emit!(asm.lea(r13, qword_ptr(rsp + 8i32))); // argv = &argv[0]

            // Allocate the Array struct on the stack (24 bytes).
            emit!(asm.sub(rsp, 24i32));

            // Allocate the pointer array: Array[str] stores one 8-byte pointer per element.
            emit!(asm.mov(rdi, r12));
            emit!(asm.shl(rdi, 3i32));
            let mut args_malloc = asm.create_label();
            let mut args_malloc_done = asm.create_label();
            emit!(asm.test(rdi, rdi));
            emit!(asm.jne(args_malloc));
            emit!(asm.xor(eax, eax));
            emit!(asm.jmp(args_malloc_done));
            emit!(asm.set_label(&mut args_malloc));
            call_ext!("malloc", RelocKind::Plt32);
            emit!(asm.set_label(&mut args_malloc_done));

            emit!(asm.mov(qword_ptr(rsp), rax)); // ptr
            emit!(asm.mov(qword_ptr(rsp + 8i32), r12)); // len
            emit!(asm.mov(qword_ptr(rsp + 16i32), r12)); // cap

            // Copy argv[i] pointers into the array.
            let mut args_loop = asm.create_label();
            let mut args_loop_end = asm.create_label();
            emit!(asm.xor(r14, r14));
            emit!(asm.set_label(&mut args_loop));
            emit!(asm.cmp(r14, r12));
            emit!(asm.jae(args_loop_end));
            emit!(asm.mov(r15, qword_ptr(rsp))); // array base
            emit!(asm.mov(rdi, qword_ptr(r13 + r14 * 8))); // argv[i]
            emit!(asm.mov(qword_ptr(r15 + r14 * 8), rdi)); // array[i]
            emit!(asm.inc(r14));
            emit!(asm.jmp(args_loop));
            emit!(asm.set_label(&mut args_loop_end));

            // Pass pointer to the stack-allocated Array struct in rdi.
            emit!(asm.mov(rdi, rsp));
        }

        call_ext!("main", RelocKind::Plt32);

        if main_takes_args {
            // Pop the stack-allocated Array struct.
            emit!(asm.add(rsp, 24i32));
        }

        emit!(asm.mov(rdi, rax));
        emit!(asm.mov(rax, 60i64));
        emit!(asm.syscall());

        // ── Assemble and compute offsets ──
        let mut bytes = asm.assemble(0).expect("assemble");

        let offsets: Vec<usize> = {
            let mut out = Vec::with_capacity(asm.instructions().len());
            let mut dec = iced_x86::Decoder::with_ip(64, &bytes, 0, iced_x86::DecoderOptions::NONE);
            let mut tmp = iced_x86::Instruction::default();
            while dec.can_decode() {
                out.push(dec.ip() as usize);
                dec.decode_out(&mut tmp);
            }
            out
        };

        // Build relocs
        let mut relocs = Vec::with_capacity(pending.len());
        for (asm_idx, disp_off, kind, sym, addend) in pending {
            let byte_off = offsets[asm_idx];
            let field = byte_off + disp_off;
            bytes[field..field + 4].fill(0);
            relocs.push(PendingReloc {
                offset_in_text: fn_offset + field,
                kind,
                symbol: sym,
                addend,
            });
        }

        // Compute function boundaries
        let sigreturn_off = offsets[sigreturn_idx];
        let sigreturn_size = offsets[crash_handler_idx] - sigreturn_off;
        let crash_handler_off = offsets[crash_handler_idx];
        let crash_handler_size = offsets[crash_handler_end_idx] - crash_handler_off;
        let print_bt_off = offsets[print_bt_idx];
        let print_bt_size = offsets[print_bt_end_idx] - print_bt_off;
        let startup_off = offsets[startup_idx];

        Self {
            bytes,
            relocs,
            extra_symbols: vec![
                ("__quazi_sigreturn".into(), sigreturn_off, sigreturn_size),
                (
                    "__quazi_crash_handler".into(),
                    crash_handler_off,
                    crash_handler_size,
                ),
                (
                    "__quazi_print_backtrace".into(),
                    print_bt_off,
                    print_bt_size,
                ),
            ],
            start_offset: startup_off,
        }
    }

    /// Minimal Linux stub without crash-handler registration.
    /// Keeps __quazi_print_backtrace for panic backtraces.
    fn generate_minimal_linux(fn_offset: usize, main_takes_args: bool) -> Self {
        use iced_x86::code_asm::*;

        let mut asm = CodeAssembler::new(64).expect("asm");
        let fn_start = asm.create_label();
        let mut pending: Vec<(usize, usize, RelocKind, String, i64)> = Vec::new();

        macro_rules! emit {
            ($e:expr) => {
                $e.expect("emit")
            };
        }

        macro_rules! call_ext {
            ($sym:expr, $kind:expr) => {{
                let idx = asm.instructions().len();
                emit!(asm.call(fn_start));
                pending.push((idx, 1, $kind, $sym.into(), -4));
            }};
        }

        macro_rules! lea_rip {
            ($reg:expr, $sym:expr) => {{
                let idx = asm.instructions().len();
                emit!(asm.lea($reg, qword_ptr(fn_start)));
                pending.push((idx, 3, RelocKind::Pc32, $sym.into(), -4));
            }};
        }

        let mut print_bt_label = asm.create_label();

        // ═══════════════════════════════════════════════════════
        // __quazi_print_backtrace (Linux: write syscalls)
        // ═══════════════════════════════════════════════════════
        let print_bt_idx = asm.instructions().len();
        emit!(asm.set_label(&mut print_bt_label));

        emit!(asm.push(rbx));
        emit!(asm.push(r12));
        emit!(asm.push(r13));
        emit!(asm.push(r14));
        emit!(asm.push(r15));

        let mut bt_done = asm.create_label();
        lea_rip!(rax, "__quazi_trace_enabled");
        emit!(asm.cmp(byte_ptr(rax), 0i32));
        emit!(asm.je(bt_done));

        // write(2, "stack backtrace:\n", 17)
        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_bt_prefix");
        emit!(asm.mov(rdx, 17i64));
        emit!(asm.syscall());

        emit!(asm.mov(r12, rbp));
        emit!(asm.mov(r13, 16i64));

        let mut bt_loop = asm.create_label();
        emit!(asm.set_label(&mut bt_loop));
        emit!(asm.test(r12, r12));
        emit!(asm.je(bt_done));
        emit!(asm.test(r13, r13));
        emit!(asm.je(bt_done));

        // write(2, "  at 0x", 7)
        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_at_0x");
        emit!(asm.mov(rdx, 7i64));
        emit!(asm.syscall());

        // format [r12+8] as 16 hex digits
        emit!(asm.mov(rbx, qword_ptr(r12 + 8)));
        emit!(asm.mov(ecx, 16i32));
        lea_rip!(r8, "__quazi_hex_buf");
        emit!(asm.add(r8, 15i32));

        let mut hbt = asm.create_label();
        let mut sbt = asm.create_label();
        emit!(asm.set_label(&mut hbt));
        emit!(asm.mov(rax, rbx));
        emit!(asm.and(rax, 0x0Fi32));
        emit!(asm.cmp(al, 10i32));
        emit!(asm.jb(sbt));
        emit!(asm.add(al, 7i32));
        emit!(asm.set_label(&mut sbt));
        emit!(asm.add(al, '0' as i32));
        emit!(asm.mov(byte_ptr(r8), al));
        emit!(asm.dec(r8));
        emit!(asm.shr(rbx, 4i32));
        emit!(asm.dec(ecx));
        emit!(asm.jne(hbt));

        // write(2, hex_buf, 16)
        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_hex_buf");
        emit!(asm.mov(rdx, 16i64));
        emit!(asm.syscall());

        // write(2, "\n", 1)
        emit!(asm.mov(rax, 1i64));
        emit!(asm.mov(rdi, 2i64));
        lea_rip!(rsi, "__quazi_nl");
        emit!(asm.mov(rdx, 1i64));
        emit!(asm.syscall());

        emit!(asm.mov(r12, qword_ptr(r12)));
        emit!(asm.dec(r13));
        emit!(asm.jmp(bt_loop));

        emit!(asm.set_label(&mut bt_done));
        emit!(asm.pop(r15));
        emit!(asm.pop(r14));
        emit!(asm.pop(r13));
        emit!(asm.pop(r12));
        emit!(asm.pop(rbx));
        emit!(asm.ret());

        let print_bt_end_idx = asm.instructions().len();

        // ═══════════════════════════════════════════════════════
        // _start (minimal)
        // ═══════════════════════════════════════════════════════
        let startup_idx = asm.instructions().len();

        emit!(asm.xor(ebp, ebp));

        emit!(asm.mov(rbx, rsp));
        emit!(asm.mov(rcx, qword_ptr(rbx)));
        emit!(asm.lea(rsi, qword_ptr(rbx + rcx * 8 + 16i32)));
        lea_rip!(rax, "__quazi_envp");
        emit!(asm.mov(qword_ptr(rax), rsi));

        // open("/proc/self/environ", O_RDONLY)
        emit!(asm.mov(rax, 2i64));
        lea_rip!(rdi, "__quazi_proc_self_environ");
        emit!(asm.xor(rsi, rsi));
        emit!(asm.syscall());
        emit!(asm.test(eax, eax));
        let mut no_trace = asm.create_label();
        emit!(asm.js(no_trace));

        emit!(asm.mov(edi, eax));
        // read(fd, buf, 512)
        emit!(asm.mov(rax, 0i64));
        emit!(asm.lea(rsi, qword_ptr(rsp)));
        emit!(asm.mov(rdx, 512i64));
        emit!(asm.syscall());

        emit!(asm.mov(rcx, rax));
        emit!(asm.lea(rsi, qword_ptr(rsp)));

        let mut search = asm.create_label();
        let mut next = asm.create_label();
        emit!(asm.set_label(&mut search));
        emit!(asm.cmp(rcx, 12i32));
        emit!(asm.jb(no_trace));

        emit!(asm.mov(edx, dword_ptr(rsi)));
        emit!(asm.cmp(edx, 0x44494F56u32 as i32)); // 'VOID'
        emit!(asm.jne(next));

        emit!(asm.mov(edx, dword_ptr(rsi + 4)));
        emit!(asm.cmp(edx, 0x4152545Fu32 as i32)); // '_TRA'
        emit!(asm.jne(next));

        emit!(asm.mov(dx, word_ptr(rsi + 8)));
        emit!(asm.cmp(dx, 0x4543u16 as i32)); // 'CE'
        emit!(asm.jne(next));

        emit!(asm.mov(dl, byte_ptr(rsi + 10)));
        emit!(asm.cmp(dl, '=' as i32));
        emit!(asm.jne(next));

        emit!(asm.mov(dl, byte_ptr(rsi + 11)));
        emit!(asm.cmp(dl, '1' as i32));
        emit!(asm.jne(next));

        lea_rip!(rax, "__quazi_trace_enabled");
        emit!(asm.mov(byte_ptr(rax), 1i32));
        emit!(asm.jmp(no_trace));

        emit!(asm.set_label(&mut next));
        emit!(asm.inc(rsi));
        emit!(asm.dec(rcx));
        emit!(asm.jmp(search));

        emit!(asm.set_label(&mut no_trace));

        if main_takes_args {
            emit!(asm.mov(r12, qword_ptr(rsp)));
            emit!(asm.lea(r13, qword_ptr(rsp + 8i32))); // argv = &argv[0]
            emit!(asm.sub(rsp, 24i32));

            emit!(asm.mov(rdi, r12));
            emit!(asm.shl(rdi, 3i32));
            let mut args_malloc = asm.create_label();
            let mut args_malloc_done = asm.create_label();
            emit!(asm.test(rdi, rdi));
            emit!(asm.jne(args_malloc));
            emit!(asm.xor(eax, eax));
            emit!(asm.jmp(args_malloc_done));
            emit!(asm.set_label(&mut args_malloc));
            call_ext!("malloc", RelocKind::Plt32);
            emit!(asm.set_label(&mut args_malloc_done));

            emit!(asm.mov(qword_ptr(rsp), rax));
            emit!(asm.mov(qword_ptr(rsp + 8i32), r12));
            emit!(asm.mov(qword_ptr(rsp + 16i32), r12));

            let mut args_loop = asm.create_label();
            let mut args_loop_end = asm.create_label();
            emit!(asm.xor(r14, r14));
            emit!(asm.set_label(&mut args_loop));
            emit!(asm.cmp(r14, r12));
            emit!(asm.jae(args_loop_end));
            emit!(asm.mov(r15, qword_ptr(rsp)));
            emit!(asm.mov(rdi, qword_ptr(r13 + r14 * 8)));
            emit!(asm.mov(qword_ptr(r15 + r14 * 8), rdi));
            emit!(asm.inc(r14));
            emit!(asm.jmp(args_loop));
            emit!(asm.set_label(&mut args_loop_end));

            emit!(asm.mov(rdi, rsp));
        }

        call_ext!("main", RelocKind::Plt32);

        if main_takes_args {
            emit!(asm.add(rsp, 24i32));
        }

        emit!(asm.mov(rdi, rax));
        emit!(asm.mov(rax, 60i64));
        emit!(asm.syscall());

        // ── Assemble and compute offsets ──
        let mut bytes = asm.assemble(0).expect("assemble");

        let offsets: Vec<usize> = {
            let mut out = Vec::with_capacity(asm.instructions().len());
            let mut dec = iced_x86::Decoder::with_ip(64, &bytes, 0, iced_x86::DecoderOptions::NONE);
            let mut tmp = iced_x86::Instruction::default();
            while dec.can_decode() {
                out.push(dec.ip() as usize);
                dec.decode_out(&mut tmp);
            }
            out
        };

        // Build relocs
        let mut relocs = Vec::with_capacity(pending.len());
        for (asm_idx, disp_off, kind, sym, addend) in pending {
            let byte_off = offsets[asm_idx];
            let field = byte_off + disp_off;
            bytes[field..field + 4].fill(0);
            relocs.push(PendingReloc {
                offset_in_text: fn_offset + field,
                kind,
                symbol: sym,
                addend,
            });
        }

        // Compute function boundaries
        let print_bt_off = offsets[print_bt_idx];
        let print_bt_size = offsets[print_bt_end_idx] - print_bt_off;
        let startup_off = offsets[startup_idx];

        Self {
            bytes,
            relocs,
            extra_symbols: vec![(
                "__quazi_print_backtrace".into(),
                print_bt_off,
                print_bt_size,
            )],
            start_offset: startup_off,
        }
    }

    /// Windows mainCRTStartup stub with AddVectoredExceptionHandler crash handler.
    ///
    /// Uses iced-x86 CodeAssembler to build a larger, more informative stub:
    ///   __quazi_crash_handler_win  — prints exception code, fault address, RIP, and
    ///                                 a backtrace (from ContextRecord->Rbp) when
    ///                                 __quazi_trace_enabled is set.
    ///   __quazi_print_backtrace    — checks __quazi_trace_enabled, walks current RBP
    ///                                 chain, prints up to 16 frames.
    ///   mainCRTStartup            — sets exception handler, reads QUAZI_TRACE env var,
    ///                                 calls main.
    fn generate_full_windows(fn_offset: usize, main_takes_args: bool) -> Self {
        use iced_x86::code_asm::*;

        let mut asm = CodeAssembler::new(64).expect("asm");
        let mut fn_start = asm.create_label();
        let mut pending: Vec<(usize, usize, RelocKind, String, i64)> = Vec::new();

        macro_rules! emit {
            ($e:expr) => {
                $e.expect("emit")
            };
        }

        macro_rules! call_ext {
            ($sym:expr, $kind:expr) => {{
                let idx = asm.instructions().len();
                emit!(asm.call(fn_start));
                pending.push((idx, 1, $kind, $sym.into(), -4));
            }};
        }

        macro_rules! lea_rip {
            ($reg:expr, $sym:expr) => {{
                let idx = asm.instructions().len();
                emit!(asm.lea($reg, qword_ptr(fn_start)));
                pending.push((idx, 3, RelocKind::Pc32, $sym.into(), -4));
            }};
        }

        // ── Labels for intra-stub references ──
        let mut print_bt_label = asm.create_label();

        // ═══════════════════════════════════════════════════════
        // __quazi_crash_handler_win
        // ═══════════════════════════════════════════════════════
        let crash_handler_idx = asm.instructions().len();
        asm.set_label(&mut fn_start).expect("label");

        // Prologue
        emit!(asm.push(rbx));
        emit!(asm.push(r12));
        emit!(asm.push(r13));
        emit!(asm.push(r14));
        emit!(asm.push(r15));
        emit!(asm.sub(rsp, 40i32));

        // Save EXCEPTION_POINTERS* (rcx is caller-saved)
        emit!(asm.mov(r14, rcx));

        // Extract exception info
        emit!(asm.mov(rax, qword_ptr(r14))); // ExceptionRecord*
        emit!(asm.mov(ebx, dword_ptr(rax))); // ExceptionCode
        emit!(asm.mov(r12, qword_ptr(rax + 0x10))); // ExceptionAddress
        emit!(asm.mov(rax, qword_ptr(r14 + 8))); // ContextRecord*
        emit!(asm.mov(r13, qword_ptr(rax + 0xF8))); // Rip

        // Get stderr handle
        emit!(asm.mov(ecx, -12i32));
        call_ext!("GetStdHandle", RelocKind::Plt32);
        emit!(asm.mov(r15, rax));

        // "== CRASHED ==\n"
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_crash_header");
        emit!(asm.mov(r8d, 14i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // ── "fatal: exception 0x" + hex code ──
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_fatal_exc");
        emit!(asm.mov(r8d, 19i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // Format ebx as 8 hex digits
        emit!(asm.mov(rbx, rbx)); // zero-extended from ebx
        emit!(asm.mov(ecx, 8i32));
        lea_rip!(r8, "__quazi_hex_buf");
        emit!(asm.add(r8, 7i32));
        let mut h1 = asm.create_label();
        let mut s1 = asm.create_label();
        emit!(asm.set_label(&mut h1));
        emit!(asm.mov(rax, rbx));
        emit!(asm.and(rax, 0x0Fi32));
        emit!(asm.cmp(al, 10i32));
        emit!(asm.jb(s1));
        emit!(asm.add(al, 7i32));
        emit!(asm.set_label(&mut s1));
        emit!(asm.add(al, '0' as i32));
        emit!(asm.mov(byte_ptr(r8), al));
        emit!(asm.dec(r8));
        emit!(asm.shr(rbx, 4i32));
        emit!(asm.dec(ecx));
        emit!(asm.jne(h1));

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_hex_buf");
        emit!(asm.mov(r8d, 8i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_nl");
        emit!(asm.mov(r8d, 1i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // ── "fault: 0x" + hex address ──
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_fault");
        emit!(asm.mov(r8d, 9i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(rbx, r12));
        emit!(asm.mov(ecx, 16i32));
        lea_rip!(r8, "__quazi_hex_buf");
        emit!(asm.add(r8, 15i32));
        let mut h2 = asm.create_label();
        let mut s2 = asm.create_label();
        emit!(asm.set_label(&mut h2));
        emit!(asm.mov(rax, rbx));
        emit!(asm.and(rax, 0x0Fi32));
        emit!(asm.cmp(al, 10i32));
        emit!(asm.jb(s2));
        emit!(asm.add(al, 7i32));
        emit!(asm.set_label(&mut s2));
        emit!(asm.add(al, '0' as i32));
        emit!(asm.mov(byte_ptr(r8), al));
        emit!(asm.dec(r8));
        emit!(asm.shr(rbx, 4i32));
        emit!(asm.dec(ecx));
        emit!(asm.jne(h2));

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_hex_buf");
        emit!(asm.mov(r8d, 16i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_nl");
        emit!(asm.mov(r8d, 1i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // ── "rip: 0x" + hex RIP ──
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_rip");
        emit!(asm.mov(r8d, 7i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(rbx, r13));
        emit!(asm.mov(ecx, 16i32));
        lea_rip!(r8, "__quazi_hex_buf");
        emit!(asm.add(r8, 15i32));
        let mut h3 = asm.create_label();
        let mut s3 = asm.create_label();
        emit!(asm.set_label(&mut h3));
        emit!(asm.mov(rax, rbx));
        emit!(asm.and(rax, 0x0Fi32));
        emit!(asm.cmp(al, 10i32));
        emit!(asm.jb(s3));
        emit!(asm.add(al, 7i32));
        emit!(asm.set_label(&mut s3));
        emit!(asm.add(al, '0' as i32));
        emit!(asm.mov(byte_ptr(r8), al));
        emit!(asm.dec(r8));
        emit!(asm.shr(rbx, 4i32));
        emit!(asm.dec(ecx));
        emit!(asm.jne(h3));

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_hex_buf");
        emit!(asm.mov(r8d, 16i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_nl");
        emit!(asm.mov(r8d, 1i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // ── Backtrace from crash context (uses ContextRecord->Rbp) ──
        let mut bt_done_c = asm.create_label();
        lea_rip!(rax, "__quazi_trace_enabled");
        emit!(asm.cmp(byte_ptr(rax), 0i32));
        emit!(asm.je(bt_done_c));

        // Reload ContextRecord* and get Rbp
        emit!(asm.mov(rax, qword_ptr(r14 + 8)));
        emit!(asm.mov(r12, qword_ptr(rax + 0xA0)));

        // "stack backtrace:\n"
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_bt_prefix");
        emit!(asm.mov(r8d, 17i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(r13, 16i64));
        let mut bt_loop_c = asm.create_label();
        emit!(asm.set_label(&mut bt_loop_c));
        emit!(asm.test(r12, r12));
        emit!(asm.je(bt_done_c));
        emit!(asm.test(r13, r13));
        emit!(asm.je(bt_done_c));

        // "  at 0x"
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_at_0x");
        emit!(asm.mov(r8d, 7i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // return address
        emit!(asm.mov(rbx, qword_ptr(r12 + 8)));
        emit!(asm.mov(ecx, 16i32));
        lea_rip!(r8, "__quazi_hex_buf");
        emit!(asm.add(r8, 15i32));
        let mut hbt = asm.create_label();
        let mut sbt = asm.create_label();
        emit!(asm.set_label(&mut hbt));
        emit!(asm.mov(rax, rbx));
        emit!(asm.and(rax, 0x0Fi32));
        emit!(asm.cmp(al, 10i32));
        emit!(asm.jb(sbt));
        emit!(asm.add(al, 7i32));
        emit!(asm.set_label(&mut sbt));
        emit!(asm.add(al, '0' as i32));
        emit!(asm.mov(byte_ptr(r8), al));
        emit!(asm.dec(r8));
        emit!(asm.shr(rbx, 4i32));
        emit!(asm.dec(ecx));
        emit!(asm.jne(hbt));

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_hex_buf");
        emit!(asm.mov(r8d, 16i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_nl");
        emit!(asm.mov(r8d, 1i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(r12, qword_ptr(r12)));
        emit!(asm.dec(r13));
        emit!(asm.jmp(bt_loop_c));

        emit!(asm.set_label(&mut bt_done_c));

        // Print hint when backtrace was skipped
        lea_rip!(rax, "__quazi_trace_enabled");
        emit!(asm.cmp(byte_ptr(rax), 0i32));
        let mut skip_hint = asm.create_label();
        emit!(asm.jne(skip_hint));
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_trace_hint");
        emit!(asm.mov(r8d, 41i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);
        emit!(asm.set_label(&mut skip_hint));

        // ExitProcess(101)
        emit!(asm.mov(ecx, 101i32));
        call_ext!("ExitProcess", RelocKind::Plt32);

        // Epilogue (unreachable)
        emit!(asm.add(rsp, 40i32));
        emit!(asm.pop(r15));
        emit!(asm.pop(r14));
        emit!(asm.pop(r13));
        emit!(asm.pop(r12));
        emit!(asm.pop(rbx));
        emit!(asm.ret());

        let crash_handler_end_idx = asm.instructions().len();

        // ═══════════════════════════════════════════════════════
        // __quazi_print_backtrace
        // ═══════════════════════════════════════════════════════
        let print_bt_idx = asm.instructions().len();
        emit!(asm.set_label(&mut print_bt_label));

        emit!(asm.push(rbx));
        emit!(asm.push(r12));
        emit!(asm.push(r13));
        emit!(asm.push(r14));
        emit!(asm.push(r15));
        emit!(asm.sub(rsp, 40i32));

        let mut bt_done = asm.create_label();
        lea_rip!(rax, "__quazi_trace_enabled");
        emit!(asm.cmp(byte_ptr(rax), 0i32));
        emit!(asm.je(bt_done));

        // Get stderr handle
        emit!(asm.mov(ecx, -12i32));
        call_ext!("GetStdHandle", RelocKind::Plt32);
        emit!(asm.mov(r15, rax));

        // "stack backtrace:\n"
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_bt_prefix");
        emit!(asm.mov(r8d, 17i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // Walk current RBP chain
        emit!(asm.mov(r12, rbp));
        emit!(asm.mov(r13, 16i64));

        let mut bt_loop = asm.create_label();
        emit!(asm.set_label(&mut bt_loop));
        emit!(asm.test(r12, r12));
        emit!(asm.je(bt_done));
        emit!(asm.test(r13, r13));
        emit!(asm.je(bt_done));

        // "  at 0x"
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_at_0x");
        emit!(asm.mov(r8d, 7i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // return address
        emit!(asm.mov(rbx, qword_ptr(r12 + 8)));
        emit!(asm.mov(ecx, 16i32));
        lea_rip!(r8, "__quazi_hex_buf");
        emit!(asm.add(r8, 15i32));
        let mut hbt2 = asm.create_label();
        let mut sbt2 = asm.create_label();
        emit!(asm.set_label(&mut hbt2));
        emit!(asm.mov(rax, rbx));
        emit!(asm.and(rax, 0x0Fi32));
        emit!(asm.cmp(al, 10i32));
        emit!(asm.jb(sbt2));
        emit!(asm.add(al, 7i32));
        emit!(asm.set_label(&mut sbt2));
        emit!(asm.add(al, '0' as i32));
        emit!(asm.mov(byte_ptr(r8), al));
        emit!(asm.dec(r8));
        emit!(asm.shr(rbx, 4i32));
        emit!(asm.dec(ecx));
        emit!(asm.jne(hbt2));

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_hex_buf");
        emit!(asm.mov(r8d, 16i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_nl");
        emit!(asm.mov(r8d, 1i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(r12, qword_ptr(r12)));
        emit!(asm.dec(r13));
        emit!(asm.jmp(bt_loop));

        emit!(asm.set_label(&mut bt_done));
        emit!(asm.add(rsp, 40i32));
        emit!(asm.pop(r15));
        emit!(asm.pop(r14));
        emit!(asm.pop(r13));
        emit!(asm.pop(r12));
        emit!(asm.pop(rbx));
        emit!(asm.ret());

        let print_bt_end_idx = asm.instructions().len();

        // ═══════════════════════════════════════════════════════
        // mainCRTStartup
        // ═══════════════════════════════════════════════════════
        let startup_idx = asm.instructions().len();

        emit!(asm.sub(rsp, 40i32));

        // AddVectoredExceptionHandler(1, crash_handler_label)
        emit!(asm.mov(ecx, 1i32));
        emit!(asm.lea(rdx, qword_ptr(fn_start)));
        call_ext!("AddVectoredExceptionHandler", RelocKind::Plt32);

        // GetEnvironmentVariableA("QUAZI_TRACE", buf, 8)
        lea_rip!(rcx, "__quazi_env_var_name");
        emit!(asm.lea(rdx, qword_ptr(rsp)));
        emit!(asm.mov(r8d, 8i32));
        call_ext!("GetEnvironmentVariableA", RelocKind::Plt32);

        // If result > 0 and buf[0] == '1', set flag
        let mut no_env = asm.create_label();
        emit!(asm.test(eax, eax));
        emit!(asm.je(no_env));
        emit!(asm.cmp(byte_ptr(rsp), '1' as i32));
        emit!(asm.jne(no_env));
        lea_rip!(rax, "__quazi_trace_enabled");
        emit!(asm.mov(byte_ptr(rax), 1i32));
        emit!(asm.set_label(&mut no_env));

        // Build `Array[str] args` if `fn main(args: Array[str])` was declared.
        if main_takes_args {
            // Parse the Unicode command line with the public Win32 API. Using
            // __getmainargs here tied generated programs to a legacy msvcrt
            // import that is absent from modern UCRT import libraries.
            call_ext!("GetCommandLineW", RelocKind::Plt32);
            emit!(asm.mov(rcx, rax));
            emit!(asm.lea(rdx, qword_ptr(rsp + 8i32)));
            emit!(asm.mov(dword_ptr(rsp + 8i32), 0i32));
            call_ext!("CommandLineToArgvW", RelocKind::Plt32);
            emit!(asm.mov(r13, rax)); // UTF-16 argv
            emit!(asm.xor(r12, r12));
            let mut args_parsed = asm.create_label();
            emit!(asm.test(r13, r13));
            emit!(asm.je(args_parsed));
            emit!(asm.movsxd(r12, dword_ptr(rsp + 8i32))); // argc
            emit!(asm.set_label(&mut args_parsed));

            // Reserve Win64 shadow/stack-argument space, one scratch slot, and
            // the 24-byte Array value. The Array must not overlap shadow space.
            emit!(asm.sub(rsp, 96i32));

            // Allocate the Array[str] pointer storage.
            emit!(asm.mov(rcx, r12));
            emit!(asm.shl(rcx, 3i32));
            let mut args_malloc = asm.create_label();
            let mut args_malloc_done = asm.create_label();
            emit!(asm.test(rcx, rcx));
            emit!(asm.jne(args_malloc));
            emit!(asm.xor(eax, eax));
            emit!(asm.jmp(args_malloc_done));
            emit!(asm.set_label(&mut args_malloc));
            emit!(asm.mov(qword_ptr(rsp + 64i32), rcx));
            call_ext!("GetProcessHeap", RelocKind::Plt32);
            emit!(asm.mov(rcx, rax));
            emit!(asm.xor(edx, edx));
            emit!(asm.mov(r8, qword_ptr(rsp + 64i32)));
            call_ext!("HeapAlloc", RelocKind::Plt32);
            emit!(asm.set_label(&mut args_malloc_done));
            emit!(asm.mov(r14, rax));
            let mut args_storage_ready = asm.create_label();
            emit!(asm.test(r14, r14));
            emit!(asm.jne(args_storage_ready));
            emit!(asm.xor(r12, r12));
            emit!(asm.set_label(&mut args_storage_ready));

            // Convert each UTF-16 argument to an owned UTF-8 C string.
            let mut args_loop = asm.create_label();
            let mut args_loop_end = asm.create_label();
            emit!(asm.xor(r15, r15));
            emit!(asm.set_label(&mut args_loop));
            emit!(asm.cmp(r15, r12));
            emit!(asm.jae(args_loop_end));
            emit!(asm.mov(ecx, 65001i32)); // CP_UTF8
            emit!(asm.xor(edx, edx));
            emit!(asm.mov(r8, qword_ptr(r13 + r15 * 8)));
            emit!(asm.mov(r9d, -1i32));
            emit!(asm.mov(qword_ptr(rsp + 32i32), 0i32));
            emit!(asm.mov(qword_ptr(rsp + 40i32), 0i32));
            emit!(asm.mov(qword_ptr(rsp + 48i32), 0i32));
            emit!(asm.mov(qword_ptr(rsp + 56i32), 0i32));
            call_ext!("WideCharToMultiByte", RelocKind::Plt32);
            emit!(asm.mov(dword_ptr(rsp + 64i32), eax));
            call_ext!("GetProcessHeap", RelocKind::Plt32);
            emit!(asm.mov(rcx, rax));
            emit!(asm.xor(edx, edx));
            emit!(asm.mov(r8d, dword_ptr(rsp + 64i32)));
            call_ext!("HeapAlloc", RelocKind::Plt32);
            emit!(asm.mov(qword_ptr(r14 + r15 * 8), rax));
            emit!(asm.mov(ecx, 65001i32));
            emit!(asm.xor(edx, edx));
            emit!(asm.mov(r8, qword_ptr(r13 + r15 * 8)));
            emit!(asm.mov(r9d, -1i32));
            emit!(asm.mov(qword_ptr(rsp + 32i32), rax));
            emit!(asm.mov(eax, dword_ptr(rsp + 64i32)));
            emit!(asm.mov(qword_ptr(rsp + 40i32), rax));
            emit!(asm.mov(qword_ptr(rsp + 48i32), 0i32));
            emit!(asm.mov(qword_ptr(rsp + 56i32), 0i32));
            call_ext!("WideCharToMultiByte", RelocKind::Plt32);
            emit!(asm.inc(r15));
            emit!(asm.jmp(args_loop));
            emit!(asm.set_label(&mut args_loop_end));

            emit!(asm.mov(rcx, r13));
            call_ext!("LocalFree", RelocKind::Plt32);

            emit!(asm.mov(qword_ptr(rsp + 72i32), r14));
            emit!(asm.mov(qword_ptr(rsp + 80i32), r12));
            emit!(asm.mov(qword_ptr(rsp + 88i32), r12));
            emit!(asm.lea(rcx, qword_ptr(rsp + 72i32)));
        }

        // call main
        call_ext!("main", RelocKind::Plt32);

        if main_takes_args {
            emit!(asm.add(rsp, 96i32));
        }

        // ExitProcess(result)
        emit!(asm.mov(ecx, eax));
        call_ext!("ExitProcess", RelocKind::Plt32);

        // Epilogue (unreachable)
        emit!(asm.add(rsp, 40i32));
        emit!(asm.ret());

        // ── Assemble and compute offsets ──
        let mut bytes = asm.assemble(0).expect("assemble");

        let offsets: Vec<usize> = {
            let mut out = Vec::with_capacity(asm.instructions().len());
            let mut dec = iced_x86::Decoder::with_ip(64, &bytes, 0, iced_x86::DecoderOptions::NONE);
            let mut tmp = iced_x86::Instruction::default();
            while dec.can_decode() {
                out.push(dec.ip() as usize);
                dec.decode_out(&mut tmp);
            }
            out
        };

        // Build relocs
        let mut relocs = Vec::with_capacity(pending.len());
        for (asm_idx, disp_off, kind, sym, addend) in pending {
            let byte_off = offsets[asm_idx];
            let field = byte_off + disp_off;
            bytes[field..field + 4].fill(0);
            relocs.push(PendingReloc {
                offset_in_text: fn_offset + field,
                kind,
                symbol: sym,
                addend,
            });
        }

        // Compute function boundaries
        let crash_handler_off = offsets[crash_handler_idx];
        let crash_handler_size = offsets[crash_handler_end_idx] - crash_handler_off;
        let print_bt_off = offsets[print_bt_idx];
        let print_bt_size = offsets[print_bt_end_idx] - print_bt_off;
        let startup_off = offsets[startup_idx];

        Self {
            bytes,
            relocs,
            extra_symbols: vec![
                (
                    "__quazi_crash_handler_win".into(),
                    crash_handler_off,
                    crash_handler_size,
                ),
                (
                    "__quazi_print_backtrace".into(),
                    print_bt_off,
                    print_bt_size,
                ),
            ],
            start_offset: startup_off,
        }
    }

    /// Minimal Windows stub without crash-handler registration.
    /// Keeps __quazi_print_backtrace for panic backtraces.
    fn generate_minimal_windows(fn_offset: usize, main_takes_args: bool) -> Self {
        use iced_x86::code_asm::*;

        let mut asm = CodeAssembler::new(64).expect("asm");
        let fn_start = asm.create_label();
        let mut pending: Vec<(usize, usize, RelocKind, String, i64)> = Vec::new();

        macro_rules! emit {
            ($e:expr) => {
                $e.expect("emit")
            };
        }

        macro_rules! call_ext {
            ($sym:expr, $kind:expr) => {{
                let idx = asm.instructions().len();
                emit!(asm.call(fn_start));
                pending.push((idx, 1, $kind, $sym.into(), -4));
            }};
        }

        macro_rules! lea_rip {
            ($reg:expr, $sym:expr) => {{
                let idx = asm.instructions().len();
                emit!(asm.lea($reg, qword_ptr(fn_start)));
                pending.push((idx, 3, RelocKind::Pc32, $sym.into(), -4));
            }};
        }

        let mut print_bt_label = asm.create_label();

        // ═══════════════════════════════════════════════════════
        // __quazi_print_backtrace (same as full Windows stub)
        // ═══════════════════════════════════════════════════════
        let print_bt_idx = asm.instructions().len();
        emit!(asm.set_label(&mut print_bt_label));

        emit!(asm.push(rbx));
        emit!(asm.push(r12));
        emit!(asm.push(r13));
        emit!(asm.push(r14));
        emit!(asm.push(r15));
        emit!(asm.sub(rsp, 40i32));

        let mut bt_done = asm.create_label();
        lea_rip!(rax, "__quazi_trace_enabled");
        emit!(asm.cmp(byte_ptr(rax), 0i32));
        emit!(asm.je(bt_done));

        // Get stderr handle
        emit!(asm.mov(ecx, -12i32));
        call_ext!("GetStdHandle", RelocKind::Plt32);
        emit!(asm.mov(r15, rax));

        // "stack backtrace:\n"
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_bt_prefix");
        emit!(asm.mov(r8d, 17i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // Walk current RBP chain
        emit!(asm.mov(r12, rbp));
        emit!(asm.mov(r13, 16i64));

        let mut bt_loop = asm.create_label();
        emit!(asm.set_label(&mut bt_loop));
        emit!(asm.test(r12, r12));
        emit!(asm.je(bt_done));
        emit!(asm.test(r13, r13));
        emit!(asm.je(bt_done));

        // "  at 0x"
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_at_0x");
        emit!(asm.mov(r8d, 7i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // return address
        emit!(asm.mov(rbx, qword_ptr(r12 + 8)));
        emit!(asm.mov(ecx, 16i32));
        lea_rip!(r8, "__quazi_hex_buf");
        emit!(asm.add(r8, 15i32));
        let mut hbt = asm.create_label();
        let mut sbt = asm.create_label();
        emit!(asm.set_label(&mut hbt));
        emit!(asm.mov(rax, rbx));
        emit!(asm.and(rax, 0x0Fi32));
        emit!(asm.cmp(al, 10i32));
        emit!(asm.jb(sbt));
        emit!(asm.add(al, 7i32));
        emit!(asm.set_label(&mut sbt));
        emit!(asm.add(al, '0' as i32));
        emit!(asm.mov(byte_ptr(r8), al));
        emit!(asm.dec(r8));
        emit!(asm.shr(rbx, 4i32));
        emit!(asm.dec(ecx));
        emit!(asm.jne(hbt));

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_hex_buf");
        emit!(asm.mov(r8d, 16i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__quazi_nl");
        emit!(asm.mov(r8d, 1i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(r12, qword_ptr(r12)));
        emit!(asm.dec(r13));
        emit!(asm.jmp(bt_loop));

        emit!(asm.set_label(&mut bt_done));
        emit!(asm.add(rsp, 40i32));
        emit!(asm.pop(r15));
        emit!(asm.pop(r14));
        emit!(asm.pop(r13));
        emit!(asm.pop(r12));
        emit!(asm.pop(rbx));
        emit!(asm.ret());

        let print_bt_end_idx = asm.instructions().len();

        // ═══════════════════════════════════════════════════════
        // mainCRTStartup (minimal — no exception handler)
        // ═══════════════════════════════════════════════════════
        let startup_idx = asm.instructions().len();

        emit!(asm.sub(rsp, 40i32));

        // GetEnvironmentVariableA("QUAZI_TRACE", buf, 8)
        lea_rip!(rcx, "__quazi_env_var_name");
        emit!(asm.lea(rdx, qword_ptr(rsp)));
        emit!(asm.mov(r8d, 8i32));
        call_ext!("GetEnvironmentVariableA", RelocKind::Plt32);

        // If result > 0 and buf[0] == '1', set flag
        let mut no_env = asm.create_label();
        emit!(asm.test(eax, eax));
        emit!(asm.je(no_env));
        emit!(asm.cmp(byte_ptr(rsp), '1' as i32));
        emit!(asm.jne(no_env));
        lea_rip!(rax, "__quazi_trace_enabled");
        emit!(asm.mov(byte_ptr(rax), 1i32));
        emit!(asm.set_label(&mut no_env));

        if main_takes_args {
            call_ext!("GetCommandLineW", RelocKind::Plt32);
            emit!(asm.mov(rcx, rax));
            emit!(asm.lea(rdx, qword_ptr(rsp + 8i32)));
            emit!(asm.mov(dword_ptr(rsp + 8i32), 0i32));
            call_ext!("CommandLineToArgvW", RelocKind::Plt32);
            emit!(asm.mov(r13, rax));
            emit!(asm.xor(r12, r12));
            let mut args_parsed = asm.create_label();
            emit!(asm.test(r13, r13));
            emit!(asm.je(args_parsed));
            emit!(asm.movsxd(r12, dword_ptr(rsp + 8i32)));
            emit!(asm.set_label(&mut args_parsed));

            emit!(asm.sub(rsp, 96i32));
            emit!(asm.mov(rcx, r12));
            emit!(asm.shl(rcx, 3i32));
            let mut args_malloc = asm.create_label();
            let mut args_malloc_done = asm.create_label();
            emit!(asm.test(rcx, rcx));
            emit!(asm.jne(args_malloc));
            emit!(asm.xor(eax, eax));
            emit!(asm.jmp(args_malloc_done));
            emit!(asm.set_label(&mut args_malloc));
            emit!(asm.mov(qword_ptr(rsp + 64i32), rcx));
            call_ext!("GetProcessHeap", RelocKind::Plt32);
            emit!(asm.mov(rcx, rax));
            emit!(asm.xor(edx, edx));
            emit!(asm.mov(r8, qword_ptr(rsp + 64i32)));
            call_ext!("HeapAlloc", RelocKind::Plt32);
            emit!(asm.set_label(&mut args_malloc_done));
            emit!(asm.mov(r14, rax));
            let mut args_storage_ready = asm.create_label();
            emit!(asm.test(r14, r14));
            emit!(asm.jne(args_storage_ready));
            emit!(asm.xor(r12, r12));
            emit!(asm.set_label(&mut args_storage_ready));

            let mut args_loop = asm.create_label();
            let mut args_loop_end = asm.create_label();
            emit!(asm.xor(r15, r15));
            emit!(asm.set_label(&mut args_loop));
            emit!(asm.cmp(r15, r12));
            emit!(asm.jae(args_loop_end));
            emit!(asm.mov(ecx, 65001i32));
            emit!(asm.xor(edx, edx));
            emit!(asm.mov(r8, qword_ptr(r13 + r15 * 8)));
            emit!(asm.mov(r9d, -1i32));
            emit!(asm.mov(qword_ptr(rsp + 32i32), 0i32));
            emit!(asm.mov(qword_ptr(rsp + 40i32), 0i32));
            emit!(asm.mov(qword_ptr(rsp + 48i32), 0i32));
            emit!(asm.mov(qword_ptr(rsp + 56i32), 0i32));
            call_ext!("WideCharToMultiByte", RelocKind::Plt32);
            emit!(asm.mov(dword_ptr(rsp + 64i32), eax));
            call_ext!("GetProcessHeap", RelocKind::Plt32);
            emit!(asm.mov(rcx, rax));
            emit!(asm.xor(edx, edx));
            emit!(asm.mov(r8d, dword_ptr(rsp + 64i32)));
            call_ext!("HeapAlloc", RelocKind::Plt32);
            emit!(asm.mov(qword_ptr(r14 + r15 * 8), rax));
            emit!(asm.mov(ecx, 65001i32));
            emit!(asm.xor(edx, edx));
            emit!(asm.mov(r8, qword_ptr(r13 + r15 * 8)));
            emit!(asm.mov(r9d, -1i32));
            emit!(asm.mov(qword_ptr(rsp + 32i32), rax));
            emit!(asm.mov(eax, dword_ptr(rsp + 64i32)));
            emit!(asm.mov(qword_ptr(rsp + 40i32), rax));
            emit!(asm.mov(qword_ptr(rsp + 48i32), 0i32));
            emit!(asm.mov(qword_ptr(rsp + 56i32), 0i32));
            call_ext!("WideCharToMultiByte", RelocKind::Plt32);
            emit!(asm.inc(r15));
            emit!(asm.jmp(args_loop));
            emit!(asm.set_label(&mut args_loop_end));

            emit!(asm.mov(rcx, r13));
            call_ext!("LocalFree", RelocKind::Plt32);

            emit!(asm.mov(qword_ptr(rsp + 72i32), r14));
            emit!(asm.mov(qword_ptr(rsp + 80i32), r12));
            emit!(asm.mov(qword_ptr(rsp + 88i32), r12));
            emit!(asm.lea(rcx, qword_ptr(rsp + 72i32)));
        }

        // call main
        call_ext!("main", RelocKind::Plt32);

        if main_takes_args {
            emit!(asm.add(rsp, 96i32));
        }

        // ExitProcess(result)
        emit!(asm.mov(ecx, eax));
        call_ext!("ExitProcess", RelocKind::Plt32);

        // Epilogue (unreachable)
        emit!(asm.add(rsp, 40i32));
        emit!(asm.ret());

        // ── Assemble and compute offsets ──
        let mut bytes = asm.assemble(0).expect("assemble");

        let offsets: Vec<usize> = {
            let mut out = Vec::with_capacity(asm.instructions().len());
            let mut dec = iced_x86::Decoder::with_ip(64, &bytes, 0, iced_x86::DecoderOptions::NONE);
            let mut tmp = iced_x86::Instruction::default();
            while dec.can_decode() {
                out.push(dec.ip() as usize);
                dec.decode_out(&mut tmp);
            }
            out
        };

        // Build relocs
        let mut relocs = Vec::with_capacity(pending.len());
        for (asm_idx, disp_off, kind, sym, addend) in pending {
            let byte_off = offsets[asm_idx];
            let field = byte_off + disp_off;
            bytes[field..field + 4].fill(0);
            relocs.push(PendingReloc {
                offset_in_text: fn_offset + field,
                kind,
                symbol: sym,
                addend,
            });
        }

        // Compute function boundaries
        let print_bt_off = offsets[print_bt_idx];
        let print_bt_size = offsets[print_bt_end_idx] - print_bt_off;
        let startup_off = offsets[startup_idx];

        Self {
            bytes,
            relocs,
            extra_symbols: vec![(
                "__quazi_print_backtrace".into(),
                print_bt_off,
                print_bt_size,
            )],
            start_offset: startup_off,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StartStub;

    #[test]
    fn windows_argument_startup_uses_supported_win32_apis() {
        for no_crash in [false, true] {
            let stub = StartStub::generate_windows(0, no_crash, true);
            let symbols: Vec<&str> = stub
                .relocs
                .iter()
                .map(|reloc| reloc.symbol.as_str())
                .collect();

            assert!(symbols.contains(&"GetCommandLineW"));
            assert!(symbols.contains(&"CommandLineToArgvW"));
            assert!(symbols.contains(&"WideCharToMultiByte"));
            assert!(symbols.contains(&"LocalFree"));
            assert!(symbols.contains(&"GetProcessHeap"));
            assert!(symbols.contains(&"HeapAlloc"));
            assert!(!symbols.contains(&"malloc"));
            assert!(!symbols.contains(&"__getmainargs"));
        }
    }
}
