//! Explicit read-only SFTP connections. Host verification precedes authentication.
use crate::files::{cancelled, hash_file, read_text, TextDocument};
use crate::snapshots::{self, Index, SnapshotEntry, SnapshotItem};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use ssh2::{FileStat, FileType, HashType, Session, Sftp};
use std::fs;
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshProfile {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub fingerprint: String,
    pub auth: String,
    pub key_path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSelection {
    pub kind: String,
    pub path: String,
    pub profile: SshProfile,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRequest {
    pub source: RemoteSelection,
    pub secret: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteResult {
    pub left: RemoteSelection,
    pub right: RemoteSelection,
    pub entries: Vec<SnapshotEntry>,
}
fn handshake(profile: &SshProfile) -> Result<(Session, String), String> {
    if profile.host.is_empty()
        || profile.host.contains(['\0', '/', '\\', '@'])
        || profile.host.chars().any(char::is_whitespace)
        || profile.port == 0
    {
        return Err("Enter a valid SSH host and port.".into());
    }
    let addresses = (profile.host.as_str(), profile.port)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?;
    let mut connection = None;
    let mut last = "No address resolved".to_owned();
    for address in addresses {
        match TcpStream::connect_timeout(&address, Duration::from_secs(12)) {
            Ok(stream) => {
                connection = Some(stream);
                break;
            }
            Err(e) => last = e.to_string(),
        }
    }
    let stream = connection.ok_or(last)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(15)))
        .map_err(|e| e.to_string())?;
    let mut session = Session::new().map_err(|e| e.to_string())?;
    session.set_timeout(15000);
    session.set_tcp_stream(stream);
    session.handshake().map_err(|e| e.to_string())?;
    let hash = session
        .host_key_hash(HashType::Sha256)
        .ok_or("Server did not provide a SHA-256 host-key hash.")?;
    let fingerprint = format!("SHA256:{}", STANDARD_NO_PAD.encode(hash));
    Ok((session, fingerprint))
}
/// Probe performs no authentication and sends no password/key. A presented
/// fingerprint must be verified through a trusted channel by the operator.
pub fn probe(profile: &SshProfile) -> Result<String, String> {
    Ok(handshake(profile)?.1)
}
fn connect(profile: &SshProfile, secret: Option<&str>) -> Result<Sftp, String> {
    let (session, actual) = handshake(profile)?;
    let expected = profile.fingerprint.trim().trim_end_matches('=');
    if expected.is_empty() || expected != actual {
        return Err(format!("SSH host key verification failed. Presented fingerprint: {actual}. Verify it through a trusted channel before connecting."));
    }
    if profile.username.trim().is_empty() || profile.username.contains('\0') {
        return Err("Enter an SSH username.".into());
    }
    match profile.auth.as_str() {
        "password" => session.userauth_password(
            &profile.username,
            secret
                .filter(|s| !s.is_empty())
                .ok_or("Enter the SSH password.")?,
        ),
        "key" => {
            if profile.key_path.is_empty() {
                return Err("Choose a private key file.".into());
            }
            session.userauth_pubkey_file(
                &profile.username,
                None,
                Path::new(&profile.key_path),
                secret.filter(|s| !s.is_empty()),
            )
        }
        "agent" => session.userauth_agent(&profile.username),
        _ => return Err("Unsupported SSH authentication method.".into()),
    }
    .map_err(|e| format!("SSH authentication failed: {e}"))?;
    if !session.authenticated() {
        return Err("SSH authentication incomplete.".into());
    }
    session.sftp().map_err(|e| e.to_string())
}
fn remote_root(path: &str) -> Result<String, String> {
    if !path.starts_with('/') || path.contains(['\\', '\0']) {
        return Err("Use an absolute POSIX remote path.".into());
    }
    let p = path.trim_end_matches('/');
    if p.is_empty() {
        return Ok("/".into());
    }
    for part in p[1..].split('/') {
        snapshots::safe_relative(part)?;
    }
    Ok(p.into())
}
fn join(root: &str, relative: &str) -> Result<String, String> {
    snapshots::safe_relative(relative)?;
    Ok(format!("{}/{}", root.trim_end_matches('/'), relative))
}
fn lstat(sftp: &Sftp, path: &str) -> Result<FileStat, String> {
    let stat = sftp
        .lstat(Path::new(path))
        .map_err(|e| format!("{path}: {e}"))?;
    if stat.perm.is_none() {
        return Err(format!("Server did not provide entry type: {path}"));
    }
    Ok(stat)
}
fn checked_remote(sftp: &Sftp, path: &str) -> Result<FileStat, String> {
    let path = remote_root(path)?;
    if path == "/" {
        return lstat(sftp, &path);
    }
    let mut prefix = String::new();
    let mut last = None;
    let parts: Vec<_> = path[1..].split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        prefix.push('/');
        prefix.push_str(part);
        let stat = lstat(sftp, &prefix)?;
        if stat.file_type() == FileType::Symlink {
            return Err(format!("Remote symbolic link is not followed: {prefix}"));
        }
        if i + 1 < parts.len() && !stat.is_dir() {
            return Err(format!("Remote parent is not a directory: {prefix}"));
        }
        last = Some(stat);
    }
    last.ok_or("Invalid remote path".into())
}
fn same_stat(a: &FileStat, b: &FileStat) -> bool {
    a.size.is_some()
        && a.mtime.is_some()
        && a.size == b.size
        && a.mtime == b.mtime
        && a.perm == b.perm
}
fn hash_remote(
    sftp: &Sftp,
    path: &str,
    relative: String,
    stat: FileStat,
    cancel: &AtomicBool,
) -> Result<SnapshotItem, String> {
    if !stat.is_file() {
        return Ok(snapshots::unsupported(
            relative,
            "special",
            "Remote entry is not a regular file.",
        ));
    }
    let mut file = sftp.open(Path::new(path)).map_err(|e| e.to_string())?;
    let before = file.stat().map_err(|e| e.to_string())?;
    if !same_stat(&stat, &before) {
        return Err(format!(
            "Remote file changed or metadata unavailable: {path}"
        ));
    }
    let (size, hash) = snapshots::stream_hash(&mut file, cancel)?;
    let after = file.stat().map_err(|e| e.to_string())?;
    if !same_stat(&stat, &after)
        || !same_stat(&stat, &lstat(sftp, path)?)
        || Some(size) != stat.size
    {
        return Err(format!("Remote file changed while reading: {path}"));
    }
    Ok(snapshots::file(relative, size, hash, None))
}
fn remote_index(sftp: &Sftp, path: &str, cancel: &AtomicBool) -> Result<(bool, Index), String> {
    let root = remote_root(path)?;
    let stat = checked_remote(sftp, &root)?;
    let mut index = Index::new();
    if stat.is_file() {
        snapshots::add(
            &mut index,
            hash_remote(sftp, &root, "payload".into(), stat, cancel)?,
        )?;
        return Ok((false, index));
    }
    if !stat.is_dir() {
        return Err("Select a regular remote file or directory.".into());
    }
    let mut pending = vec![String::new()];
    while let Some(relative) = pending.pop() {
        cancelled(cancel)?;
        let path = if relative.is_empty() {
            root.clone()
        } else {
            join(&root, &relative)?
        };
        if !checked_remote(sftp, &path)?.is_dir() {
            return Err(format!("Remote directory changed: {path}"));
        }
        for (entry, _) in sftp.readdir(Path::new(&path)).map_err(|e| e.to_string())? {
            cancelled(cancel)?;
            let name = entry
                .file_name()
                .and_then(|p| p.to_str())
                .ok_or("Remote entry is not Unicode")?;
            if name == "." || name == ".." {
                continue;
            }
            snapshots::safe_relative(name)?;
            let relative = if relative.is_empty() {
                name.to_owned()
            } else {
                format!("{relative}/{name}")
            };
            let full = join(&root, &relative)?;
            let stat = lstat(sftp, &full)?;
            let item = if stat.is_dir() {
                pending.push(relative.clone());
                snapshots::directory(relative)
            } else if stat.is_file() {
                hash_remote(sftp, &full, relative, stat, cancel)?
            } else {
                snapshots::unsupported(
                    relative,
                    if stat.file_type() == FileType::Symlink {
                        "link"
                    } else {
                        "special"
                    },
                    "Remote links and special entries are not followed.",
                )
            };
            snapshots::add(&mut index, item)?;
        }
    }
    Ok((true, index))
}
fn local_index(path: &str, cancel: &AtomicBool) -> Result<(bool, Index), String> {
    let path = PathBuf::from(path);
    let stat = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    let mut index = Index::new();
    if stat.file_type().is_symlink() {
        return Err("Local symbolic links are not followed.".into());
    }
    if stat.is_file() {
        let hash = hash_file(&path, cancel)?;
        snapshots::add(
            &mut index,
            snapshots::file("payload".into(), stat.len(), hash, None),
        )?;
        return Ok((false, index));
    }
    if !stat.is_dir() {
        return Err("Select a regular local file or directory.".into());
    }
    let mut pending = vec![String::new()];
    while let Some(relative) = pending.pop() {
        cancelled(cancel)?;
        let dir = if relative.is_empty() {
            path.clone()
        } else {
            snapshots::checked_path(&path, &relative)?
        };
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            cancelled(cancel)?;
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "Non-Unicode file name")?;
            snapshots::safe_relative(&name)?;
            let relative = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            let item = snapshots::local_item(&path, &relative, None, cancel)?
                .ok_or("Local file disappeared")?;
            if item.kind == "directory" {
                pending.push(relative.clone())
            }
            snapshots::add(&mut index, item)?;
        }
    }
    Ok((true, index))
}
fn load(request: &RemoteRequest, cancel: &AtomicBool) -> Result<(bool, Index), String> {
    match request.source.kind.as_str() {
        "local" => local_index(&request.source.path, cancel),
        "sftp" => {
            let sftp = connect(&request.source.profile, request.secret.as_deref())?;
            remote_index(&sftp, &request.source.path, cancel)
        }
        _ => Err("Unsupported source type.".into()),
    }
}
pub fn compare(
    left: RemoteRequest,
    right: RemoteRequest,
    cancel: &AtomicBool,
) -> Result<RemoteResult, String> {
    let (l_dir, a) = load(&left, cancel)?;
    let (r_dir, b) = load(&right, cancel)?;
    if l_dir != r_dir {
        return Err("Compare two files or two directories, not a file with a directory.".into());
    }
    Ok(RemoteResult {
        left: left.source,
        right: right.source,
        entries: snapshots::compare(a, b, cancel)?,
    })
}
pub fn read(
    request: RemoteRequest,
    relative: &str,
    expected: &str,
    encoding: Option<&str>,
) -> Result<TextDocument, String> {
    snapshots::safe_relative(relative)?;
    match request.source.kind.as_str() {
        "local" => {
            let path = Path::new(&request.source.path);
            let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
            if meta.is_file() && relative == "payload" {
                let doc = read_text(&request.source.path, encoding)?;
                if doc.fingerprint.as_deref() != Some(expected) {
                    return Err("File content changed after comparison.".into());
                }
                return Ok(TextDocument {
                    path: None,
                    fingerprint: None,
                    ..doc
                });
            }
            snapshots::read_local_snapshot(path, relative, expected, encoding)
        }
        "sftp" => {
            let sftp = connect(&request.source.profile, request.secret.as_deref())?;
            let root = remote_root(&request.source.path)?;
            let root_stat = checked_remote(&sftp, &root)?;
            let path = if root_stat.is_file() && relative == "payload" {
                root
            } else {
                join(&root, relative)?
            };
            let before = checked_remote(&sftp, &path)?;
            if !before.is_file() {
                return Err("Not a regular remote file.".into());
            }
            let mut file = sftp.open(Path::new(&path)).map_err(|e| e.to_string())?;
            if !same_stat(&before, &file.stat().map_err(|e| e.to_string())?) {
                return Err("Remote file changed before reading.".into());
            }
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            if !same_stat(&before, &lstat(&sftp, &path)?) || Some(bytes.len() as u64) != before.size
            {
                return Err("Remote file changed while reading.".into());
            }
            snapshots::checked_text(&bytes, path, expected, encoding)
        }
        _ => Err("Unsupported source type.".into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn local(path: &Path) -> RemoteRequest {
        RemoteRequest {
            source: RemoteSelection {
                kind: "local".into(),
                path: path.to_str().unwrap().into(),
                profile: SshProfile {
                    name: "".into(),
                    host: "".into(),
                    port: 22,
                    username: "".into(),
                    fingerprint: "".into(),
                    auth: "agent".into(),
                    key_path: "".into(),
                },
            },
            secret: None,
        }
    }
    #[test]
    fn local_file_and_directory_source_semantics() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        fs::write(a.path().join("x"), "old").unwrap();
        fs::write(b.path().join("x"), "new").unwrap();
        fs::write(b.path().join("y"), "extra").unwrap();
        let r = compare(local(a.path()), local(b.path()), &AtomicBool::new(false)).unwrap();
        assert_eq!(r.entries[0].status, "different");
        assert_eq!(r.entries[1].status, "rightOnly");
        let hash = r.entries[0]
            .left
            .as_ref()
            .unwrap()
            .fingerprint
            .as_deref()
            .unwrap();
        assert_eq!(
            read(local(a.path()), "x", hash, None).unwrap().content,
            "old"
        );
        let r = compare(
            local(&a.path().join("x")),
            local(&b.path().join("y")),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(r.entries[0].path, "payload");
        assert_eq!(r.entries[0].status, "different");
        assert!(compare(
            local(a.path()),
            local(&b.path().join("y")),
            &AtomicBool::new(false)
        )
        .is_err());
        assert!(remote_root("/a/../b").is_err());
    }
    // Optional isolated localhost integration (set by a dedicated fixture
    // runner, never using a user's production SSH credentials).
    #[test]
    fn sftp_fixture_if_configured() {
        let Ok(host) = std::env::var("DIFF_EX_SFTP_TEST_HOST") else {
            return;
        };
        let profile = SshProfile {
            name: "fixture".into(),
            host,
            port: std::env::var("DIFF_EX_SFTP_TEST_PORT")
                .unwrap()
                .parse()
                .unwrap(),
            username: std::env::var("DIFF_EX_SFTP_TEST_USER").unwrap(),
            fingerprint: std::env::var("DIFF_EX_SFTP_TEST_PIN").unwrap(),
            auth: "key".into(),
            key_path: std::env::var("DIFF_EX_SFTP_TEST_KEY").unwrap(),
        };
        let path = std::env::var("DIFF_EX_SFTP_TEST_PATH").unwrap();
        let root = Path::new(&path);
        let request = || RemoteRequest {
            source: RemoteSelection {
                kind: "sftp".into(),
                path: path.clone(),
                profile: profile.clone(),
            },
            secret: None,
        };
        let result = compare(local(root), request(), &AtomicBool::new(false)).unwrap();
        assert!(!result.entries.is_empty());
        assert!(result.entries.iter().all(|e| e.status == "same"));
        let entry = result
            .entries
            .iter()
            .find(|e| e.right.as_ref().is_some_and(|i| i.kind == "file"))
            .unwrap();
        assert!(read(
            request(),
            &entry.path,
            entry
                .right
                .as_ref()
                .unwrap()
                .fingerprint
                .as_deref()
                .unwrap(),
            None
        )
        .is_ok());
        let mut bad = request();
        bad.source.profile.fingerprint = "SHA256:invalid".into();
        assert!(load(&bad, &AtomicBool::new(false)).is_err());
    }
}
