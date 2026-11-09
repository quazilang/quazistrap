// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Experimental `qz run --jit` (D-019): execute a freshly linked program from
//! memory inside the `qz` process instead of writing an executable to disk.
//!
//! The built-in linker produces a static x86-64 ELF image linked at fixed
//! addresses (no dynamic loader, no libc). This loader maps each `PT_LOAD`
//! segment at its link address with `MAP_FIXED_NOREPLACE`, builds the initial
//! process stack the program's `_start` expects, and jumps to the entry point.
//! The program ends the process with its own `exit_group` system call, so its
//! exit status becomes `qz`'s. Only Linux x86-64 is supported.
//!
//! Linking stores the image (`store_image`) instead of writing a file; the
//! build then finishes normally (temporary objects are removed) and the run
//! step executes the stored image (`take_image` + `execute`).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static REQUESTED: AtomicBool = AtomicBool::new(false);
static IMAGE: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// Record that `qz run --jit` asked for in-memory execution.
pub fn request() {
    REQUESTED.store(true, Ordering::Relaxed);
}

pub fn requested() -> bool {
    REQUESTED.load(Ordering::Relaxed)
}

/// Keep the linked executable image for the run step.
pub fn store_image(image: Vec<u8>) {
    *IMAGE.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(image);
}

/// The image stored by the last link, if any.
pub fn take_image() -> Option<Vec<u8>> {
    IMAGE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .take()
}

/// A loadable segment taken from the ELF program headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub offset: u64,
    pub address: u64,
    pub file_size: u64,
    pub memory_size: u64,
    pub flags: u32,
}

/// Parse a static little-endian x86-64 ELF executable: entry point and loads.
pub fn parse_image(image: &[u8]) -> Result<(u64, Vec<Segment>), String> {
    let read = |offset: usize, width: usize| -> Result<u64, String> {
        let bytes = image
            .get(offset..offset + width)
            .ok_or_else(|| "JIT: truncated ELF image".to_string())?;
        let mut value = 0u64;
        for (index, byte) in bytes.iter().enumerate() {
            value |= (*byte as u64) << (index * 8);
        }
        Ok(value)
    };
    if image.get(0..4) != Some(b"\x7fELF".as_slice())
        || image.get(4) != Some(&2)
        || image.get(5) != Some(&1)
    {
        return Err("JIT: not a little-endian ELF64 image".into());
    }
    if read(16, 2)? != 2 || read(18, 2)? != 62 {
        return Err("JIT: only static x86-64 executables (ET_EXEC) can run".into());
    }
    let entry = read(24, 8)?;
    let header_offset = read(32, 8)? as usize;
    let header_size = read(54, 2)? as usize;
    let header_count = read(56, 2)? as usize;
    let mut segments = Vec::new();
    for index in 0..header_count {
        let base = header_offset + index * header_size;
        match read(base, 4)? {
            1 => segments.push(Segment {
                flags: read(base + 4, 4)? as u32,
                offset: read(base + 8, 8)?,
                address: read(base + 16, 8)?,
                file_size: read(base + 32, 8)?,
                memory_size: read(base + 40, 8)?,
            }),
            // PT_DYNAMIC / PT_INTERP would need a dynamic loader.
            2 | 3 => return Err("JIT: dynamically linked programs are not supported".into()),
            _ => {}
        }
    }
    if segments.is_empty() {
        return Err("JIT: the image has no loadable segments".into());
    }
    Ok((entry, segments))
}

/// Build the initial stack content below `top` (16-byte aligned): argc, argv
/// pointers, NULL, envp pointers, NULL, an empty auxiliary vector, with the
/// strings stored above. Returns the words and string bytes to place at
/// `stack_pointer` and the resulting stack pointer.
pub fn initial_stack(top: u64, args: &[Vec<u8>], env: &[Vec<u8>]) -> (u64, Vec<u8>) {
    let mut strings = Vec::new();
    let mut offsets = Vec::new();
    for text in args.iter().chain(env) {
        offsets.push(strings.len() as u64);
        strings.extend_from_slice(text);
        strings.push(0);
    }
    let words = 1 + args.len() + 1 + env.len() + 1 + 2;
    let strings_start = (top - strings.len() as u64) & !15;
    let stack_pointer = (strings_start - (words as u64) * 8) & !15;
    let mut block = Vec::with_capacity((top - stack_pointer) as usize);
    let mut push = |value: u64| block.extend_from_slice(&value.to_le_bytes());
    push(args.len() as u64);
    for offset in &offsets[..args.len()] {
        push(strings_start + offset);
    }
    push(0);
    for offset in &offsets[args.len()..] {
        push(strings_start + offset);
    }
    push(0);
    push(0); // AT_NULL
    push(0);
    block.resize((strings_start - stack_pointer) as usize, 0);
    block.extend_from_slice(&strings);
    (stack_pointer, block)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod linux {
    use super::{Segment, initial_stack, parse_image};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    const PROT_READ: u64 = 1;
    const PROT_WRITE: u64 = 2;
    const PROT_EXEC: u64 = 4;
    const MAP_PRIVATE: u64 = 0x02;
    const MAP_ANONYMOUS: u64 = 0x20;
    const MAP_FIXED_NOREPLACE: u64 = 0x100000;
    const PAGE: u64 = 4096;
    const STACK_SIZE: u64 = 8 * 1024 * 1024;

    unsafe fn syscall6(number: u64, a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> i64 {
        let result: i64;
        unsafe {
            core::arch::asm!(
                "syscall",
                inlateout("rax") number as i64 => result,
                in("rdi") a, in("rsi") b, in("rdx") c,
                in("r10") d, in("r8") e, in("r9") f,
                lateout("rcx") _, lateout("r11") _,
                options(nostack),
            );
        }
        result
    }

    fn mmap(address: u64, length: u64, protection: u64, flags: u64) -> Result<u64, String> {
        let result = unsafe { syscall6(9, address, length, protection, flags, u64::MAX, 0) };
        if result < 0 {
            return Err(format!(
                "JIT: cannot map {length} bytes at {address:#x} (errno {})",
                -result
            ));
        }
        Ok(result as u64)
    }

    fn mprotect(address: u64, length: u64, protection: u64) -> Result<(), String> {
        let result = unsafe { syscall6(10, address, length, protection, 0, 0, 0) };
        if result < 0 {
            return Err(format!(
                "JIT: cannot protect {address:#x} (errno {})",
                -result
            ));
        }
        Ok(())
    }

    fn map_segment(image: &[u8], segment: &Segment) -> Result<(), String> {
        let start = segment.address & !(PAGE - 1);
        let end = (segment.address + segment.memory_size + PAGE - 1) & !(PAGE - 1);
        if end <= start {
            return Ok(());
        }
        let mapped = mmap(
            start,
            end - start,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE,
        )?;
        if mapped != start {
            return Err(format!(
                "JIT: segment {start:#x} is occupied in this process"
            ));
        }
        let data = image
            .get(segment.offset as usize..(segment.offset + segment.file_size) as usize)
            .ok_or_else(|| "JIT: segment data lies outside the image".to_string())?;
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), segment.address as *mut u8, data.len());
        }
        // Keep W^X: executable segments become read+execute only.
        let mut protection = 0;
        if segment.flags & 4 != 0 {
            protection |= PROT_READ;
        }
        if segment.flags & 2 != 0 {
            protection |= PROT_WRITE;
        }
        if segment.flags & 1 != 0 {
            protection |= PROT_EXEC;
        }
        mprotect(start, end - start, protection)
    }

    /// A fresh `exec` starts with default handlers; `qz`'s Rust runtime
    /// ignores SIGPIPE and handles SIGSEGV/SIGBUS for stack-overflow reports.
    fn reset_signal_dispositions() -> Result<(), String> {
        const SIGBUS: u64 = 7;
        const SIGSEGV: u64 = 11;
        const SIGPIPE: u64 = 13;
        // struct kernel_sigaction { handler = SIG_DFL, flags, restorer, mask }
        let default_action = [0u64; 4];
        for signal in [SIGBUS, SIGSEGV, SIGPIPE] {
            let result =
                unsafe { syscall6(13, signal, default_action.as_ptr() as u64, 0, 8, 0, 0) };
            if result < 0 {
                return Err(format!(
                    "JIT: cannot reset signal {signal} (errno {})",
                    -result
                ));
            }
        }
        Ok(())
    }

    pub fn execute(image: &[u8], args: &[Vec<u8>]) -> Result<std::convert::Infallible, String> {
        let (entry, segments) = parse_image(image)?;
        for segment in &segments {
            map_segment(image, segment)?;
        }
        let stack = mmap(
            0,
            STACK_SIZE,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANONYMOUS,
        )?;
        // Raw bytes: the environment need not be UTF-8.
        let env: Vec<Vec<u8>> = std::env::vars_os()
            .map(|(key, value)| {
                let mut entry = key.into_vec();
                entry.push(b'=');
                entry.extend_from_slice(value.as_bytes());
                entry
            })
            .collect();
        let (stack_pointer, block) = initial_stack(stack + STACK_SIZE, args, &env);
        if block.len() as u64 > STACK_SIZE / 2 {
            return Err("JIT: arguments and environment are too large".into());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(block.as_ptr(), stack_pointer as *mut u8, block.len());
        }
        reset_signal_dispositions()?;
        unsafe {
            // Enter `_start` as the kernel would: rsp at argc, rdx = no
            // dynamic-linker finalizer, rbp = 0. The program exits itself.
            // Fixed registers: `xor edx` must not clobber an operand.
            core::arch::asm!(
                "mov rsp, rdi",
                "xor ebp, ebp",
                "xor edx, edx",
                "jmp rax",
                in("rdi") stack_pointer,
                in("rax") entry,
                options(noreturn),
            );
        }
    }
}

/// Run `image` in this process. Returns only on a loading error.
pub fn execute(image: &[u8], args: &[Vec<u8>]) -> Result<std::convert::Infallible, String> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        linux::execute(image, args)
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        let _ = (image, args);
        Err("qz run --jit currently supports Linux x86-64 hosts only".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_stack_lays_out_argc_argv_envp_and_auxv() {
        let top = 0x10000u64;
        let (stack_pointer, block) = initial_stack(top, &[b"prog".to_vec()], &[b"A=1".to_vec()]);
        assert_eq!(stack_pointer % 16, 0);
        let word =
            |index: usize| u64::from_le_bytes(block[index * 8..index * 8 + 8].try_into().unwrap());
        assert_eq!(word(0), 1, "argc");
        let argv0 = word(1);
        assert_eq!(word(2), 0, "argv terminator");
        let envp0 = word(3);
        assert_eq!(word(4), 0, "envp terminator");
        assert_eq!((word(5), word(6)), (0, 0), "AT_NULL");
        let string = |address: u64| {
            let start = (address - stack_pointer) as usize;
            let end = start + block[start..].iter().position(|byte| *byte == 0).unwrap();
            String::from_utf8(block[start..end].to_vec()).unwrap()
        };
        assert_eq!(string(argv0), "prog");
        assert_eq!(string(envp0), "A=1");
    }

    #[test]
    fn parses_the_builtin_linker_image() {
        // A minimal ELF64 ET_EXEC header with one PT_LOAD program header.
        let mut image = vec![0u8; 64 + 56];
        image[..4].copy_from_slice(b"\x7fELF");
        image[4] = 2;
        image[5] = 1;
        image[16] = 2;
        image[18] = 62;
        image[24..32].copy_from_slice(&0x401000u64.to_le_bytes());
        image[32..40].copy_from_slice(&64u64.to_le_bytes());
        image[54] = 56;
        image[56] = 1;
        image[64] = 1;
        image[68] = 5;
        image[80..88].copy_from_slice(&0x400000u64.to_le_bytes());
        image[96..104].copy_from_slice(&120u64.to_le_bytes());
        image[104..112].copy_from_slice(&120u64.to_le_bytes());
        let (entry, segments) = parse_image(&image).expect("parse");
        assert_eq!(entry, 0x401000);
        assert_eq!(segments[0].address, 0x400000);
        assert_eq!(segments[0].flags, 5);
        image[64] = 3; // PT_INTERP
        assert!(parse_image(&image).is_err());
    }
}
