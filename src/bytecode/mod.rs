// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

pub mod chunk;
pub mod codegen;
pub mod constprop;
pub mod instruction;
pub mod opcode;
pub mod regalloc;

pub use chunk::{Chunk, ConstPoolEntry, deserialize_qzi, serialize_qzi};
pub use codegen::Codegen;
pub use instruction::Instruction;
pub use opcode::Opcode;
