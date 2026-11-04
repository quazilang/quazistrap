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
    None,
    Windows,
    NetBsd,
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
    /// Whether to emit the platform process entry stub (`_start` on Linux,
    /// `mainCRTStartup` on Windows). False for compile-only objects and shared libraries.
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

    /// Freestanding x86-64 ELF: no OS startup, syscall ABI, or implicit runtime.
    /// The project supplies its own entry point (typically from an assembly file).
    pub fn x86_64_none() -> Self {
        Self {
            arch: Arch::X86_64,
            os: Os::None,
            abi: Abi::SysV,
            emit_start: false,
            no_crash: true,
        }
    }

    pub fn x86_64_netbsd() -> Self {
        Self {
            arch: Arch::X86_64,
            os: Os::NetBsd,
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
            Os::None => "x86_64-none",
            Os::Windows => "x86_64-windows",
            Os::NetBsd => "x86_64-netbsd",
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

        #[cfg(target_os = "netbsd")]
        return Self::x86_64_netbsd();

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

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "netbsd", target_os = "windows")))]
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
            Os::Linux | Os::None | Os::NetBsd | Os::MacOs => object::BinaryFormat::Elf,
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
