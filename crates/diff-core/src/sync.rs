use crate::directories::{
    canonical_root, compare_directories, inspect, CompareMethod, DirectoryOptions, EntryStatus,
    FileInfo, FileKind,
};
use crate::files::{cancelled, path_string, same_metadata, WRITE_LOCK};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, FileTimes};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;
use tempfile::NamedTempFile;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SyncMode {
    LeftToRight,
    RightToLeft,
    MirrorLeft,
    MirrorRight,
    Bidirectional,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SyncAction {
    CopyLeft,
    CopyRight,
    DeleteLeft,
    DeleteRight,
    CreateLeftDir,
    CreateRightDir,
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncOperation {
    pub relative_path: String,
    pub action: SyncAction,
    pub reason: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPlan {
    pub id: String,
    pub left_root: String,
    pub right_root: String,
    pub mode: SyncMode,
    pub operations: Vec<SyncOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub completed: usize,
    pub skipped: usize,
    pub backup_root: Option<String>,
    pub errors: Vec<String>,
}

/// Only the preview is public. The executable operation set, roots and expected
/// state remain native-owned and cannot be replaced through a frontend payload.
#[derive(Debug)]
pub struct PreparedSyncPlan {
    pub preview: SyncPlan,
    left: PathBuf,
    right: PathBuf,
    options: DirectoryOptions,
    snapshots: BTreeMap<String, SnapshotPair>,
    operations: Vec<SyncOperation>,
    id: String,
}
#[derive(Debug, Clone)]
struct SnapshotPair {
    left: Option<FileInfo>,
    right: Option<FileInfo>,
}

fn validate_relative(relative: &str) -> Result<&Path, String> {
    let path = Path::new(relative);
    if relative.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!("Unsafe relative path: {relative}"));
    }
    if path
        .components()
        .any(|c| c.as_os_str() == ".diff-ex-backups")
    {
        return Err("Backup directories are not synchronization targets.".into());
    }
    Ok(path)
}

fn distinct_roots(left: &Path, right: &Path) -> Result<(), String> {
    if left == right || left.starts_with(right) || right.starts_with(left) {
        return Err("Synchronization requires two separate directories. Identical or nested roots are not allowed.".into());
    }
    Ok(())
}

fn has_parent_conflict(path: &str, blocked: &[String]) -> bool {
    blocked.iter().any(|parent| {
        path == parent
            || path
                .strip_prefix(parent)
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

pub fn plan_sync(
    left: &str,
    right: &str,
    options: &DirectoryOptions,
    mode: SyncMode,
    cancel: &AtomicBool,
) -> Result<PreparedSyncPlan, String> {
    let left = canonical_root(left)?;
    let right = canonical_root(right)?;
    distinct_roots(&left, &right)?;
    // Metadata is useful for a fast view, but never enough to authorize writes.
    let mut options = options.clone();
    options.method = CompareMethod::Content;
    let comparison = compare_directories(
        &path_string(&left)?,
        &path_string(&right)?,
        &options,
        cancel,
    )?;
    if let Some(error) = comparison
        .entries
        .iter()
        .find(|e| e.status == EntryStatus::Error)
    {
        return Err(format!(
            "Cannot plan synchronization while a path could not be read: {}: {}",
            error.relative_path,
            error.error.as_deref().unwrap_or("Unknown error")
        ));
    }
    let mut operations = Vec::new();
    let mut snapshots = BTreeMap::new();
    let mut blocked = Vec::new();
    for entry in &comparison.entries {
        cancelled(cancel)?;
        validate_relative(&entry.relative_path)?;
        snapshots.insert(
            entry.relative_path.clone(),
            SnapshotPair {
                left: entry.left.clone(),
                right: entry.right.clone(),
            },
        );
        if has_parent_conflict(&entry.relative_path, &blocked) {
            continue;
        }
        let mut add = |action, reason: &str, bytes| {
            operations.push(SyncOperation {
                relative_path: entry.relative_path.clone(),
                action,
                reason: reason.to_owned(),
                bytes,
            })
        };
        if entry.status == EntryStatus::Conflict {
            add(SyncAction::Conflict, "File and directory types differ. Resolve this path manually before synchronization.", 0);
            blocked.push(entry.relative_path.clone());
            continue;
        }
        if entry
            .left
            .as_ref()
            .or(entry.right.as_ref())
            .is_some_and(|f| f.kind == FileKind::Symlink)
        {
            if entry.status != EntryStatus::Same {
                add(SyncAction::Conflict, "Symbolic links are compared but are never followed, copied, or deleted by synchronization.", 0);
            }
            blocked.push(entry.relative_path.clone());
            continue;
        }
        match (&entry.left, &entry.right) {
            (Some(l), Some(r)) if l.kind == FileKind::File && entry.status == EntryStatus::Different => match mode {
                SyncMode::LeftToRight | SyncMode::MirrorLeft => add(SyncAction::CopyRight,"Replace right file with left content; back up the previous right file.",l.size),
                SyncMode::RightToLeft | SyncMode::MirrorRight => add(SyncAction::CopyLeft,"Replace left file with right content; back up the previous left file.",r.size),
                SyncMode::Bidirectional => add(SyncAction::Conflict,"Both files differ. Without a common baseline neither side can be chosen automatically.",0),
            },
            (Some(l), None) => match mode {
                SyncMode::LeftToRight | SyncMode::MirrorLeft | SyncMode::Bidirectional => {
                    if l.kind == FileKind::Directory { add(SyncAction::CreateRightDir,"Create missing directory on the right.",0); }
                    else { add(SyncAction::CopyRight,"Copy left-only file to the right.",l.size); }
                }
                SyncMode::MirrorRight => add(SyncAction::DeleteLeft,"Remove left-only path after making a backup; excluded contents are retained.",l.size),
                SyncMode::RightToLeft => (),
            },
            (None, Some(r)) => match mode {
                SyncMode::RightToLeft | SyncMode::MirrorRight | SyncMode::Bidirectional => {
                    if r.kind == FileKind::Directory { add(SyncAction::CreateLeftDir,"Create missing directory on the left.",0); }
                    else { add(SyncAction::CopyLeft,"Copy right-only file to the left.",r.size); }
                }
                SyncMode::MirrorLeft => add(SyncAction::DeleteRight,"Remove right-only path after making a backup; excluded contents are retained.",r.size),
                SyncMode::LeftToRight => (),
            },
            _ => (),
        }
    }
    operations.sort_by(|a, b| {
        let rank = |op: &SyncOperation| match op.action {
            SyncAction::Conflict => 0,
            SyncAction::CreateLeftDir | SyncAction::CreateRightDir => 1,
            SyncAction::CopyLeft | SyncAction::CopyRight => 2,
            SyncAction::DeleteLeft | SyncAction::DeleteRight => {
                let pair = &snapshots[&op.relative_path];
                let info = if op.action == SyncAction::DeleteLeft {
                    &pair.left
                } else {
                    &pair.right
                };
                if info.as_ref().is_some_and(|f| f.kind == FileKind::Directory) {
                    4
                } else {
                    3
                }
            }
        };
        let ra = rank(a);
        let rb = rank(b);
        ra.cmp(&rb)
            .then_with(|| {
                let da = a.relative_path.matches('/').count();
                let db = b.relative_path.matches('/').count();
                if ra == 4 {
                    db.cmp(&da)
                } else if ra == 1 {
                    da.cmp(&db)
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });
    let id = uuid::Uuid::new_v4().to_string();
    Ok(PreparedSyncPlan {
        preview: SyncPlan {
            id: id.clone(),
            left_root: path_string(&left)?,
            right_root: path_string(&right)?,
            mode,
            operations: operations.clone(),
        },
        left,
        right,
        options,
        snapshots,
        operations,
        id,
    })
}

fn same_info(actual: Option<&FileInfo>, expected: Option<&FileInfo>) -> bool {
    match (actual, expected) {
        (None, None) => true,
        (Some(a), Some(b)) if a.kind == FileKind::Directory && b.kind == FileKind::Directory => {
            true
        }
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Check each ancestor without following a symbolic link. The final component is
/// checked separately so a planned deletion never dereferences it.
fn guarded_path(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    let root_metadata =
        fs::symlink_metadata(root).map_err(|e| format!("Root is unavailable: {e}"))?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err("The selected root changed into a link or non-directory.".into());
    }
    if fs::canonicalize(root).map_err(|e| e.to_string())? != root {
        return Err("The selected root moved or changed. Compare the directories again.".into());
    }
    let mut path = root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for (i, component) in components.iter().enumerate() {
        if !matches!(component, Component::Normal(_)) {
            return Err("Unsafe path component.".into());
        }
        path.push(component.as_os_str());
        if i + 1 < components.len() {
            let meta = fs::symlink_metadata(&path)
                .map_err(|e| format!("Parent directory changed: {}: {e}", path.display()))?;
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(format!(
                    "Refusing to traverse a symlink or non-directory: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(path)
}

fn verify_path(
    root: &Path,
    relative: &Path,
    expected: Option<&FileInfo>,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    let path = guarded_path(root, relative)?;
    let actual = match fs::symlink_metadata(&path) {
        Ok(_) => Some(inspect(&path, CompareMethod::Content, cancel)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("Cannot inspect {}: {e}", path.display())),
    };
    if !same_info(actual.as_ref(), expected) {
        return Err(format!(
            "Path changed since the synchronization preview: {}. Compare and preview again.",
            path.display()
        ));
    }
    Ok(path)
}

fn verify_plan(plan: &PreparedSyncPlan, cancel: &AtomicBool) -> Result<(), String> {
    distinct_roots(&plan.left, &plan.right)?;
    if canonical_root(&path_string(&plan.left)?)? != plan.left
        || canonical_root(&path_string(&plan.right)?)? != plan.right
    {
        return Err("A synchronization root changed. Compare again.".into());
    }
    let actual = compare_directories(
        &path_string(&plan.left)?,
        &path_string(&plan.right)?,
        &plan.options,
        cancel,
    )?;
    if actual.entries.len() != plan.snapshots.len() {
        return Err(
            "The directory contents changed since the preview. Compare and preview again.".into(),
        );
    }
    for entry in actual.entries {
        let Some(expected) = plan.snapshots.get(&entry.relative_path) else {
            return Err("A path was added or removed since the preview. Compare again.".into());
        };
        if entry.error.is_some()
            || !same_info(entry.left.as_ref(), expected.left.as_ref())
            || !same_info(entry.right.as_ref(), expected.right.as_ref())
        {
            return Err(format!(
                "{} changed since the preview. Compare and preview again.",
                entry.relative_path
            ));
        }
    }
    Ok(())
}

fn make_directories_guarded(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    let mut path = root.to_path_buf();
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err("Unsafe backup path.".into());
        }
        path.push(component.as_os_str());
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => (),
            Ok(_) => {
                return Err(format!(
                    "Backup parent is not a regular directory: {}",
                    path.display()
                ))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&path).map_err(|e| format!("Cannot create backup directory: {e}"))?
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(path)
}

/// Source is streamed into a temporary file on the destination filesystem. The
/// source and destination snapshots are checked again before atomic publication.
fn atomic_copy(
    source_root: &Path,
    source_relative: &Path,
    source_info: &FileInfo,
    target_root: &Path,
    target_relative: &Path,
    target_info: Option<&FileInfo>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let source = verify_path(source_root, source_relative, Some(source_info), cancel)?;
    let target = verify_path(target_root, target_relative, target_info, cancel)?;
    let before = fs::symlink_metadata(&source).map_err(|e| e.to_string())?;
    if source_info.kind != FileKind::File {
        return Err("Only regular files can be copied.".into());
    }
    let mut input = File::open(&source).map_err(|e| e.to_string())?;
    if !same_metadata(&before, &input.metadata().map_err(|e| e.to_string())?) {
        return Err("Source changed while opening.".into());
    }
    let mut temporary = NamedTempFile::new_in(target.parent().ok_or("Invalid target.")?)
        .map_err(|e| e.to_string())?;
    let mut hash = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        cancelled(cancel)?;
        let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        temporary
            .write_all(&buffer[..n])
            .map_err(|e| e.to_string())?;
        hash.update(&buffer[..n]);
    }
    if Some(hash.finalize().to_hex().as_str()) != source_info.fingerprint.as_deref() {
        return Err("Source contents changed during copy.".into());
    }
    temporary
        .as_file()
        .set_permissions(before.permissions())
        .map_err(|e| e.to_string())?;
    if let Ok(modified) = before.modified() {
        temporary
            .as_file()
            .set_times(FileTimes::new().set_modified(modified))
            .map_err(|e| e.to_string())?;
    }
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    verify_path(source_root, source_relative, Some(source_info), cancel)?;
    verify_path(target_root, target_relative, target_info, cancel)?;
    cancelled(cancel)?;
    if target_info.is_some() {
        temporary
            .persist(&target)
            .map_err(|e| format!("Atomic replacement failed: {}", e.error))?;
    } else {
        temporary
            .persist_noclobber(&target)
            .map_err(|e| format!("Destination appeared or could not be created: {}", e.error))?;
    }
    Ok(())
}

fn backup(
    plan: &PreparedSyncPlan,
    root: &Path,
    relative: &Path,
    info: &FileInfo,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let backup_relative = Path::new(".diff-ex-backups").join(&plan.id);
    let side = if root == plan.left { "left" } else { "right" };
    let destination_relative = backup_relative.join(side).join(relative);
    let parent = destination_relative
        .parent()
        .ok_or("Invalid backup path.")?;
    make_directories_guarded(root, parent)?;
    if info.kind == FileKind::Directory {
        make_directories_guarded(root, &destination_relative)?;
    } else {
        atomic_copy(
            root,
            relative,
            info,
            root,
            &destination_relative,
            None,
            cancel,
        )?;
    }
    Ok(path_string(&root.join(backup_relative))?)
}

enum Applied {
    Done,
    Skipped(String),
}
fn apply(
    plan: &PreparedSyncPlan,
    op: &SyncOperation,
    cancel: &AtomicBool,
    backup_root: &mut Option<String>,
) -> Result<Applied, String> {
    let relative = validate_relative(&op.relative_path)?;
    let expected = plan
        .snapshots
        .get(&op.relative_path)
        .ok_or("Missing native plan snapshot.")?;
    match op.action {
        SyncAction::Conflict => Ok(Applied::Skipped(format!(
            "{}: {}",
            op.relative_path, op.reason
        ))),
        SyncAction::CreateLeftDir | SyncAction::CreateRightDir => {
            let (root, info) = if op.action == SyncAction::CreateLeftDir {
                (&plan.left, expected.left.as_ref())
            } else {
                (&plan.right, expected.right.as_ref())
            };
            let path = verify_path(root, relative, info, cancel)?;
            fs::create_dir(&path)
                .map_err(|e| format!("Cannot create directory {}: {e}", path.display()))?;
            Ok(Applied::Done)
        }
        SyncAction::CopyLeft | SyncAction::CopyRight => {
            let (source_root, source_info, target_root, target_info) =
                if op.action == SyncAction::CopyLeft {
                    (
                        &plan.right,
                        expected.right.as_ref(),
                        &plan.left,
                        expected.left.as_ref(),
                    )
                } else {
                    (
                        &plan.left,
                        expected.left.as_ref(),
                        &plan.right,
                        expected.right.as_ref(),
                    )
                };
            let source_info = source_info.ok_or("Missing source in native plan.")?;
            verify_path(source_root, relative, Some(source_info), cancel)?;
            verify_path(target_root, relative, target_info, cancel)?;
            if let Some(info) = target_info {
                *backup_root = Some(backup(plan, target_root, relative, info, cancel)?);
            }
            atomic_copy(
                source_root,
                relative,
                source_info,
                target_root,
                relative,
                target_info,
                cancel,
            )?;
            Ok(Applied::Done)
        }
        SyncAction::DeleteLeft | SyncAction::DeleteRight => {
            let (root, info) = if op.action == SyncAction::DeleteLeft {
                (&plan.left, expected.left.as_ref())
            } else {
                (&plan.right, expected.right.as_ref())
            };
            let info = info.ok_or("Missing deletion snapshot.")?;
            let path = verify_path(root, relative, Some(info), cancel)?;
            if info.kind == FileKind::Directory {
                if fs::read_dir(&path)
                    .map_err(|e| e.to_string())?
                    .next()
                    .is_some()
                {
                    return Ok(Applied::Skipped(format!(
                        "{}: directory retained because excluded or newly added contents remain.",
                        op.relative_path
                    )));
                }
                *backup_root = Some(backup(plan, root, relative, info, cancel)?);
                verify_path(root, relative, Some(info), cancel)?;
                cancelled(cancel)?;
                fs::remove_dir(&path).map_err(|e| format!("Directory was retained: {e}"))?;
            } else if info.kind == FileKind::File {
                *backup_root = Some(backup(plan, root, relative, info, cancel)?);
                verify_path(root, relative, Some(info), cancel)?;
                cancelled(cancel)?;
                fs::remove_file(&path).map_err(|e| e.to_string())?;
            } else {
                return Err("Symlinks cannot be deleted by synchronization.".into());
            }
            Ok(Applied::Done)
        }
    }
}

pub fn execute_sync(plan: PreparedSyncPlan, cancel: &AtomicBool) -> Result<SyncResult, String> {
    let _write_guard = WRITE_LOCK
        .lock()
        .map_err(|_| "The file operation lock is unavailable.")?;
    cancelled(cancel)?;
    // All included paths are checked before the first mutation. A changed plan
    // never starts; changes during execution stop further operations.
    verify_plan(&plan, cancel)?;
    let mut result = SyncResult {
        completed: 0,
        skipped: 0,
        backup_root: None,
        errors: Vec::new(),
    };
    for op in &plan.operations {
        let action =
            cancelled(cancel).and_then(|_| apply(&plan, op, cancel, &mut result.backup_root));
        match action {
            Ok(Applied::Done) => result.completed += 1,
            Ok(Applied::Skipped(reason)) => {
                result.skipped += 1;
                result.errors.push(reason);
            }
            Err(error) => {
                result.errors.push(format!("{}: {error}", op.relative_path));
                result.skipped = plan.operations.len().saturating_sub(result.completed);
                break;
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn roots() -> (tempfile::TempDir, tempfile::TempDir) {
        (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap())
    }
    fn plan(l: &Path, r: &Path, mode: SyncMode) -> PreparedSyncPlan {
        plan_sync(
            l.to_str().unwrap(),
            r.to_str().unwrap(),
            &DirectoryOptions::default(),
            mode,
            &AtomicBool::new(false),
        )
        .unwrap()
    }
    #[test]
    fn changed_destination_invalidates_entire_preview_before_writes() {
        let (l, r) = roots();
        fs::write(l.path().join("a"), "left").unwrap();
        fs::write(r.path().join("a"), "right").unwrap();
        fs::write(l.path().join("b"), "new").unwrap();
        let pending = plan(l.path(), r.path(), SyncMode::LeftToRight);
        fs::write(r.path().join("a"), "external edit").unwrap();
        assert!(execute_sync(pending, &AtomicBool::new(false)).is_err());
        assert_eq!(
            fs::read_to_string(r.path().join("a")).unwrap(),
            "external edit"
        );
        assert!(!r.path().join("b").exists());
    }
    #[test]
    fn overwrite_and_mirror_delete_have_recoverable_backups() {
        let (l, r) = roots();
        fs::write(l.path().join("a"), "updated").unwrap();
        fs::write(r.path().join("a"), "previous").unwrap();
        fs::create_dir(r.path().join("obsolete")).unwrap();
        fs::write(r.path().join("obsolete/b"), "deleted").unwrap();
        let result = execute_sync(
            plan(l.path(), r.path(), SyncMode::MirrorLeft),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.completed, 3);
        let backups = PathBuf::from(result.backup_root.unwrap());
        assert_eq!(
            fs::read_to_string(backups.join("right/a")).unwrap(),
            "previous"
        );
        assert_eq!(
            fs::read_to_string(backups.join("right/obsolete/b")).unwrap(),
            "deleted"
        );
        assert!(!r.path().join("obsolete").exists());
        assert_eq!(fs::read_to_string(r.path().join("a")).unwrap(), "updated");
    }
    #[test]
    fn bidirectional_conflicts_are_not_guessed() {
        let (l, r) = roots();
        fs::write(l.path().join("conflict"), "left").unwrap();
        fs::write(r.path().join("conflict"), "right").unwrap();
        fs::create_dir(l.path().join("nested")).unwrap();
        fs::write(l.path().join("nested/new"), "copy me").unwrap();
        fs::write(r.path().join("other"), "right only").unwrap();
        let pending = plan(l.path(), r.path(), SyncMode::Bidirectional);
        assert_eq!(
            pending
                .preview
                .operations
                .iter()
                .filter(|op| op.action == SyncAction::Conflict)
                .count(),
            1
        );
        let result = execute_sync(pending, &AtomicBool::new(false)).unwrap();
        assert_eq!(result.skipped, 1);
        assert_eq!(
            fs::read_to_string(l.path().join("conflict")).unwrap(),
            "left"
        );
        assert_eq!(
            fs::read_to_string(r.path().join("conflict")).unwrap(),
            "right"
        );
        assert_eq!(
            fs::read_to_string(r.path().join("nested/new")).unwrap(),
            "copy me"
        );
        assert!(l.path().join("other").exists());
    }
    #[test]
    fn overlapping_roots_and_cancelled_plan_are_rejected() {
        let (l, r) = roots();
        fs::create_dir(l.path().join("inside")).unwrap();
        assert!(plan_sync(
            l.path().to_str().unwrap(),
            l.path().join("inside").to_str().unwrap(),
            &DirectoryOptions::default(),
            SyncMode::MirrorLeft,
            &AtomicBool::new(false)
        )
        .is_err());
        fs::write(l.path().join("a"), "x").unwrap();
        let p = plan(l.path(), r.path(), SyncMode::LeftToRight);
        assert!(execute_sync(p, &AtomicBool::new(true)).is_err());
        assert!(!r.path().join("a").exists());
    }
    #[test]
    fn ignored_destination_contents_are_never_mirror_deleted() {
        let (l, r) = roots();
        fs::write(l.path().join(".gitignore"), "cache/\n").unwrap();
        fs::create_dir(r.path().join("cache")).unwrap();
        fs::write(r.path().join("cache/local"), "important").unwrap();
        let result = execute_sync(
            plan(l.path(), r.path(), SyncMode::MirrorLeft),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(result.errors.is_empty());
        assert_eq!(
            fs::read_to_string(r.path().join("cache/local")).unwrap(),
            "important"
        );
    }
    #[cfg(unix)]
    #[test]
    fn symlink_swap_after_preview_is_blocked() {
        let (l, r) = roots();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(l.path().join("nested")).unwrap();
        fs::write(l.path().join("nested/a"), "source").unwrap();
        fs::create_dir(r.path().join("nested")).unwrap();
        let pending = plan(l.path(), r.path(), SyncMode::LeftToRight);
        fs::remove_dir(r.path().join("nested")).unwrap();
        std::os::unix::fs::symlink(outside.path(), r.path().join("nested")).unwrap();
        assert!(execute_sync(pending, &AtomicBool::new(false)).is_err());
        assert!(!outside.path().join("a").exists());
    }
}
