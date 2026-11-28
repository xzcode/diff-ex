use encoding_rs::{Encoding, GB18030, GBK, UTF_16BE, UTF_16LE, UTF_8};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, Metadata};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;
use tempfile::NamedTempFile;

pub(crate) static WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LineEnding {
    Lf,
    Crlf,
    Cr,
    Mixed,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocument {
    pub path: Option<String>,
    pub name: String,
    pub content: String,
    pub encoding: String,
    pub bom: bool,
    pub line_ending: LineEnding,
    pub fingerprint: Option<String>,
    pub byte_length: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveTextRequest {
    pub path: String,
    pub content: String,
    pub encoding: String,
    pub bom: bool,
    pub line_ending: LineEnding,
    pub expected_fingerprint: Option<String>,
}

pub(crate) fn cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("Operation cancelled.".into())
    } else {
        Ok(())
    }
}

pub(crate) fn path_string(path: &Path) -> Result<String, String> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        format!(
            "This path is not valid Unicode and cannot be represented safely: {}",
            path.display()
        )
    })
}

pub(crate) fn modified_ms(metadata: &Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
}

/// Content fingerprint without loading the file into memory. Refuses unstable reads.
pub(crate) fn hash_file(path: &Path, cancel: &AtomicBool) -> Result<String, String> {
    cancelled(cancel)?;
    let before = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !before.is_file() || before.file_type().is_symlink() {
        return Err(format!("Not a regular file: {}", path.display()));
    }
    let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let opened = file.metadata().map_err(|e| e.to_string())?;
    if !same_metadata(&before, &opened) {
        return Err(format!("File changed while opening: {}", path.display()));
    }
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        cancelled(cancel)?;
        let n = file
            .read(&mut buffer)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let after = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !same_metadata(&before, &after) {
        return Err(format!("File changed while reading: {}", path.display()));
    }
    Ok(hasher.finalize().to_hex().to_string())
}

pub(crate) fn same_metadata(a: &Metadata, b: &Metadata) -> bool {
    let common = a.file_type() == b.file_type()
        && a.len() == b.len()
        && a.modified().ok() == b.modified().ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        common && a.dev() == b.dev() && a.ino() == b.ino()
    }
    #[cfg(not(unix))]
    {
        common
    }
}

fn encoding_for(label: &str) -> Result<&'static Encoding, String> {
    match label.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "utf8" | "utf-8" => Ok(UTF_8),
        "utf16" | "utf-16" | "utf16le" | "utf-16le" => Ok(UTF_16LE),
        "utf16be" | "utf-16be" => Ok(UTF_16BE),
        "gbk" | "cp936" => Ok(GBK),
        "gb18030" => Ok(GB18030),
        _ => Err(format!(
            "Unsupported encoding: {label}. Choose UTF-8, UTF-16LE, UTF-16BE, GBK, or GB18030."
        )),
    }
}

fn decode(
    bytes: &[u8],
    selected: Option<&str>,
) -> Result<(String, &'static Encoding, bool), String> {
    let bom = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        Some((UTF_8, 3))
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        Some((UTF_16LE, 2))
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        Some((UTF_16BE, 2))
    } else {
        None
    };
    let requested = selected
        .filter(|s| !s.is_empty() && *s != "auto")
        .map(encoding_for)
        .transpose()?;
    if let (Some(requested), Some((detected, _))) = (requested, bom) {
        if requested != detected {
            return Err(format!(
                "The file has a {} BOM, which conflicts with {}.",
                detected.name(),
                requested.name()
            ));
        }
    }
    let encoding = requested.or(bom.map(|(e, _)| e)).unwrap_or(UTF_8);
    let data = &bytes[bom.map(|(_, n)| n).unwrap_or(0)..];
    let text = encoding.decode_without_bom_handling_and_without_replacement(data)
        .ok_or_else(|| format!("Invalid {} data. Select the correct encoding; replacement characters were not inserted.", encoding.name()))?;
    // A zero byte in a UTF-8/legacy file usually means a binary file or BOM-less UTF-16.
    if encoding != UTF_16LE && encoding != UTF_16BE && text.contains('\0') {
        return Err("The file contains NUL bytes. Select UTF-16 explicitly if appropriate; binary files cannot be edited as text.".into());
    }
    Ok((text.into_owned(), encoding, bom.is_some()))
}

pub fn detect_line_ending(content: &str) -> LineEnding {
    let mut lf = false;
    let mut crlf = false;
    let mut cr = false;
    let mut bytes = content.bytes().peekable();
    while let Some(c) = bytes.next() {
        if c == b'\r' {
            if bytes.peek() == Some(&b'\n') {
                crlf = true;
                bytes.next();
            } else {
                cr = true;
            }
        } else if c == b'\n' {
            lf = true;
        }
    }
    match (lf, crlf, cr) {
        (false, false, false) => LineEnding::None,
        (true, false, false) => LineEnding::Lf,
        (false, true, false) => LineEnding::Crlf,
        (false, false, true) => LineEnding::Cr,
        _ => LineEnding::Mixed,
    }
}

pub fn read_text(path: &str, encoding: Option<&str>) -> Result<TextDocument, String> {
    let path = Path::new(path);
    let initial =
        fs::symlink_metadata(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    if initial.file_type().is_symlink() {
        return Err("Open the symlink target explicitly to edit it safely.".into());
    }
    if !initial.is_file() {
        return Err("Only regular files can be opened in the text editor.".into());
    }
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    if !same_metadata(&initial, &file.metadata().map_err(|e| e.to_string())?) {
        return Err("File changed while opening. Reopen it.".into());
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve(
            initial
                .len()
                .try_into()
                .map_err(|_| "File is too large for this machine.".to_string())?,
        )
        .map_err(|e| format!("Not enough memory to open this file: {e}"))?;
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if !same_metadata(
        &initial,
        &fs::symlink_metadata(path).map_err(|e| e.to_string())?,
    ) {
        return Err("File changed while reading. Reopen it.".into());
    }
    let (content, encoding, bom) = decode(&bytes, encoding)?;
    let canonical = fs::canonicalize(path).map_err(|e| e.to_string())?;
    Ok(TextDocument {
        name: path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("The filename is not valid Unicode.")?
            .to_owned(),
        path: Some(path_string(&canonical)?),
        line_ending: detect_line_ending(&content),
        content,
        encoding: encoding.name().to_owned(),
        bom,
        fingerprint: Some(blake3::hash(&bytes).to_hex().to_string()),
        byte_length: bytes.len() as u64,
    })
}

fn encode(request: &SaveTextRequest) -> Result<Vec<u8>, String> {
    let encoding = encoding_for(&request.encoding)?;
    let content = match request.line_ending {
        LineEnding::None | LineEnding::Mixed => request.content.clone(),
        ending => {
            let lf = request.content.replace("\r\n", "\n").replace('\r', "\n");
            match ending {
                LineEnding::Crlf => lf.replace('\n', "\r\n"),
                LineEnding::Cr => lf.replace('\n', "\r"),
                _ => lf,
            }
        }
    };
    if encoding != UTF_16LE && encoding != UTF_16BE && content.contains('\0') {
        return Err("Text contains NUL bytes. Remove them before saving in this encoding.".into());
    }
    let mut bytes = Vec::new();
    if encoding == UTF_16LE || encoding == UTF_16BE {
        if request.bom {
            bytes.extend_from_slice(if encoding == UTF_16LE {
                &[0xFF, 0xFE]
            } else {
                &[0xFE, 0xFF]
            });
        }
        for unit in content.encode_utf16() {
            bytes.extend_from_slice(&if encoding == UTF_16LE {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
    } else {
        if request.bom && encoding != UTF_8 {
            return Err("BOM is supported only for UTF-8 and UTF-16.".into());
        }
        if request.bom {
            bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        let (encoded, _, errors) = encoding.encode(&content);
        if errors {
            return Err(format!(
                "Some characters cannot be represented in {}. Save as UTF-8 or UTF-16.",
                encoding.name()
            ));
        }
        bytes.extend_from_slice(&encoded);
    }
    Ok(bytes)
}

fn save_target(path: &Path) -> Result<PathBuf, String> {
    let name = path.file_name().ok_or("Select a file path.")?;
    if name == "." || name == ".." {
        return Err("Invalid filename.".into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|e| format!("The destination directory is unavailable: {e}"))?;
    Ok(canonical_parent.join(name))
}

/// Inspect a Save As destination without decoding its current contents.
/// Missing paths are distinct from permission errors and unsafe file types.
pub fn inspect_save_target(path: &str) -> Result<Option<String>, String> {
    let path = save_target(Path::new(path))?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err("Refusing to overwrite a directory, symlink, or special file.".into());
            }
            Ok(Some(hash_file(&path, &AtomicBool::new(false))?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Cannot inspect destination: {error}")),
    }
}

fn verify_save_target(path: &Path, expected: Option<&str>) -> Result<Option<Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err("Refusing to overwrite a directory, symlink, or special file.".into());
            }
            let expected = expected.ok_or(
                "The destination already exists. Reopen it or confirm Save As before overwriting.",
            )?;
            let actual = hash_file(path, &AtomicBool::new(false))?;
            if actual != expected {
                return Err("The file changed on disk since it was opened. Reopen it or save to a different path.".into());
            }
            Ok(Some(metadata))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if expected.is_some() {
                Err(
                    "The original file was moved or deleted. Use Save As to create a new file."
                        .into(),
                )
            } else {
                Ok(None)
            }
        }
        Err(e) => Err(format!("Cannot inspect destination: {e}")),
    }
}

/// Reject aliases and hard links to input documents before delegating to the
/// normal fingerprint-checked, atomic save path.
pub fn save_merge_result(
    request: SaveTextRequest,
    sources: &[String],
) -> Result<TextDocument, String> {
    let target = Path::new(&request.path);
    for source in sources {
        let original = Path::new(source);
        if original == target
            || (target.exists()
                && original.exists()
                && same_file::is_same_file(target, original).map_err(|e| e.to_string())?)
        {
            return Err("Choose a result file separate from the three sources.".into());
        }
    }
    save_text(request)
}

pub fn save_text(request: SaveTextRequest) -> Result<TextDocument, String> {
    let _write_guard = WRITE_LOCK
        .lock()
        .map_err(|_| "The file operation lock is unavailable.")?;
    let path = save_target(Path::new(&request.path))?;
    let previous = verify_save_target(&path, request.expected_fingerprint.as_deref())?;
    let bytes = encode(&request)?;
    let mut temporary = NamedTempFile::new_in(path.parent().ok_or("Invalid destination.")?)
        .map_err(|e| e.to_string())?;
    temporary.write_all(&bytes).map_err(|e| e.to_string())?;
    if let Some(metadata) = &previous {
        temporary
            .as_file()
            .set_permissions(metadata.permissions())
            .map_err(|e| e.to_string())?;
    }
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    verify_save_target(&path, request.expected_fingerprint.as_deref())?;
    if previous.is_some() {
        temporary
            .persist(&path)
            .map_err(|e| format!("Could not replace the file atomically: {}", e.error))?;
    } else {
        temporary.persist_noclobber(&path).map_err(|e| {
            format!(
                "Could not create the file; the destination may have appeared: {}",
                e.error
            )
        })?;
    }
    read_text(
        path.to_str().ok_or("Invalid Unicode path.")?,
        Some(&request.encoding),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merge_result_cannot_overwrite_a_source_or_hardlink() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        let alias = dir.path().join("alias.txt");
        let output = dir.path().join("result.txt");
        fs::write(&source, "original").unwrap();
        fs::hard_link(&source, &alias).unwrap();
        let original = read_text(source.to_str().unwrap(), None).unwrap();
        let sources = vec![path_string(&source).unwrap()];
        let request = SaveTextRequest {
            path: path_string(&source).unwrap(),
            content: "merged".into(),
            encoding: "UTF-8".into(),
            bom: false,
            line_ending: LineEnding::Lf,
            expected_fingerprint: original.fingerprint,
        };
        assert!(save_merge_result(request.clone(), &sources).is_err());
        let mut request = request;
        request.path = path_string(&alias).unwrap();
        assert!(save_merge_result(request.clone(), &sources).is_err());
        request.path = path_string(&output).unwrap();
        request.expected_fingerprint = None;
        save_merge_result(request, &sources).unwrap();
        assert_eq!(fs::read_to_string(source).unwrap(), "original");
        assert_eq!(fs::read_to_string(output).unwrap(), "merged");
    }
    #[test]
    fn save_refuses_stale_or_unconfirmed_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        fs::write(&path, "first").unwrap();
        let opened = read_text(path.to_str().unwrap(), None).unwrap();
        let mut request = SaveTextRequest {
            path: path_string(&path).unwrap(),
            content: "edited".into(),
            encoding: "UTF-8".into(),
            bom: false,
            line_ending: LineEnding::Lf,
            expected_fingerprint: None,
        };
        assert!(save_text(request.clone()).is_err());
        request.expected_fingerprint = opened.fingerprint;
        fs::write(&path, "external change").unwrap();
        assert!(save_text(request).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "external change");
    }
    #[test]
    fn utf16_bom_and_crlf_survive_edit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("utf16.txt");
        let request = SaveTextRequest {
            path: path_string(&path).unwrap(),
            content: "第一行\nsecond\n".into(),
            encoding: "UTF-16BE".into(),
            bom: true,
            line_ending: LineEnding::Crlf,
            expected_fingerprint: None,
        };
        let result = save_text(request).unwrap();
        assert!(result.bom);
        assert_eq!(result.encoding, "UTF-16BE");
        assert_eq!(result.line_ending, LineEnding::Crlf);
        assert_eq!(result.content, "第一行\r\nsecond\r\n");
        assert!(fs::read(path).unwrap().starts_with(&[0xFE, 0xFF]));
    }
    #[test]
    fn invalid_encoding_is_never_lossily_decoded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gbk.txt");
        fs::write(&path, [0xD6, 0xD0, 0xCE, 0xC4]).unwrap();
        assert!(read_text(path.to_str().unwrap(), None).is_err());
        assert_eq!(
            read_text(path.to_str().unwrap(), Some("GBK"))
                .unwrap()
                .content,
            "中文"
        );
        fs::write(&path, [0xFF, 0xFE, 0x00]).unwrap();
        assert!(read_text(path.to_str().unwrap(), None).is_err());
    }
    #[test]
    fn gb18030_round_trip_and_binary_save_as_inspection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.txt");
        let request = SaveTextRequest {
            path: path_string(&path).unwrap(),
            content: "中文 🦀 𠮷".into(),
            encoding: "GB18030".into(),
            bom: false,
            line_ending: LineEnding::None,
            expected_fingerprint: None,
        };
        let result = save_text(request).unwrap();
        assert_eq!(result.content, "中文 🦀 𠮷");
        fs::write(&path, [0, 255, 128, 0]).unwrap();
        assert!(read_text(path.to_str().unwrap(), None).is_err());
        assert_eq!(
            inspect_save_target(path.to_str().unwrap()).unwrap(),
            Some(blake3::hash(&[0, 255, 128, 0]).to_hex().to_string())
        );
        assert_eq!(
            inspect_save_target(dir.path().join("new.txt").to_str().unwrap()).unwrap(),
            None
        );
        assert!(inspect_save_target(dir.path().to_str().unwrap()).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_target_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        let link = dir.path().join("link.txt");
        fs::write(&target, "keep").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(read_text(link.to_str().unwrap(), None).is_err());
        let request = SaveTextRequest {
            path: path_string(&link).unwrap(),
            content: "bad".into(),
            encoding: "UTF-8".into(),
            bom: false,
            line_ending: LineEnding::Lf,
            expected_fingerprint: Some(blake3::hash(b"keep").to_hex().to_string()),
        };
        assert!(save_text(request).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "keep");
    }
}
