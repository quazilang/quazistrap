// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

pub mod chunk;
pub mod codegen;
pub mod constprop;
pub mod instruction;
pub mod interface;
pub mod opcode;
pub mod regalloc;

pub use chunk::{
    Chunk, ConstPoolEntry, QziCallRelocation, QziMetadata, QziModule, QziModuleKind,
    deserialize_qzi, deserialize_qzi_module, link_qzi_modules, serialize_qzi, serialize_qzi_module,
};
pub use codegen::Codegen;
pub use instruction::Instruction;
pub use interface::{build_qzi_interface, parse_qzi_interface};
pub use opcode::Opcode;
