//! Read-only assets with bounded IPC reads and streaming positional comparison.
use serde::{Deserialize, Serialize};
use std::fs::{self, File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRef {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: String,
}
fn stamp(metadata: &Metadata) -> Result<String, String> {
    let time = metadata.modified().map_err(|e| e.to_string())?;
    Ok(match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos().to_string(),
        Err(error) => format!("-{}", error.duration().as_nanos()),
    })
}
fn verify_metadata(metadata: &Metadata, asset: &AssetRef) -> Result<(), String> {
    if !metadata.is_file() || metadata.len() != asset.size || stamp(metadata)? != asset.modified {
        return Err(format!("File changed or is no longer a regular file: {}. Open it again.", asset.path));
    }
    Ok(())
}
pub fn inspect_asset(path: &str) -> Result<AssetRef, String> {
    let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("Select a regular file, not a directory or special file.".into());
    }
    Ok(AssetRef {
        path: path.to_owned(),
        name: Path::new(path).file_name().ok_or("Invalid file name")?.to_string_lossy().into_owned(),
        size: metadata.len(),
        modified: stamp(&metadata)?,
    })
}
pub(crate) fn verify_asset(asset: &AssetRef) -> Result<(), String> {
    verify_metadata(&fs::metadata(&asset.path).map_err(|e| e.to_string())?, asset)
}
pub(crate) fn open_verified(asset: &AssetRef) -> Result<File, String> {
    verify_asset(asset)?;
    let file = File::open(&asset.path).map_err(|e| e.to_string())?;
    verify_metadata(&file.metadata().map_err(|e| e.to_string())?, asset)?;
    Ok(file)
}
const CHUNK: usize = 1024 * 1024;
pub fn read_asset_chunk(asset: &AssetRef, offset: u64, length: usize) -> Result<Vec<u8>, String> {
    // IPC chunk size, not a limit on supported file sizes.
    if length > CHUNK {
        return Err("Read request exceeds one IPC chunk. Request consecutive chunks.".into());
    }
    if offset > asset.size {
        return Err("Offset is beyond the end of the file.".into());
    }
    let mut file = open_verified(asset)?;
    file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
    let mut bytes = vec![0; length.min((asset.size - offset).min(CHUNK as u64) as usize)];
    file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    verify_metadata(&file.metadata().map_err(|e| e.to_string())?, asset)?;
    verify_asset(asset)?;
    Ok(bytes)
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryStats {
    pub compared: u64,
    pub changed: u64,
    pub left_only: u64,
    pub right_only: u64,
    pub first_difference: Option<u64>,
    pub last_difference: Option<u64>,
}
pub fn compare_binary(left: &AssetRef, right: &AssetRef, cancel: &AtomicBool) -> Result<BinaryStats, String> {
    let mut a = open_verified(left)?;
    let mut b = open_verified(right)?;
    let mut x = vec![0u8; CHUNK];
    let mut y = vec![0u8; CHUNK];
    let common = left.size.min(right.size);
    let total = left.size.max(right.size);
    let mut stats = BinaryStats {
        left_only: left.size.saturating_sub(right.size),
        right_only: right.size.saturating_sub(left.size),
        ..BinaryStats::default()
    };
    let mut offset = 0;
    while offset < common {
        if cancel.load(Ordering::Relaxed) {
            return Err("Comparison cancelled.".into());
        }
        let n = (common - offset).min(CHUNK as u64) as usize;
        a.read_exact(&mut x[..n]).map_err(|e| e.to_string())?;
        b.read_exact(&mut y[..n]).map_err(|e| e.to_string())?;
        for (i, (&l, &r)) in x[..n].iter().zip(&y[..n]).enumerate() {
            if l != r {
                let address = offset + i as u64;
                stats.changed += 1;
                stats.first_difference.get_or_insert(address);
                stats.last_difference = Some(address);
            }
        }
        offset += n as u64;
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Comparison cancelled.".into());
    }
    if total > common {
        stats.first_difference.get_or_insert(common);
        stats.last_difference = Some(total - 1);
    }
    stats.compared = total;
    verify_metadata(&a.metadata().map_err(|e| e.to_string())?, left)?;
    verify_metadata(&b.metadata().map_err(|e| e.to_string())?, right)?;
    verify_asset(left)?;
    verify_asset(right)?;
    Ok(stats)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_pages_and_compares_tail_and_changed_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let l = dir.path().join("left.bin");
        let r = dir.path().join("right.bin");
        fs::write(&l, [0, 1, 2, 3]).unwrap();
        fs::write(&r, [0, 9, 2, 3, 5]).unwrap();
        let a = inspect_asset(l.to_str().unwrap()).unwrap();
        let b = inspect_asset(r.to_str().unwrap()).unwrap();
        assert_eq!(read_asset_chunk(&a, 1, 99).unwrap(), [1, 2, 3]);
        assert!(read_asset_chunk(&a, 4, 5).unwrap().is_empty());
        assert!(read_asset_chunk(&a, 5, 1).is_err());
        assert_eq!(compare_binary(&a, &b, &AtomicBool::new(false)).unwrap(),
            BinaryStats { compared: 5, changed: 1, left_only: 0, right_only: 1, first_difference: Some(1), last_difference: Some(4) });
        assert!(compare_binary(&a, &b, &AtomicBool::new(true)).is_err());
        fs::write(&l, [0, 1]).unwrap();
        assert!(read_asset_chunk(&a, 0, 1).is_err());
    }
    #[test]
    fn crosses_chunk_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let l = dir.path().join("a");
        let r = dir.path().join("b");
        let a = vec![0; CHUNK + 10];
        let mut b = a.clone();
        b[CHUNK - 1] = 1;
        b[CHUNK] = 2;
        fs::write(&l, a).unwrap();
        fs::write(&r, b).unwrap();
        let left = inspect_asset(l.to_str().unwrap()).unwrap();
        let right = inspect_asset(r.to_str().unwrap()).unwrap();
        let result = compare_binary(&left, &right, &AtomicBool::new(false)).unwrap();
        assert_eq!(result.changed, 2);
        assert_eq!(result.first_difference, Some(CHUNK as u64 - 1));
        assert_eq!(result.last_difference, Some(CHUNK as u64));
    }
}
