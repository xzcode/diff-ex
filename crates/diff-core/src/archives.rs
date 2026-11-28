//! Read-only archive comparison. No archive entry is ever extracted to disk.
use crate::assets::{open_verified, verify_asset, AssetRef};
use flate2::read::MultiGzDecoder;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveItem {
    path: String,
    kind: &'static str,
    size: u64,
    fingerprint: Option<String>,
    error: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntry {
    path: String,
    status: &'static str,
    left: Option<ArchiveItem>,
    right: Option<ArchiveItem>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveResult {
    entries: Vec<ArchiveEntry>,
    left_format: &'static str,
    right_format: &'static str,
}
fn cancelled(flag: &AtomicBool) -> Result<(), String> {
    if flag.load(Ordering::Relaxed) {
        Err("Comparison cancelled.".into())
    } else {
        Ok(())
    }
}
fn fingerprint(
    reader: &mut (impl Read + ?Sized),
    flag: &AtomicBool,
) -> Result<(u64, String), String> {
    let mut hash = blake3::Hasher::new();
    let mut count = 0u64;
    let mut buffer = vec![0u8; 128 * 1024];
    loop {
        cancelled(flag)?;
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        count = count
            .checked_add(n as u64)
            .ok_or("Archive entry size overflow")?;
        hash.update(&buffer[..n]);
    }
    Ok((count, hash.finalize().to_hex().to_string()))
}
// Even though no extraction occurs, ambiguous/unsafe path names are refused
// instead of conflated. Both slash styles are normalized for matching.
fn normalized_path(raw: &str) -> Result<String, String> {
    let path = raw.replace('\\', "/");
    if path.contains('\0')
        || path.starts_with('/')
        || path
            .split('/')
            .next()
            .is_some_and(|part| part.contains(':'))
    {
        return Err(format!("Unsafe archive path: {raw:?}"));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(format!("Unsafe archive path: {raw:?}")),
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}
#[derive(Default)]
struct Index {
    items: BTreeMap<String, ArchiveItem>,
    explicit: BTreeSet<String>,
}
fn directory(path: String) -> ArchiveItem {
    ArchiveItem {
        path,
        kind: "directory",
        size: 0,
        fingerprint: None,
        error: None,
    }
}
impl Index {
    fn add(&mut self, item: ArchiveItem) -> Result<(), String> {
        let path = item.path.clone();
        if path.is_empty() {
            if item.kind == "directory" {
                return Ok(());
            }
            return Err("Empty archive file name".into());
        }
        if !self.explicit.insert(path.clone()) {
            return Err(format!("Duplicate archive path: {path}"));
        }
        // Synthesize parent directories so explicit directory headers and
        // implicit directories compare the same across container formats.
        for (offset, _) in path.match_indices('/') {
            let parent = &path[..offset];
            if let Some(existing) = self.items.get(parent) {
                if existing.kind != "directory" {
                    return Err(format!("Archive parent is not a directory: {parent}"));
                }
            } else {
                self.items.insert(parent.into(), directory(parent.into()));
            }
        }
        if let Some(existing) = self.items.get(&path) {
            if existing.kind != "directory" || item.kind != "directory" {
                return Err(format!("Archive path/type collision: {path}"));
            }
        }
        self.items.insert(path, item);
        Ok(())
    }
}
fn unsupported(path: String, kind: &'static str, size: u64) -> ArchiveItem {
    ArchiveItem {
        path,
        kind,
        size,
        fingerprint: None,
        error: Some("Links and special entries are not followed or compared.".into()),
    }
}
fn file_item(
    path: String,
    declared: u64,
    reader: &mut (impl Read + ?Sized),
    flag: &AtomicBool,
) -> Result<ArchiveItem, String> {
    let (size, hash) = fingerprint(reader, flag)?;
    if size != declared {
        return Err(format!("Archive entry size mismatch: {path}"));
    }
    Ok(ArchiveItem {
        path,
        kind: "file",
        size,
        fingerprint: Some(hash),
        error: None,
    })
}
fn read_zip(reader: impl Read + io::Seek, flag: &AtomicBool) -> Result<Index, String> {
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| e.to_string())?;
    let mut index = Index::default();
    for i in 0..archive.len() {
        cancelled(flag)?;
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let path = normalized_path(&entry.name().map_err(|e| e.to_string())?)?;
        let size = entry.size();
        let mode = entry.unix_mode().unwrap_or(0) & 0o170000;
        let item = if mode == 0o120000 {
            unsupported(path, "link", size)
        } else if mode != 0 && mode != 0o100000 && mode != 0o040000 {
            unsupported(path, "special", size)
        } else if entry.is_dir() || mode == 0o040000 {
            directory(path)
        } else {
            file_item(path, size, &mut entry, flag)?
        };
        index.add(item)?;
    }
    Ok(index)
}
fn read_tar(reader: impl Read, flag: &AtomicBool) -> Result<Index, String> {
    let mut archive = tar::Archive::new(reader);
    let mut index = Index::default();
    for entry in archive.entries().map_err(|e| e.to_string())? {
        cancelled(flag)?;
        let mut entry = entry.map_err(|e| e.to_string())?;
        let bytes = entry.path_bytes();
        let path =
            normalized_path(std::str::from_utf8(&bytes).map_err(|_| "Non-UTF-8 TAR entry name")?)?;
        let kind = entry.header().entry_type();
        let size = entry.size();
        let item = if kind.is_dir() {
            directory(path)
        } else if kind.is_symlink() || kind.is_hard_link() {
            unsupported(path, "link", size)
        } else if kind.is_file() {
            file_item(path, size, &mut entry, flag)?
        } else {
            unsupported(path, "special", size)
        };
        index.add(item)?;
    }
    // Consume remaining padding and, for tar.gz, validate the gzip trailer.
    let mut reader = archive.into_inner();
    let mut buffer = [0u8; 8192];
    loop {
        cancelled(flag)?;
        if reader.read(&mut buffer).map_err(|e| e.to_string())? == 0 {
            break;
        }
    }
    Ok(index)
}
fn read_7z(reader: impl Read + io::Seek, flag: &AtomicBool) -> Result<Index, String> {
    use sevenz_rust2::{ArchiveReader, Error, Password};
    let mut archive = ArchiveReader::new(reader, Password::empty()).map_err(|e| e.to_string())?;
    let mut index = Index::default();
    archive
        .for_each_entries(|entry, reader| {
            let map = |message: String| Error::from(io::Error::other(message));
            cancelled(flag).map_err(map)?;
            let path = normalized_path(entry.name()).map_err(map)?;
            let attrs = entry.windows_attributes();
            let mode = (attrs >> 16) & 0o170000;
            let size = entry.size();
            let item = if entry.is_anti_item() || mode == 0o120000 || attrs & 0x400 != 0 {
                if entry.has_stream() {
                    fingerprint(reader, flag).map_err(map)?;
                }
                unsupported(path, "link", size)
            } else if mode != 0 && mode != 0o100000 && mode != 0o040000 {
                if entry.has_stream() {
                    fingerprint(reader, flag).map_err(map)?;
                }
                unsupported(path, "special", size)
            } else if entry.is_directory() || mode == 0o040000 {
                if entry.has_stream() {
                    fingerprint(reader, flag).map_err(map)?;
                }
                directory(path)
            } else {
                file_item(path, size, reader, flag).map_err(map)?
            };
            index.add(item).map_err(map)?;
            Ok(true)
        })
        .map_err(|e| e.to_string())?;
    Ok(index)
}
fn read_gzip(reader: impl Read, flag: &AtomicBool) -> Result<Index, String> {
    // A plain .gz stream has one logical payload, independent of container
    // filename and optional header name; concatenated gzip members are decoded.
    let mut reader = MultiGzDecoder::new(reader);
    let (size, hash) = fingerprint(&mut reader, flag)?;
    let mut index = Index::default();
    index.add(ArchiveItem {
        path: "payload".into(),
        kind: "file",
        size,
        fingerprint: Some(hash),
        error: None,
    })?;
    Ok(index)
}
fn load(asset: &AssetRef, flag: &AtomicBool) -> Result<(Index, &'static str), String> {
    cancelled(flag)?;
    let name = asset.name.to_ascii_lowercase();
    let file = open_verified(asset)?;
    let (index, format) = if name.ends_with(".zip")
        || name.ends_with(".jar")
        || name.ends_with(".apk")
        || name.ends_with(".war")
    {
        (read_zip(file, flag)?, "ZIP")
    } else if name.ends_with(".7z") {
        (read_7z(file, flag)?, "7Z")
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        (read_tar(MultiGzDecoder::new(file), flag)?, "TAR.GZ")
    } else if name.ends_with(".tar") {
        (read_tar(file, flag)?, "TAR")
    } else if name.ends_with(".gz") {
        (read_gzip(file, flag)?, "GZIP")
    } else {
        return Err(
            "Unsupported archive format. Select ZIP/JAR/APK/WAR, 7Z, TAR, TAR.GZ/TGZ or GZIP."
                .into(),
        );
    };
    verify_asset(asset)?;
    Ok((index, format))
}
pub fn compare_archives(
    left: &AssetRef,
    right: &AssetRef,
    flag: &AtomicBool,
) -> Result<ArchiveResult, String> {
    let (mut a, left_format) = load(left, flag)?;
    let (mut b, right_format) = load(right, flag)?;
    let paths: BTreeSet<String> = a.items.keys().chain(b.items.keys()).cloned().collect();
    let mut entries = Vec::with_capacity(paths.len());
    for path in paths {
        cancelled(flag)?;
        let l = a.items.remove(&path);
        let r = b.items.remove(&path);
        let status = match (&l, &r) {
            (Some(x), Some(y)) if x.error.is_some() || y.error.is_some() => "error",
            (Some(x), None) if x.error.is_some() => "error",
            (None, Some(y)) if y.error.is_some() => "error",
            (Some(x), Some(y)) if x.kind != y.kind => "conflict",
            (Some(x), Some(y)) if x.size == y.size && x.fingerprint == y.fingerprint => "same",
            (Some(_), Some(_)) => "different",
            (Some(_), None) => "leftOnly",
            (None, Some(_)) => "rightOnly",
            (None, None) => unreachable!(),
        };
        entries.push(ArchiveEntry {
            path,
            status,
            left: l,
            right: r,
        });
    }
    verify_asset(left)?;
    verify_asset(right)?;
    Ok(ArchiveResult {
        entries,
        left_format,
        right_format,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::inspect_asset;
    use flate2::{write::GzEncoder, Compression};
    use std::fs::File;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    #[test]
    fn zip_tar_cross_format_and_changed_contents() {
        let dir = tempfile::tempdir().unwrap();
        let left = dir.path().join("a.zip");
        let right = dir.path().join("b.tar");
        let mut zip = zip::ZipWriter::new(File::create(&left).unwrap());
        zip.add_directory("folder/", SimpleFileOptions::default())
            .unwrap();
        zip.start_file("folder/a.txt", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"same").unwrap();
        zip.start_file("changed.txt", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"old").unwrap();
        zip.finish().unwrap();
        let mut tar = tar::Builder::new(File::create(&right).unwrap());
        for (path, data) in [
            ("folder/a.txt", &b"same"[..]),
            ("changed.txt", &b"new"[..]),
            ("new.txt", &b"new"[..]),
        ] {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            tar.append_data(&mut h, path, data).unwrap();
        }
        tar.finish().unwrap();
        let result = compare_archives(
            &inspect_asset(left.to_str().unwrap()).unwrap(),
            &inspect_asset(right.to_str().unwrap()).unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let statuses: BTreeMap<_, _> = result
            .entries
            .iter()
            .map(|e| (e.path.as_str(), e.status))
            .collect();
        assert_eq!(statuses["folder"], "same");
        assert_eq!(statuses["folder/a.txt"], "same");
        assert_eq!(statuses["changed.txt"], "different");
        assert_eq!(statuses["new.txt"], "rightOnly");
    }
    #[test]
    fn seven_zip_contents_are_streamed_and_compared() {
        use sevenz_rust2::{ArchiveEntry, ArchiveWriter};
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.7z");
        let b = dir.path().join("b.7z");
        for (path, bytes) in [(&a, &b"same"[..]), (&b, &b"diff"[..])] {
            let mut writer = ArchiveWriter::create(path).unwrap();
            let mut entry = ArchiveEntry::new();
            entry.name = "folder/item".into();
            writer.push_archive_entry(entry, Some(bytes)).unwrap();
            writer.finish().unwrap();
        }
        let a = inspect_asset(a.to_str().unwrap()).unwrap();
        let b = inspect_asset(b.to_str().unwrap()).unwrap();
        let result = compare_archives(&a, &b, &AtomicBool::new(false)).unwrap();
        assert_eq!(result.left_format, "7Z");
        assert_eq!(result.entries[1].path, "folder/item");
        assert_eq!(result.entries[1].status, "different");
    }
    #[test]
    fn gzip_payload_hash_and_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.gz");
        let b = dir.path().join("b.gz");
        for path in [&a, &b] {
            let mut encoder = GzEncoder::new(File::create(path).unwrap(), Compression::default());
            encoder.write_all(b"payload").unwrap();
            encoder.finish().unwrap();
        }
        let a = inspect_asset(a.to_str().unwrap()).unwrap();
        let b = inspect_asset(b.to_str().unwrap()).unwrap();
        let r = compare_archives(&a, &b, &AtomicBool::new(false)).unwrap();
        assert_eq!(r.entries[0].path, "payload");
        assert_eq!(r.entries[0].status, "same");
        assert!(compare_archives(&a, &b, &AtomicBool::new(true)).is_err());
    }
    #[test]
    fn unsafe_duplicate_and_parent_collision_are_not_silently_matched() {
        assert!(normalized_path("../outside").is_err());
        assert!(normalized_path("C:\\outside").is_err());
        assert_eq!(normalized_path("./folder\\file").unwrap(), "folder/file");
        let mut index = Index::default();
        index.add(directory("folder".into())).unwrap();
        assert!(index.add(directory("folder".into())).is_err());
        let mut index = Index::default();
        index.add(unsupported("link".into(), "link", 0)).unwrap();
        assert!(index.add(directory("link/child".into())).is_err());
    }
}
