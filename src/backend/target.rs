// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Windows,
    MacOs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Abi {
    SysV,
    Win64,
}

#[derive(Debug, Clone)]
pub struct TargetSpec {
    pub arch: Arch,
    pub os: Os,
    pub abi: Abi,
    /// Whether to emit a _start stub (false for -c / shared lib).
    pub emit_start: bool,
    /// Whether to omit crash-handler registration in the entry stub.
    pub no_crash: bool,
}

impl TargetSpec {
    pub fn x86_64_linux() -> Self {
        Self {
            arch: Arch::X86_64,
            os: Os::Linux,
            abi: Abi::SysV,
            emit_start: true,
            no_crash: false,
        }
    }

    pub fn x86_64_windows() -> Self {
        Self {
            arch: Arch::X86_64,
            os: Os::Windows,
            abi: Abi::Win64,
            emit_start: true,
            no_crash: false,
        }
    }

    pub fn triple(&self) -> &'static str {
        match self.os {
            Os::Linux => "x86_64-linux",
            Os::Windows => "x86_64-windows",
            Os::MacOs => "x86_64-macos",
        }
    }

    pub fn host() -> Self {
        #[cfg(target_os = "linux")]
        return Self {
            arch: Arch::X86_64,
            os: Os::Linux,
            abi: Abi::SysV,
            emit_start: true,
            no_crash: false,
        };

        #[cfg(target_os = "macos")]
        return Self {
            arch: Arch::X86_64,
            os: Os::MacOs,
            abi: Abi::SysV,
            emit_start: false,
            no_crash: false,
        };

        #[cfg(target_os = "windows")]
        return Self {
            arch: Arch::X86_64,
            os: Os::Windows,
            abi: Abi::Win64,
            emit_start: true, // emit mainCRTStartup stub
            no_crash: false,
        };

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        return Self {
            arch: Arch::X86_64,
            os: Os::Linux,
            abi: Abi::SysV,
            emit_start: true,
            no_crash: false,
        };
    }

    pub fn binary_format(&self) -> object::BinaryFormat {
        match self.os {
            Os::Linux | Os::MacOs => object::BinaryFormat::Elf,
            Os::Windows => object::BinaryFormat::Coff,
        }
    }

    pub fn object_architecture(&self) -> object::Architecture {
        match self.arch {
            Arch::X86_64 => object::Architecture::X86_64,
        }
    }

    pub fn without_start(mut self) -> Self {
        self.emit_start = false;
        self
    }

    pub fn with_no_crash(mut self) -> Self {
        self.no_crash = true;
        self
    }
}
