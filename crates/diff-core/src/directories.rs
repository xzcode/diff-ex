use crate::files::{cancelled, hash_file, modified_ms, path_string};
use ignore::{
    gitignore::{Gitignore, GitignoreBuilder},
    WalkBuilder,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CompareMethod {
    Content,
    Metadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryOptions {
    pub method: CompareMethod,
    pub show_hidden: bool,
    pub respect_gitignore: bool,
    pub excludes: Vec<String>,
}
impl Default for DirectoryOptions {
    fn default() -> Self {
        Self {
            method: CompareMethod::Content,
            show_hidden: true,
            respect_gitignore: true,
            excludes: vec![],
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FileKind {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub kind: FileKind,
    pub size: u64,
    pub modified_ms: Option<u64>,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EntryStatus {
    Same,
    Different,
    LeftOnly,
    RightOnly,
    Conflict,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryEntry {
    pub relative_path: String,
    pub status: EntryStatus,
    pub left: Option<FileInfo>,
    pub right: Option<FileInfo>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryResult {
    pub left_root: String,
    pub right_root: String,
    pub entries: Vec<DirectoryEntry>,
}

pub(crate) fn canonical_root(path: &str) -> Result<PathBuf, String> {
    if path.trim().is_empty() {
        return Err("Choose a directory on both sides.".into());
    }
    let canonical =
        fs::canonicalize(path).map_err(|e| format!("Cannot open directory {path}: {e}"))?;
    if !canonical.is_dir() {
        return Err(format!("Not a directory: {path}"));
    }
    path_string(&canonical)?;
    Ok(canonical)
}

pub(crate) fn inspect(
    path: &Path,
    method: CompareMethod,
    cancel: &AtomicBool,
) -> Result<FileInfo, String> {
    cancelled(cancel)?;
    let metadata = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let kind = if metadata.file_type().is_symlink() {
        FileKind::Symlink
    } else if metadata.is_dir() {
        FileKind::Directory
    } else if metadata.is_file() {
        FileKind::File
    } else {
        return Err(format!(
            "Special files (device, socket, or pipe) are not compared: {}",
            path.display()
        ));
    };
    let fingerprint = match kind {
        FileKind::File if method == CompareMethod::Content => Some(hash_file(path, cancel)?),
        FileKind::Symlink => {
            let target = fs::read_link(path).map_err(|e| e.to_string())?;
            Some(
                blake3::hash(target.as_os_str().as_encoded_bytes())
                    .to_hex()
                    .to_string(),
            )
        }
        _ => None,
    };
    Ok(FileInfo {
        kind,
        size: if kind == FileKind::Directory {
            0
        } else {
            metadata.len()
        },
        modified_ms: modified_ms(&metadata),
        fingerprint,
    })
}

struct Scanned {
    info: Option<FileInfo>,
    error: Option<String>,
}

fn custom_ignores(root: &Path, excludes: &[String]) -> Result<Gitignore, String> {
    let mut builder = GitignoreBuilder::new(root);
    for pattern in excludes.iter().filter(|s| !s.trim().is_empty()) {
        builder
            .add_line(None, pattern)
            .map_err(|e| format!("Invalid exclusion pattern '{pattern}': {e}"))?;
    }
    builder
        .build()
        .map_err(|e| format!("Invalid exclusion rules: {e}"))
}

fn scan(
    root: &Path,
    options: &DirectoryOptions,
    cancel: &AtomicBool,
) -> Result<BTreeMap<String, Scanned>, String> {
    let excludes = custom_ignores(root, &options.excludes)?;
    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(false)
        .hidden(!options.show_hidden)
        .git_ignore(options.respect_gitignore)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .ignore(false)
        .require_git(false);
    builder.filter_entry(move |entry| {
        if entry.depth() == 0 {
            return true;
        }
        if entry.file_name() == ".diff-ex-backups" {
            return false;
        }
        !excludes
            .matched_path_or_any_parents(
                entry.path(),
                entry.file_type().is_some_and(|t| t.is_dir()),
            )
            .is_ignore()
    });
    let mut entries = BTreeMap::new();
    for item in builder.build() {
        cancelled(cancel)?;
        let item = item.map_err(|e| format!("Directory scan failed: {e}"))?;
        if let Some(error) = item.error() {
            return Err(format!("Directory ignore rules failed: {error}"));
        }
        if item.depth() == 0 {
            continue;
        }
        let relative = item.path().strip_prefix(root).map_err(|e| e.to_string())?;
        let name = path_string(relative)?.replace(std::path::MAIN_SEPARATOR, "/");
        let (info, error) = match inspect(item.path(), options.method, cancel) {
            Ok(info) => (Some(info), None),
            Err(error) => (None, Some(error)),
        };
        entries.insert(name, Scanned { info, error });
    }
    Ok(entries)
}

/// Both trees use the union of their ignore policies. A path ignored only on the
/// source side must never become a mirror deletion on the other side.
struct IgnorePolicy<'a> {
    root: &'a Path,
    options: &'a DirectoryOptions,
    custom: Gitignore,
    cache: BTreeMap<PathBuf, Option<Gitignore>>,
}
impl<'a> IgnorePolicy<'a> {
    fn new(root: &'a Path, options: &'a DirectoryOptions) -> Result<Self, String> {
        Ok(Self {
            root,
            options,
            custom: custom_ignores(root, &options.excludes)?,
            cache: BTreeMap::new(),
        })
    }
    fn load(&mut self, dir: &Path) -> Result<(), String> {
        if self.cache.contains_key(dir) {
            return Ok(());
        }
        let rule_path = dir.join(".gitignore");
        let value = match fs::symlink_metadata(&rule_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "Symlinked ignore files are not followed: {}",
                    rule_path.display()
                ))
            }
            Ok(metadata) if metadata.is_file() => {
                let mut builder = GitignoreBuilder::new(dir);
                if let Some(error) = builder.add(&rule_path) {
                    return Err(format!(
                        "Invalid ignore rules in {}: {error}",
                        rule_path.display()
                    ));
                }
                Some(builder.build().map_err(|e| e.to_string())?)
            }
            Ok(_) => None,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    || error.kind() == std::io::ErrorKind::NotADirectory =>
            {
                None
            }
            Err(error) => {
                return Err(format!(
                    "Cannot read ignore rules {}: {error}",
                    rule_path.display()
                ))
            }
        };
        self.cache.insert(dir.to_path_buf(), value);
        Ok(())
    }
    fn ignored(&mut self, relative: &str, is_dir: bool) -> Result<bool, String> {
        let path = Path::new(relative);
        let components: Vec<_> = path.components().collect();
        if components
            .iter()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err("Unsafe relative path in scan.".into());
        }
        if components
            .iter()
            .any(|c| c.as_os_str() == ".diff-ex-backups")
        {
            return Ok(true);
        }
        if !self.options.show_hidden
            && components
                .iter()
                .any(|c| c.as_os_str().to_str().is_some_and(|s| s.starts_with('.')))
        {
            return Ok(true);
        }
        if self
            .custom
            .matched_path_or_any_parents(self.root.join(path), is_dir)
            .is_ignore()
        {
            return Ok(true);
        }
        if !self.options.respect_gitignore {
            return Ok(false);
        }
        let mut rules = vec![];
        let mut prefix = self.root.to_path_buf();
        self.load(&prefix)?;
        rules.push(prefix.clone());
        let mut can_read_child_rules = true;
        for (i, component) in components.iter().enumerate() {
            prefix.push(component.as_os_str());
            let directory = i + 1 < components.len() || is_dir;
            let mut ignored = false;
            for rule_dir in &rules {
                if let Some(Some(rule)) = self.cache.get(rule_dir) {
                    let matched = rule.matched(&prefix, directory);
                    if matched.is_ignore() {
                        ignored = true;
                    } else if matched.is_whitelist() {
                        ignored = false;
                    }
                }
            }
            if ignored {
                return Ok(true);
            }
            if directory && can_read_child_rules {
                can_read_child_rules = fs::symlink_metadata(&prefix)
                    .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink());
                if can_read_child_rules {
                    self.load(&prefix)?;
                    rules.push(prefix.clone());
                }
            }
        }
        Ok(false)
    }
}

pub fn compare_directories(
    left: &str,
    right: &str,
    options: &DirectoryOptions,
    cancel: &AtomicBool,
) -> Result<DirectoryResult, String> {
    let left = canonical_root(left)?;
    let right = canonical_root(right)?;
    let left_items = scan(&left, options, cancel)?;
    let right_items = scan(&right, options, cancel)?;
    let keys: BTreeSet<_> = left_items
        .keys()
        .chain(right_items.keys())
        .cloned()
        .collect();
    let mut left_policy = IgnorePolicy::new(&left, options)?;
    let mut right_policy = IgnorePolicy::new(&right, options)?;
    let mut entries = BTreeMap::new();
    for key in keys {
        cancelled(cancel)?;
        let l = left_items.get(&key);
        let r = right_items.get(&key);
        let li = l.and_then(|v| v.info.clone());
        let ri = r.and_then(|v| v.info.clone());
        let is_dir = li
            .as_ref()
            .or(ri.as_ref())
            .is_some_and(|info| info.kind == FileKind::Directory);
        if left_policy.ignored(&key, is_dir)? || right_policy.ignored(&key, is_dir)? {
            continue;
        }
        let error = l
            .and_then(|v| v.error.as_ref())
            .or(r.and_then(|v| v.error.as_ref()))
            .cloned();
        let status = if error.is_some() {
            EntryStatus::Error
        } else {
            match (&li, &ri) {
                (Some(_), None) => EntryStatus::LeftOnly,
                (None, Some(_)) => EntryStatus::RightOnly,
                (Some(a), Some(b)) if a.kind != b.kind => EntryStatus::Conflict,
                (Some(a), Some(_b)) if a.kind == FileKind::Directory => EntryStatus::Same,
                (Some(a), Some(b)) => {
                    let same = if options.method == CompareMethod::Content
                        || a.kind == FileKind::Symlink
                    {
                        a.fingerprint == b.fingerprint
                    } else {
                        a.size == b.size && a.modified_ms == b.modified_ms
                    };
                    if same {
                        EntryStatus::Same
                    } else {
                        EntryStatus::Different
                    }
                }
                _ => EntryStatus::Error,
            }
        };
        entries.insert(
            key.clone(),
            DirectoryEntry {
                relative_path: key,
                status,
                left: li,
                right: ri,
                error,
            },
        );
    }
    // A shared directory reflects differences in its included descendants.
    let differences: Vec<_> = entries
        .iter()
        .filter(|(_, e)| e.status != EntryStatus::Same)
        .map(|(p, _)| p.clone())
        .collect();
    for relative in differences {
        let mut parent = Path::new(&relative).parent();
        while let Some(path) = parent.filter(|p| !p.as_os_str().is_empty()) {
            if let Some(entry) = entries.get_mut(path.to_str().ok_or("Invalid Unicode path.")?) {
                if entry.status == EntryStatus::Same {
                    entry.status = EntryStatus::Different;
                }
            }
            parent = path.parent();
        }
    }
    Ok(DirectoryResult {
        left_root: path_string(&left)?,
        right_root: path_string(&right)?,
        entries: entries.into_values().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_size_different_bytes_and_type_conflicts_are_visible() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        fs::write(l.path().join("file"), "abc").unwrap();
        fs::write(r.path().join("file"), "xyz").unwrap();
        fs::create_dir(l.path().join("kind")).unwrap();
        fs::write(r.path().join("kind"), "x").unwrap();
        let result = compare_directories(
            l.path().to_str().unwrap(),
            r.path().to_str().unwrap(),
            &DirectoryOptions::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            result
                .entries
                .iter()
                .find(|e| e.relative_path == "file")
                .unwrap()
                .status,
            EntryStatus::Different
        );
        assert_eq!(
            result
                .entries
                .iter()
                .find(|e| e.relative_path == "kind")
                .unwrap()
                .status,
            EntryStatus::Conflict
        );
    }
    #[test]
    fn ignore_union_protects_paths_hidden_only_on_one_side() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        fs::write(l.path().join(".gitignore"), "cache/\n*.log\n").unwrap();
        fs::create_dir(r.path().join("cache")).unwrap();
        fs::write(r.path().join("cache/keep"), "keep").unwrap();
        fs::create_dir(l.path().join("nested")).unwrap();
        fs::create_dir(r.path().join("nested")).unwrap();
        fs::write(l.path().join("nested/.gitignore"), "!keep.log\n").unwrap();
        fs::write(r.path().join("nested/keep.log"), "visible").unwrap();
        fs::write(r.path().join("other.log"), "hidden").unwrap();
        let result = compare_directories(
            l.path().to_str().unwrap(),
            r.path().to_str().unwrap(),
            &DirectoryOptions::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(result
            .entries
            .iter()
            .all(|e| !e.relative_path.starts_with("cache") && e.relative_path != "other.log"));
        assert!(result
            .entries
            .iter()
            .any(|e| e.relative_path == "nested/keep.log"));
    }
    #[cfg(unix)]
    #[test]
    fn scan_does_not_follow_symlinks() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("private"), "keep").unwrap();
        std::os::unix::fs::symlink(outside.path(), l.path().join("link")).unwrap();
        let result = compare_directories(
            l.path().to_str().unwrap(),
            r.path().to_str().unwrap(),
            &DirectoryOptions::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(
            result.entries[0].left.as_ref().unwrap().kind,
            FileKind::Symlink
        );
    }
}
