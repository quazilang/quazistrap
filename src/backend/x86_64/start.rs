// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD
//
// Entry-point stubs with crash handlers.
//
// Linux layout (fn_offset = start of stub in .text):
//   0:   __void_sigreturn        (7 B)   — restorer for rt_sigaction
//   7:   __void_crash_handler    (241 B) — SA_SIGINFO handler: signal/addr/RIP hex + backtrace
//   248: __void_print_backtrace  (192 B) — RBP chain walk (max 16 frames)
//   440: _start                  (242 B) — sigaction setup, VOID_TRACE=1 env check, calls main
//
// Windows layout:
//   0:  __void_crash_handler_win (69 B) — unhandled-exception filter: WriteFile + ExitProcess
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
    /// Linux _start stub with SIGSEGV/SIGABRT/SIGFPE/SIGBUS crash handler,
    /// backtrace printer, and VOID_TRACE=1 environment detection.
    pub fn generate(fn_offset: usize) -> Self {
        #[rustfmt::skip]
        let bytes: Vec<u8> = vec![
            // __void_sigreturn (offset 0, 7 bytes): sys_rt_sigreturn restorer
            0xb8, 0x0f, 0x00, 0x00, 0x00, 0x0f, 0x05,
            // __void_crash_handler (offset 7, 241 bytes): SA_SIGINFO handler
            0x57, 0x56, 0x52, 0x53, 0x41, 0x54, 0x41, 0x55, 0x49, 0x89, 0xfc, 0x49,
            0x89, 0xf5, 0xb8, 0x01, 0x00, 0x00, 0x00, 0xbf, 0x02, 0x00, 0x00, 0x00,
            0x48, 0x8d, 0x35, 0x00, 0x00, 0x00, 0x00, 0xba, 0x0e, 0x00, 0x00, 0x00,
            0x0f, 0x05, 0x4c, 0x89, 0xe3, 0x48, 0x83, 0xe3, 0x0f, 0x80, 0xfb, 0x0a,
            0x72, 0x03, 0x80, 0xc3, 0x27, 0x80, 0xc3, 0x30, 0x88, 0x1d, 0x00, 0x00,
            0x00, 0x00, 0xb8, 0x01, 0x00, 0x00, 0x00, 0xbf, 0x02, 0x00, 0x00, 0x00,
            0x48, 0x8d, 0x35, 0x00, 0x00, 0x00, 0x00, 0xba, 0x01, 0x00, 0x00, 0x00,
            0x0f, 0x05, 0xb8, 0x01, 0x00, 0x00, 0x00, 0xbf, 0x02, 0x00, 0x00, 0x00,
            0x48, 0x8d, 0x35, 0x00, 0x00, 0x00, 0x00, 0xba, 0x0e, 0x00, 0x00, 0x00,
            0x0f, 0x05, 0x49, 0x8b, 0x5d, 0x10, 0xb9, 0x10, 0x00, 0x00, 0x00, 0x48,
            0x8d, 0x3d, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xd8, 0x48, 0x83, 0xe0,
            0x0f, 0x3c, 0x0a, 0x72, 0x02, 0x04, 0x27, 0x04, 0x30, 0x88, 0x07, 0x48,
            0xff, 0xcf, 0x48, 0xc1, 0xeb, 0x04, 0xff, 0xc9, 0x75, 0xe4, 0xb8, 0x01,
            0x00, 0x00, 0x00, 0xbf, 0x02, 0x00, 0x00, 0x00, 0x48, 0x8d, 0x35, 0x00,
            0x00, 0x00, 0x00, 0xba, 0x10, 0x00, 0x00, 0x00, 0x0f, 0x05, 0xb8, 0x01,
            0x00, 0x00, 0x00, 0xbf, 0x02, 0x00, 0x00, 0x00, 0x48, 0x8d, 0x35, 0x00,
            0x00, 0x00, 0x00, 0xba, 0x01, 0x00, 0x00, 0x00, 0x0f, 0x05, 0x80, 0x3d,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x74, 0x05, 0xe8, 0x00, 0x00, 0x00, 0x00,
            0xbf, 0x65, 0x00, 0x00, 0x00, 0x90, 0x90, 0x90, 0x90, 0xb8, 0xe7, 0x00,
            0x00, 0x00, 0x0f, 0x05, 0x41, 0x5d, 0x41, 0x5c, 0x5b, 0x5a, 0x5e, 0x5f,
            0xc3,
            // __void_print_backtrace (offset 248, 192 bytes): walk RBP chain
            0x53, 0x41, 0x54, 0x41, 0x55, 0x49, 0x89, 0xec, 0x49, 0xc7, 0xc5, 0x10,
            0x00, 0x00, 0x00, 0xb8, 0x01, 0x00, 0x00, 0x00, 0xbf, 0x02, 0x00, 0x00,
            0x00, 0x48, 0x8d, 0x35, 0x00, 0x00, 0x00, 0x00, 0xba, 0x12, 0x00, 0x00,
            0x00, 0x0f, 0x05, 0x4d, 0x85, 0xe4, 0x0f, 0x84, 0x8a, 0x00, 0x00, 0x00,
            0x4d, 0x85, 0xed, 0x0f, 0x84, 0x81, 0x00, 0x00, 0x00, 0x49, 0x8b, 0x5c,
            0x24, 0x08, 0xb8, 0x01, 0x00, 0x00, 0x00, 0xbf, 0x02, 0x00, 0x00, 0x00,
            0x48, 0x8d, 0x35, 0x00, 0x00, 0x00, 0x00, 0xba, 0x07, 0x00, 0x00, 0x00,
            0x0f, 0x05, 0xb9, 0x10, 0x00, 0x00, 0x00, 0x48, 0x8d, 0x3d, 0x00, 0x00,
            0x00, 0x00, 0x48, 0x89, 0xd8, 0x48, 0x83, 0xe0, 0x0f, 0x3c, 0x0a, 0x72,
            0x02, 0x04, 0x27, 0x04, 0x30, 0x88, 0x07, 0x48, 0xff, 0xcf, 0x48, 0xc1,
            0xeb, 0x04, 0xff, 0xc9, 0x75, 0xe4, 0xb8, 0x01, 0x00, 0x00, 0x00, 0xbf,
            0x02, 0x00, 0x00, 0x00, 0x48, 0x8d, 0x35, 0x00, 0x00, 0x00, 0x00, 0xba,
            0x10, 0x00, 0x00, 0x00, 0x0f, 0x05, 0xb8, 0x01, 0x00, 0x00, 0x00, 0xbf,
            0x02, 0x00, 0x00, 0x00, 0x48, 0x8d, 0x35, 0x00, 0x00, 0x00, 0x00, 0xba,
            0x01, 0x00, 0x00, 0x00, 0x0f, 0x05, 0x4d, 0x8b, 0x24, 0x24, 0x49, 0xff,
            0xcd, 0xe9, 0x6d, 0xff, 0xff, 0xff, 0x41, 0x5d, 0x41, 0x5c, 0x5b, 0xc3,
            // _start (offset 440, 242 bytes): entry point
            0x31, 0xed, 0x48, 0x81, 0xec, 0xa0, 0x02, 0x00, 0x00, 0x48, 0x31, 0xc0,
            0x48, 0xc7, 0xc1, 0x14, 0x00, 0x00, 0x00, 0x48, 0x89, 0xe7, 0xf3, 0x48,
            0xab, 0x48, 0x8d, 0x05, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0x04, 0x24,
            0xc7, 0x44, 0x24, 0x08, 0x00, 0x00, 0x00, 0x04, 0x48, 0x8d, 0x05, 0x00,
            0x00, 0x00, 0x00, 0x48, 0x89, 0x44, 0x24, 0x10, 0x48, 0x89, 0xe6, 0x48,
            0x31, 0xd2, 0x49, 0xc7, 0xc2, 0x08, 0x00, 0x00, 0x00, 0xb8, 0x0d, 0x00,
            0x00, 0x00, 0xbf, 0x0b, 0x00, 0x00, 0x00, 0x0f, 0x05, 0xbf, 0x06, 0x00,
            0x00, 0x00, 0x0f, 0x05, 0xbf, 0x08, 0x00, 0x00, 0x00, 0x0f, 0x05, 0xbf,
            0x07, 0x00, 0x00, 0x00, 0x0f, 0x05, 0xb8, 0x02, 0x00, 0x00, 0x00, 0x48,
            0x8d, 0x3d, 0x00, 0x00, 0x00, 0x00, 0x31, 0xf6, 0x0f, 0x05, 0x85, 0xc0,
            0x78, 0x62, 0x41, 0x89, 0xc4, 0x44, 0x89, 0xe7, 0xb8, 0x00, 0x00, 0x00,
            0x00, 0x48, 0x8d, 0xb4, 0x24, 0xa0, 0x00, 0x00, 0x00, 0xba, 0x00, 0x02,
            0x00, 0x00, 0x0f, 0x05, 0x44, 0x89, 0xe7, 0xb8, 0x03, 0x00, 0x00, 0x00,
            0x0f, 0x05, 0x89, 0xc1, 0x83, 0xe9, 0x0b, 0x78, 0x37, 0x48, 0x8d, 0x9c,
            0x24, 0xa0, 0x00, 0x00, 0x00, 0x81, 0x3b, 0x56, 0x4f, 0x49, 0x44, 0x75,
            0x20, 0x81, 0x7b, 0x04, 0x54, 0x52, 0x41, 0x43, 0x75, 0x17, 0x66, 0x81,
            0x7b, 0x08, 0x45, 0x3d, 0x75, 0x0f, 0x80, 0x7b, 0x0a, 0x31, 0x75, 0x09,
            0xc6, 0x05, 0x00, 0x00, 0x00, 0x00, 0x01, 0xeb, 0x07, 0x48, 0xff, 0xc3,
            0xff, 0xc9, 0x75, 0xd1, 0x48, 0x81, 0xc4, 0xa0, 0x02, 0x00, 0x00, 0xe8,
            0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xc7, 0xb8, 0x3c, 0x00, 0x00, 0x00,
            0x0f, 0x05,
        ];

        let relocs = vec![
            // __void_crash_handler relocs
            PendingReloc { offset_in_text: fn_offset + 0x22,  kind: RelocKind::Pc32,  symbol: "__void_fatal_sig".into(),         addend: -4 },
            PendingReloc { offset_in_text: fn_offset + 0x41,  kind: RelocKind::Pc32,  symbol: "__void_hex_buf".into(),           addend: -4 },
            PendingReloc { offset_in_text: fn_offset + 0x52,  kind: RelocKind::Pc32,  symbol: "__void_hex_buf".into(),           addend: -4 },
            PendingReloc { offset_in_text: fn_offset + 0x6a,  kind: RelocKind::Pc32,  symbol: "__void_at_addr".into(),           addend: -4 },
            PendingReloc { offset_in_text: fn_offset + 0x81,  kind: RelocKind::Pc32,  symbol: "__void_hex_buf".into(),           addend: 11 },
            PendingReloc { offset_in_text: fn_offset + 0xae,  kind: RelocKind::Pc32,  symbol: "__void_hex_buf".into(),           addend: -4 },
            PendingReloc { offset_in_text: fn_offset + 0xc6,  kind: RelocKind::Pc32,  symbol: "__void_nl".into(),                addend: -4 },
            PendingReloc { offset_in_text: fn_offset + 0xd3,  kind: RelocKind::Pc32,  symbol: "__void_trace_enabled".into(),     addend: -5 },
            PendingReloc { offset_in_text: fn_offset + 0xdb,  kind: RelocKind::Plt32, symbol: "__void_print_backtrace".into(),   addend: -4 },
            // __void_print_backtrace relocs
            PendingReloc { offset_in_text: fn_offset + 0x114, kind: RelocKind::Pc32,  symbol: "__void_bt_prefix".into(),         addend: -4 },
            PendingReloc { offset_in_text: fn_offset + 0x143, kind: RelocKind::Pc32,  symbol: "__void_at_0x".into(),             addend: -4 },
            PendingReloc { offset_in_text: fn_offset + 0x156, kind: RelocKind::Pc32,  symbol: "__void_hex_buf".into(),           addend: 11 },
            PendingReloc { offset_in_text: fn_offset + 0x183, kind: RelocKind::Pc32,  symbol: "__void_hex_buf".into(),           addend: -4 },
            PendingReloc { offset_in_text: fn_offset + 0x19b, kind: RelocKind::Pc32,  symbol: "__void_nl".into(),                addend: -4 },
            // _start relocs
            PendingReloc { offset_in_text: fn_offset + 0x1d4, kind: RelocKind::Pc32,  symbol: "__void_crash_handler".into(),     addend: -4 },
            PendingReloc { offset_in_text: fn_offset + 0x1e7, kind: RelocKind::Pc32,  symbol: "__void_sigreturn".into(),         addend: -4 },
            PendingReloc { offset_in_text: fn_offset + 0x226, kind: RelocKind::Pc32,  symbol: "__void_proc_self_environ".into(), addend: -4 },
            PendingReloc { offset_in_text: fn_offset + 0x286, kind: RelocKind::Pc32,  symbol: "__void_trace_enabled".into(),     addend: -5 },
            PendingReloc { offset_in_text: fn_offset + 0x29c, kind: RelocKind::Plt32, symbol: "main".into(),                     addend: -4 },
        ];

        Self {
            bytes,
            relocs,
            extra_symbols: vec![
                ("__void_sigreturn".into(), 0, 7),
                ("__void_crash_handler".into(), 7, 241),
                ("__void_print_backtrace".into(), 248, 192),
            ],
            start_offset: 440,
        }
    }

    /// Windows mainCRTStartup stub with AddVectoredExceptionHandler crash handler.
    ///
    /// Uses iced-x86 CodeAssembler to build a larger, more informative stub:
    ///   __void_crash_handler_win  — prints exception code, fault address, RIP, and
    ///                                 a backtrace (from ContextRecord->Rbp) when
    ///                                 __void_trace_enabled is set.
    ///   __void_print_backtrace    — checks __void_trace_enabled, walks current RBP
    ///                                 chain, prints up to 16 frames.
    ///   mainCRTStartup            — sets exception handler, reads VOID_TRACE env var,
    ///                                 calls main.
    pub fn generate_windows(fn_offset: usize) -> Self {
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
        let mut crash_handler_label = asm.create_label();
        let mut print_bt_label = asm.create_label();

        // ═══════════════════════════════════════════════════════
        // __void_crash_handler_win
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
        lea_rip!(rdx, "__void_crash_header");
        emit!(asm.mov(r8d, 14i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // ── "fatal: exception 0x" + hex code ──
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__void_fatal_exc");
        emit!(asm.mov(r8d, 19i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // Format ebx as 8 hex digits
        emit!(asm.mov(rbx, rbx)); // zero-extended from ebx
        emit!(asm.mov(ecx, 8i32));
        lea_rip!(r8, "__void_hex_buf");
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
        lea_rip!(rdx, "__void_hex_buf");
        emit!(asm.mov(r8d, 8i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__void_nl");
        emit!(asm.mov(r8d, 1i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // ── "fault: 0x" + hex address ──
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__void_fault");
        emit!(asm.mov(r8d, 9i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(rbx, r12));
        emit!(asm.mov(ecx, 16i32));
        lea_rip!(r8, "__void_hex_buf");
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
        lea_rip!(rdx, "__void_hex_buf");
        emit!(asm.mov(r8d, 16i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__void_nl");
        emit!(asm.mov(r8d, 1i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // ── "rip: 0x" + hex RIP ──
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__void_rip");
        emit!(asm.mov(r8d, 7i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(rbx, r13));
        emit!(asm.mov(ecx, 16i32));
        lea_rip!(r8, "__void_hex_buf");
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
        lea_rip!(rdx, "__void_hex_buf");
        emit!(asm.mov(r8d, 16i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__void_nl");
        emit!(asm.mov(r8d, 1i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // ── Backtrace from crash context (uses ContextRecord->Rbp) ──
        let mut bt_done_c = asm.create_label();
        lea_rip!(rax, "__void_trace_enabled");
        emit!(asm.cmp(byte_ptr(rax), 0i32));
        emit!(asm.je(bt_done_c));

        // Reload ContextRecord* and get Rbp
        emit!(asm.mov(rax, qword_ptr(r14 + 8)));
        emit!(asm.mov(r12, qword_ptr(rax + 0xA0)));

        // "stack backtrace:\n"
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__void_bt_prefix");
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
        lea_rip!(rdx, "__void_at_0x");
        emit!(asm.mov(r8d, 7i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // return address
        emit!(asm.mov(rbx, qword_ptr(r12 + 8)));
        emit!(asm.mov(ecx, 16i32));
        lea_rip!(r8, "__void_hex_buf");
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
        lea_rip!(rdx, "__void_hex_buf");
        emit!(asm.mov(r8d, 16i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__void_nl");
        emit!(asm.mov(r8d, 1i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(r12, qword_ptr(r12)));
        emit!(asm.dec(r13));
        emit!(asm.jmp(bt_loop_c));

        emit!(asm.set_label(&mut bt_done_c));

        // Print hint when backtrace was skipped
        lea_rip!(rax, "__void_trace_enabled");
        emit!(asm.cmp(byte_ptr(rax), 0i32));
        let mut skip_hint = asm.create_label();
        emit!(asm.jne(skip_hint));
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__void_trace_hint");
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
        // __void_print_backtrace
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
        lea_rip!(rax, "__void_trace_enabled");
        emit!(asm.cmp(byte_ptr(rax), 0i32));
        emit!(asm.je(bt_done));

        // Get stderr handle
        emit!(asm.mov(ecx, -12i32));
        call_ext!("GetStdHandle", RelocKind::Plt32);
        emit!(asm.mov(r15, rax));

        // "stack backtrace:\n"
        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__void_bt_prefix");
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
        lea_rip!(rdx, "__void_at_0x");
        emit!(asm.mov(r8d, 7i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        // return address
        emit!(asm.mov(rbx, qword_ptr(r12 + 8)));
        emit!(asm.mov(ecx, 16i32));
        lea_rip!(r8, "__void_hex_buf");
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
        lea_rip!(rdx, "__void_hex_buf");
        emit!(asm.mov(r8d, 16i32));
        emit!(asm.xor(r9d, r9d));
        emit!(asm.mov(qword_ptr(rsp + 32), 0i32));
        call_ext!("WriteFile", RelocKind::Plt32);

        emit!(asm.mov(rcx, r15));
        lea_rip!(rdx, "__void_nl");
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

        // GetEnvironmentVariableA("VOID_TRACE", buf, 8)
        lea_rip!(rcx, "__void_env_var_name");
        emit!(asm.lea(rdx, qword_ptr(rsp)));
        emit!(asm.mov(r8d, 8i32));
        call_ext!("GetEnvironmentVariableA", RelocKind::Plt32);

        // If result > 0 and buf[0] == '1', set flag
        let mut no_env = asm.create_label();
        emit!(asm.test(eax, eax));
        emit!(asm.je(no_env));
        emit!(asm.cmp(byte_ptr(rsp), '1' as i32));
        emit!(asm.jne(no_env));
        lea_rip!(rax, "__void_trace_enabled");
        emit!(asm.mov(byte_ptr(rax), 1i32));
        emit!(asm.set_label(&mut no_env));

        // call main
        call_ext!("main", RelocKind::Plt32);

        // ExitProcess(result)
        emit!(asm.mov(ecx, eax));
        call_ext!("ExitProcess", RelocKind::Plt32);

        // Epilogue (unreachable)
        emit!(asm.add(rsp, 40i32));
        emit!(asm.ret());

        let startup_end_idx = asm.instructions().len();

        // ── Assemble and compute offsets ──
        let mut bytes = asm.assemble(0).expect("assemble");

        let offsets: Vec<usize> = {
            let mut out = Vec::with_capacity(asm.instructions().len());
            let mut dec = iced_x86::Decoder::with_ip(
                64,
                &bytes,
                0,
                iced_x86::DecoderOptions::NONE,
            );
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
                ("__void_crash_handler_win".into(), crash_handler_off, crash_handler_size),
                ("__void_print_backtrace".into(), print_bt_off, print_bt_size),
            ],
            start_offset: startup_off,
        }
    }
}
