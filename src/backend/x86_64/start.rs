// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD
//
// Entry-point stubs (no CRT needed).
//
// Linux _start:
//   xor rbp, rbp          ; clear frame pointer (ABI requirement)
//   call main             ; E8 rel32 — PLT32 reloc → "main"
//   mov rdi, rax          ; exit code
//   mov rax, 60           ; sys_exit
//   syscall
//
// Windows mainCRTStartup (Win64):
//   sub rsp, 40           ; align + 32-byte shadow space for callee
//   call main             ; E8 rel32 — REL32 reloc → "main"
//   mov rcx, rax          ; ExitProcess(exitCode)
//   call ExitProcess      ; E8 rel32 — REL32 reloc → "ExitProcess"

use super::relocations::{PendingReloc, RelocKind};

pub struct StartStub {
    pub bytes: Vec<u8>,
    pub relocs: Vec<PendingReloc>,
}

impl StartStub {
    /// Linux _start stub.
    pub fn generate(fn_offset: usize) -> Self {
        let bytes: Vec<u8> = vec![
            0x48, 0x31, 0xED,                         // xor rbp, rbp
            0xE8, 0x00, 0x00, 0x00, 0x00,             // call main
            0x48, 0x89, 0xC7,                         // mov rdi, rax
            0x48, 0xC7, 0xC0, 0x3C, 0x00, 0x00, 0x00, // mov rax, 60
            0x0F, 0x05,                               // syscall
        ];
        let relocs = vec![PendingReloc {
            offset_in_text: fn_offset + 4,
            kind: RelocKind::Plt32,
            symbol: "main".into(),
            addend: -4,
        }];
        Self { bytes, relocs }
    }

    /// Windows mainCRTStartup stub (Win64 ABI).
    /// Calls main() then ExitProcess(return_value) from kernel32.
    pub fn generate_windows(fn_offset: usize) -> Self {
        // sub rsp, 40     (4 bytes): 40 = 32 shadow + 8 alignment
        // call main       (5 bytes): E8 rel32
        // mov rcx, rax    (3 bytes): 48 89 C1
        // call ExitProcess(5 bytes): E8 rel32
        let bytes: Vec<u8> = vec![
            0x48, 0x83, 0xEC, 0x28,       // sub rsp, 40
            0xE8, 0x00, 0x00, 0x00, 0x00, // call main
            0x48, 0x89, 0xC1,             // mov rcx, rax
            0xE8, 0x00, 0x00, 0x00, 0x00, // call ExitProcess
        ];
        // call main displacement at fn_offset + 4+1 = fn_offset+5
        // call ExitProcess displacement at fn_offset + 4+5+3+1 = fn_offset+13
        let relocs = vec![
            PendingReloc {
                offset_in_text: fn_offset + 5,
                kind: RelocKind::Plt32,
                symbol: "main".into(),
                addend: -4,
            },
            PendingReloc {
                offset_in_text: fn_offset + 13,
                kind: RelocKind::Plt32,
                symbol: "ExitProcess".into(),
                addend: -4,
            },
        ];
        Self { bytes, relocs }
    }
}
