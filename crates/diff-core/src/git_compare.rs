//! Local Git objects, refs and workspace comparison. No checkout, hooks, fetch,
//! external diff drivers, or Git shell commands.
use crate::files::{cancelled, TextDocument};
use crate::snapshots::{self, Index, SnapshotEntry};
use git2::{ObjectType, Repository, StatusOptions, TreeWalkMode, TreeWalkResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::atomic::AtomicBool;

const WORKSPACE: &str = "WORKTREE";
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSelection {
    pub repository: String,
    pub revision: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitResult {
    pub left: GitSelection,
    pub right: GitSelection,
    pub entries: Vec<SnapshotEntry>,
}
fn open(path: &str) -> Result<Repository, String> {
    Repository::open(path).map_err(|e| e.to_string())
}
pub fn refs(repository: &str) -> Result<Vec<String>, String> {
    let repo = open(repository)?;
    let mut names = BTreeSet::new();
    for reference in repo.references().map_err(|e| e.to_string())? {
        let r = reference.map_err(|e| e.to_string())?;
        if let Ok(name) = r.name() {
            if name.starts_with("refs/heads/")
                || name.starts_with("refs/remotes/")
                || name.starts_with("refs/tags/")
            {
                names.insert(name.to_owned());
            }
        }
    }
    let mut result = Vec::new();
    if repo.workdir().is_some() {
        result.push(WORKSPACE.into());
    }
    if repo.head().is_ok() {
        result.push("HEAD".into());
    }
    result.extend(names);
    Ok(result)
}
fn tree<'a>(repo: &'a Repository, rev: &str) -> Result<git2::Tree<'a>, String> {
    let object = repo.revparse_single(rev).map_err(|e| e.to_string())?;
    object.peel_to_tree().map_err(|e| e.to_string())
}
fn version_index(
    repo: &Repository,
    revision: &str,
    cancel: &AtomicBool,
) -> Result<(String, Index), String> {
    if revision == WORKSPACE {
        let root = repo
            .workdir()
            .ok_or("Bare repositories have no working tree.")?;
        let mut options = StatusOptions::new();
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_unmodified(true)
            .include_ignored(false)
            .exclude_submodules(false);
        let statuses = repo
            .statuses(Some(&mut options))
            .map_err(|e| e.to_string())?;
        let index_entries = repo.index().map_err(|e| e.to_string())?;
        let mut index = Index::new();
        for status in statuses.iter() {
            cancelled(cancel)?;
            let path = status.path().map_err(|e| e.to_string())?;
            let mode = index_entries
                .get_path(std::path::Path::new(path), 0)
                .map(|e| e.mode);
            if mode == Some(0o160000) {
                snapshots::add(
                    &mut index,
                    snapshots::unsupported(
                        path.into(),
                        "special",
                        "Git submodules are not traversed.",
                    ),
                )?;
                continue;
            }
            let mode = match mode {
                Some(0o100755) => Some("100755".into()),
                Some(0o100644) => Some("100644".into()),
                _ => None,
            };
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                let full = root.join(path);
                match std::fs::symlink_metadata(&full) {
                    Ok(meta) if meta.is_file() => Some(
                        if meta.permissions().mode() & 0o111 != 0 {
                            "100755"
                        } else {
                            "100644"
                        }
                        .into(),
                    ),
                    _ => mode,
                }
            };
            if let Some(item) = snapshots::local_item(root, path, mode, cancel)? {
                snapshots::add(&mut index, item)?;
            }
        }
        return Ok((WORKSPACE.into(), index));
    }
    let tree = tree(repo, revision)?;
    let oid = tree.id().to_string();
    let mut index = Index::new();
    let mut failure = None;
    tree.walk(TreeWalkMode::PreOrder, |root, entry| {
        let result = (|| {
            cancelled(cancel)?;
            let path = format!("{root}{}", entry.name().map_err(|e| e.to_string())?);
            let mode = entry.filemode();
            if entry.kind() == Some(ObjectType::Tree) {
                return Ok(());
            }
            let item = if mode == 0o120000 {
                snapshots::unsupported(path, "link", "Git symbolic links are not followed.")
            } else if entry.kind() != Some(ObjectType::Blob) || mode == 0o160000 {
                snapshots::unsupported(
                    path,
                    "special",
                    "Git submodules and special objects are not traversed.",
                )
            } else {
                let blob = repo.find_blob(entry.id()).map_err(|e| e.to_string())?;
                snapshots::file(
                    path,
                    blob.size() as u64,
                    blake3::hash(blob.content()).to_hex().to_string(),
                    Some(format!("{mode:o}")),
                )
            };
            snapshots::add(&mut index, item)
        })();
        match result {
            Ok(()) => TreeWalkResult::Ok,
            Err(e) => {
                failure = Some(e);
                TreeWalkResult::Abort
            }
        }
    })
    .map_err(|e| e.to_string())?;
    if let Some(e) = failure {
        return Err(e);
    }
    Ok((oid, index))
}
pub fn compare(
    left: GitSelection,
    right: GitSelection,
    cancel: &AtomicBool,
) -> Result<GitResult, String> {
    let l = open(&left.repository)?;
    let r = open(&right.repository)?;
    let (lrev, a) = version_index(&l, &left.revision, cancel)?;
    let (rrev, b) = version_index(&r, &right.revision, cancel)?;
    Ok(GitResult {
        left: GitSelection {
            revision: lrev,
            ..left
        },
        right: GitSelection {
            revision: rrev,
            ..right
        },
        entries: snapshots::compare(a, b, cancel)?,
    })
}
pub fn read(
    selection: GitSelection,
    path: &str,
    fingerprint: &str,
    encoding: Option<&str>,
) -> Result<TextDocument, String> {
    snapshots::safe_relative(path)?;
    let repo = open(&selection.repository)?;
    if selection.revision == WORKSPACE {
        let root = repo.workdir().ok_or("Bare repository")?;
        return snapshots::read_local_snapshot(root, path, fingerprint, encoding);
    }
    let tree = tree(&repo, &selection.revision)?;
    let entry = tree
        .get_path(std::path::Path::new(path))
        .map_err(|e| e.to_string())?;
    if entry.kind() != Some(ObjectType::Blob) || entry.filemode() == 0o120000 {
        return Err("Not a regular Git blob".into());
    }
    let blob = repo.find_blob(entry.id()).map_err(|e| e.to_string())?;
    snapshots::checked_text(blob.content(), path.into(), fingerprint, encoding)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn commit(repo: &Repository, path: &str, text: &str) -> git2::Oid {
        let root = repo.workdir().unwrap();
        fs::write(root.join(path), text).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new(path)).unwrap();
        index.write().unwrap();
        let id = index.write_tree().unwrap();
        let tree = repo.find_tree(id).unwrap();
        let sig = git2::Signature::now("Diff EX", "test@example.invalid").unwrap();
        let parent = repo.head().ok().and_then(|r| r.peel_to_commit().ok());
        let parents: Vec<_> = parent.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, "test", &tree, &parents)
            .unwrap()
    }
    #[test]
    fn real_repository_refs_commits_and_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let a = commit(&repo, "code.txt", "one");
        let b = commit(&repo, "code.txt", "two");
        let root = dir.path().to_str().unwrap().to_owned();
        let sel = |rev: String| GitSelection {
            repository: root.clone(),
            revision: rev,
        };
        assert!(refs(&root).unwrap().contains(&"HEAD".into()));
        let result = compare(
            sel(a.to_string()),
            sel(b.to_string()),
            &AtomicBool::new(false),
        )
        .unwrap();
        let row = &result.entries[0];
        assert_eq!(row.status, "different");
        let doc = read(
            result.left,
            "code.txt",
            row.left.as_ref().unwrap().fingerprint.as_deref().unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(doc.content, "one");
        assert!(doc.path.is_none());
        fs::write(dir.path().join("code.txt"), "three").unwrap();
        fs::write(dir.path().join("new.txt"), "new").unwrap();
        let result = compare(
            sel(b.to_string()),
            sel(WORKSPACE.into()),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            result
                .entries
                .iter()
                .find(|e| e.path == "code.txt")
                .unwrap()
                .status,
            "different"
        );
        assert_eq!(
            result
                .entries
                .iter()
                .find(|e| e.path == "new.txt")
                .unwrap()
                .status,
            "rightOnly"
        );
        let hash = result.entries[0]
            .right
            .as_ref()
            .unwrap()
            .fingerprint
            .as_deref()
            .unwrap();
        fs::write(dir.path().join("code.txt"), "changed").unwrap();
        assert!(read(result.right, "code.txt", hash, None).is_err());
    }
}
