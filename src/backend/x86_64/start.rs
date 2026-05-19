// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD
//
// Entry-point stubs with crash handlers.
//
// Linux layout (fn_offset = start of stub in .text):
//   0:  __void_sigreturn        (7 B)  — restorer for rt_sigaction
//   7:  __void_crash_handler    (39 B) — signal handler: write msg + exit(128+sig)
//   46: _start                  (120 B) — registers handlers for 4 signals, calls main
//
// Windows layout:
//   0:  __void_crash_handler_win (69 B) — unhandled-exception filter: WriteFile + ExitProcess
//   69: mainCRTStartup           (32 B) — SetUnhandledExceptionFilter + call main

use super::relocations::{PendingReloc, RelocKind};

// Must match sections::CRASH_MSG.len().
const CRASH_MSG_LEN: u32 = 78;

pub struct StartStub {
    pub bytes: Vec<u8>,
    pub relocs: Vec<PendingReloc>,
    /// Internal symbols to define: (name, offset_in_stub, size).
    pub extra_symbols: Vec<(String, usize, usize)>,
    /// Offset of the executable entry point within `bytes`.
    pub start_offset: usize,
}

impl StartStub {
    /// Linux _start stub with SIGSEGV/SIGABRT/SIGFPE/SIGBUS crash handler.
    pub fn generate(fn_offset: usize) -> Self {
        // Within-stub RIP-relative displacements (computed from fixed layout).
        //
        // lea rax, [rip+crash_handler] at _start+23 (stub 69): rip_after=76, target=7 → -69
        // lea rax, [rip+sigreturn]     at _start+43 (stub 89): rip_after=96, target=0 → -96
        let [dh0, dh1, dh2, dh3] = (-69i32).to_le_bytes();
        let [ds0, ds1, ds2, ds3] = (-96i32).to_le_bytes();

        let msg_len = CRASH_MSG_LEN.to_le_bytes();

        #[rustfmt::skip]
        let bytes: Vec<u8> = vec![
            // __void_sigreturn (offset 0, 7 bytes): sys_rt_sigreturn restorer
            0xB8, 0x0F, 0x00, 0x00, 0x00,       // mov eax, 15
            0x0F, 0x05,                           // syscall

            // __void_crash_handler (offset 7, 39 bytes): rdi=signum on entry
            0x57,                                 // push rdi
            0x48, 0x8D, 0x35, 0x00,0x00,0x00,0x00, // lea rsi,[rip+__void_crash_msg]  ← PC32@11
            0xBA, msg_len[0],msg_len[1],msg_len[2],msg_len[3], // mov edx, CRASH_MSG_LEN
            0xBF, 0x02, 0x00, 0x00, 0x00,        // mov edi, 2 (stderr)
            0xB8, 0x01, 0x00, 0x00, 0x00,        // mov eax, 1 (sys_write)
            0x0F, 0x05,                           // syscall
            0x5F,                                 // pop rdi
            0x81, 0xC7, 0x80, 0x00, 0x00, 0x00,  // add edi, 128
            0xB8, 0xE7, 0x00, 0x00, 0x00,        // mov eax, 231 (sys_exit_group)
            0x0F, 0x05,                           // syscall

            // _start (offset 46, 120 bytes)
            0x48, 0x31, 0xED,                     // xor rbp, rbp
            0x48, 0x83, 0xEC, 0xA0,               // sub rsp, 160  (sigaction struct)
            0x48, 0x31, 0xC0,                     // xor rax, rax
            0x48, 0xC7, 0xC1, 0x14,0x00,0x00,0x00,// mov rcx, 20  (160/8 qwords)
            0x48, 0x89, 0xE7,                     // mov rdi, rsp
            0xF3, 0x48, 0xAB,                     // rep stosq     (zero fill)
            0x48, 0x8D, 0x05, dh0,dh1,dh2,dh3,   // lea rax,[rip+crash_handler]
            0x48, 0x89, 0x04, 0x24,               // mov [rsp], rax    (sa_handler)
            // mov qword [rsp+8], SA_RESTORER (0x04000000)
            0x48, 0xC7, 0x44, 0x24, 0x08, 0x00,0x00,0x00,0x04,
            0x48, 0x8D, 0x05, ds0,ds1,ds2,ds3,   // lea rax,[rip+sigreturn]
            0x48, 0x89, 0x44, 0x24, 0x10,         // mov [rsp+16], rax (sa_restorer)
            0x48, 0x89, 0xE6,                     // mov rsi, rsp      (&act)
            0x48, 0x31, 0xD2,                     // xor rdx, rdx      (old_act=NULL)
            0x49, 0xC7, 0xC2, 0x08,0x00,0x00,0x00,// mov r10, 8       (sigsetsize)
            0xB8, 0x0D, 0x00, 0x00, 0x00,        // mov eax, 13 (sys_rt_sigaction)
            0xBF, 0x0B, 0x00, 0x00, 0x00, 0x0F, 0x05, // mov edi,11(SIGSEGV); syscall
            0xBF, 0x06, 0x00, 0x00, 0x00, 0x0F, 0x05, // mov edi,6 (SIGABRT); syscall
            0xBF, 0x08, 0x00, 0x00, 0x00, 0x0F, 0x05, // mov edi,8 (SIGFPE);  syscall
            0xBF, 0x07, 0x00, 0x00, 0x00, 0x0F, 0x05, // mov edi,7 (SIGBUS);  syscall
            0x48, 0x83, 0xC4, 0xA0,               // add rsp, 160
            0xE8, 0x00, 0x00, 0x00, 0x00,         // call main         ← PLT32@152
            0x48, 0x89, 0xC7,                     // mov rdi, rax
            0xB8, 0x3C, 0x00, 0x00, 0x00,        // mov eax, 60 (sys_exit)
            0x0F, 0x05,                           // syscall
        ];

        let relocs = vec![
            PendingReloc {
                offset_in_text: fn_offset + 11,
                kind: RelocKind::Pc32,
                symbol: "__void_crash_msg".into(),
                addend: -4,
            },
            PendingReloc {
                offset_in_text: fn_offset + 152,
                kind: RelocKind::Plt32,
                symbol: "main".into(),
                addend: -4,
            },
        ];

        Self {
            bytes,
            relocs,
            extra_symbols: vec![
                ("__void_sigreturn".into(), 0, 7),
                ("__void_crash_handler".into(), 7, 39),
            ],
            start_offset: 46,
        }
    }

    /// Windows mainCRTStartup stub with AddVectoredExceptionHandler crash handler.
    ///
    /// Handler layout (134 bytes):
    ///   Extracts ExceptionCode + ExceptionAddress from EXCEPTION_RECORD,
    ///   Rip from CONTEXT, formats via sprintf into a stack buffer, WriteFile to
    ///   stderr, then ExitProcess(1).
    ///
    ///   Stack: 6 pushes (48 B) + sub 0xB0 (176 B) = 224 B total delta.
    ///   Entry RSP ≡ 8 (mod 16); after 6 pushes → RSP ≡ 0; sub 176 → RSP ≡ 0.
    ///   Layout: [rsp+0..31]=shadow, [rsp+32..39]=5th-arg slot, [rsp+40..167]=buf.
    ///
    /// Entry layout (34 bytes):
    ///   AddVectoredExceptionHandler(1, handler) — fires before WER, unlike
    ///   SetUnhandledExceptionFilter which is bypassed on Vista+ for crashes.
    pub fn generate_windows(fn_offset: usize) -> Self {
        // lea rdx,[rip+handler_win] at mainCRTStartup+9 (stub 264): rip_after=271, target=0 → -271
        let [de0, de1, de2, de3] = (-271i32).to_le_bytes();

        // Inline name string offsets (data placed after handler code at off 192):
        //   av_str  at 192 ("access violation\0"        17 B)
        //   so_str  at 209 ("stack overflow\0"           15 B)
        //   dz_str  at 224 ("integer divide by zero\0"   23 B)
        //   unk_str at 247 ("unknown\0"                   8 B)
        // RIP-relative displacements (all within-stub, no reloc needed):
        //   lea r8,[rip+unk] at off 36, rip_after=43:  247-43=204=0xCC
        //   lea r8,[rip+av]  at off 51, rip_after=58:  192-58=134=0x86
        //   lea r8,[rip+so]  at off 68, rip_after=75:  209-75=134=0x86
        //   lea r8,[rip+dz]  at off 85, rip_after=92:  224-92=132=0x84
        //
        // Stack layout (sub rsp,176=0xB0; 5 pushes→RSP≡0; sub 176≡0→RSP≡0 aligned):
        //   [rsp+0..31]  = shadow space
        //   [rsp+32..39] = arg5 (FaultAddr for sprintf, NULL for WriteFile)
        //   [rsp+40..47] = arg6 (Rip for sprintf)
        //   [rsp+48..175]= 128-byte sprintf output buffer

        #[rustfmt::skip]
        let bytes: Vec<u8> = vec![
            // __void_crash_handler_win (offset 0, code 192 bytes)
            // rcx = EXCEPTION_POINTERS* on entry
            0x53,                                       // push rbx
            0x41, 0x54,                                 // push r12
            0x41, 0x55,                                 // push r13
            0x41, 0x56,                                 // push r14
            0x55,                                       // push rbp
            0x48, 0x81, 0xEC, 0xB0, 0x00, 0x00, 0x00,  // sub rsp, 176

            // Extract fault info
            0x48, 0x8B, 0x29,                           // mov rbp, [rcx]       (EXCEPTION_RECORD*)
            0x8B, 0x5D, 0x00,                           // mov ebx, [rbp+0]     (ExceptionCode)
            0x4C, 0x8B, 0x6D, 0x28,                    // mov r13, [rbp+40]    (ExceptionInformation[1]=fault addr)
            0x48, 0x8B, 0x41, 0x08,                    // mov rax, [rcx+8]     (CONTEXT*)
            0x4C, 0x8B, 0xB0, 0xF8, 0x00, 0x00, 0x00, // mov r14, [rax+0xF8]  (Rip)

            // Map ExceptionCode → name string in r8 (no reloc: within-stub RIP displacement)
            0x4C, 0x8D, 0x05, 0xCC, 0x00, 0x00, 0x00, // lea r8, [rip+unk_str]     off36, default
            0x81, 0xFB, 0x05, 0x00, 0x00, 0xC0,        // cmp ebx, 0xC0000005       off43
            0x75, 0x09,                                 // jne .check_so             off49 → 60
            0x4C, 0x8D, 0x05, 0x86, 0x00, 0x00, 0x00, // lea r8, [rip+av_str]      off51
            0xEB, 0x20,                                 // jmp .name_done            off58 → 92

            0x81, 0xFB, 0xFD, 0x00, 0x00, 0xC0,        // cmp ebx, 0xC00000FD  .check_so off60
            0x75, 0x09,                                 // jne .check_dz             off66 → 77
            0x4C, 0x8D, 0x05, 0x86, 0x00, 0x00, 0x00, // lea r8, [rip+so_str]      off68
            0xEB, 0x0F,                                 // jmp .name_done            off75 → 92

            0x81, 0xFB, 0x94, 0x00, 0x00, 0xC0,        // cmp ebx, 0xC0000094  .check_dz off77
            0x75, 0x07,                                 // jne .name_done            off83 → 92
            0x4C, 0x8D, 0x05, 0x84, 0x00, 0x00, 0x00, // lea r8, [rip+dz_str]      off85

            // .name_done: (off 92)
            // sprintf(buf, fmt, name_str, exception_code, fault_addr, rip)
            0x48, 0x8D, 0x4C, 0x24, 0x30,              // lea rcx, [rsp+48]    (buf)
            0x48, 0x8D, 0x15, 0x00, 0x00, 0x00, 0x00,  // lea rdx, [rip+__void_crash_fmt]  ← PC32@100
            0x41, 0x89, 0xD9,                           // mov r9d, ebx         (ExceptionCode = 4th arg)
            0x4C, 0x89, 0x6C, 0x24, 0x20,              // mov [rsp+32], r13    (FaultAddr = 5th arg)
            0x4C, 0x89, 0x74, 0x24, 0x28,              // mov [rsp+40], r14    (Rip = 6th arg)
            0xE8, 0x00, 0x00, 0x00, 0x00,              // call sprintf          ← PLT32@118
            0x44, 0x8B, 0xC0,                           // mov r12d, eax        (byte count)

            // GetStdHandle(STD_ERROR_HANDLE)
            0xB9, 0xF4, 0xFF, 0xFF, 0xFF,              // mov ecx, 0xFFFFFFF4
            0xE8, 0x00, 0x00, 0x00, 0x00,              // call GetStdHandle     ← PLT32@131
            0x48, 0x89, 0xC3,                           // mov rbx, rax         (stderr handle)

            // WriteFile(handle, buf, len, NULL, NULL)
            0x48, 0x89, 0xD9,                           // mov rcx, rbx
            0x48, 0x8D, 0x54, 0x24, 0x30,              // lea rdx, [rsp+48]    (buf)
            0x45, 0x8B, 0xC4,                           // mov r8d, r12d        (byte count)
            0x45, 0x31, 0xC9,                           // xor r9d, r9d         (NULL)
            0x48, 0xC7, 0x44, 0x24, 0x20,
            0x00, 0x00, 0x00, 0x00,                     // mov qword [rsp+32], 0 (NULL lpOverlapped)
            0xE8, 0x00, 0x00, 0x00, 0x00,              // call WriteFile        ← PLT32@162

            // ExitProcess(1)
            0xB9, 0x01, 0x00, 0x00, 0x00,              // mov ecx, 1
            0xE8, 0x00, 0x00, 0x00, 0x00,              // call ExitProcess      ← PLT32@172

            // unreachable epilogue
            0x48, 0x81, 0xC4, 0xB0, 0x00, 0x00, 0x00, // add rsp, 176
            0x5D,                                       // pop rbp
            0x41, 0x5E,                                 // pop r14
            0x41, 0x5D,                                 // pop r13
            0x41, 0x5C,                                 // pop r12
            0x5B,                                       // pop rbx
            0xC3,                                       // ret

            // Inline name strings (off 192..254, data in .text — not executed)
            // av_str:  "access violation\0"       off 192
            b'a',b'c',b'c',b'e',b's',b's',b' ',b'v',b'i',b'o',b'l',b'a',b't',b'i',b'o',b'n',0,
            // so_str:  "stack overflow\0"         off 209
            b's',b't',b'a',b'c',b'k',b' ',b'o',b'v',b'e',b'r',b'f',b'l',b'o',b'w',0,
            // dz_str:  "integer divide by zero\0" off 224
            b'i',b'n',b't',b'e',b'g',b'e',b'r',b' ',b'd',b'i',b'v',b'i',b'd',b'e',b' ',
            b'b',b'y',b' ',b'z',b'e',b'r',b'o',0,
            // unk_str: "unknown\0"                off 247
            b'u',b'n',b'k',b'n',b'o',b'w',b'n',0,

            // mainCRTStartup (offset 255, 34 bytes)
            0x48, 0x83, 0xEC, 0x28,                    // sub rsp, 40
            0xB9, 0x01, 0x00, 0x00, 0x00,              // mov ecx, 1  (First=1)
            0x48, 0x8D, 0x15, de0,de1,de2,de3,         // lea rdx,[rip+handler_win]
            0xE8, 0x00, 0x00, 0x00, 0x00,              // call AddVectoredExceptionHandler ← PLT32@272
            0xE8, 0x00, 0x00, 0x00, 0x00,              // call main                        ← PLT32@277
            0x48, 0x89, 0xC1,                           // mov rcx, rax
            0xE8, 0x00, 0x00, 0x00, 0x00,              // call ExitProcess                 ← PLT32@285
        ];

        let relocs = vec![
            PendingReloc {
                offset_in_text: fn_offset + 100,
                kind: RelocKind::Pc32,
                symbol: "__void_crash_fmt".into(),
                addend: -4,
            },
            PendingReloc {
                offset_in_text: fn_offset + 118,
                kind: RelocKind::Plt32,
                symbol: "sprintf".into(),
                addend: -4,
            },
            PendingReloc {
                offset_in_text: fn_offset + 131,
                kind: RelocKind::Plt32,
                symbol: "GetStdHandle".into(),
                addend: -4,
            },
            PendingReloc {
                offset_in_text: fn_offset + 162,
                kind: RelocKind::Plt32,
                symbol: "WriteFile".into(),
                addend: -4,
            },
            PendingReloc {
                offset_in_text: fn_offset + 172,
                kind: RelocKind::Plt32,
                symbol: "ExitProcess".into(),
                addend: -4,
            },
            PendingReloc {
                offset_in_text: fn_offset + 272,
                kind: RelocKind::Plt32,
                symbol: "AddVectoredExceptionHandler".into(),
                addend: -4,
            },
            PendingReloc {
                offset_in_text: fn_offset + 277,
                kind: RelocKind::Plt32,
                symbol: "main".into(),
                addend: -4,
            },
            PendingReloc {
                offset_in_text: fn_offset + 285,
                kind: RelocKind::Plt32,
                symbol: "ExitProcess".into(),
                addend: -4,
            },
        ];

        Self {
            bytes,
            relocs,
            extra_symbols: vec![("__void_crash_handler_win".into(), 0, 192)],
            start_offset: 255,
        }
    }
}
