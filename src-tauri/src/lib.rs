use diff_ex_core::{
    archives, assets, batch, database, directories, documents, files, git_compare, remote,
    structured, sync, text, workbooks,
};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Manager, State};

#[derive(Default)]
struct AppState {
    jobs: Mutex<HashMap<String, Arc<AtomicBool>>>,
    plans: Mutex<HashMap<String, sync::PreparedSyncPlan>>,
    workspace_lock: Mutex<()>,
}
impl AppState {
    fn start(&self, id: &str) -> Result<Arc<AtomicBool>, String> {
        let mut jobs = self.jobs.lock().map_err(|e| e.to_string())?;
        if jobs.contains_key(id) {
            return Err("A job with this identifier is already running".into());
        }
        let flag = Arc::new(AtomicBool::new(false));
        jobs.insert(id.into(), flag.clone());
        Ok(flag)
    }
    fn finish(&self, id: &str) {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.remove(id);
        }
    }
}

#[tauri::command]
async fn run_batch_task(
    state: State<'_, AppState>,
    task: batch::BatchTask,
    job_id: String,
) -> Result<batch::TaskResult, String> {
    let flag = state.start(&job_id)?;
    let result = tauri::async_runtime::spawn_blocking(move || batch::run_task(&task, &flag))
        .await
        .map_err(|e| e.to_string());
    state.finish(&job_id);
    result
}
#[tauri::command]
async fn database_catalog(request: database::DbRequest) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || database::catalog(&request))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn compare_database(
    state: State<'_, AppState>,
    left: database::DbRequest,
    right: database::DbRequest,
    options: database::DbOptions,
    job_id: String,
) -> Result<database::DbResult, String> {
    let flag = state.start(&job_id)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        database::compare(left, right, options, &flag)
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    state.finish(&job_id);
    result
}
#[tauri::command]
async fn git_refs(repository: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || git_compare::refs(&repository))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn compare_git(
    state: State<'_, AppState>,
    left: git_compare::GitSelection,
    right: git_compare::GitSelection,
    job_id: String,
) -> Result<git_compare::GitResult, String> {
    let flag = state.start(&job_id)?;
    let result =
        tauri::async_runtime::spawn_blocking(move || git_compare::compare(left, right, &flag))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
    state.finish(&job_id);
    result
}
#[tauri::command]
async fn read_git(
    selection: git_compare::GitSelection,
    path: String,
    fingerprint: String,
    encoding: Option<String>,
) -> Result<files::TextDocument, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_compare::read(selection, &path, &fingerprint, encoding.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn probe_ssh(profile: remote::SshProfile) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || remote::probe(&profile))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn compare_remote(
    state: State<'_, AppState>,
    left: remote::RemoteRequest,
    right: remote::RemoteRequest,
    job_id: String,
) -> Result<remote::RemoteResult, String> {
    let flag = state.start(&job_id)?;
    let result = tauri::async_runtime::spawn_blocking(move || remote::compare(left, right, &flag))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
    state.finish(&job_id);
    result
}
#[tauri::command]
async fn read_remote(
    request: remote::RemoteRequest,
    path: String,
    fingerprint: String,
    encoding: Option<String>,
) -> Result<files::TextDocument, String> {
    tauri::async_runtime::spawn_blocking(move || {
        remote::read(request, &path, &fingerprint, encoding.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn read_document(
    state: State<'_, AppState>,
    asset: assets::AssetRef,
    job_id: String,
) -> Result<documents::DocumentData, String> {
    let flag = state.start(&job_id)?;
    let result = tauri::async_runtime::spawn_blocking(move || documents::load(&asset, &flag))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
    state.finish(&job_id);
    result
}
#[tauri::command]
async fn render_pdf_page(
    state: State<'_, AppState>,
    asset: assets::AssetRef,
    page: u32,
    dpi: u32,
    job_id: String,
) -> Result<tauri::ipc::Response, String> {
    let flag = state.start(&job_id)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        documents::render_pdf(&asset, page, dpi, &flag)
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    state.finish(&job_id);
    Ok(tauri::ipc::Response::new(result?))
}
#[tauri::command]
async fn compare_archive_assets(
    state: State<'_, AppState>,
    left: assets::AssetRef,
    right: assets::AssetRef,
    job_id: String,
) -> Result<archives::ArchiveResult, String> {
    let cancel = state.start(&job_id)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        archives::compare_archives(&left, &right, &cancel)
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    state.finish(&job_id);
    result
}
#[tauri::command]
async fn inspect_asset(path: String) -> Result<assets::AssetRef, String> {
    tauri::async_runtime::spawn_blocking(move || assets::inspect_asset(&path))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn read_asset_chunk(
    asset: assets::AssetRef,
    offset: u64,
    length: usize,
) -> Result<tauri::ipc::Response, String> {
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        assets::read_asset_chunk(&asset, offset, length)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(tauri::ipc::Response::new(bytes))
}
#[tauri::command]
async fn compare_binary_assets(
    state: State<'_, AppState>,
    left: assets::AssetRef,
    right: assets::AssetRef,
    job_id: String,
) -> Result<assets::BinaryStats, String> {
    let cancel = state.start(&job_id)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        assets::compare_binary(&left, &right, &cancel)
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    state.finish(&job_id);
    result
}
#[tauri::command]
async fn workbook_sheets(asset: assets::AssetRef) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || workbooks::sheet_names(&asset))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn workbook_sheet(
    asset: assets::AssetRef,
    sheet: String,
) -> Result<workbooks::TableSheet, String> {
    tauri::async_runtime::spawn_blocking(move || workbooks::read_sheet(&asset, &sheet))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn read_text_file(
    path: String,
    encoding: Option<String>,
) -> Result<files::TextDocument, String> {
    tauri::async_runtime::spawn_blocking(move || files::read_text(&path, encoding.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn inspect_save_target(path: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || files::inspect_save_target(&path))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn save_merge_result(
    request: files::SaveTextRequest,
    sources: Vec<String>,
) -> Result<files::TextDocument, String> {
    tauri::async_runtime::spawn_blocking(move || files::save_merge_result(request, &sources))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn save_text_file(request: files::SaveTextRequest) -> Result<files::TextDocument, String> {
    tauri::async_runtime::spawn_blocking(move || files::save_text(request))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn compare_texts(
    left: String,
    right: String,
    options: text::TextOptions,
) -> Result<text::TextDiffResult, String> {
    tauri::async_runtime::spawn_blocking(move || text::compare_text(&left, &right, &options))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn compare_structures(
    left: String,
    right: String,
    options: structured::StructuredOptions,
) -> Result<structured::StructuredResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        structured::compare_structured(&left, &right, &options)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn compare_folders(
    state: State<'_, AppState>,
    left_root: String,
    right_root: String,
    options: directories::DirectoryOptions,
    job_id: String,
) -> Result<directories::DirectoryResult, String> {
    let cancel = state.start(&job_id)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        directories::compare_directories(&left_root, &right_root, &options, &cancel)
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    state.finish(&job_id);
    result
}
#[tauri::command]
async fn preview_sync(
    state: State<'_, AppState>,
    left_root: String,
    right_root: String,
    options: directories::DirectoryOptions,
    mode: sync::SyncMode,
    job_id: String,
) -> Result<sync::SyncPlan, String> {
    let cancel = state.start(&job_id)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        sync::plan_sync(&left_root, &right_root, &options, mode, &cancel)
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    state.finish(&job_id);
    let prepared = result?;
    let preview = prepared.preview.clone();
    state
        .plans
        .lock()
        .map_err(|e| e.to_string())?
        .insert(preview.id.clone(), prepared);
    Ok(preview)
}
#[tauri::command]
async fn execute_sync(
    state: State<'_, AppState>,
    plan_id: String,
    job_id: String,
) -> Result<sync::SyncResult, String> {
    let cancel = state.start(&job_id)?;
    let prepared = state
        .plans
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&plan_id);
    let Some(prepared) = prepared else {
        state.finish(&job_id);
        return Err("This preview has expired. Preview synchronization again.".into());
    };
    let result =
        tauri::async_runtime::spawn_blocking(move || sync::execute_sync(prepared, &cancel))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
    state.finish(&job_id);
    result
}
#[tauri::command]
fn discard_sync(state: State<'_, AppState>, plan_id: String) -> Result<(), String> {
    state
        .plans
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&plan_id);
    Ok(())
}
#[tauri::command]
fn cancel_job(state: State<'_, AppState>, job_id: String) -> Result<bool, String> {
    let jobs = state.jobs.lock().map_err(|e| e.to_string())?;
    if let Some(flag) = jobs.get(&job_id) {
        flag.store(true, Ordering::Relaxed);
        Ok(true)
    } else {
        Ok(false)
    }
}
#[tauri::command]
async fn load_workspace(
    app: tauri::AppHandle,
    previous: Option<bool>,
) -> Result<Option<String>, String> {
    let file = if previous.unwrap_or(false) {
        "workspace.previous.json"
    } else {
        "workspace.json"
    };
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join(file);
    tauri::async_runtime::spawn_blocking(move || match fs::read_to_string(&path) {
        Ok(value) => {
            serde_json::from_str::<serde_json::Value>(&value)
                .map_err(|e| format!("Cannot read workspace: {e}"))?;
            Ok(Some(value))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Cannot read workspace: {e}")),
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn archive_workspace(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _guard = state.workspace_lock.lock().map_err(|e| e.to_string())?;
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        for name in ["workspace.json", "workspace.previous.json"] {
            let source = dir.join(name);
            if source.exists() {
                let target = dir.join(format!("recovery-{timestamp}-{name}"));
                fs::copy(source, target)
                    .map_err(|e| format!("Could not archive workspace: {e}"))?;
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn save_workspace(app: tauri::AppHandle, data: String) -> Result<(), String> {
    serde_json::from_str::<serde_json::Value>(&data).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _guard = state.workspace_lock.lock().map_err(|e| e.to_string())?;
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join("workspace.json");
        let mut temp = tempfile::NamedTempFile::new_in(&dir).map_err(|e| e.to_string())?;
        temp.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        if path.exists() {
            fs::copy(&path, dir.join("workspace.previous.json")).map_err(|e| e.to_string())?;
        }
        temp.persist(&path).map_err(|e| e.error.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            run_batch_task,
            database_catalog,
            compare_database,
            git_refs,
            compare_git,
            read_git,
            probe_ssh,
            compare_remote,
            read_remote,
            read_document,
            render_pdf_page,
            compare_archive_assets,
            inspect_asset,
            read_asset_chunk,
            compare_binary_assets,
            workbook_sheets,
            workbook_sheet,
            read_text_file,
            inspect_save_target,
            save_text_file,
            save_merge_result,
            compare_texts,
            compare_structures,
            compare_folders,
            preview_sync,
            execute_sync,
            discard_sync,
            cancel_job,
            load_workspace,
            archive_workspace,
            save_workspace
        ])
        .run(tauri::generate_context!())
        .expect("Unable to start Diff EX");
}
