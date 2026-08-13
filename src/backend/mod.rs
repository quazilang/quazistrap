// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

pub mod builtin_linker;
pub mod builtin_pe_linker;
pub mod linker;
pub mod target;
pub mod x86_64;

pub use target::TargetSpec;

use crate::bytecode::Chunk;
use crate::semantic::SemanticReport;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectFormat {
    Elf,
    PeCoff,
    MachO,
}

#[derive(Debug)]
pub struct ObjectOutput {
    pub bytes: Vec<u8>,
    pub format: ObjectFormat,
}

#[derive(Debug)]
pub struct BackendError(pub String);

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub trait Backend {
    fn compile(
        &self,
        chunks: &[Chunk],
        target: &TargetSpec,
        report: Option<&SemanticReport>,
        main_takes_args: bool,
    ) -> Result<ObjectOutput, BackendError>;
}

pub fn select_backend(target: &TargetSpec) -> Box<dyn Backend> {
    match target.os {
        target::Os::Windows => Box::new(x86_64::PeBackend),
        _ => Box::new(x86_64::ElfBackend),
    }
}
