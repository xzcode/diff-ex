/**
 * Real Chromium integration smoke test against the production bundle.
 * Run: npm run build && node scripts/ui-smoke.mjs
 * One-time browser setup: npx playwright install --with-deps chromium
 * Optional: UI_BASE_URL (existing server), UI_CHROMIUM_PATH, UI_ARTIFACTS_DIR.
 * No app-only testing hooks or simulated native filesystem actions are used.
 */
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'
import { chromium } from 'playwright'

const artifacts = resolve(process.env.UI_ARTIFACTS_DIR || 'artifacts/ui')
const baseUrl = process.env.UI_BASE_URL || 'http://127.0.0.1:4173'
const workspaceKey = 'diff-ex.workspace.v1'
const ownServer = !process.env.UI_BASE_URL
const errors = []
const serverLog = []
const checks = []
let browser
let page
let server
let lastInputAttempt
await mkdir(artifacts, { recursive: true })

async function screenshot(name) {
  await page.screenshot({ path: resolve(artifacts, `${name}.png`), fullPage: true })
}
async function state() {
  return page.evaluate(key => JSON.parse(localStorage.getItem(key) || 'null'), workspaceKey)
}
async function activeSession() {
  const workspace = await state()
  return workspace?.sessions.find(session => session.id === workspace.activeId)
}
async function waitState(predicate, label) {
  const deadline = Date.now() + 15_000
  while (Date.now() < deadline) {
    const current = await state()
    if (predicate(current)) return current
    await delay(100)
  }
  throw new Error(`Timed out waiting for persisted state: ${label}`)
}
async function waitEnabled(locator) {
  await locator.waitFor({ state: 'visible' })
  const deadline = Date.now() + 15_000
  while (Date.now() < deadline) {
    if (await locator.isEnabled()) return
    await delay(100)
  }
  throw new Error('Comparison did not finish rendering its first mergeable hunk.')
}
const activeView = () => page.locator('.session-view:visible')
async function pasteText(text) {
  // Input.insertText is typed input to Monaco: multiline text can trigger
  // on-enter indentation and automatic closing brackets. A real clipboard
  // paste uses Monaco's paste path and must preserve the supplied document.
  await page.evaluate(value => navigator.clipboard.writeText(value), text)
  await page.keyboard.press('ControlOrMeta+V')
}
async function typeSource(side, text) {
  const pane = activeView().locator('.structure-editors .monaco-editor').nth(side === 'left' ? 0 : 1)
  await pane.locator('textarea.inputarea, .native-edit-context').first().focus()
  await page.keyboard.press('ControlOrMeta+A')
  lastInputAttempt = { side, method: 'clipboard-paste', expected: text }
  await pasteText(text)
  await waitState(w => w?.sessions.find(s => s.id === w.activeId)?.[side].content === text, `pasted ${side} source`)
}
async function openSettings() {
  await page.getByRole('button', { name: 'Settings', exact: true }).click()
  return page.getByRole('dialog', { name: 'Settings', exact: true })
}

try {
  if (ownServer) {
    server = spawn(process.execPath, ['node_modules/vite/bin/vite.js', 'preview', '--host', '127.0.0.1', '--port', '4173', '--strictPort'], { stdio: ['ignore', 'pipe', 'pipe'] })
    server.stdout.on('data', data => serverLog.push(data.toString()))
    server.stderr.on('data', data => serverLog.push(data.toString()))
    server.on('error', cause => serverLog.push(String(cause)))
  }
  let serving = false
  for (let attempt = 0; attempt < 100; attempt++) {
    try { if ((await fetch(baseUrl)).ok) { serving = true; break } } catch {}
    if (server && server.exitCode !== null) throw new Error(`Preview server exited: ${serverLog.join('')}`)
    await delay(100)
  }
  assert.ok(serving, `Preview unavailable at ${baseUrl}`)
  browser = await chromium.launch({
    headless: true,
    ...(process.env.UI_CHROMIUM_PATH ? { executablePath: process.env.UI_CHROMIUM_PATH } : {}),
  })
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, locale: 'en-US', acceptDownloads: true })
  await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: new URL(baseUrl).origin })
  page = await context.newPage()
  page.on('pageerror', cause => errors.push(cause.message))
  page.on('dialog', dialog => { void dialog.accept() })
  await page.goto(baseUrl, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Open example', exact: true }).waitFor()
  await screenshot('01-overview')

  // Real Monaco worker + merge/undo + persistence: no direct model mutation.
  await page.getByRole('button', { name: 'Open example', exact: true }).click()
  const mergeRight = page.getByRole('button', { name: 'To right', exact: true })
  await waitEnabled(mergeRight)
  const initial = await waitState(w => w?.sessions.length === 1, 'example session')
  const example = initial.sessions[0]
  await mergeRight.click()
  await waitState(w => w?.sessions[0].right.content !== example.right.content, 'hunk merge')
  assert.equal((await activeSession()).left.content, example.left.content, 'Merging right must leave the source unchanged.')
  // Switching tabs must retain this model's undo stack.
  await page.getByRole('button', { name: 'New text comparison', exact: true }).click()
  await waitState(w => w?.sessions.length === 2 && w.activeId !== example.id, 'new scratch tab')
  await page.locator('.comparison-tab .tab-select').filter({ hasText: example.title }).click()
  await waitState(w => w?.activeId === example.id, 'return to original tab')
  await page.getByRole('button', { name: 'Undo', exact: true }).click()
  await waitState(w => w?.sessions.find(s => s.id === example.id)?.right.content === example.right.content, 'undo after tab switch restores exact original content')
  await page.locator('.comparison-tab').nth(1).getByRole('button', { name: 'Close tab', exact: true }).click()
  await waitState(w => w?.sessions.length === 1 && w.activeId === example.id, 'scratch tab closed')
  await page.getByRole('button', { name: 'Next difference', exact: true }).click()
  await screenshot('02-text-comparison')
  checks.push('Monaco renders; hunk merge changes only target; switching tabs retains undo and restores exact text.')

  // A metadata-only change must show dirty state and become clean after export.
  const rightHeader = activeView().locator('.file-header.right')
  await rightHeader.locator('.file-menu summary').click()
  await rightHeader.getByLabel('Write BOM', { exact: true }).check()
  await waitState(w => w?.sessions[0].right.bom === true, 'BOM metadata edit')
  await rightHeader.locator('.dirty-dot').waitFor()
  await page.locator('.comparison-tab.active .dirty-dot').waitFor()
  await rightHeader.locator('.file-menu summary').click()
  let downloading = page.waitForEvent('download')
  await activeView().getByRole('button', { name: 'Save right file', exact: true }).click()
  let download = await downloading
  await download.saveAs(resolve(artifacts, 'exported-bom.txt'))
  const bomBytes = await readFile(resolve(artifacts, 'exported-bom.txt'))
  assert.equal(bomBytes.subarray(0, 3).toString('hex'), 'efbbbf')
  assert.equal(bomBytes.subarray(3).toString('utf8'), example.right.content)
  await waitState(w => w?.sessions[0].savedRightMetadata === JSON.stringify(['utf-8', true, 'lf']), 'saved metadata baseline')
  assert.equal(await rightHeader.locator('.dirty-dot').count(), 0)
  assert.equal(await page.locator('.comparison-tab.active .dirty-dot').count(), 0)

  // File import with mixed EOL requires an explicit output convention.
  await page.getByRole('button', { name: 'New text comparison', exact: true }).click()
  await waitState(w => w?.sessions.length === 2 && w.activeId !== example.id, 'mixed-EOL scratch tab')
  const picking = page.waitForEvent('filechooser')
  await activeView().getByRole('button', { name: 'Open right file', exact: true }).click()
  await (await picking).setFiles({ name: 'mixed.txt', mimeType: 'text/plain', buffer: Buffer.from('alpha\r\nbeta\ngamma\r') })
  await waitState(w => w?.sessions.find(s => s.id === w.activeId)?.right.lineEnding === 'mixed', 'mixed-EOL import')
  await activeView().getByRole('button', { name: 'Save right file', exact: true }).click()
  await page.getByRole('status').filter({ hasText: 'This file has mixed line endings' }).waitFor()
  const mixedHeader = activeView().locator('.file-header.right')
  await mixedHeader.locator('.file-menu summary').click()
  await mixedHeader.getByLabel('Save line endings', { exact: true }).selectOption('lf')
  await mixedHeader.locator('.file-menu summary').click()
  downloading = page.waitForEvent('download')
  await activeView().getByRole('button', { name: 'Save right file', exact: true }).click()
  download = await downloading
  await download.saveAs(resolve(artifacts, 'exported-lf.txt'))
  assert.equal(await readFile(resolve(artifacts, 'exported-lf.txt'), 'utf8'), 'alpha\nbeta\ngamma\n')
  await waitState(w => w?.sessions.find(s => s.id === w.activeId)?.savedRightMetadata === JSON.stringify(['utf-8', false, 'lf']), 'mixed-EOL saved baseline')
  assert.equal(await mixedHeader.locator('.dirty-dot').count(), 0)
  await page.locator('.comparison-tab.active').getByRole('button', { name: 'Close tab', exact: true }).click()
  await waitState(w => w?.sessions.length === 1 && w.activeId === example.id, 'mixed-EOL scratch tab closed')
  checks.push('Metadata-only edits set dirty badges; BOM and selected line endings export correctly and reset saved baselines; mixed EOL requires a choice.')

  // Comparison rules and a named snapshot must survive a reload.
  await page.getByRole('button', { name: 'Rules', exact: true }).click()
  await page.getByLabel('Ignore case', { exact: true }).check()
  await page.getByRole('button', { name: 'Rules', exact: true }).click()
  await page.getByRole('button', { name: 'Save session', exact: true }).click()
  const named = page.locator('.name-modal')
  await named.getByLabel('Session name', { exact: true }).fill('Configuration review')
  await named.getByRole('button', { name: 'Save session', exact: true }).click()
  await waitState(w => w?.savedSessions.some(s => s.name === 'Configuration review' && s.session.textOptions.ignoreCase), 'named session with rules')
  await page.reload({ waitUntil: 'networkidle' })
  await waitEnabled(page.getByRole('button', { name: 'To right', exact: true }))
  assert.equal((await activeSession()).right.content, example.right.content)
  await page.locator('.saved-session').getByRole('button', { name: 'Configuration review', exact: true }).click()
  const restored = await waitState(w => w?.sessions.length === 2, 'saved snapshot restore into its own tab')
  assert.equal(restored.sessions[1].textOptions.ignoreCase, true)
  assert.notEqual(restored.sessions[0].id, restored.sessions[1].id)
  checks.push('Workspace reload restores documents and rules; a saved snapshot opens with a new session identity.')

  // Persisted theme/language settings and compact-window layout.
  let settings = await openSettings()
  await settings.getByRole('button', { name: 'Soft light', exact: true }).click()
  await settings.getByLabel('Language', { exact: false }).selectOption('zh-CN')
  await page.getByRole('button', { name: '完成', exact: true }).click()
  await waitState(w => w?.preferences.theme === 'light' && w.preferences.locale === 'zh-CN', 'theme and locale')
  await page.reload({ waitUntil: 'networkidle' })
  await page.getByRole('button', { name: '合并到右侧', exact: true }).waitFor()
  assert.equal(await page.locator('html').getAttribute('data-theme'), 'light')
  assert.equal(await page.locator('html').getAttribute('lang'), 'zh-CN')
  await page.setViewportSize({ width: 1100, height: 760 })
  await screenshot('03-light-chinese')
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1), false, 'Workspace should not create a horizontal page scrollbar.')
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await page.locator('#language-setting').selectOption('en')
  await page.getByRole('dialog', { name: 'Settings', exact: true }).getByRole('button', { name: 'Midnight', exact: true }).click()
  await page.getByRole('button', { name: 'Done', exact: true }).click()
  await page.setViewportSize({ width: 1440, height: 1000 })
  checks.push('Theme and Chinese locale persist across reload; compact viewport has no page overflow.')

  // Real editor input -> semantic equality, accurate errors, then recovery.
  await page.locator('.sidebar').getByRole('button', { name: 'Structured compare', exact: true }).click()
  await activeView().locator('.structure-editors .monaco-editor').first().waitFor()
  await activeView().getByText('Open or paste two documents to compare.', { exact: true }).waitFor()
  assert.equal(await activeView().locator('.parse-error').count(), 0, 'A new empty comparison should show the start state.')
  const leftJson = '{\n  "name": "Diff EX",\n  "version": 1,\n  "enabled": true,\n  "features": ["text", "folders"]\n}'
  const reorderedJson = '{"features":["text","folders"],"enabled":true,"version":1,"name":"Diff EX"}'
  await typeSource('left', leftJson)
  await typeSource('right', reorderedJson)
  await page.getByText('Structurally equal', { exact: true }).waitFor()
  await typeSource('right', '{"name":"Diff EX","version":"1","features":["text","folders","json"],"new":null}')
  await activeView().locator('.change-row').first().waitFor()
  await activeView().locator('.kind-pill.typeChanged').first().waitFor()
  await activeView().locator('.change-row').first().click()
  await activeView().locator('.change-detail').waitFor()
  await screenshot('04-structured-comparison')
  await page.getByRole('button', { name: 'Canonical JSON', exact: true }).click()
  // The canonical view is read-only and does not change either persisted source.
  const canonicalState = await activeSession()
  const canonicalInput = activeView().locator('.structure-editors .monaco-editor').first().locator('textarea.inputarea, .native-edit-context').first()
  const renderedCanonical = activeView().locator('.structure-editors .monaco-editor').first().locator('.view-lines')
  const canonicalText = await renderedCanonical.textContent()
  await canonicalInput.focus()
  await page.keyboard.press('ControlOrMeta+A')
  await pasteText('SHOULD_NOT_EDIT_CANONICAL')
  assert.equal(await renderedCanonical.textContent(), canonicalText, 'Canonical content must reject pasted edits.')
  await page.keyboard.insertText('x')
  assert.equal(await renderedCanonical.textContent(), canonicalText, 'Canonical content must reject ordinary typing.')
  await page.getByRole('button', { name: 'Original', exact: true }).click()
  assert.equal((await activeSession()).left.content, canonicalState.left.content)
  assert.equal((await activeSession()).right.content, canonicalState.right.content)
  await typeSource('right', '{"broken":')
  await page.getByRole('alert').filter({ hasText: 'Cannot compare this document' }).waitFor()
  await screenshot('05-parse-error')
  await typeSource('right', reorderedJson)
  await page.getByText('Structurally equal', { exact: true }).waitFor()
  assert.equal(await activeView().locator('.parse-error').count(), 0)
  checks.push('JSON ignores object order, detects type changes, keeps canonical output read-only, and recovers after parse errors.')


  // Switch all locales through the actual settings UI. Verify a real localized
  // control, selected preference and document language without reloading editors.
  const localeCases = [
    ['en','Settings'], ['zh-CN','设置'], ['zh-TW','設定'], ['ja','設定'],
    ['ko','설정'], ['de','Einstellungen'], ['fr','Paramètres'], ['es','Configuración'],
    ['pt-BR','Configurações'], ['ru','Настройки'], ['ms','Tetapan'], ['id','Pengaturan'],
  ]
  await page.locator('.comparison-tab .tab-select').first().click()
  await activeView().locator('.text-workspace').waitFor()
  await waitState(w => w?.activeId === w?.sessions[0]?.id, 'return to text comparison before switching locales')
  const editorSnapshot = await activeSession()
  await page.locator('.header-actions button').last().click()
  const languageSelect = page.locator('#language-setting')
  assert.equal(await languageSelect.locator('option').count(), 13, '12 languages plus system language')
  for (const [locale, settingsLabel] of localeCases) {
    await languageSelect.selectOption(locale)
    await page.locator('html[lang="'+locale+'"]').waitFor()
    await page.getByRole('dialog', { name: settingsLabel, exact: true }).waitFor()
    await waitState(w => w?.preferences.locale === locale, 'language '+locale)
  }
  await page.locator('.settings-modal footer button').click()
  await page.reload({ waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Pengaturan', exact: true }).waitFor()
  assert.equal((await activeSession()).left.content, editorSnapshot.left.content)
  assert.equal((await activeSession()).right.content, editorSnapshot.right.content)
  await page.getByRole('button', { name: 'Pengaturan', exact: true }).click()
  await page.locator('#language-setting').selectOption('en')
  await page.locator('.settings-modal footer button').click()
  // Verify each selected theme changes the actual UI surface and editor.
  const themeCases = [
    ['black','rgb(0, 0, 0)'], ['gray','rgb(42, 44, 48)'],
    ['white','rgb(255, 255, 255)'], ['light','rgb(250, 248, 243)'],
    ['dark','rgb(21, 27, 37)'],
  ]
  for (const [theme, expectedEditor] of themeCases) {
    await page.getByRole('button', { name: 'Settings', exact: true }).click()
    await page.locator('[data-theme-option="'+theme+'"]').click()
    await page.locator('.settings-modal footer button').click()
    await page.locator('html[data-theme="'+theme+'"]').waitFor()
    await waitState(w => w?.preferences.theme === theme, 'theme '+theme)
    const actual = await activeView().locator('.monaco-editor-background').first().evaluate(el => getComputedStyle(el).backgroundColor)
    assert.equal(actual, expectedEditor, 'editor color for '+theme)
    await screenshot('theme-'+theme)
  }
  checks.push('All 12 interface languages switch and persist, and all five themes apply to the Monaco editor without losing documents.')

  assert.deepEqual(errors, [], 'No uncaught browser errors are permitted.')
  await writeFile(resolve(artifacts, 'report.json'), JSON.stringify({ ok: true, checks, errors }, null, 2))
  console.log(checks.map(check => `PASS ${check}`).join('\n'))
} catch (cause) {
  if (page) await screenshot('failure').catch(() => {})
  // Only this smoke test's synthetic editor fixture is included. Keeping the
  // expected and persisted bytes makes input-path regressions diagnosable.
  let inputDiagnostics
  if (page && lastInputAttempt) {
    const current = await activeSession().catch(() => undefined)
    const observed = current?.[lastInputAttempt.side]?.content
    inputDiagnostics = { ...lastInputAttempt, observed, expectedLength: lastInputAttempt.expected.length, observedLength: observed?.length, activeSessionKind: current?.kind }
  }
  await writeFile(resolve(artifacts, 'report.json'), JSON.stringify({ ok: false, checks, errors, failure: String(cause), inputDiagnostics, serverLog: serverLog.join('') }, null, 2))
  console.error(cause)
  process.exitCode = 1
} finally {
  await browser?.close()
  server?.kill('SIGTERM')
}
