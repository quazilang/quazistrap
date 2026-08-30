// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::bytecode::codegen::{CachedCodegenCall, CachedCodegenUnit};
use crate::parser::ast::{Attribute, ItemKind, Program, TypeKind};

const QZC_MAGIC: &[u8; 4] = b"\0QZC";
const QZC_VERSION: u8 = 6;

#[derive(Debug, Clone)]
pub struct QzcHit {
    pub qzi: Vec<u8>,
    pub no_crash: bool,
}

struct QzcFile {
    inputs: Vec<(PathBuf, [u8; 32])>,
    no_crash: bool,
    qzi: Vec<u8>,
    context_hash: [u8; 32],
    units: Vec<CachedCodegenUnit>,
}

pub fn load(path: &Path) -> Result<Option<QzcHit>, String> {
    let Some(cache) = read_cache(path)? else {
        return Ok(None);
    };
    for (input, expected) in &cache.inputs {
        let Ok(actual) = hash_file(input) else {
            return Ok(None);
        };
        if &actual != expected {
            return Ok(None);
        }
    }
    Ok(Some(QzcHit {
        qzi: cache.qzi,
        no_crash: cache.no_crash,
    }))
}

pub fn load_codegen_units(
    path: &Path,
    context_hash: [u8; 32],
    source_hashes: &std::collections::HashMap<String, [u8; 32]>,
) -> Result<Vec<CachedCodegenUnit>, String> {
    let Some(cache) = read_cache(path)? else {
        return Ok(Vec::new());
    };
    if cache.context_hash != context_hash {
        return Ok(Vec::new());
    }
    Ok(cache
        .units
        .into_iter()
        .filter(|unit| source_hashes.get(&unit.source_path) == Some(&unit.source_hash))
        .collect())
}

pub fn has_codegen_units(path: &Path) -> bool {
    read_cache(path)
        .ok()
        .flatten()
        .is_some_and(|cache| !cache.units.is_empty())
}

fn read_cache(path: &Path) -> Result<Option<QzcFile>, String> {
    let backup = path.with_extension("qzc.bak");
    let readable_path = if path.exists() {
        path
    } else if backup.exists() {
        &backup
    } else {
        return Ok(None);
    };
    let bytes = fs::read(readable_path).map_err(|error| {
        format!(
            "cannot read incremental cache '{}': {error}",
            readable_path.display()
        )
    })?;
    let mut pos = 0usize;
    if bytes.get(..4) != Some(QZC_MAGIC.as_slice()) {
        return Ok(None);
    }
    pos += 4;
    if read_u8(&bytes, &mut pos)? != QZC_VERSION {
        return Ok(None);
    }
    let compiler = read_string(&bytes, &mut pos)?;
    if compiler != compiler_identity() {
        return Ok(None);
    }
    let input_count = read_u32(&bytes, &mut pos)? as usize;
    if input_count > bytes.len().saturating_sub(pos) / 36 {
        return Ok(None);
    }
    let mut inputs = Vec::with_capacity(input_count);
    for _ in 0..input_count {
        let input = PathBuf::from(read_string(&bytes, &mut pos)?);
        let expected: [u8; 32] = read_exact(&bytes, &mut pos, 32)?.try_into().unwrap();
        inputs.push((input, expected));
    }
    let no_crash = read_u8(&bytes, &mut pos)? != 0;
    let qzi_len = read_u64(&bytes, &mut pos)? as usize;
    let qzi = read_exact(&bytes, &mut pos, qzi_len)?.to_vec();
    let expected_checksum = read_exact(&bytes, &mut pos, 32)?;
    if Sha256::digest(&qzi).as_slice() != expected_checksum {
        return Ok(None);
    }
    crate::bytecode::deserialize_qzi_module(&qzi)
        .map_err(|error| format!("cached QZI is invalid: {error}"))?;
    let context_hash: [u8; 32] = read_exact(&bytes, &mut pos, 32)?.try_into().unwrap();
    let raw_len = read_u64(&bytes, &mut pos)? as usize;
    let raw_qzi = read_exact(&bytes, &mut pos, raw_len)?;
    let raw_chunks = crate::bytecode::deserialize_qzi(raw_qzi)
        .map_err(|error| format!("cached reusable QZI is invalid: {error}"))?;
    let unit_count = read_u32(&bytes, &mut pos)? as usize;
    if unit_count != raw_chunks.len() {
        return Err("QZC reusable-unit table does not match its QZI chunks".to_string());
    }
    let mut units = Vec::with_capacity(unit_count);
    for chunk in raw_chunks {
        let source_path = read_string(&bytes, &mut pos)?;
        let source_hash = read_exact(&bytes, &mut pos, 32)?.try_into().unwrap();
        let call_count = read_u32(&bytes, &mut pos)? as usize;
        if call_count > chunk.code.len() {
            return Err("QZC function has more call records than instructions".to_string());
        }
        let mut calls = Vec::with_capacity(call_count);
        for _ in 0..call_count {
            calls.push(CachedCodegenCall {
                instruction_index: read_u32(&bytes, &mut pos)? as usize,
                target: read_string(&bytes, &mut pos)?,
            });
        }
        units.push(CachedCodegenUnit {
            source_path,
            source_hash,
            chunk,
            calls,
        });
    }
    if pos != bytes.len() {
        return Err("QZC has trailing data".to_string());
    }
    Ok(Some(QzcFile {
        inputs,
        no_crash,
        qzi,
        context_hash,
        units,
    }))
}

pub fn store(
    path: &Path,
    inputs: &[PathBuf],
    captured_hashes: &std::collections::HashMap<String, [u8; 32]>,
    qzi: &[u8],
    no_crash: bool,
    context_hash: [u8; 32],
    units: &[CachedCodegenUnit],
) -> Result<(), String> {
    let mut unique_inputs: Vec<PathBuf> = inputs
        .iter()
        .filter(|input| input.is_file())
        .map(|input| input.canonicalize().unwrap_or_else(|_| input.clone()))
        .collect();
    unique_inputs.sort();
    unique_inputs.dedup();
    if unique_inputs.len() > u32::MAX as usize {
        return Err("incremental cache has too many inputs".to_string());
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(QZC_MAGIC);
    bytes.push(QZC_VERSION);
    write_string(&mut bytes, &compiler_identity())?;
    bytes.extend_from_slice(&(unique_inputs.len() as u32).to_le_bytes());
    for input in unique_inputs {
        write_string(&mut bytes, &input.to_string_lossy())?;
        let input_key = input.to_string_lossy();
        let hash = captured_hashes
            .get(input_key.as_ref())
            .copied()
            .map(Ok)
            .unwrap_or_else(|| hash_file(&input))?;
        bytes.extend_from_slice(&hash);
    }
    bytes.push(u8::from(no_crash));
    bytes.extend_from_slice(
        &u64::try_from(qzi.len())
            .map_err(|_| "cached QZI is too large".to_string())?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(qzi);
    bytes.extend_from_slice(&Sha256::digest(qzi));
    bytes.extend_from_slice(&context_hash);
    let raw_qzi = crate::bytecode::serialize_qzi(
        &units
            .iter()
            .map(|unit| unit.chunk.clone())
            .collect::<Vec<_>>(),
    )?;
    bytes.extend_from_slice(&(raw_qzi.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&raw_qzi);
    bytes.extend_from_slice(&(units.len() as u32).to_le_bytes());
    for unit in units {
        write_string(&mut bytes, &unit.source_path)?;
        bytes.extend_from_slice(&unit.source_hash);
        bytes.extend_from_slice(&(unit.calls.len() as u32).to_le_bytes());
        for call in &unit.calls {
            let instruction_index = u32::try_from(call.instruction_index)
                .map_err(|_| "cached function has too many instructions".to_string())?;
            bytes.extend_from_slice(&instruction_index.to_le_bytes());
            write_string(&mut bytes, &call.target)?;
        }
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "cannot create incremental cache '{}': {error}",
                parent.display()
            )
        })?;
    }
    let temporary = path.with_extension(format!("qzc.{}.tmp", std::process::id()));
    let backup = path.with_extension("qzc.bak");
    let mut file = File::create(&temporary)
        .map_err(|error| format!("cannot create incremental cache: {error}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("cannot write incremental cache: {error}"))?;
    drop(file);
    if backup.exists() {
        fs::remove_file(&backup)
            .map_err(|error| format!("cannot clear incremental cache backup: {error}"))?;
    }
    if path.exists() {
        fs::rename(path, &backup)
            .map_err(|error| format!("cannot back up incremental cache: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if backup.exists() {
            let _ = fs::rename(&backup, path);
        }
        return Err(format!("cannot install incremental cache: {error}"));
    }
    if backup.exists() {
        fs::remove_file(backup)
            .map_err(|error| format!("cannot remove incremental cache backup: {error}"))?;
    }
    Ok(())
}

fn compiler_identity() -> String {
    format!(
        "{}:{}:{}:{}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH,
        std::env::consts::OS,
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    )
}

fn hash_file(path: &Path) -> Result<[u8; 32], String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("cannot hash cache input '{}': {error}", path.display()))?;
    Ok(Sha256::digest(bytes).into())
}

#[cfg(test)]
fn source_hashes(paths: &[PathBuf]) -> Result<std::collections::HashMap<String, [u8; 32]>, String> {
    paths
        .iter()
        .map(|path| {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            Ok((
                canonical.to_string_lossy().into_owned(),
                hash_file(&canonical)?,
            ))
        })
        .collect()
}

fn hash_attributes(hasher: &mut Sha256, attributes: &[Attribute]) {
    for attribute in attributes {
        hasher.update(attribute.name.as_bytes());
        hasher.update(format!("{:?}", attribute.args).as_bytes());
    }
}

#[allow(clippy::too_many_arguments)]
fn hash_function_signature(
    hasher: &mut Sha256,
    name: &str,
    generic_params: &[String],
    params: &[crate::parser::ast::Param],
    return_ty: &TypeKind,
    attributes: &[Attribute],
    unsafe_fn: bool,
    pub_fn: bool,
    c_variadic: bool,
) {
    hasher.update(b"fn");
    hasher.update(name.as_bytes());
    hasher.update(format!("{:?}", generic_params).as_bytes());
    for param in params {
        hasher.update(param.name.as_bytes());
        hasher.update(param.ty.node.to_string().as_bytes());
        hasher.update([u8::from(param.variadic)]);
        hash_attributes(hasher, &param.attributes);
    }
    hasher.update(return_ty.to_string().as_bytes());
    hasher.update([u8::from(unsafe_fn), u8::from(pub_fn), u8::from(c_variadic)]);
    hash_attributes(hasher, attributes);
}

pub fn semantic_context_hash(
    program: &Program,
    target: &str,
    configuration_inputs: &[PathBuf],
) -> Result<[u8; 32], String> {
    let mut hasher = Sha256::new();
    hasher.update(b"qzc-v2-semantic-context");
    hasher.update(target.as_bytes());
    for path in configuration_inputs {
        if !path.is_file() {
            continue;
        }
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        hasher.update(canonical.to_string_lossy().as_bytes());
        hasher.update(hash_file(&canonical)?);
    }
    for item in &program.items {
        match &item.node {
            ItemKind::Fn {
                name,
                generic_params,
                params,
                return_ty,
                attributes,
                unsafe_fn,
                pub_fn,
                c_variadic,
                ..
            } => hash_function_signature(
                &mut hasher,
                name,
                generic_params,
                params,
                &return_ty.node,
                attributes,
                *unsafe_fn,
                *pub_fn,
                *c_variadic,
            ),
            ItemKind::Impl {
                trait_ty,
                for_ty,
                methods,
            } => {
                hasher.update(b"impl");
                hasher.update(
                    trait_ty
                        .as_ref()
                        .map(|ty| ty.node.to_string())
                        .unwrap_or_default()
                        .as_bytes(),
                );
                hasher.update(for_ty.node.to_string().as_bytes());
                for method in methods {
                    if let ItemKind::Fn {
                        name,
                        generic_params,
                        params,
                        return_ty,
                        attributes,
                        unsafe_fn,
                        pub_fn,
                        c_variadic,
                        ..
                    } = &method.node
                    {
                        hash_function_signature(
                            &mut hasher,
                            name,
                            generic_params,
                            params,
                            &return_ty.node,
                            attributes,
                            *unsafe_fn,
                            *pub_fn,
                            *c_variadic,
                        );
                    }
                }
            }
            ItemKind::Struct {
                name,
                generic_params,
                fields,
                bit_widths,
                is_union,
                attributes,
                public,
            } => {
                hasher.update(b"struct");
                hasher.update(name.as_bytes());
                hasher.update(format!("{:?}", generic_params).as_bytes());
                for (field, ty, constant) in fields {
                    hasher.update(field.as_bytes());
                    hasher.update(ty.node.to_string().as_bytes());
                    hasher.update([u8::from(*constant)]);
                }
                hasher.update(format!("{:?}", bit_widths).as_bytes());
                hasher.update([u8::from(*is_union), u8::from(*public)]);
                hash_attributes(&mut hasher, attributes);
            }
            ItemKind::Trait {
                name,
                generic_params,
                methods,
                attributes,
                public,
            } => {
                hasher.update(b"trait");
                hasher.update(name.as_bytes());
                hasher.update(format!("{:?}", generic_params).as_bytes());
                for method in methods {
                    hasher.update(method.name.as_bytes());
                    hasher.update(format!("{:?}", method.generic_params).as_bytes());
                    for (param_name, ty) in method.param_names.iter().zip(&method.params) {
                        hasher.update(param_name.as_bytes());
                        hasher.update(ty.node.to_string().as_bytes());
                    }
                    hasher.update(method.return_ty.node.to_string().as_bytes());
                }
                hasher.update([u8::from(*public)]);
                hash_attributes(&mut hasher, attributes);
            }
            ItemKind::Enum {
                name,
                generic_params,
                variants,
                attributes,
                public,
            } => {
                hasher.update(b"enum");
                hasher.update(name.as_bytes());
                hasher.update(format!("{:?}", generic_params).as_bytes());
                for variant in variants {
                    hasher.update(variant.name.as_bytes());
                    for ty in &variant.payload_types {
                        hasher.update(ty.node.to_string().as_bytes());
                    }
                }
                hasher.update([u8::from(*public)]);
                hash_attributes(&mut hasher, attributes);
            }
            ItemKind::Import(import) => {
                hasher.update(b"import");
                hasher.update(import.path.join(".").as_bytes());
                hasher.update(format!("{:?}", import.items).as_bytes());
                hasher.update([u8::from(import.pub_import), u8::from(import.relative)]);
            }
            ItemKind::TypeAlias {
                name,
                generic_params,
                aliased_type,
                attributes,
                public,
            } => {
                hasher.update(b"type");
                hasher.update(name.as_bytes());
                hasher.update(format!("{:?}", generic_params).as_bytes());
                hasher.update(aliased_type.node.to_string().as_bytes());
                hasher.update([u8::from(*public)]);
                hash_attributes(&mut hasher, attributes);
            }
            ItemKind::ForeignGlobal {
                name,
                ty,
                attributes,
                public,
            } => {
                hasher.update(b"global");
                hasher.update(name.as_bytes());
                hasher.update(ty.node.to_string().as_bytes());
                hasher.update([u8::from(*public)]);
                hash_attributes(&mut hasher, attributes);
            }
        }
    }
    Ok(hasher.finalize().into())
}

fn write_string(output: &mut Vec<u8>, value: &str) -> Result<(), String> {
    let len = u32::try_from(value.len()).map_err(|_| "QZC string is too long".to_string())?;
    output.extend_from_slice(&len.to_le_bytes());
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn read_string(bytes: &[u8], pos: &mut usize) -> Result<String, String> {
    let len = read_u32(bytes, pos)? as usize;
    let value = read_exact(bytes, pos, len)?;
    String::from_utf8(value.to_vec()).map_err(|_| "invalid UTF-8 in QZC".to_string())
}

fn read_u8(bytes: &[u8], pos: &mut usize) -> Result<u8, String> {
    let value = *bytes.get(*pos).ok_or_else(|| "truncated QZC".to_string())?;
    *pos += 1;
    Ok(value)
}

fn read_u32(bytes: &[u8], pos: &mut usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        read_exact(bytes, pos, 4)?.try_into().unwrap(),
    ))
}

fn read_u64(bytes: &[u8], pos: &mut usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(
        read_exact(bytes, pos, 8)?.try_into().unwrap(),
    ))
}

fn read_exact<'a>(bytes: &'a [u8], pos: &mut usize, len: usize) -> Result<&'a [u8], String> {
    let end = pos
        .checked_add(len)
        .ok_or_else(|| "QZC range overflow".to_string())?;
    let value = bytes
        .get(*pos..end)
        .ok_or_else(|| "truncated QZC".to_string())?;
    *pos = end;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn cache_hit_requires_unchanged_inputs() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("qzc_{}_{}", std::process::id(), suffix));
        fs::create_dir_all(&root).unwrap();
        let input = root.join("main.qz");
        fs::write(&input, "fn main() void {}").unwrap();
        let mut chunk = crate::bytecode::Chunk::new("main");
        chunk.code.push(crate::bytecode::instruction::rrr(
            crate::bytecode::Opcode::Ret,
            0,
            0,
            0,
        ));
        let qzi = crate::bytecode::serialize_qzi(&[chunk]).unwrap();
        let cache = root.join("incremental.qzc");
        let captured_hashes = source_hashes(std::slice::from_ref(&input)).unwrap();
        store(
            &cache,
            std::slice::from_ref(&input),
            &captured_hashes,
            &qzi,
            false,
            [7; 32],
            &[],
        )
        .unwrap();
        assert!(load(&cache).unwrap().is_some());
        fs::write(&input, "fn main() void { ret; }").unwrap();
        assert!(load(&cache).unwrap().is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn qzc_v4_is_rejected_after_generic_abi_validation_changed() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("qzc_version_{}_{}", std::process::id(), suffix));
        fs::create_dir_all(&root).unwrap();
        let input = root.join("main.qz");
        fs::write(&input, "fn main() void {}").unwrap();
        let mut chunk = crate::bytecode::Chunk::new("main");
        chunk.code.push(crate::bytecode::instruction::rrr(
            crate::bytecode::Opcode::Ret,
            0,
            0,
            0,
        ));
        let qzi = crate::bytecode::serialize_qzi(&[chunk]).unwrap();
        let cache = root.join("incremental.qzc");
        let hashes = source_hashes(std::slice::from_ref(&input)).unwrap();
        store(
            &cache,
            std::slice::from_ref(&input),
            &hashes,
            &qzi,
            false,
            [0; 32],
            &[],
        )
        .unwrap();

        let mut legacy = fs::read(&cache).unwrap();
        assert_eq!(&legacy[..4], QZC_MAGIC);
        legacy[4] = 4;
        fs::write(&cache, legacy).unwrap();
        assert!(load(&cache).unwrap().is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn body_changes_keep_semantic_context_stable() {
        let parse = |source: &str| Parser::new(Lexer::new(source).tokenize()).parse().unwrap();
        let first = parse("fn helper() i32 { ret 1; } fn main() i32 { ret helper(); }");
        let second = parse(
            "fn helper() i32 { const padding = 200; ret padding; } fn main() i32 { ret helper(); }",
        );
        assert_eq!(
            semantic_context_hash(&first, "x86_64-windows", &[]).unwrap(),
            semantic_context_hash(&second, "x86_64-windows", &[]).unwrap()
        );
        let changed_signature =
            parse("fn helper(value: i32) i32 { ret value; } fn main() i32 { ret helper(1); }");
        assert_ne!(
            semantic_context_hash(&first, "x86_64-windows", &[]).unwrap(),
            semantic_context_hash(&changed_signature, "x86_64-windows", &[]).unwrap()
        );
    }

    #[test]
    fn cache_miss_restores_only_unchanged_codegen_units() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("qzc_units_{}_{}", std::process::id(), suffix));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("first.qz");
        let second = root.join("second.qz");
        fs::write(&first, "fn first() i32 { ret 1; }").unwrap();
        fs::write(&second, "fn second() i32 { ret 2; }").unwrap();
        let paths = vec![
            first.canonicalize().unwrap(),
            second.canonicalize().unwrap(),
        ];
        let hashes = source_hashes(&paths).unwrap();
        let unit = |path: &Path, name: &str| CachedCodegenUnit {
            source_path: path.to_string_lossy().into_owned(),
            source_hash: hashes[&path.to_string_lossy().into_owned()],
            chunk: {
                let mut chunk = crate::bytecode::Chunk::new(name);
                chunk.reg_count = 1;
                chunk.code.push(crate::bytecode::instruction::rrr(
                    crate::bytecode::Opcode::Ret,
                    0,
                    0,
                    0,
                ));
                chunk
            },
            calls: Vec::new(),
        };
        let units = vec![unit(&paths[0], "first"), unit(&paths[1], "second")];
        let qzi = crate::bytecode::serialize_qzi(
            &units
                .iter()
                .map(|unit| unit.chunk.clone())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let cache = root.join("incremental.qzc");
        store(&cache, &paths, &hashes, &qzi, false, [9; 32], &units).unwrap();
        fs::write(&second, "fn second() i32 { ret 3; }").unwrap();
        let changed_hashes = source_hashes(&paths).unwrap();
        let restored = load_codegen_units(&cache, [9; 32], &changed_hashes).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].chunk.name, "first");
        let _ = fs::remove_dir_all(root);
    }
}
