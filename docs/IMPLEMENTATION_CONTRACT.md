# Initial implementation contract

The TypeScript IPC contracts in `src/contracts.ts` are authoritative. Rust uses serde camelCase for fields and variants. All text hunk ranges are zero-based and half-open into original lines, including blank lines. Never apply a normalized-text range to an unnormalized file. A string ending in newline has a trailing empty line for hunk coordinates, matching Monaco. Filesystem IO and diff jobs run on a blocking worker; UI cancellation never authorizes a filesystem mutation.

Core public modules:
- `files`: `read_text(path: &str, encoding: Option<&str>) -> Result<TextDocument, String>`; `save_text(request: SaveTextRequest) -> Result<TextDocument, String>`.
- `directories`: `compare_directories(left: &str, right: &str, options: &DirectoryOptions, cancel: &AtomicBool) -> Result<DirectoryResult, String>`.
- `sync`: `plan_sync(left: &str, right: &str, options: &DirectoryOptions, mode: SyncMode, cancel: &AtomicBool) -> Result<PreparedSyncPlan, String>`. PreparedSyncPlan exposes `pub preview: SyncPlan`. Native state retains this complete trusted plan by ID. `execute_sync(plan: PreparedSyncPlan, cancel: &AtomicBool) -> Result<SyncResult, String>`. Never accept client supplied operations for execution.
- `text`: `compare_text(left: &str, right: &str, options: &TextOptions) -> Result<TextDiffResult, String>`.
- `structured`: `compare_structured(left: &str, right: &str, options: &StructuredOptions) -> Result<StructuredResult, String>`.

Text/structured options and results owned by their respective modules. Files own TextDocument/SaveTextRequest, directories own directory types, sync owns sync types. Rust crate is `diff-ex-core` / `diff_ex_core`.

Frontend components:
- TextCompare.vue receives `session: CompareSession`, `preferences: Preferences`, `language: 'en' | 'zh-CN'`; emits `change(side, content)` and `open(side)`, `save(side, saveAs?)`, `update-options(TextOptions)`. Calls `compareText(left, right, options)` from `src/lib/bridge.ts` and can use Monaco's diff engine for display. Rules-based comparison results must be distinguished from Monaco's visual raw diff if rules differ. Editing and hunk merge keep original data. Use lucide-vue-next icons.
- StructuredCompare.vue receives same props; emits same `change/open/save` plus `update-options(StructuredOptions)`; calls `compareStructured(...)` from bridge. JSON/YAML/XML tree diff list with searchable paths, readable values, precise parse errors, options, and original/canonical text toggle.
- FolderCompare.vue root implementation.

Web preview may use pure JS implementations for text/structured and selected browser files but must clearly report that native directory sync requires desktop. Do not present simulated filesystem results as real.
