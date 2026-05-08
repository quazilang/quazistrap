// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

pub mod chunk;
pub mod codegen;
pub mod instruction;
pub mod opcode;

pub use chunk::{Chunk, ConstPoolEntry, serialize_vbc};
pub use codegen::Codegen;
pub use instruction::Instruction;
pub use opcode::Opcode;
