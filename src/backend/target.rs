// void - the programming language
// Copyright titago (C) 2026
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
}

impl TargetSpec {
    pub fn host() -> Self {
        #[cfg(target_os = "linux")]
        return Self {
            arch: Arch::X86_64,
            os: Os::Linux,
            abi: Abi::SysV,
            emit_start: true,
        };

        #[cfg(target_os = "macos")]
        return Self {
            arch: Arch::X86_64,
            os: Os::MacOs,
            abi: Abi::SysV,
            emit_start: false,
        };

        #[cfg(target_os = "windows")]
        return Self {
            arch: Arch::X86_64,
            os: Os::Windows,
            abi: Abi::Win64,
            emit_start: true, // emit mainCRTStartup stub
        };

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        return Self {
            arch: Arch::X86_64,
            os: Os::Linux,
            abi: Abi::SysV,
            emit_start: true,
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
}
