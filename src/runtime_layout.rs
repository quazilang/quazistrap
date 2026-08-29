// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use crate::parser::ast::TypeKind;

/// Target-neutral physical shape of a resolved Quazi value in the internal
/// bytecode ABI.
///
/// This is deliberately separate from `AbiType`: the latter describes values
/// crossing a platform C ABI, while this type describes Quazi virtual-register
/// slots. Types must be alias-resolved and generic parameters substituted before
/// asking for their layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeValueLayout {
    /// No value crosses the ABI boundary (`void` and `!`).
    Empty,
    /// A scalar or an indirect handle carried in one eight-byte VM slot.
    Slot,
    /// An inline value carried in a contiguous virtual-register block.
    RegisterBlock { slots: usize },
    /// A slice is physically passed as a pointer and a length.
    Slice,
    /// Flexible arrays have no standalone runtime representation.
    Unsized,
    /// Recovery-only or erased types cannot enter executable bytecode.
    Unrepresentable,
}

impl RuntimeValueLayout {
    pub fn slot_count(self) -> Option<usize> {
        match self {
            Self::Empty => Some(0),
            Self::Slot => Some(1),
            Self::RegisterBlock { slots } => Some(slots),
            Self::Slice => Some(2),
            Self::Unsized | Self::Unrepresentable => None,
        }
    }

    pub fn fits_single_slot(self) -> bool {
        self.slot_count() == Some(1)
    }

    /// Byte footprint of one value in container storage. The current internal
    /// model stores every value in whole eight-byte slots, so this derives
    /// from the slot count; sized per-type layouts arrive with the generic
    /// storage milestone.
    pub fn byte_size(self) -> Option<usize> {
        self.slot_count().map(|slots| slots * 8)
    }

    /// Alignment of one value in container storage. Every internal value is
    /// slot-aligned today.
    pub fn align(self) -> Option<usize> {
        self.slot_count().map(|_| 8)
    }
}

/// Whether copying a value's slot bits produces an independent value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveKind {
    /// A bit-copy is an independent value (scalars, immutable views, raw
    /// pointers, C function pointers, borrows).
    Plain,
    /// A bit-copy creates a second apparent owner; the source must be
    /// deactivated (named aggregates, `String`, `fn` environments, `dyn`).
    Owned,
}

/// Recorded internal-ABI layout of one resolved type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutInfo {
    pub layout: RuntimeValueLayout,
    pub move_kind: MoveKind,
}

/// Recorded internal-ABI layout of a function signature after alias
/// resolution and generic substitution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnValueLayout {
    /// One entry per fixed parameter, in declaration order.
    pub params: Vec<LayoutInfo>,
    /// The element layout of a trailing Quazi variadic parameter, if any.
    pub variadic_element: Option<LayoutInfo>,
    pub result: LayoutInfo,
}

/// Return whether a resolved source type can be duplicated by copying its
/// slot bits.
pub fn runtime_move_kind(ty: &TypeKind) -> MoveKind {
    match ty {
        TypeKind::Named { .. } | TypeKind::Fn { .. } | TypeKind::Dyn { .. } => MoveKind::Owned,
        TypeKind::Array { elem_ty, .. } => runtime_move_kind(&elem_ty.node),
        _ => MoveKind::Plain,
    }
}

/// Return the recorded internal-ABI layout for a fully resolved source type.
pub fn runtime_layout_info(ty: &TypeKind) -> LayoutInfo {
    LayoutInfo {
        layout: runtime_value_layout(ty),
        move_kind: runtime_move_kind(ty),
    }
}

/// Return the internal runtime layout for a fully resolved source type.
pub fn runtime_value_layout(ty: &TypeKind) -> RuntimeValueLayout {
    match ty {
        TypeKind::Void | TypeKind::Never => RuntimeValueLayout::Empty,
        TypeKind::Error | TypeKind::Any => RuntimeValueLayout::Unrepresentable,
        TypeKind::FlexibleArray { .. } => RuntimeValueLayout::Unsized,
        TypeKind::Slice { .. } => RuntimeValueLayout::Slice,
        TypeKind::Array { elem_ty, len } => {
            let Some(element_slots) = runtime_value_layout(&elem_ty.node).slot_count() else {
                return RuntimeValueLayout::Unrepresentable;
            };
            let Ok(length) = usize::try_from(*len) else {
                return RuntimeValueLayout::Unrepresentable;
            };
            let Some(slots) = element_slots.checked_mul(length) else {
                return RuntimeValueLayout::Unrepresentable;
            };
            RuntimeValueLayout::RegisterBlock { slots }
        }
        TypeKind::Int8
        | TypeKind::Int16
        | TypeKind::Int32
        | TypeKind::Int64
        | TypeKind::Uint8
        | TypeKind::Uint16
        | TypeKind::Uint32
        | TypeKind::Uint64
        | TypeKind::Isize
        | TypeKind::Usize
        | TypeKind::Float16
        | TypeKind::Float32
        | TypeKind::Float64
        | TypeKind::Bool
        | TypeKind::Str
        | TypeKind::Bytes
        | TypeKind::Named { .. }
        | TypeKind::Ref { .. }
        | TypeKind::RawPtr { .. }
        | TypeKind::Fn { .. }
        | TypeKind::CFn { .. }
        | TypeKind::Dyn { .. } => RuntimeValueLayout::Slot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{Span, Spanned};

    fn ty(node: TypeKind) -> Spanned<TypeKind> {
        Spanned::new(node, Span::new(1, 1, 0, 0))
    }

    #[test]
    fn distinguishes_handles_blocks_and_slices() {
        assert_eq!(
            runtime_value_layout(&TypeKind::Int32),
            RuntimeValueLayout::Slot
        );
        assert_eq!(
            runtime_value_layout(&TypeKind::Named {
                name: "Header".to_string(),
                type_args: vec![],
            }),
            RuntimeValueLayout::Slot
        );
        assert_eq!(
            runtime_value_layout(&TypeKind::Array {
                elem_ty: Box::new(ty(TypeKind::Int64)),
                len: 3,
            }),
            RuntimeValueLayout::RegisterBlock { slots: 3 }
        );
        assert_eq!(
            runtime_value_layout(&TypeKind::Slice {
                elem_ty: Box::new(ty(TypeKind::Int64)),
            }),
            RuntimeValueLayout::Slice
        );
    }

    #[test]
    fn nested_fixed_arrays_multiply_their_register_shape() {
        let inner = TypeKind::Array {
            elem_ty: Box::new(ty(TypeKind::Uint8)),
            len: 3,
        };
        assert_eq!(
            runtime_value_layout(&TypeKind::Array {
                elem_ty: Box::new(ty(inner)),
                len: 2,
            }),
            RuntimeValueLayout::RegisterBlock { slots: 6 }
        );
    }

    #[test]
    fn move_kind_distinguishes_plain_copies_from_owners() {
        assert_eq!(runtime_move_kind(&TypeKind::Int32), MoveKind::Plain);
        assert_eq!(runtime_move_kind(&TypeKind::Str), MoveKind::Plain);
        assert_eq!(
            runtime_move_kind(&TypeKind::RawPtr {
                inner: Box::new(ty(TypeKind::Uint8)),
            }),
            MoveKind::Plain
        );
        for owner in [
            TypeKind::Named {
                name: "String".to_string(),
                type_args: vec![],
            },
            TypeKind::Fn {
                params: vec![],
                return_ty: Box::new(ty(TypeKind::Void)),
            },
            TypeKind::Dyn {
                trait_name: "Display".to_string(),
            },
        ] {
            assert_eq!(runtime_move_kind(&owner), MoveKind::Owned, "{owner:?}");
        }
        let owned_block = TypeKind::Array {
            elem_ty: Box::new(ty(TypeKind::Named {
                name: "String".to_string(),
                type_args: vec![],
            })),
            len: 2,
        };
        assert_eq!(runtime_move_kind(&owned_block), MoveKind::Owned);
    }

    #[test]
    fn layout_info_tracks_container_storage_facts() {
        let block = runtime_layout_info(&TypeKind::Array {
            elem_ty: Box::new(ty(TypeKind::Int64)),
            len: 3,
        });
        assert_eq!(block.layout.byte_size(), Some(24));
        assert_eq!(block.layout.align(), Some(8));
        assert_eq!(block.move_kind, MoveKind::Plain);
        let owner = runtime_layout_info(&TypeKind::Named {
            name: "String".to_string(),
            type_args: vec![],
        });
        assert_eq!(owner.layout.byte_size(), Some(8));
        assert_eq!(owner.move_kind, MoveKind::Owned);
        assert_eq!(
            runtime_layout_info(&TypeKind::Void).layout.byte_size(),
            Some(0)
        );
    }
}
