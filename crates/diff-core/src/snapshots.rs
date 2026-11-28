//! Read-only version/remote file snapshots. Never interpret metadata alone as equality.
use crate::files::{cancelled, decode_snapshot, hash_file, same_metadata, TextDocument};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotItem {
    pub path: String,
    pub kind: String,
    pub size: u64,
    pub fingerprint: Option<String>,
    pub mode: Option<String>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEntry {
    pub path: String,
    pub status: &'static str,
    pub left: Option<SnapshotItem>,
    pub right: Option<SnapshotItem>,
}
pub type Index = BTreeMap<String, SnapshotItem>;

pub fn safe_relative(path: &str) -> Result<(), String> {
    if path.is_empty() || path.contains(['\\', '\0']) || path.split('/').any(|part| part.is_empty() || part == "." || part == ".." || part.contains(':')) {
        return Err(format!("Unsafe or unsupported relative path: {path:?}"));
    }
    Ok(())
}
pub fn add(index: &mut Index, item: SnapshotItem) -> Result<(), String> {
    safe_relative(&item.path)?;
    if index.contains_key(&item.path) {
        return Err(format!("Duplicate path: {}", item.path));
    }
    index.insert(item.path.clone(), item);
    Ok(())
}
pub fn compare(mut left: Index, mut right: Index, cancel: &AtomicBool) -> Result<Vec<SnapshotEntry>, String> {
    let paths: BTreeSet<String> = left.keys().chain(right.keys()).cloned().collect();
    let mut entries = Vec::with_capacity(paths.len());
    for path in paths {
        cancelled(cancel)?;
        let l = left.remove(&path);
        let r = right.remove(&path);
        let status = match (&l, &r) {
            (Some(a), _) if a.error.is_some() => "error",
            (_, Some(b)) if b.error.is_some() => "error",
            (Some(a), Some(b)) if a.kind != b.kind => "conflict",
            (Some(a), Some(b)) if a.kind == "directory" => "same",
            (Some(a), Some(b)) if a.size == b.size && a.fingerprint.is_some() && a.fingerprint == b.fingerprint
                && (a.mode.is_none() || b.mode.is_none() || a.mode == b.mode) => "same",
            (Some(_), Some(_)) => "different",
            (Some(_), None) => "leftOnly",
            (None, Some(_)) => "rightOnly",
            _ => unreachable!(),
        };
        entries.push(SnapshotEntry { path, status, left:l, right:r });
    }
    Ok(entries)
}
pub fn stream_hash(reader: &mut impl Read, cancel: &AtomicBool) -> Result<(u64,String),String> {
    let mut hash = blake3::Hasher::new();
    let mut size = 0u64;
    let mut buffer = [0u8;64*1024];
    loop {
        cancelled(cancel)?;
        let n = reader.read(&mut buffer).map_err(|e|e.to_string())?;
        if n == 0 { break; }
        size = size.checked_add(n as u64).ok_or("File size overflow")?;
        hash.update(&buffer[..n]);
    }
    Ok((size,hash.finalize().to_hex().to_string()))
}
pub fn file(path: String, size:u64, fingerprint:String, mode:Option<String>) -> SnapshotItem {
    SnapshotItem { path,kind:"file".into(),size,fingerprint:Some(fingerprint),mode,error:None }
}
pub fn directory(path:String) -> SnapshotItem {
    SnapshotItem { path,kind:"directory".into(),size:0,fingerprint:None,mode:None,error:None }
}
pub fn unsupported(path:String,kind:&str,error:&str) -> SnapshotItem {
    SnapshotItem { path,kind:kind.into(),size:0,fingerprint:None,mode:None,error:Some(error.into()) }
}
pub fn local_item(root:&Path, relative:&str, mode:Option<String>, cancel:&AtomicBool) -> Result<Option<SnapshotItem>,String> {
    safe_relative(relative)?;
    // Never follow intermediate symlinks or junctions to another root.
    let mut p=root.to_path_buf();
    let parts:Vec<_>=relative.split('/').collect();
    for (i,part) in parts.iter().enumerate() {
        p.push(part);
        let meta=match fs::symlink_metadata(&p) {
            Ok(m)=>m,
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),
            Err(e)=>return Err(e.to_string())
        };
        if meta.file_type().is_symlink() {
            return Ok(Some(unsupported(relative.into(),"link","Symbolic links are not followed.")));
        }
        if i+1<parts.len() && !meta.is_dir() {
            return Ok(Some(unsupported(relative.into(),"special","Parent is not a directory.")));
        }
    }
    let meta=fs::symlink_metadata(&p).map_err(|e|e.to_string())?;
    if meta.is_dir(){return Ok(Some(directory(relative.into())))}
    if !meta.is_file(){return Ok(Some(unsupported(relative.into(),"special","Special files are not read.")))}
    let hash=hash_file(&p,cancel)?;
    let after=fs::symlink_metadata(&p).map_err(|e|e.to_string())?;
    if !same_metadata(&meta,&after){return Err(format!("File changed: {}",p.display()))}
    Ok(Some(file(relative.into(),meta.len(),hash,mode)))
}
pub fn checked_path(root:&Path,relative:&str)->Result<PathBuf,String>{
    safe_relative(relative)?;
    let mut p=root.to_path_buf();
    for part in relative.split('/'){
        p.push(part);
        if fs::symlink_metadata(&p).map_err(|e|e.to_string())?.file_type().is_symlink(){
            return Err("Symbolic links are not followed.".into())
        }
    }
    let real=fs::canonicalize(&p).map_err(|e|e.to_string())?;
    let root=fs::canonicalize(root).map_err(|e|e.to_string())?;
    if !real.starts_with(root){return Err("Path escaped the selected root.".into())}
    Ok(real)
}
pub fn checked_text(bytes:&[u8], name:String, fingerprint:&str, encoding:Option<&str>)->Result<TextDocument,String>{
    if blake3::hash(bytes).to_hex().as_str()!=fingerprint{
        return Err("File content changed after comparison. Compare again.".into())
    }
    decode_snapshot(bytes,name,encoding)
}
pub fn read_local_snapshot(root:&Path,relative:&str,expected:&str,encoding:Option<&str>)->Result<TextDocument,String>{
    let path=checked_path(root,relative)?;
    let doc=crate::files::read_text(path.to_str().ok_or("Non-Unicode file path")?,encoding)?;
    if doc.fingerprint.as_deref()!=Some(expected){return Err("File content changed after comparison. Compare again.".into())}
    Ok(TextDocument{path:None,fingerprint:None,..doc})
}
