# Diff EX contributor notes

## Product

Diff EX is a local offline desktop comparison tool in the EX family. Follow `docs/ROADMAP.md` for actual milestone status. Planned features must not be advertised as implemented. README.md is English by default; maintain README.zh-CN.md alongside meaningful user-facing changes. UI supports system language detection, English and Simplified Chinese.

## Layout

- `src`: Vue 3 / TypeScript app; Monaco models live for the lifetime of their tabs.
- `crates/diff-core`: independent Rust algorithms, encoding, files, directory scan and sync.
- `src-tauri`: commands, task cancellation, trusted sync plans and workspace persistence.
- `docs`: product, implementation, format guarantees and handoff.

## Behavior to preserve

- Never normalize source data in place merely because an ignore rule is enabled.
- No arbitrary caps on sessions, file counts or file sizes. Fail with actionable errors when actual resources/format capabilities prevent an operation.
- Save only on explicit user action. Compare expected content fingerprints before overwrite.
- Sync previews are server-owned plans, never frontend-supplied execution operations.
- Preserve backups before overwrite/delete; refuse stale or unsafe paths.
- Track metadata-only dirty state, in-flight edits, undo history across tab switches, and workspace recovery errors.
- Original files, snapshots and credentials are never uploaded by the app.
- Do not mark Windows/macOS packaging or browser interaction checks as passed unless their jobs actually ran successfully.

## Focused checks

`npm run check`, `npm test`, `npm run build`; `cargo fmt --all -- --check`, `cargo test -p diff-ex-core`, and `cargo check -p diff-ex` with platform prerequisites. Browser smoke: `npx playwright install --with-deps chromium`, then `npm run test:ui` against an existing production build. Use checks that address changed behavior; do not repeatedly rebuild all platforms for documentation-only changes.
