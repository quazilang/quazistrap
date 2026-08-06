// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

/// A target-neutral description of a value crossing the C ABI.
///
/// QZI stores source-level widths and aggregate layouts, not SysV or Win64
/// register classes. Each native backend classifies the same description for
/// its own calling convention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbiType {
    Void,
    Integer {
        bytes: u8,
        signed: bool,
    },
    Float32,
    Float64,
    Pointer,
    Aggregate {
        size: u16,
        align: u8,
        fields: Vec<AbiField>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbiField {
    pub offset: u16,
    pub ty: AbiType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbiSignature {
    /// All physical arguments at this call site. For a C-variadic call this
    /// includes the promoted variadic arguments as well as the fixed prefix.
    pub params: Vec<AbiType>,
    pub return_type: AbiType,
    pub variadic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignSymbol {
    pub symbol: String,
    pub signature: AbiSignature,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignGlobal {
    pub symbol: String,
    pub ty: AbiType,
}

impl AbiType {
    pub fn size(&self) -> usize {
        match self {
            Self::Void => 0,
            Self::Integer { bytes, .. } => *bytes as usize,
            Self::Float32 => 4,
            Self::Float64 | Self::Pointer => 8,
            Self::Aggregate { size, .. } => *size as usize,
        }
    }

    pub fn align(&self) -> usize {
        match self {
            Self::Void => 1,
            Self::Integer { bytes, .. } => (*bytes).max(1) as usize,
            Self::Float32 => 4,
            Self::Float64 | Self::Pointer => 8,
            Self::Aggregate { align, .. } => (*align).max(1) as usize,
        }
    }

    pub(crate) fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::Void => out.push(0),
            Self::Integer { bytes, signed } => {
                out.push(1);
                out.push(*bytes);
                out.push(u8::from(*signed));
            }
            Self::Float32 => out.push(2),
            Self::Float64 => out.push(3),
            Self::Pointer => out.push(4),
            Self::Aggregate {
                size,
                align,
                fields,
            } => {
                out.push(5);
                out.extend_from_slice(&size.to_le_bytes());
                out.push(*align);
                out.extend_from_slice(&(fields.len() as u16).to_le_bytes());
                for field in fields {
                    out.extend_from_slice(&field.offset.to_le_bytes());
                    field.ty.encode(out);
                }
            }
        }
    }

    pub(crate) fn decode(input: &[u8], pos: &mut usize) -> Result<Self, String> {
        let tag = take_u8(input, pos, "ABI type tag")?;
        match tag {
            0 => Ok(Self::Void),
            1 => {
                let bytes = take_u8(input, pos, "ABI integer width")?;
                let signed = take_u8(input, pos, "ABI integer signedness")? != 0;
                if !matches!(bytes, 1 | 2 | 4 | 8) {
                    return Err(format!("invalid ABI integer width {bytes}"));
                }
                Ok(Self::Integer { bytes, signed })
            }
            2 => Ok(Self::Float32),
            3 => Ok(Self::Float64),
            4 => Ok(Self::Pointer),
            5 => {
                let size = take_u16(input, pos, "ABI aggregate size")?;
                let align = take_u8(input, pos, "ABI aggregate alignment")?;
                let field_count = take_u16(input, pos, "ABI aggregate field count")? as usize;
                let mut fields = Vec::with_capacity(field_count);
                for _ in 0..field_count {
                    let offset = take_u16(input, pos, "ABI field offset")?;
                    let ty = Self::decode(input, pos)?;
                    fields.push(AbiField { offset, ty });
                }
                Ok(Self::Aggregate {
                    size,
                    align,
                    fields,
                })
            }
            _ => Err(format!("unknown ABI type tag {tag}")),
        }
    }
}

impl AbiSignature {
    pub(crate) fn encode(&self, out: &mut Vec<u8>) {
        out.push(u8::from(self.variadic));
        out.extend_from_slice(&(self.params.len() as u16).to_le_bytes());
        for param in &self.params {
            param.encode(out);
        }
        self.return_type.encode(out);
    }

    pub(crate) fn decode(input: &[u8], pos: &mut usize) -> Result<Self, String> {
        let variadic = take_u8(input, pos, "ABI variadic flag")? != 0;
        let param_count = take_u16(input, pos, "ABI parameter count")? as usize;
        let mut params = Vec::with_capacity(param_count);
        for _ in 0..param_count {
            params.push(AbiType::decode(input, pos)?);
        }
        let return_type = AbiType::decode(input, pos)?;
        Ok(Self {
            params,
            return_type,
            variadic,
        })
    }
}

impl ForeignSymbol {
    pub(crate) fn encode(&self, out: &mut Vec<u8>) {
        let name = self.symbol.as_bytes();
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name);
        self.signature.encode(out);
    }

    pub(crate) fn decode(input: &[u8], pos: &mut usize) -> Result<Self, String> {
        let name_len = take_u16(input, pos, "foreign symbol length")? as usize;
        if input.len() < *pos + name_len {
            return Err("truncated foreign symbol".to_string());
        }
        let symbol = String::from_utf8(input[*pos..*pos + name_len].to_vec())
            .map_err(|_| "invalid UTF-8 in foreign symbol".to_string())?;
        *pos += name_len;
        let signature = AbiSignature::decode(input, pos)?;
        Ok(Self { symbol, signature })
    }
}

impl ForeignGlobal {
    pub(crate) fn encode(&self, out: &mut Vec<u8>) {
        let name = self.symbol.as_bytes();
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name);
        self.ty.encode(out);
    }

    pub(crate) fn decode(input: &[u8], pos: &mut usize) -> Result<Self, String> {
        let name_len = take_u16(input, pos, "foreign global symbol length")? as usize;
        if input.len() < *pos + name_len {
            return Err("truncated foreign global symbol".to_string());
        }
        let symbol = String::from_utf8(input[*pos..*pos + name_len].to_vec())
            .map_err(|_| "invalid UTF-8 in foreign global symbol".to_string())?;
        *pos += name_len;
        let ty = AbiType::decode(input, pos)?;
        Ok(Self { symbol, ty })
    }
}

fn take_u8(input: &[u8], pos: &mut usize, what: &str) -> Result<u8, String> {
    if input.len() <= *pos {
        return Err(format!("truncated {what}"));
    }
    let value = input[*pos];
    *pos += 1;
    Ok(value)
}

fn take_u16(input: &[u8], pos: &mut usize, what: &str) -> Result<u16, String> {
    if input.len() < *pos + 2 {
        return Err(format!("truncated {what}"));
    }
    let value = u16::from_le_bytes(input[*pos..*pos + 2].try_into().unwrap());
    *pos += 2;
    Ok(value)
}
