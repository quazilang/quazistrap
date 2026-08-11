// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const QZC_MAGIC: &[u8; 4] = b"\0QZC";
const QZC_VERSION: u8 = 1;

#[derive(Debug, Clone)]
pub struct QzcHit {
    pub qzi: Vec<u8>,
    pub no_crash: bool,
}

pub fn load(path: &Path) -> Result<Option<QzcHit>, String> {
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
    for _ in 0..input_count {
        let input = PathBuf::from(read_string(&bytes, &mut pos)?);
        let expected = read_exact(&bytes, &mut pos, 32)?;
        let actual = match hash_file(&input) {
            Ok(hash) => hash,
            Err(_) => return Ok(None),
        };
        if actual.as_slice() != expected {
            return Ok(None);
        }
    }
    let no_crash = read_u8(&bytes, &mut pos)? != 0;
    let qzi_len = read_u64(&bytes, &mut pos)? as usize;
    let qzi = read_exact(&bytes, &mut pos, qzi_len)?.to_vec();
    let expected_checksum = read_exact(&bytes, &mut pos, 32)?;
    if Sha256::digest(&qzi).as_slice() != expected_checksum || pos != bytes.len() {
        return Ok(None);
    }
    crate::bytecode::deserialize_qzi_module(&qzi)
        .map_err(|error| format!("cached QZI is invalid: {error}"))?;
    Ok(Some(QzcHit { qzi, no_crash }))
}

pub fn store(path: &Path, inputs: &[PathBuf], qzi: &[u8], no_crash: bool) -> Result<(), String> {
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
        bytes.extend_from_slice(&hash_file(&input)?);
    }
    bytes.push(u8::from(no_crash));
    bytes.extend_from_slice(
        &u64::try_from(qzi.len())
            .map_err(|_| "cached QZI is too large".to_string())?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(qzi);
    bytes.extend_from_slice(&Sha256::digest(qzi));

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
        store(&cache, std::slice::from_ref(&input), &qzi, false).unwrap();
        assert!(load(&cache).unwrap().is_some());
        fs::write(&input, "fn main() void { ret; }").unwrap();
        assert!(load(&cache).unwrap().is_none());
        let _ = fs::remove_dir_all(root);
    }
}
