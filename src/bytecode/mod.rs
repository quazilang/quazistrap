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
pub mod vtable;

pub use chunk::{
    Chunk, ConstPoolEntry, QziCallRelocation, QziContentBinding, QziMetadata, QziModule,
    QziModuleKind, QziOwnershipArtifact,
    deserialize_qzi, deserialize_qzi_module, link_qzi_modules, select_qzi_target, serialize_qzi,
    serialize_qzi_module, serialize_unified_qzi, serialize_universal_qzi, universal_qzi_variants,
};
pub use codegen::Codegen;
pub use instruction::Instruction;
pub use interface::{build_qzi_interface, parse_qzi_interface};
pub use opcode::Opcode;
