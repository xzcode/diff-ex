<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { ArrowLeftRight, ArrowRight, Bookmark, Braces, Check, ChevronRight, Code2, FileDiff, FolderOpen, Github, Home, Keyboard, Laptop, LockKeyhole, PanelLeftClose, Plus, Search, Settings2, ShieldCheck, X } from 'lucide-vue-next'
import TextCompare from './components/TextCompare.vue'
import StructuredCompare from './components/StructuredCompare.vue'
import FolderCompare from './components/FolderCompare.vue'
import type { CompareKind, CompareSession, DirectoryEntry, Side, TextDocument, Theme } from './contracts'
import { activeSyncJobs, browserDocument, chooseSavePath, chooseText, confirmAction, downloadText, isDesktop, loadWorkspaceRaw, readText, saveWorkspaceRaw, writeText } from './lib/bridge'
import { clone, decodeWorkspace, defaultPreferences, isDirty, isSideDirty, metadataSignature, newSession, sampleSession, type SavedSession, type Workspace } from './lib/workspace'

const sessions = ref<CompareSession[]>([])
const activeId = ref('')
const preferences = ref({ ...defaultPreferences })
const savedSessions = ref<SavedSession[]>([])
const search = ref('')
const settingsOpen = ref(false)
const helpOpen = ref(false)
const loaded = ref(false)
const restoringFailed = ref(false)
const closing = ref(false)
const closingTabs = new Set<string>()
const busy = ref(false)
const workspaceError = ref('')
const notification = ref<{ text: string; error: boolean } | null>(null)
const nameDialog = ref(false)
const sessionName = ref('')
const nameInput = ref<HTMLInputElement>()
const dragging = ref(false)
const active = computed(() => sessions.value.find(s => s.id === activeId.value))
const language = computed<'en' | 'zh-CN'>(() => preferences.value.locale === 'system' ? (navigator.language.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en') : preferences.value.locale)
const tr = (en: string, zh: string) => language.value === 'zh-CN' ? zh : en
const filteredSaved = computed(() => savedSessions.value.filter(s => s.name.toLowerCase().includes(search.value.toLowerCase())))
const kindName = (kind: CompareKind) => ({ text: tr('Text compare', '文本对比'), folder: tr('Folder compare', '目录对比'), structured: tr('Structured compare', '结构化对比') }[kind])
const titleFor = (s: CompareSession) => s.title || (s.kind === 'folder' ? [s.leftRoot.split(/[/\\]/).filter(Boolean).pop(), s.rightRoot.split(/[/\\]/).filter(Boolean).pop()].filter(Boolean).join(' ↔ ') : [s.left.name !== 'Untitled' ? s.left.name : '', s.right.name !== 'Untitled' ? s.right.name : ''].filter(Boolean).filter((v, i, a) => !i || v !== a[i - 1]).join(' ↔ ')) || kindName(s.kind)
const kindIcon = (kind: CompareKind) => kind === 'folder' ? FolderOpen : kind === 'structured' ? Braces : FileDiff
let persistTimer: ReturnType<typeof setTimeout> | undefined
let notificationTimer: ReturnType<typeof setTimeout> | undefined
let persistQueue: Promise<boolean> = Promise.resolve(true)
const unlisten: (() => void)[] = []
function notify(text: string, error = false) {
  notification.value = { text, error }
  if (notificationTimer) clearTimeout(notificationTimer)
  if (!error) notificationTimer = setTimeout(() => { notification.value = null }, 4500)
}
function snapshot(): Workspace {
  return { version: 1, sessions: sessions.value, activeId: activeId.value, preferences: preferences.value, savedSessions: savedSessions.value }
}
function persist(): Promise<boolean> {
  if (persistTimer) clearTimeout(persistTimer)
  if (!loaded.value) return Promise.resolve(false)
  if (restoringFailed.value) return Promise.resolve(false)
  const data = JSON.stringify(snapshot())
  persistQueue = persistQueue.then(async () => {
    try { await saveWorkspaceRaw(data); workspaceError.value = ''; return true }
    catch (e) { workspaceError.value = String(e); return false }
  })
  return persistQueue
}
watch([sessions, activeId, preferences, savedSessions], () => {
  if (!loaded.value) return
  if (persistTimer) clearTimeout(persistTimer)
  persistTimer = setTimeout(() => { void persist() }, 450)
}, { deep: true })
watch(() => preferences.value.theme, theme => { document.documentElement.dataset.theme = theme }, { immediate: true })
watch(language, value => { document.documentElement.lang = value }, { immediate: true })
function addSession(kind: CompareKind, sample = false) {
  if (!loaded.value || restoringFailed.value || closing.value || activeSyncJobs.value) return
  const s = sample ? sampleSession() : newSession(kind)
  if (sample && kind === 'structured') { s.kind = 'structured'; s.title = tr('JSON example', 'JSON 示例') }
  sessions.value.push(s)
  activeId.value = s.id
}
async function closeSession(s: CompareSession) {
  if (closingTabs.has(s.id) || closing.value || busy.value || activeSyncJobs.value) return
  closingTabs.add(s.id)
  try {
    if (isDirty(s) && !(await confirmAction(tr('This comparison has unsaved file edits. Close and discard these edits?', '此对比含有尚未保存到文件的修改。确定关闭并丢弃这些修改？'), tr('Close comparison', '关闭对比')))) return
    const i = sessions.value.findIndex(v => v.id === s.id)
    if (i < 0) return
    sessions.value.splice(i, 1)
    if (activeId.value === s.id) activeId.value = sessions.value[Math.min(i, sessions.value.length - 1)]?.id || ''
  } finally { closingTabs.delete(s.id) }
}
async function replaceDocument(s: CompareSession, side: Side, doc: TextDocument): Promise<boolean> {
  if (isSideDirty(s, side) && !(await confirmAction(tr('Replace this side and discard its unsaved edits?', '替换这一侧并丢弃尚未保存的修改？'), tr('Open file', '打开文件')))) return false
  s[side] = doc
  if (side === 'left') { s.savedLeft = doc.content; s.savedLeftMetadata = metadataSignature(doc) }
  else { s.savedRight = doc.content; s.savedRightMetadata = metadataSignature(doc) }
  return true
}
async function openSide(side: Side, encoding?: string, session?: CompareSession) {
  const s = session || active.value
  if (!s || busy.value) return
  busy.value = true
  try {
    const doc = await chooseText(encoding)
    if (doc && sessions.value.includes(s)) await replaceDocument(s, side, doc)
  } catch (e) { notify(String(e), true) }
  finally { busy.value = false }
}
async function saveSide(side: Side, saveAs = false, session?: CompareSession) {
  const s = session || active.value
  if (!s || busy.value) return
  const doc = clone(s[side])
  if (doc.lineEnding === 'mixed') { notify(tr('This file has mixed line endings. Choose LF, CRLF or CR in file settings before saving.', '此文件包含混合换行符，请先在文件设置中选择 LF、CRLF 或 CR 再保存。'), true); return }
  busy.value = true
  try {
    if (!isDesktop) {
      downloadText(doc)
      if (side === 'left') { s.savedLeft = doc.content; s.savedLeftMetadata = metadataSignature(doc) } else { s.savedRight = doc.content; s.savedRightMetadata = metadataSignature(doc) }
      notify(tr('File exported as a download.', '文件已导出下载。'))
      return
    }
    let path = doc.path
    let fingerprint = doc.fingerprint
    if (saveAs || !path) {
      path = await chooseSavePath(doc.path || (doc.name === 'Untitled' ? 'untitled.txt' : doc.name))
      if (!path) return
      if (path !== doc.path) {
        fingerprint = await invoke<string | null>('inspect_save_target', { path })
        if (fingerprint && !(await confirmAction(tr(`Replace the existing file?\n${path}`, `是否替换已有文件？\n${path}`), tr('Save as', '另存为')))) return
      }
    }
    const result = await writeText({ path, content: doc.content, encoding: doc.encoding, bom: doc.bom, lineEnding: doc.lineEnding, expectedFingerprint: fingerprint })
    // Keep edits made while the save was in flight; update the disk baseline separately.
    const current = s[side]
    s[side] = { ...result, content: current.content === doc.content ? result.content : current.content,
      encoding: current.encoding === doc.encoding ? result.encoding : current.encoding,
      bom: current.bom === doc.bom ? result.bom : current.bom,
      lineEnding: current.lineEnding === doc.lineEnding ? result.lineEnding : current.lineEnding }
    if (side === 'left') { s.savedLeft = result.content; s.savedLeftMetadata = metadataSignature(result) } else { s.savedRight = result.content; s.savedRightMetadata = metadataSignature(result) }
    notify(tr(`Saved ${result.name}`, `已保存 ${result.name}`))
  } catch (e) { notify(String(e), true) }
  finally { busy.value = false }
}
function saveNamedSession() {
  if (!active.value) return
  sessionName.value = titleFor(active.value)
  nameDialog.value = true
  void nextTick(() => { nameInput.value?.focus(); nameInput.value?.select() })
}
function commitNamedSession() {
  if (!active.value || !sessionName.value.trim()) return
  const existing = savedSessions.value.find(s => s.session.id === active.value!.id)
  const entry: SavedSession = { id: existing?.id || crypto.randomUUID(), name: sessionName.value.trim(), savedAt: new Date().toISOString(), session: clone(active.value) }
  if (existing) Object.assign(existing, entry)
  else savedSessions.value.unshift(entry)
  active.value.title = entry.name
  nameDialog.value = false
  notify(tr('Session saved with files, edits and comparison rules.', '会话已保存，包含文件、编辑内容和比较规则。'))
}
function restoreSaved(saved: SavedSession) {
  const s = clone(saved.session)
  s.id = crypto.randomUUID()
  s.title = saved.name
  sessions.value.push(s)
  activeId.value = s.id
}
async function removeSaved(saved: SavedSession) {
  if (await confirmAction(tr(`Remove the saved session “${saved.name}”? Original files are unaffected.`, `删除已保存的会话“${saved.name}”？原始文件不受影响。`), tr('Remove session', '删除会话'))) savedSessions.value = savedSessions.value.filter(s => s.id !== saved.id)
}
function joinPath(root: string, relative: string): string { return root.replace(/[/\\]+$/, '') + '/' + relative }
async function openFolderFile(entry: DirectoryEntry, leftRoot: string, rightRoot: string) {
  if (busy.value) return
  busy.value = true
  try {
    const s = newSession('text')
    s.title = entry.relativePath
    const [left, right] = await Promise.all([
      entry.left?.kind === 'file' ? readText(joinPath(leftRoot, entry.relativePath)) : Promise.resolve(null),
      entry.right?.kind === 'file' ? readText(joinPath(rightRoot, entry.relativePath)) : Promise.resolve(null),
    ])
    if (left) { s.left = left; s.savedLeft = left.content; s.savedLeftMetadata = metadataSignature(left) } else s.left.name = tr('(missing on left)', '（左侧不存在）')
    if (right) { s.right = right; s.savedRight = right.content; s.savedRightMetadata = metadataSignature(right) } else s.right.name = tr('(missing on right)', '（右侧不存在）')
    sessions.value.push(s); activeId.value = s.id
  } catch (e) { notify(String(e), true) }
  finally { busy.value = false }
}
async function droppedFiles(files: File[]) {
  dragging.value = false
  if (!files.length || isDesktop) return
  try {
    for (let i = 0; i < files.length; i += 2) {
      const s = newSession('text')
      s.left = await browserDocument(files[i]); s.savedLeft = s.left.content; s.savedLeftMetadata = metadataSignature(s.left)
      if (files[i + 1]) { s.right = await browserDocument(files[i + 1]); s.savedRight = s.right.content; s.savedRightMetadata = metadataSignature(s.right) }
      sessions.value.push(s); activeId.value = s.id
    }
  } catch (e) { notify(String(e), true) }
}
function keyboard(event: KeyboardEvent) {
  if (event.key === 'Escape') { settingsOpen.value = false; helpOpen.value = false; nameDialog.value = false; return }
  if (event.defaultPrevented || !loaded.value || restoringFailed.value || closing.value || activeSyncJobs.value || document.querySelector('[aria-modal="true"]') || !(event.metaKey || event.ctrlKey)) return
  if (event.key.toLowerCase() === 'n') { event.preventDefault(); addSession('text') }
  if (event.key.toLowerCase() === 's' && active.value && active.value.kind !== 'folder') { event.preventDefault(); void saveSide('right', event.shiftKey) }
  if (event.key.toLowerCase() === 'o' && active.value && active.value.kind !== 'folder') { event.preventDefault(); void openSide(active.value.left.content ? 'right' : 'left') }
  if (event.key === ',') { event.preventDefault(); settingsOpen.value = true }
}
async function recoverWorkspace(reset: boolean) {
  if (busy.value) return
  if (reset && !(await confirmAction(tr('Archive the existing workspace and start a new empty workspace?', '备份现有工作区后，重新开始一个空工作区？'), tr('Reset workspace', '重置工作区')))) return
  busy.value = true
  try {
    let restored: Workspace | null = null
    if (!reset) {
      if (!isDesktop) throw new Error(tr('No previous snapshot is available in the browser preview.', '浏览器预览没有上次快照。'))
      const previous = await invoke<string | null>('load_workspace', { previous: true })
      if (!previous) throw new Error(tr('No previous workspace snapshot exists.', '没有可恢复的上次快照。'))
      restored = decodeWorkspace(previous)
    }
    if (isDesktop) await invoke('archive_workspace')
    else {
      const raw = localStorage.getItem('diff-ex.workspace.v1')
      if (raw) localStorage.setItem('diff-ex.workspace.recovery.' + Date.now(), raw)
    }
    if (restored) { sessions.value = restored.sessions; activeId.value = restored.activeId; preferences.value = restored.preferences; savedSessions.value = restored.savedSessions }
    else { sessions.value = []; activeId.value = ''; savedSessions.value = [] }
    restoringFailed.value = false
    workspaceError.value = ''
    await persist()
  } catch (e) { notify(String(e), true) }
  finally { busy.value = false }
}
function beforeUnload(event: BeforeUnloadEvent) {
  if (isDesktop) return
  if (persistTimer && !restoringFailed.value && loaded.value) { clearTimeout(persistTimer); try { localStorage.setItem('diff-ex.workspace.v1', JSON.stringify(snapshot())) } catch { event.preventDefault(); return } }
  if (sessions.value.some(isDirty) || restoringFailed.value) event.preventDefault()
}
onMounted(async () => {
  try {
    const raw = await loadWorkspaceRaw()
    if (raw) { const state = decodeWorkspace(raw); sessions.value = state.sessions; activeId.value = state.activeId; preferences.value = state.preferences; savedSessions.value = state.savedSessions }
  } catch (e) { restoringFailed.value = true; workspaceError.value = tr('Workspace could not be restored. Autosave paused: ', '工作区恢复失败，自动保存已暂停：') + String(e) }
  loaded.value = true
  window.addEventListener('keydown', keyboard)
  window.addEventListener('beforeunload', beforeUnload)
  if (isDesktop) {
    unlisten.push(await getCurrentWindow().onCloseRequested(async event => {
      event.preventDefault()
      if (closing.value) return
      if (busy.value || activeSyncJobs.value) { notify(tr('Wait for the current file operation, or cancel synchronization before closing.', '请等待当前文件操作结束，或取消同步后再关闭。'), true); return }
      closing.value = true
      try {
        if (restoringFailed.value) {
          if (await confirmAction(tr('Workspace recovery is unresolved. Close without replacing the saved workspace?', '工作区尚未恢复。是否关闭并保留原工作区文件？'), tr('Close Diff EX', '关闭 Diff EX'))) await getCurrentWindow().destroy()
          return
        }
        if (sessions.value.some(isDirty) && !(await confirmAction(tr('Some edits are not saved to original files. Keep them in the workspace and close Diff EX?', '部分修改尚未保存到原始文件。是否将编辑保存在工作区并关闭？'), tr('Unsaved file edits', '尚未保存的文件修改')))) return
        if (!(await persist())) { notify(tr('Workspace could not be saved. Keep this window open and retry after checking disk space.', '工作区保存失败，请保持窗口打开，检查磁盘空间后重试。'), true); return }
        await getCurrentWindow().destroy()
      } finally { closing.value = false }
    }))
    unlisten.push(await getCurrentWebview().onDragDropEvent(async event => {
      if (event.payload.type === 'over' || event.payload.type === 'enter') dragging.value = true
      if (event.payload.type === 'leave') dragging.value = false
      if (event.payload.type !== 'drop') return
      dragging.value = false
      try {
        const paths = event.payload.paths
        for (let i = 0; i < paths.length; i += 2) {
          const s = newSession('text')
          s.left = await readText(paths[i]); s.savedLeft = s.left.content; s.savedLeftMetadata = metadataSignature(s.left)
          if (paths[i + 1]) { s.right = await readText(paths[i + 1]); s.savedRight = s.right.content; s.savedRightMetadata = metadataSignature(s.right) }
          sessions.value.push(s); activeId.value = s.id
        }
      } catch (e) { notify(String(e), true) }
    }))
  }
})
onBeforeUnmount(() => { unlisten.forEach(fn => fn()); window.removeEventListener('keydown', keyboard); window.removeEventListener('beforeunload', beforeUnload); if (persistTimer) clearTimeout(persistTimer); if (notificationTimer) clearTimeout(notificationTimer) })
const themes: { value: Theme; en: string; zh: string; colors: string[] }[] = [
  { value: 'dark', en: 'Midnight', zh: '深色', colors: ['#10131a','#1a1f2b','#7899ff'] },
  { value: 'black', en: 'Pure black', zh: '纯黑', colors: ['#000000','#151515','#9dacf6'] },
  { value: 'gray', en: 'Graphite', zh: '灰色', colors: ['#292b2f','#3c3f45','#a8b8f5'] },
  { value: 'light', en: 'Soft light', zh: '柔和白色', colors: ['#f3f2ef','#e7e6e2','#536bc4'] },
]
</script>

<template>
  <div class="app-shell" @dragover.prevent="!isDesktop && (dragging = true)" @dragleave.self="dragging = false" @drop.prevent="droppedFiles(Array.from($event.dataTransfer?.files || []))">
    <header class="app-header" :inert="!loaded || closing || restoringFailed || !!activeSyncJobs"><div class="brand"><img src="/diff-ex.svg" alt="" /><span>Diff <b>EX</b></span><span class="brand-divider" /><span class="header-label">{{ tr('Compare workspace', '文件对比工作区') }}</span></div><div class="header-actions"><span v-if="!isDesktop" class="preview-badge">{{ tr('BROWSER PREVIEW', '浏览器预览') }}</span><span class="local-badge"><span />{{ tr('Local & offline', '本地 · 离线') }}</span><button class="btn-icon" :title="tr('Keyboard shortcuts', '快捷键')" @click="helpOpen = true"><Keyboard :size="17" /></button><button class="btn-icon" :title="tr('Settings', '设置')" @click="settingsOpen = true"><Settings2 :size="18" /></button></div></header>
    <div class="app-layout" :class="{ 'sidebar-hidden': !preferences.showSidebar }">
      <aside v-if="preferences.showSidebar" class="sidebar" :inert="!loaded || closing || restoringFailed || !!activeSyncJobs"><div class="sidebar-top"><span class="eyebrow">{{ tr('WORKSPACE', '工作区') }}</span><button class="btn-icon subtle" :title="tr('Hide sidebar', '收起侧栏')" @click="preferences.showSidebar = false"><PanelLeftClose :size="16" /></button></div><button class="nav-item" :class="{ active: !active }" @click="activeId = ''"><Home :size="17" /><span>{{ tr('Overview', '概览') }}</span></button><div class="sidebar-label">{{ tr('NEW COMPARISON', '新建对比') }}</div><button v-for="kind in (['text','folder','structured'] as const)" :key="kind" class="nav-item" @click="addSession(kind)"><component :is="kindIcon(kind)" :size="18" /><span>{{ kindName(kind) }}</span><Plus :size="14" class="nav-plus" /></button><div class="sidebar-section-heading"><span class="sidebar-label">{{ tr('SAVED SESSIONS', '已保存会话') }}</span><span class="count">{{ savedSessions.length }}</span></div><label v-if="savedSessions.length" class="session-search"><Search :size="14" /><input v-model="search" :placeholder="tr('Find a session…', '查找会话…')"></label><div class="saved-session-list"><div v-for="s in filteredSaved" :key="s.id" class="saved-session"><button :title="s.name" @click="restoreSaved(s)"><component :is="kindIcon(s.session.kind)" :size="15" /><span>{{ s.name }}</span></button><button class="remove-saved" :title="tr('Remove saved session', '删除已保存会话')" @click="removeSaved(s)"><X :size="13" /></button></div><div v-if="!savedSessions.length" class="saved-empty"><Bookmark :size="20" :stroke-width="1.4" /><p>{{ tr('Keep your comparisons close.', '把常用对比保存在这里。') }}</p><span>{{ tr('Save a session to return to its files and rules.', '保存会话，再次打开时恢复文件和规则。') }}</span></div></div><div class="sidebar-footer"><ShieldCheck :size="17" /><div><strong>{{ tr('Your files, your device.', '文件始终在你手中。') }}</strong><span>{{ tr('No account. No uploads.', '无需账号，无需上传。') }}</span></div></div></aside>
      <main class="main-workspace" :inert="!loaded || closing"><div class="tab-bar" :inert="!loaded || closing || restoringFailed || !!activeSyncJobs"><button v-if="!preferences.showSidebar" class="btn-icon" :title="tr('Show sidebar', '展开侧栏')" @click="preferences.showSidebar = true"><PanelLeftClose :size="16" style="transform:rotate(180deg)" /></button><button class="home-tab" :class="{ active: !active }" :title="tr('Overview', '概览')" @click="activeId = ''"><Home :size="16" /></button><div class="tabs"><div v-for="s in sessions" :key="s.id" class="comparison-tab" :class="{ active: activeId === s.id }"><button class="tab-select" :title="titleFor(s)" @click="activeId = s.id"><component :is="kindIcon(s.kind)" :size="15" /><span>{{ titleFor(s) }}</span><i v-if="isDirty(s)" class="dirty-dot" /></button><button class="tab-close" :title="tr('Close tab', '关闭标签')" @click="closeSession(s)"><X :size="13" /></button></div></div><button class="new-tab" :title="tr('New text comparison', '新建文本对比')" @click="addSession('text')"><Plus :size="17" /></button><button v-if="active" class="save-session" :title="tr('Save this session', '保存当前会话')" @click="saveNamedSession"><Bookmark :size="15" /><span>{{ tr('Save session', '保存会话') }}</span></button></div>
        <div v-if="workspaceError" class="error-banner" role="alert"><span>{{ tr('Workspace save failed: ', '工作区保存失败：') }}{{ workspaceError }}</span><template v-if="restoringFailed"><button class="btn" @click="recoverWorkspace(false)">{{ tr('Try previous snapshot', '恢复上次快照') }}</button><button class="btn" @click="recoverWorkspace(true)">{{ tr('Archive and reset', '备份后重置') }}</button></template><button v-else class="btn" @click="persist">{{ tr('Retry', '重试') }}</button></div>
        <div v-if="!active" class="welcome" :inert="restoringFailed"><div class="welcome-inner"><div class="welcome-eyebrow"><span />{{ tr('A LITTLE CLARITY GOES A LONG WAY', '每一处变化，都清晰可见') }}</div><h1>{{ tr('See what changed.', '看清每一处差异。') }}<br><span>{{ tr('Keep what matters.', '合并每一个决定。') }}</span></h1><p class="welcome-intro">{{ tr('A focused workspace for comparing files, reviewing changes, and bringing the right version forward.', '比较文件、审阅变更、合并内容。一个专注于差异的本地工作区。') }}</p><div class="comparison-cards"><button class="comparison-card text-card" @click="addSession('text')"><div class="card-icon"><FileDiff :size="25" :stroke-width="1.5" /></div><span class="card-number">01</span><h2>{{ tr('Text & code', '文本与代码') }}</h2><p>{{ tr('Compare line by line. Edit and merge in either direction.', '逐行逐字查看差异，自由编辑，双向合并。') }}</p><div class="card-footer"><span>TXT · CODE · LOG</span><ArrowRight :size="18" /></div></button><button class="comparison-card folder-card" @click="addSession('folder')"><div class="card-icon"><FolderOpen :size="25" :stroke-width="1.5" /></div><span class="card-number">02</span><h2>{{ tr('Folders', '文件夹') }}</h2><p>{{ tr('Find changed files. Preview and apply a safe synchronization.', '找出目录中的变更，预览并执行文件同步。') }}</p><div class="card-footer"><span>{{ tr('RECURSIVE · SYNC', '递归比较 · 目录同步') }}</span><ArrowRight :size="18" /></div></button><button class="comparison-card structure-card" @click="addSession('structured')"><div class="card-icon"><Braces :size="25" :stroke-width="1.5" /></div><span class="card-number">03</span><h2>{{ tr('Structured data', '结构化数据') }}</h2><p>{{ tr('Understand changes by their meaning, keys and values.', '以字段、节点和数据类型，理解内容的变化。') }}</p><div class="card-footer"><span>JSON · YAML · XML</span><ArrowRight :size="18" /></div></button></div><div class="welcome-bottom"><div><Code2 :size="19" /><div><strong>{{ tr('Start with an example', '先试一个示例') }}</strong><span>{{ tr('Explore a small configuration change.', '通过配置文件，快速体验对比与合并。') }}</span></div></div><button class="btn" @click="addSession('text', true)">{{ tr('Open example', '打开示例') }}<ChevronRight :size="15" /></button></div><div class="welcome-notes"><span><LockKeyhole :size="13" />{{ tr('Processed on your device', '仅在本机处理') }}</span><span><ArrowLeftRight :size="14" />{{ tr('No session limits', '不限制会话数量') }}</span><span><Laptop :size="14" />Windows · macOS · Linux</span></div></div><div class="welcome-watermark">DIFF / EX</div></div>
        <div v-show="active" class="compare-container" :inert="restoringFailed"><div v-for="s in sessions" :key="s.id" v-show="activeId === s.id" class="session-view"><TextCompare v-if="s.kind === 'text'" :session="s" :preferences="preferences" :language="language" @change="(side, content) => s[side].content = content" @open="(side, encoding) => openSide(side, encoding, s)" @save="(side, as) => saveSide(side, as, s)" @update-options="s.textOptions = $event" @update-document="(side, patch) => Object.assign(s[side], patch)"/><StructuredCompare v-else-if="s.kind === 'structured'" :session="s" :preferences="preferences" :language="language" @change="(side, content) => s[side].content = content" @open="(side, encoding) => openSide(side, encoding, s)" @save="(side, as) => saveSide(side, as, s)" @update-options="s.structuredOptions = $event" @update-document="(side, patch) => Object.assign(s[side], patch)"/><FolderCompare v-else :session="s" :language="language" @update-roots="(left, right) => { s.leftRoot = left; s.rightRoot = right }" @update-options="s.directoryOptions = $event" @open-file="openFolderFile"/></div></div>
      </main>
    </div>
    <footer class="app-status"><span><span class="connection-dot" />{{ busy ? tr('Working…', '处理中…') : tr('Ready', '就绪') }}</span><span v-if="active">{{ kindName(active.kind) }}</span><span class="status-spacer"/><span v-if="!isDesktop">{{ tr('Native filesystem features require desktop', '原生文件系统功能需要桌面版') }}</span><span>{{ restoringFailed || workspaceError ? tr('Autosave paused', '自动保存已暂停') : tr('Workspace auto-save', '工作区自动保存') }}</span><span>v0.1.0</span></footer>
    <div v-if="notification" class="toast" :class="{ error: notification.error }" role="status"><Check v-if="!notification.error" :size="17" /><span>{{ notification.text }}</span><button :title="tr('Dismiss', '关闭提示')" @click="notification = null"><X :size="16" /></button></div>
    <div v-if="!loaded" class="loading-overlay"><span>{{ tr('Restoring workspace…', '正在恢复工作区…') }}</span></div>
    <div v-if="dragging" class="drop-overlay" @dragleave.prevent="dragging = false" @drop.prevent="droppedFiles(Array.from($event.dataTransfer?.files || []))"><FileDiff :size="45"/><h2>{{ tr('Drop files to compare', '拖入文件开始对比') }}</h2><p>{{ tr('Files open in pairs as new comparisons.', '文件将成对打开为新的对比标签。') }}</p></div>
    <Teleport to="body"><div v-if="settingsOpen" class="modal-backdrop" @click.self="settingsOpen = false"><section class="modal settings-modal" role="dialog" aria-modal="true" :aria-label="tr('Settings', '设置')"><header><div><span class="eyebrow">{{ tr('MAKE IT YOURS', '按你的习惯') }}</span><h2>{{ tr('Workspace settings', '工作区设置') }}</h2></div><button class="btn-icon" :title="tr('Close settings', '关闭设置')" @click="settingsOpen = false"><X :size="20"/></button></header><div class="setting-section"><h3>{{ tr('Appearance', '外观主题') }}</h3><div class="theme-choices"><button v-for="theme in themes" :key="theme.value" :class="{ selected: preferences.theme === theme.value }" @click="preferences.theme = theme.value"><div :style="{ background: theme.colors[0] }"><i :style="{ background: theme.colors[1] }"/><span :style="{ background: theme.colors[2] }"/><Check v-if="preferences.theme === theme.value" :size="16"/></div><span>{{ tr(theme.en, theme.zh) }}</span></button></div></div><div class="setting-row"><label for="language-setting">{{ tr('Language', '显示语言') }}<span>{{ tr('Applied immediately and remembered.', '立即生效，重启后保留。') }}</span></label><select id="language-setting" v-model="preferences.locale"><option value="system">{{ tr('Follow system', '跟随系统') }}</option><option value="en">English</option><option value="zh-CN">简体中文</option></select></div><div class="setting-row"><label for="font-setting">{{ tr('Editor font size', '编辑器字号') }}</label><div class="font-control"><input id="font-setting" v-model.number="preferences.fontSize" type="number" min="8" max="40" @change="preferences.fontSize = Math.max(8, Math.min(40, Number(preferences.fontSize) || 13))"><span>px</span></div></div><div class="setting-row"><label for="wrap-setting">{{ tr('Wrap long lines', '长行自动换行') }}</label><input id="wrap-setting" v-model="preferences.wordWrap" type="checkbox"></div><div class="setting-row"><label for="sidebar-setting">{{ tr('Show workspace sidebar', '显示工作区侧栏') }}</label><input id="sidebar-setting" v-model="preferences.showSidebar" type="checkbox"></div><p class="settings-note"><ShieldCheck :size="16"/>{{ tr('Sessions, unsaved edits and settings are stored locally. Original files are only written when you save or apply a sync.', '会话、未保存的编辑和设置均保存在本机。原始文件仅在保存或执行同步时写入。') }}</p><footer><button class="btn primary" @click="settingsOpen = false">{{ tr('Done', '完成') }}</button></footer></section></div>
    <div v-if="nameDialog" class="modal-backdrop" @click.self="nameDialog = false"><form class="modal name-modal" role="dialog" aria-modal="true" @submit.prevent="commitNamedSession"><header><h2>{{ tr('Save session', '保存会话') }}</h2><button type="button" class="btn-icon" @click="nameDialog = false"><X :size="19"/></button></header><label for="session-name">{{ tr('Session name', '会话名称') }}</label><input ref="nameInput" id="session-name" v-model="sessionName" required/><p class="muted small">{{ tr('Includes both documents, comparison rules and unsaved edits.', '包含两侧文件内容、比较规则和未保存的编辑。') }}</p><footer><button type="button" class="btn" @click="nameDialog = false">{{ tr('Cancel', '取消') }}</button><button class="btn primary" type="submit">{{ tr('Save session', '保存会话') }}</button></footer></form></div>
    <div v-if="helpOpen" class="modal-backdrop" @click.self="helpOpen = false"><section class="modal help-modal" role="dialog" aria-modal="true"><header><h2>{{ tr('Keyboard shortcuts', '键盘快捷键') }}</h2><button class="btn-icon" @click="helpOpen = false"><X :size="20"/></button></header><div v-for="item in [[tr('New text comparison', '新建文本对比'),'⌘ / Ctrl + N'],[tr('Open a file', '打开文件'),'⌘ / Ctrl + O'],[tr('Save active editor / right side', '保存当前编辑器 / 右侧'),'⌘ / Ctrl + S'],[tr('Save as', '另存为'),'⌘ / Ctrl + Shift + S'],[tr('Next / previous difference', '下一个 / 上一个差异'),'F7 / Shift + F7'],[tr('Find in editor', '编辑器内搜索'),'⌘ / Ctrl + F'],[tr('Undo merge / edit', '撤销合并 / 编辑'),'⌘ / Ctrl + Z'],[tr('Settings', '设置'),'⌘ / Ctrl + ,']]" :key="item[0]" class="shortcut-row"><span>{{ item[0] }}</span><kbd>{{ item[1] }}</kbd></div><a class="source-link" href="https://github.com/xzcode/diff-ex" target="_blank" rel="noreferrer"><Github :size="16"/>xzcode/diff-ex</a></section></div></Teleport>
  </div>
</template>
