//! Shared local batch operations for the desktop workspace and standalone CLI.
use crate::{archives, assets, directories, documents, files, git_compare, structured, text};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchTask {
    pub id: String,
    pub kind: String,
    pub left: String,
    pub right: String,
    #[serde(default)]
    pub format: Option<structured::StructuredFormat>,
    #[serde(default)]
    pub text_options: Option<text::TextOptions>,
    #[serde(default)]
    pub structured_options: Option<structured::StructuredOptions>,
    #[serde(default)]
    pub directory_options: Option<directories::DirectoryOptions>,
    #[serde(default)]
    pub encoding: Option<String>,
    #[serde(default)]
    pub left_revision: Option<String>,
    #[serde(default)]
    pub right_revision: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResult {
    pub id: String,
    pub kind: String,
    pub left: String,
    pub right: String,
    pub status: String,
    pub differences: u64,
    pub details: Option<Value>,
    pub error: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchManifest {
    pub version: u32,
    pub tasks: Vec<BatchTask>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchReport {
    pub version: u32,
    pub complete: bool,
    pub results: Vec<TaskResult>,
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn serialize<T: Serialize>(v: T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(err)
}
fn count_entries(value: &Value) -> Result<(u64, bool), String> {
    let rows = value
        .get("entries")
        .and_then(Value::as_array)
        .ok_or("Missing comparison entries")?;
    let mut n = 0;
    let mut uncertain = false;
    for row in rows {
        match row.get("status").and_then(Value::as_str) {
            Some("same") => {}
            Some("error") => uncertain = true,
            Some("different" | "leftOnly" | "rightOnly" | "conflict") => n += 1,
            _ => return Err("Unknown entry status".into()),
        }
    }
    Ok((n, uncertain))
}
fn execute(task: &BatchTask, flag: &AtomicBool) -> Result<(String, u64, Value), String> {
    files::cancelled(flag)?;
    if task.left.trim().is_empty() || task.right.trim().is_empty() {
        return Err("Both source paths are required.".into());
    }
    let (details, count, uncertain) = match task.kind.as_str() {
        "text" => {
            let a = files::read_text(&task.left, task.encoding.as_deref())?;
            let b = files::read_text(&task.right, task.encoding.as_deref())?;
            let options = task.text_options.clone().unwrap_or_default();
            let result = text::compare_text(&a.content, &b.content, &options)?;
            let n = (result.added + result.removed + result.modified) as u64;
            (serialize(result)?, n, false)
        }
        "structured" => {
            let a = files::read_text(&task.left, task.encoding.as_deref())?;
            let b = files::read_text(&task.right, task.encoding.as_deref())?;
            let mut opts = task.structured_options.clone().unwrap_or_default();
            if let Some(format) = task.format {
                opts.format = format;
            }
            let result = structured::compare_structured(&a.content, &b.content, &opts)?;
            let n = result.changes.len() as u64;
            // Canonical source content is unnecessary in automated reports.
            (
                json!({"equal":result.equal,"changes":result.changes}),
                n,
                false,
            )
        }
        "binary" => {
            let a = assets::inspect_asset(&task.left)?;
            let b = assets::inspect_asset(&task.right)?;
            let result = assets::compare_binary(&a, &b, flag)?;
            let n = result
                .changed
                .saturating_add(result.left_only)
                .saturating_add(result.right_only);
            (serialize(result)?, n, false)
        }
        "folder" => {
            let opts = task.directory_options.clone().unwrap_or_default();
            let value = serialize(directories::compare_directories(
                &task.left,
                &task.right,
                &opts,
                flag,
            )?)?;
            let (n, unknown) = count_entries(&value)?;
            // Metadata-only comparison cannot prove content equality.
            let uncertain = unknown || matches!(opts.method, directories::CompareMethod::Metadata);
            (value, n, uncertain)
        }
        "archive" => {
            let a = assets::inspect_asset(&task.left)?;
            let b = assets::inspect_asset(&task.right)?;
            let value = serialize(archives::compare_archives(&a, &b, flag)?)?;
            let (n, unknown) = count_entries(&value)?;
            (value, n, unknown)
        }
        "document" => {
            let a = documents::load(&assets::inspect_asset(&task.left)?, flag)?;
            let b = documents::load(&assets::inspect_asset(&task.right)?, flag)?;
            // This CLI mode checks bytes, extracted text and structural units;
            // it does not assert visual equivalence for differing bytes.
            let bytes_equal = a.fingerprint == b.fingerprint;
            let mut changes = Vec::new();
            let mut uncertain = false;
            for i in 0..a.units.len().max(b.units.len()) {
                files::cancelled(flag)?;
                let l = a.units.get(i);
                let r = b.units.get(i);
                let text_unknown = l.is_some_and(|u| u.warning.is_some())
                    || r.is_some_and(|u| u.warning.is_some());
                let layout_unknown = l.is_some_and(|u| u.layout_fingerprint.is_none())
                    || r.is_some_and(|u| u.layout_fingerprint.is_none());
                let text_diff = l.map(|u| &u.text) != r.map(|u| &u.text);
                let layout_diff = l.and_then(|u| u.layout_fingerprint.as_ref())
                    != r.and_then(|u| u.layout_fingerprint.as_ref());
                if text_unknown || layout_unknown {
                    uncertain = true
                }
                if text_diff || layout_diff || text_unknown || layout_unknown {
                    changes.push(json!({"position":i+1,"textDifferent":text_diff,"layoutDifferent":layout_diff,"textUnknown":text_unknown,"layoutUnknown":layout_unknown,"left":l,"right":r}))
                }
            }
            let n = changes.len() as u64;
            (
                json!({"byteEqual":bytes_equal,"leftFormat":a.format,"rightFormat":b.format,"changes":changes,"leftNotes":a.notes,"rightNotes":b.notes}),
                if bytes_equal { 0 } else { n.max(1) },
                !bytes_equal && uncertain,
            )
        }
        "git" => {
            let left = git_compare::GitSelection {
                repository: task.left.clone(),
                revision: task.left_revision.clone().unwrap_or_else(|| "HEAD".into()),
            };
            let right = git_compare::GitSelection {
                repository: task.right.clone(),
                revision: task
                    .right_revision
                    .clone()
                    .unwrap_or_else(|| "WORKTREE".into()),
            };
            let value = serialize(git_compare::compare(left, right, flag)?)?;
            let (n, unknown) = count_entries(&value)?;
            (value, n, unknown)
        }
        _ => return Err(format!("Unsupported comparison kind: {}", task.kind)),
    };
    files::cancelled(flag)?;
    let status = if uncertain {
        "incomplete"
    } else if count == 0 {
        "equal"
    } else {
        "different"
    };
    Ok((status.into(), count, details))
}
pub fn run_task(task: &BatchTask, flag: &AtomicBool) -> TaskResult {
    let mut result = TaskResult {
        id: task.id.clone(),
        kind: task.kind.clone(),
        left: task.left.clone(),
        right: task.right.clone(),
        status: "error".into(),
        differences: 0,
        details: None,
        error: None,
    };
    match execute(task, flag) {
        Ok((status, n, details)) => {
            result.status = status;
            result.differences = n;
            result.details = Some(details)
        }
        Err(e) => result.error = Some(e),
    }
    result
}
pub fn run_manifest(manifest: &BatchManifest, flag: &AtomicBool) -> Result<BatchReport, String> {
    if manifest.version != 1 {
        return Err("Unsupported batch manifest version.".into());
    }
    if manifest.tasks.iter().any(|t| t.id.trim().is_empty())
        || manifest
            .tasks
            .iter()
            .map(|t| &t.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != manifest.tasks.len()
    {
        return Err("Batch task identifiers must be unique and nonempty.".into());
    }
    let mut results = Vec::new();
    for task in &manifest.tasks {
        if files::cancelled(flag).is_err() {
            break;
        }
        results.push(run_task(task, flag));
    }
    Ok(BatchReport {
        version: 1,
        complete: results.len() == manifest.tasks.len() && files::cancelled(flag).is_ok(),
        results,
    })
}
pub fn exit_code(report: &BatchReport) -> i32 {
    if !report.complete
        || report
            .results
            .iter()
            .any(|r| r.status == "error" || r.status == "incomplete")
    {
        2
    } else if report.results.iter().any(|r| r.status == "different") {
        1
    } else {
        0
    }
}
fn escape(text: &str) -> String {
    let mut result = String::new();
    for c in text.chars() {
        match c {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&#39;"),
            _ => result.push(c),
        }
    }
    result
}
pub fn html(report: &BatchReport) -> Result<String, String> {
    let mut page=String::from("<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Diff EX report</title><style>body{font:14px system-ui;margin:32px;background:#f6f7f9;color:#20242a}table{border-collapse:collapse;width:100%;background:white}th,td{padding:10px;border-bottom:1px solid #ddd;text-align:left;overflow-wrap:anywhere}pre{white-space:pre-wrap;overflow-wrap:anywhere;background:white;padding:16px;border:1px solid #ddd}summary{cursor:pointer}h1{font-size:26px}</style><h1>Diff EX comparison report</h1>");
    page.push_str(&format!("<p>Complete: {} · {} tasks</p><table><thead><tr><th>Task</th><th>Type</th><th>Left</th><th>Right</th><th>Status</th><th>Differences</th></tr></thead><tbody>",report.complete,report.results.len()));
    for r in &report.results {
        page.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            escape(&r.id),
            escape(&r.kind),
            escape(&r.left),
            escape(&r.right),
            escape(&r.status),
            r.differences
        ));
    }
    page.push_str("</tbody></table>");
    for r in &report.results {
        page.push_str(&format!(
            "<details><summary>{} · {}</summary><pre>{}</pre></details>",
            escape(&r.id),
            escape(&r.status),
            escape(&serde_json::to_string_pretty(r).map_err(err)?)
        ))
    }
    page.push_str("</html>");
    Ok(page)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batch_results_exit_codes_and_escaped_html() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a");
        let b = d.path().join("b");
        std::fs::write(&a, "one\n").unwrap();
        std::fs::write(&b, "two\n").unwrap();
        let task = BatchTask {
            id: "<script>x</script>".into(),
            kind: "text".into(),
            left: a.to_str().unwrap().into(),
            right: b.to_str().unwrap().into(),
            format: None,
            text_options: None,
            structured_options: None,
            directory_options: None,
            encoding: None,
            left_revision: None,
            right_revision: None,
        };
        let flag = AtomicBool::new(false);
        let report = run_manifest(
            &BatchManifest {
                version: 1,
                tasks: vec![task.clone()],
            },
            &flag,
        )
        .unwrap();
        assert_eq!(exit_code(&report), 1);
        assert!(!html(&report).unwrap().contains("<script>"));
        let mut equal = task.clone();
        equal.right = equal.left.clone();
        assert_eq!(
            exit_code(
                &run_manifest(
                    &BatchManifest {
                        version: 1,
                        tasks: vec![equal]
                    },
                    &flag
                )
                .unwrap()
            ),
            0
        );
        let mut missing = task.clone();
        missing.right = "missing.txt".into();
        assert_eq!(
            exit_code(
                &run_manifest(
                    &BatchManifest {
                        version: 1,
                        tasks: vec![missing]
                    },
                    &flag
                )
                .unwrap()
            ),
            2
        );
        assert_eq!(
            exit_code(
                &run_manifest(
                    &BatchManifest {
                        version: 1,
                        tasks: vec![task]
                    },
                    &AtomicBool::new(true)
                )
                .unwrap()
            ),
            2
        );
    }
}
