// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use crate::abi::AbiType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EightbyteClass {
    Integer,
    Sse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeClass {
    Registers(Vec<EightbyteClass>),
    Memory,
}

/// Classify one C ABI value according to the SysV AMD64 eightbyte rules for
/// the scalar and naturally-aligned `@repr(C)` types Quazi currently exposes.
pub fn classify(ty: &AbiType) -> TypeClass {
    match ty {
        AbiType::Void => TypeClass::Registers(Vec::new()),
        AbiType::Integer { .. } | AbiType::Pointer => {
            TypeClass::Registers(vec![EightbyteClass::Integer])
        }
        AbiType::Float32 | AbiType::Float64 => TypeClass::Registers(vec![EightbyteClass::Sse]),
        AbiType::Aggregate {
            size,
            align: _,
            fields,
        } => {
            let size = *size as usize;
            if size == 0 || size > 16 {
                return TypeClass::Memory;
            }
            let count = size.div_ceil(8);
            let mut classes: Vec<Option<EightbyteClass>> = vec![None; count];
            for field in fields {
                let field_size = field.ty.size();
                let field_align = field.ty.align();
                let offset = field.offset as usize;
                if field_size == 0
                    || !offset.is_multiple_of(field_align)
                    || offset + field_size > size
                {
                    return TypeClass::Memory;
                }
                let TypeClass::Registers(field_classes) = classify(&field.ty) else {
                    return TypeClass::Memory;
                };
                for (index, class) in field_classes.into_iter().enumerate() {
                    let aggregate_index = offset / 8 + index;
                    if aggregate_index >= classes.len() {
                        return TypeClass::Memory;
                    }
                    classes[aggregate_index] = Some(merge(classes[aggregate_index], class));
                }
            }
            TypeClass::Registers(
                classes
                    .into_iter()
                    // Padding-only eightbytes are carried as integer data.
                    .map(|class| class.unwrap_or(EightbyteClass::Integer))
                    .collect(),
            )
        }
    }
}

fn merge(current: Option<EightbyteClass>, incoming: EightbyteClass) -> EightbyteClass {
    match (current, incoming) {
        (Some(EightbyteClass::Integer), _) | (_, EightbyteClass::Integer) => {
            EightbyteClass::Integer
        }
        _ => EightbyteClass::Sse,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::AbiField;

    #[test]
    fn classifies_two_doubles_as_two_sse_eightbytes() {
        let point = AbiType::Aggregate {
            size: 16,
            align: 8,
            fields: vec![
                AbiField {
                    offset: 0,
                    ty: AbiType::Float64,
                },
                AbiField {
                    offset: 8,
                    ty: AbiType::Float64,
                },
            ],
        };
        assert_eq!(
            classify(&point),
            TypeClass::Registers(vec![EightbyteClass::Sse, EightbyteClass::Sse])
        );
    }

    #[test]
    fn integer_wins_when_fields_share_an_eightbyte() {
        let mixed = AbiType::Aggregate {
            size: 8,
            align: 4,
            fields: vec![
                AbiField {
                    offset: 0,
                    ty: AbiType::Float32,
                },
                AbiField {
                    offset: 4,
                    ty: AbiType::Integer {
                        bytes: 4,
                        signed: true,
                    },
                },
            ],
        };
        assert_eq!(
            classify(&mixed),
            TypeClass::Registers(vec![EightbyteClass::Integer])
        );
    }

    #[test]
    fn large_aggregate_uses_memory() {
        assert_eq!(
            classify(&AbiType::Aggregate {
                size: 24,
                align: 8,
                fields: Vec::new(),
            }),
            TypeClass::Memory
        );
    }
}
