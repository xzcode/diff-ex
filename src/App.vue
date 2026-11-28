<script setup lang="ts">
import { languageOptions, resolveLocale, translate } from './lib/i18n'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { ArrowLeftRight, ArrowRight, Bookmark, Braces, Check, ChevronRight, Code2, FileDiff, FileDigit, ImageIcon, Table2, GitMerge, Archive, FolderOpen, Github, Home, Keyboard, Laptop, LockKeyhole, PanelLeftClose, Plus, Search, Settings2, ShieldCheck, X } from 'lucide-vue-next'
import TextCompare from './components/TextCompare.vue'
import StructuredCompare from './components/StructuredCompare.vue'
import FolderCompare from './components/FolderCompare.vue'
import AssetCompare from './components/AssetCompare.vue'
import ThreeWayCompare from './components/ThreeWayCompare.vue'
import { mergeReady } from './lib/three-way'
import type { AssetRef, AssetSettings, MergeDraft, CompareKind, CompareSession, DirectoryEntry, Side, TextDocument, Theme } from './contracts'
import { activeSyncJobs, browserDocument, chooseSavePath, chooseText, confirmAction, downloadText, isDesktop, loadWorkspaceRaw, readText, saveWorkspaceRaw, writeText } from './lib/bridge'
import { clone, decodeWorkspace, defaultPreferences, defaultAssetSettings, isDirty, isSideDirty, metadataSignature, newSession, sampleSession, type SavedSession, type Workspace } from './lib/workspace'

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
const namingSessionId = ref('')
const nameInput = ref<HTMLInputElement>()
const dragging = ref(false)
const active = computed(() => sessions.value.find(s => s.id === activeId.value))
const systemLanguage = ref(navigator.language)
const updateSystemLanguage = () => { systemLanguage.value = navigator.language }
const language = computed(() => resolveLocale(preferences.value.locale, systemLanguage.value))
const tr = (key: string, ...args: (string | number)[]) => translate(language.value, key, ...args)
const filteredSaved = computed(() => savedSessions.value.filter(s => s.name.toLowerCase().includes(search.value.toLowerCase())))
const kindName = (kind: CompareKind) => ({ text: tr('Text compare'), folder: tr('Folder compare'), structured: tr('Structured compare'), binary: tr('Binary compare'), image: tr('Image compare'), table: tr('Table compare'), merge: tr('Three-way merge'), archive: tr('Archive compare') }[kind])
const isAssetKind = (kind: CompareKind) => ['binary','image','table','archive'].includes(kind)
const titleFor = (s: CompareSession) => s.title || (isAssetKind(s.kind) ? [s.assetSources?.left?.name,s.assetSources?.right?.name].filter(Boolean).filter((v,i,a)=>!i||v!==a[i-1]).join(' ↔ ') : '') || (s.kind === 'folder' ? [s.leftRoot.split(/[/\\]/).filter(Boolean).pop(), s.rightRoot.split(/[/\\]/).filter(Boolean).pop()].filter(Boolean).join(' ↔ ') : [s.left.name !== 'Untitled' ? s.left.name : '', s.right.name !== 'Untitled' ? s.right.name : ''].filter(Boolean).filter((v, i, a) => !i || v !== a[i - 1]).join(' ↔ ')) || kindName(s.kind)
const kindIcon = (kind: CompareKind) => ({text:FileDiff,folder:FolderOpen,structured:Braces,binary:FileDigit,image:ImageIcon,table:Table2,merge:GitMerge,archive:Archive}[kind])
function setAssetSource(session: CompareSession,side:Side,value:AssetRef|null){session.assetSources={left:null,right:null,...session.assetSources,[side]:value}}
function setAssetSettings(session: CompareSession,patch:Partial<AssetSettings>){session.assetSettings={...defaultAssetSettings(),...session.assetSettings,...patch}}
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
  if (sample && kind === 'structured') { s.kind = 'structured'; s.title = tr('JSON example') }
  sessions.value.push(s)
  activeId.value = s.id
}
async function closeSession(s: CompareSession) {
  if (closingTabs.has(s.id) || closing.value || busy.value || activeSyncJobs.value) return
  closingTabs.add(s.id)
  try {
    if (isDirty(s) && !(await confirmAction(tr('This comparison has unsaved file edits. Close and discard these edits?'), tr('Close comparison')))) return
    const i = sessions.value.findIndex(v => v.id === s.id)
    if (i < 0) return
    sessions.value.splice(i, 1)
    if (activeId.value === s.id) activeId.value = sessions.value[Math.min(i, sessions.value.length - 1)]?.id || ''
  } finally { closingTabs.delete(s.id) }
}
async function replaceDocument(s: CompareSession, side: Side, doc: TextDocument): Promise<boolean> {
  if (isSideDirty(s, side) && !(await confirmAction(tr('Replace this side and discard its unsaved edits?'), tr('Open file')))) return false
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
function setMergeDraft(s: CompareSession,draft:MergeDraft){if(s.merge)s.merge=draft}
function changeMergeSource(s:CompareSession,side:'base'|Side,text:string){
  if(!s.merge)return
  const doc=side==='base'?s.merge.base:s[side]
  if(doc.content===text)return
  doc.content=text
  s.merge.prepared=false;s.merge.resolutions={};s.merge.manual=false
}
async function openMergeSource(s:CompareSession,side:'base'|Side){
  if(!s.merge||busy.value)return
  busy.value=true
  try{
    const doc=await chooseText()
    if(!doc||!sessions.value.includes(s)||!s.merge)return
    if(side==='base'){
      if(s.merge.base.content!==s.merge.savedBase&&!(await confirmAction(tr('Replace this side and discard its unsaved edits?'),tr('Open file'))))return
      s.merge.base=doc;s.merge.savedBase=doc.content
    }else if(!await replaceDocument(s,side,doc))return
    s.merge.prepared=false;s.merge.resolutions={};s.merge.manual=false
  }catch(e){notify(String(e),true)}finally{busy.value=false}
}
async function saveSide(side: Side | 'result', saveAs = false, session?: CompareSession) {
  const s = session || active.value
  if (!s || busy.value) return
  if (side === 'result' && !mergeReady(s.merge,s.left.content,s.right.content)) return
  const doc = clone(side === 'result' ? s.merge!.output : s[side])
  const mergeSourceSnapshot = side === 'result' ? JSON.stringify([s.merge!.base,s.left,s.right,s.merge!.resolutions,s.merge!.prepared]) : ''
  if (doc.lineEnding === 'mixed') { notify(tr('This file has mixed line endings. Choose LF, CRLF or CR in file settings before saving.'), true); return }
  busy.value = true
  try {
    if (!isDesktop) {
      downloadText(doc)
      if (side === 'result') { s.merge!.savedOutput=doc.content; s.merge!.savedOutputMetadata=metadataSignature(doc) } else if (side === 'left') { s.savedLeft = doc.content; s.savedLeftMetadata = metadataSignature(doc) } else { s.savedRight = doc.content; s.savedRightMetadata = metadataSignature(doc) }
      notify(tr('File exported as a download.'))
      return
    }
    let path = doc.path
    let fingerprint = doc.fingerprint
    if (saveAs || !path) {
      path = await chooseSavePath(doc.path || (doc.name === 'Untitled' ? 'untitled.txt' : doc.name))
      if (!path) return
      if (path !== doc.path) {
        fingerprint = await invoke<string | null>('inspect_save_target', { path })
        if (fingerprint && !(await confirmAction(tr('Replace the existing file?\n{0}', path), tr('Save as')))) return
      }
    }
    if (side==='result' && [s.merge!.base.path,s.left.path,s.right.path].some(source=>source!==null && source===path)) { notify(tr('Choose a result file separate from the three sources.'),true); return }
    const request = { path, content: doc.content, encoding: doc.encoding, bom: doc.bom, lineEnding: doc.lineEnding, expectedFingerprint: fingerprint }
    if (side==='result' && mergeSourceSnapshot!==JSON.stringify([s.merge!.base,s.left,s.right,s.merge!.resolutions,s.merge!.prepared])) { notify(tr('Comparison changed. Compare again before merging.'),true); return }
    const result = side==='result' ? await invoke<TextDocument>('save_merge_result',{request,sources:[s.merge!.base.path,s.left.path,s.right.path].filter((path):path is string=>!!path)}) : await writeText(request)
    // Keep edits made while the save was in flight; update the disk baseline separately.
    const current = side === 'result' ? s.merge!.output : s[side]
    const savedDocument = { ...result, content: current.content === doc.content ? result.content : current.content,
      encoding: current.encoding === doc.encoding ? result.encoding : current.encoding,
      bom: current.bom === doc.bom ? result.bom : current.bom,
      lineEnding: current.lineEnding === doc.lineEnding ? result.lineEnding : current.lineEnding }
    if (side === 'result') { s.merge!.output=savedDocument; s.merge!.savedOutput=result.content; s.merge!.savedOutputMetadata=metadataSignature(result) }
    else { s[side]=savedDocument; if (side === 'left') { s.savedLeft = result.content; s.savedLeftMetadata = metadataSignature(result) } else { s.savedRight = result.content; s.savedRightMetadata = metadataSignature(result) } }
    notify(tr('Saved {0}', result.name))
  } catch (e) { notify(String(e), true) }
  finally { busy.value = false }
}
function saveNamedSession() {
  if (!active.value) return
  namingSessionId.value = active.value.id
  sessionName.value = titleFor(active.value)
  nameDialog.value = true
  void nextTick(() => { nameInput.value?.focus(); nameInput.value?.select() })
}
function commitNamedSession() {
  const target = sessions.value.find(s => s.id === namingSessionId.value)
  if (!target || !sessionName.value.trim()) return
  const existing = savedSessions.value.find(s => s.session.id === target.id)
  const entry: SavedSession = { id: existing?.id || crypto.randomUUID(), name: sessionName.value.trim(), savedAt: new Date().toISOString(), session: clone(target) }
  if (existing) Object.assign(existing, entry)
  else savedSessions.value.unshift(entry)
  target.title = entry.name
  nameDialog.value = false
  notify(tr(isAssetKind(target.kind) ? 'File references saved; reopen browser files when restoring.' : 'Session saved with files, edits and comparison rules.'))
}
function restoreSaved(saved: SavedSession) {
  const s = clone(saved.session)
  s.id = crypto.randomUUID()
  s.title = saved.name
  sessions.value.push(s)
  activeId.value = s.id
}
async function removeSaved(saved: SavedSession) {
  if (await confirmAction(tr('Remove the saved session “{0}”? Original files are unaffected.', saved.name), tr('Remove session'))) savedSessions.value = savedSessions.value.filter(s => s.id !== saved.id)
}
function joinPath(root: string, relative: string): string { return root.replace(/[/\\]+$/, '') + '/' + relative }
async function openFolderFile(entry: DirectoryEntry, leftRoot: string, rightRoot: string, rightRelative?: string) {
  if (busy.value) return
  busy.value = true
  try {
    const s = newSession('text')
    s.title = rightRelative && rightRelative !== entry.relativePath ? entry.relativePath + ' ↔ ' + rightRelative : entry.relativePath
    const [left, right] = await Promise.all([
      entry.left?.kind === 'file' ? readText(joinPath(leftRoot, entry.relativePath)) : Promise.resolve(null),
      entry.right?.kind === 'file' ? readText(joinPath(rightRoot, rightRelative || entry.relativePath)) : Promise.resolve(null),
    ])
    if (left) { s.left = left; s.savedLeft = left.content; s.savedLeftMetadata = metadataSignature(left) } else s.left.name = '(missing on left)'
    if (right) { s.right = right; s.savedRight = right.content; s.savedRightMetadata = metadataSignature(right) } else s.right.name = '(missing on right)'
    sessions.value.push(s); activeId.value = s.id
  } catch (e) { notify(String(e), true) }
  finally { busy.value = false }
}
async function droppedFiles(files: File[]) {
  dragging.value = false
  if (!files.length || isDesktop || !loaded.value || restoringFailed.value || closing.value || document.querySelector('[aria-modal="true"]')) return
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
  const modal = document.querySelector<HTMLElement>('[aria-modal="true"]')
  if (modal && event.key === 'Tab') {
    const focusable = Array.from(modal.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], [tabindex="0"]')).filter(el => el.getClientRects().length > 0)
    const first = focusable[0], last = focusable[focusable.length - 1]
    if (first && (!modal.contains(document.activeElement) || (event.shiftKey && document.activeElement === first) || (!event.shiftKey && document.activeElement === last))) { event.preventDefault(); (event.shiftKey ? last : first).focus() }
    return
  }
  if (event.key === 'Escape') { settingsOpen.value = false; helpOpen.value = false; nameDialog.value = false; return }
  if (event.defaultPrevented || !loaded.value || restoringFailed.value || closing.value || activeSyncJobs.value || document.querySelector('[aria-modal="true"]') || !(event.metaKey || event.ctrlKey)) return
  if (event.key.toLowerCase() === 'n') { event.preventDefault(); addSession('text') }
  if (event.key.toLowerCase() === 's' && active.value && ['text','structured','merge'].includes(active.value.kind)) { event.preventDefault(); void saveSide(active.value.kind==='merge'?'result':'right', event.shiftKey) }
  if (event.key.toLowerCase() === 'o' && active.value && ['text','structured'].includes(active.value.kind)) { event.preventDefault(); void openSide(active.value.left.content ? 'right' : 'left') }
  if (event.key === ',') { event.preventDefault(); settingsOpen.value = true }
}
async function recoverWorkspace(reset: boolean) {
  if (busy.value) return
  if (reset && !(await confirmAction(tr('Archive the existing workspace and start a new empty workspace?'), tr('Reset workspace')))) return
  busy.value = true
  try {
    let restored: Workspace | null = null
    if (!reset) {
      if (!isDesktop) throw new Error(tr('No previous snapshot is available in the browser preview.'))
      const previous = await invoke<string | null>('load_workspace', { previous: true })
      if (!previous) throw new Error(tr('No previous workspace snapshot exists.'))
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
  } catch (e) { restoringFailed.value = true; workspaceError.value = tr('Workspace could not be restored. Autosave paused: ') + String(e) }
  loaded.value = true
  window.addEventListener('languagechange', updateSystemLanguage)
  window.addEventListener('keydown', keyboard)
  window.addEventListener('beforeunload', beforeUnload)
  if (isDesktop) {
    unlisten.push(await getCurrentWindow().onCloseRequested(async event => {
      event.preventDefault()
      if (closing.value) return
      if (busy.value || activeSyncJobs.value) { notify(tr('Wait for the current file operation, or cancel synchronization before closing.'), true); return }
      closing.value = true
      try {
        if (restoringFailed.value) {
          if (await confirmAction(tr('Workspace recovery is unresolved. Close without replacing the saved workspace?'), tr('Close Diff EX'))) await getCurrentWindow().destroy()
          return
        }
        if (sessions.value.some(isDirty) && !(await confirmAction(tr('Some edits are not saved to original files. Keep them in the workspace and close Diff EX?'), tr('Unsaved file edits')))) return
        if (!(await persist())) { notify(tr('Workspace could not be saved. Keep this window open and retry after checking disk space.'), true); return }
        await getCurrentWindow().destroy()
      } finally { closing.value = false }
    }))
    unlisten.push(await getCurrentWebview().onDragDropEvent(async event => {
      if (!loaded.value || restoringFailed.value || closing.value || activeSyncJobs.value || busy.value || document.querySelector('[aria-modal="true"]')) { dragging.value = false; return }
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
onBeforeUnmount(() => { unlisten.forEach(fn => fn()); window.removeEventListener('languagechange', updateSystemLanguage); window.removeEventListener('keydown', keyboard); window.removeEventListener('beforeunload', beforeUnload); if (persistTimer) clearTimeout(persistTimer); if (notificationTimer) clearTimeout(notificationTimer) })
const themes: { value: Theme; en: string; colors: string[] }[] = [
  { value: 'dark', en: 'Midnight', colors: ['#10131a','#1a1f2b','#7899ff'] },
  { value: 'black', en: 'Pure black', colors: ['#000000','#171717','#a6baff'] },
  { value: 'gray', en: 'Graphite', colors: ['#323437','#484b50','#b0c6ff'] },
  { value: 'white', en: 'White', colors: ['#ffffff','#e9edf2','#245ac6'] },
  { value: 'light', en: 'Soft light', colors: ['#faf8f3','#e8e5de','#526bba'] },
]
</script>

<template>
  <div class="app-shell" @dragover.prevent="!isDesktop && (dragging = true)" @dragleave.self="dragging = false" @drop.prevent="droppedFiles(Array.from($event.dataTransfer?.files || []))">
    <header class="app-header" :inert="!loaded || closing || restoringFailed || !!activeSyncJobs"><div class="brand"><img src="/diff-ex.svg" alt="" /><span>Diff <b>EX</b></span><span class="brand-divider" /><span class="header-label">{{ tr('Compare workspace') }}</span></div><div class="header-actions"><span v-if="!isDesktop" class="preview-badge">{{ tr('BROWSER PREVIEW') }}</span><span class="local-badge"><span />{{ tr('Local & offline') }}</span><button class="btn-icon" :title="tr('Keyboard shortcuts')" @click="helpOpen = true"><Keyboard :size="17" /></button><button class="btn-icon" :title="tr('Settings')" @click="settingsOpen = true"><Settings2 :size="18" /></button></div></header>
    <div class="app-layout" :class="{ 'sidebar-hidden': !preferences.showSidebar }">
      <aside v-if="preferences.showSidebar" class="sidebar" :inert="!loaded || closing || restoringFailed || !!activeSyncJobs"><div class="sidebar-top"><span class="eyebrow">{{ tr('WORKSPACE') }}</span><button class="btn-icon subtle" :title="tr('Hide sidebar')" @click="preferences.showSidebar = false"><PanelLeftClose :size="16" /></button></div><button class="nav-item" :class="{ active: !active }" @click="activeId = ''"><Home :size="17" /><span>{{ tr('Overview') }}</span></button><div class="sidebar-label">{{ tr('NEW COMPARISON') }}</div><button v-for="kind in (['text','folder','structured','binary','image','table','merge','archive'] as const)" :key="kind" class="nav-item" @click="addSession(kind)"><component :is="kindIcon(kind)" :size="18" /><span>{{ kindName(kind) }}</span><Plus :size="14" class="nav-plus" /></button><div class="sidebar-section-heading"><span class="sidebar-label">{{ tr('SAVED SESSIONS') }}</span><span class="count">{{ savedSessions.length }}</span></div><label v-if="savedSessions.length" class="session-search"><Search :size="14" /><input v-model="search" :placeholder="tr('Find a session…')"></label><div class="saved-session-list"><div v-for="s in filteredSaved" :key="s.id" class="saved-session"><button :title="s.name" @click="restoreSaved(s)"><component :is="kindIcon(s.session.kind)" :size="15" /><span>{{ s.name }}</span></button><button class="remove-saved" :title="tr('Remove saved session')" @click="removeSaved(s)"><X :size="13" /></button></div><div v-if="!savedSessions.length" class="saved-empty"><Bookmark :size="20" :stroke-width="1.4" /><p>{{ tr('Keep your comparisons close.') }}</p><span>{{ tr('Save a session to return to its files and rules.') }}</span></div></div><div class="sidebar-footer"><ShieldCheck :size="17" /><div><strong>{{ tr('Your files, your device.') }}</strong><span>{{ tr('No account. No uploads.') }}</span></div></div></aside>
      <main class="main-workspace" :inert="!loaded || closing"><div class="tab-bar" :inert="!loaded || closing || restoringFailed || !!activeSyncJobs"><button v-if="!preferences.showSidebar" class="btn-icon" :title="tr('Show sidebar')" @click="preferences.showSidebar = true"><PanelLeftClose :size="16" style="transform:rotate(180deg)" /></button><button class="home-tab" :class="{ active: !active }" :title="tr('Overview')" @click="activeId = ''"><Home :size="16" /></button><div class="tabs"><div v-for="s in sessions" :key="s.id" class="comparison-tab" :class="{ active: activeId === s.id }"><button class="tab-select" :title="titleFor(s)" @click="activeId = s.id"><component :is="kindIcon(s.kind)" :size="15" /><span>{{ titleFor(s) }}</span><i v-if="isDirty(s)" class="dirty-dot" /></button><button class="tab-close" :title="tr('Close tab')" @click="closeSession(s)"><X :size="13" /></button></div></div><button class="new-tab" :title="tr('New text comparison')" @click="addSession('text')"><Plus :size="17" /></button><button v-if="active" class="save-session" :title="tr('Save this session')" @click="saveNamedSession"><Bookmark :size="15" /><span>{{ tr('Save session') }}</span></button></div>
        <div v-if="workspaceError" class="error-banner" role="alert"><span>{{ tr('Workspace save failed: ') }}{{ workspaceError }}</span><template v-if="restoringFailed"><button class="btn" @click="recoverWorkspace(false)">{{ tr('Try previous snapshot') }}</button><button class="btn" @click="recoverWorkspace(true)">{{ tr('Archive and reset') }}</button></template><button v-else class="btn" @click="persist">{{ tr('Retry') }}</button></div>
        <div v-if="!active" class="welcome" :inert="restoringFailed"><div class="welcome-inner"><h1>{{ tr('Compare workspace') }}</h1><p class="welcome-intro">{{ tr('A focused workspace for comparing files, reviewing changes, and bringing the right version forward.') }}</p><div class="comparison-cards"><button class="comparison-card text-card" @click="addSession('text')"><div class="card-icon"><FileDiff :size="25" :stroke-width="1.5" /></div><h2>{{ tr('Text & code') }}</h2><p>{{ tr('Compare line by line. Edit and merge in either direction.') }}</p><div class="card-footer"><span>TXT · CODE · LOG</span><ArrowRight :size="18" /></div></button><button class="comparison-card folder-card" @click="addSession('folder')"><div class="card-icon"><FolderOpen :size="25" :stroke-width="1.5" /></div><h2>{{ tr('Folders') }}</h2><p>{{ tr('Find changed files. Preview and apply a safe synchronization.') }}</p><div class="card-footer"><span>{{ tr('RECURSIVE · SYNC') }}</span><ArrowRight :size="18" /></div></button><button class="comparison-card structure-card" @click="addSession('structured')"><div class="card-icon"><Braces :size="25" :stroke-width="1.5" /></div><h2>{{ tr('Structured data') }}</h2><p>{{ tr('Understand changes by their meaning, keys and values.') }}</p><div class="card-footer"><span>JSON · YAML · XML</span><ArrowRight :size="18" /></div></button><button class="comparison-card binary-card" @click="addSession('binary')"><div class="card-icon"><FileDigit :size="25" :stroke-width="1.5" /></div><h2>{{ tr('Binary files') }}</h2><p>{{ tr('Compare hexadecimal bytes and ASCII at matching offsets.') }}</p><div class="card-footer"><span>HEX · ASCII</span><ArrowRight :size="18" /></div></button><button class="comparison-card image-card" @click="addSession('image')"><div class="card-icon"><ImageIcon :size="25" :stroke-width="1.5" /></div><h2>{{ tr('Images') }}</h2><p>{{ tr('Compare raster pixels with overlays, split view and heatmaps.') }}</p><div class="card-footer"><span>PNG · JPEG · WEBP</span><ArrowRight :size="18" /></div></button><button class="comparison-card table-card" @click="addSession('table')"><div class="card-icon"><Table2 :size="25" :stroke-width="1.5" /></div><h2>{{ tr('Tables') }}</h2><p>{{ tr('Align spreadsheet rows and columns; inspect cell differences.') }}</p><div class="card-footer"><span>CSV · TSV · XLSX · XLS</span><ArrowRight :size="18" /></div></button><button class="comparison-card" @click="addSession('merge')"><div class="card-icon"><GitMerge :size="25" :stroke-width="1.5" /></div><h2>{{ tr('Three-way merge') }}</h2><p>{{ tr('Merge changes from two versions using a common base.') }}</p><div class="card-footer"><span>BASE · LEFT · RIGHT</span><ArrowRight :size="18"/></div></button><button class="comparison-card" @click="addSession('archive')"><div class="card-icon"><Archive :size="25" :stroke-width="1.5" /></div><h2>{{ tr('Archives') }}</h2><p>{{ tr('Compare archive contents without extracting files.') }}</p><div class="card-footer"><span>ZIP · TAR · GZIP · 7Z</span><ArrowRight :size="18"/></div></button></div><div class="welcome-bottom"><div><Code2 :size="19" /><div><strong>{{ tr('Start with an example') }}</strong><span>{{ tr('Explore a small configuration change.') }}</span></div></div><button class="btn" @click="addSession('text', true)">{{ tr('Open example') }}<ChevronRight :size="15" /></button></div><div class="welcome-notes"><span><LockKeyhole :size="13" />{{ tr('Processed on your device') }}</span><span><ArrowLeftRight :size="14" />{{ tr('No session limits') }}</span><span><Laptop :size="14" />Windows · macOS · Linux</span></div></div></div>
        <div v-show="active" class="compare-container" :inert="restoringFailed"><div v-for="s in sessions" :key="s.id" v-show="activeId === s.id" class="session-view"><TextCompare v-if="s.kind === 'text'" :session="s" :preferences="preferences" :language="language" @change="(side, content) => s[side].content = content" @open="(side, encoding) => openSide(side, encoding, s)" @save="(side, as) => saveSide(side, as, s)" @update-options="s.textOptions = $event" @update-document="(side, patch) => Object.assign(s[side], patch)"/><StructuredCompare v-else-if="s.kind === 'structured'" :session="s" :preferences="preferences" :language="language" @change="(side, content) => s[side].content = content" @open="(side, encoding) => openSide(side, encoding, s)" @save="(side, as) => saveSide(side, as, s)" @update-options="s.structuredOptions = $event" @update-document="(side, patch) => Object.assign(s[side], patch)"/><FolderCompare v-else-if="s.kind === 'folder'" :session="s" :language="language" @update-roots="(left, right) => { s.leftRoot = left; s.rightRoot = right }" @update-options="s.directoryOptions = $event" @open-file="openFolderFile"/><ThreeWayCompare v-else-if="s.kind === 'merge'" :session="s" :language="language" @open-source="side=>openMergeSource(s,side)" @change-source="(side,text)=>changeMergeSource(s,side,text)" @update-merge="draft=>setMergeDraft(s,draft)" @save-result="as=>saveSide('result',as,s)"/><AssetCompare v-else :session="s" :language="language" @update-source="(side,value)=>setAssetSource(s,side,value)" @update-settings="patch=>setAssetSettings(s,patch)"/></div></div>
      </main>
    </div>
    <footer class="app-status"><span><span class="connection-dot" />{{ busy ? tr('Working…') : tr('Ready') }}</span><span v-if="active">{{ kindName(active.kind) }}</span><span class="status-spacer"/><span v-if="!isDesktop">{{ tr('Native filesystem features require desktop') }}</span><span>{{ restoringFailed || workspaceError ? tr('Autosave paused') : tr('Workspace auto-save') }}</span><span>v0.1.0</span></footer>
    <div v-if="notification" class="toast" :class="{ error: notification.error }" role="status"><Check v-if="!notification.error" :size="17" /><span>{{ notification.text }}</span><button :title="tr('Dismiss')" @click="notification = null"><X :size="16" /></button></div>
    <div v-if="!loaded" class="loading-overlay"><span>{{ tr('Restoring workspace…') }}</span></div>
    <div v-if="dragging" class="drop-overlay" @dragleave.prevent="dragging = false" @drop.prevent="droppedFiles(Array.from($event.dataTransfer?.files || []))"><FileDiff :size="45"/><h2>{{ tr('Drop files to compare') }}</h2><p>{{ tr('Files open in pairs as new comparisons.') }}</p></div>
    <Teleport to="body"><div v-if="settingsOpen" class="modal-backdrop" @click.self="settingsOpen = false"><section class="modal settings-modal" role="dialog" aria-modal="true" :aria-label="tr('Settings')"><header><div><span class="eyebrow">{{ tr('MAKE IT YOURS') }}</span><h2>{{ tr('Workspace settings') }}</h2></div><button class="btn-icon" :title="tr('Close settings')" @click="settingsOpen = false"><X :size="20"/></button></header><div class="setting-section"><h3>{{ tr('Appearance') }}</h3><div class="theme-choices"><button v-for="theme in themes" :key="theme.value" :class="{ selected: preferences.theme === theme.value }" :aria-pressed="preferences.theme === theme.value" :data-theme-option="theme.value" @click="preferences.theme = theme.value"><div :style="{ background: theme.colors[0] }"><i :style="{ background: theme.colors[1] }"/><span :style="{ background: theme.colors[2] }"/><Check v-if="preferences.theme === theme.value" :size="16"/></div><span>{{ tr(theme.en) }}</span></button></div></div><div class="setting-row"><label for="language-setting">{{ tr('Language') }}<span>{{ tr('Applied immediately and remembered.') }}</span></label><select id="language-setting" v-model="preferences.locale"><option value="system">{{ tr('Follow system') }}</option><option v-for="option in languageOptions" :key="option.code" :value="option.code">{{ option.name }}</option></select></div><div class="setting-row"><label for="font-setting">{{ tr('Editor font size') }}</label><div class="font-control"><input id="font-setting" v-model.number="preferences.fontSize" type="number" min="8" max="40" @change="preferences.fontSize = Math.max(8, Math.min(40, Number(preferences.fontSize) || 13))"><span>px</span></div></div><div class="setting-row"><label for="wrap-setting">{{ tr('Wrap long lines') }}</label><input id="wrap-setting" v-model="preferences.wordWrap" type="checkbox"></div><div class="setting-row"><label for="sidebar-setting">{{ tr('Show workspace sidebar') }}</label><input id="sidebar-setting" v-model="preferences.showSidebar" type="checkbox"></div><p class="settings-note"><ShieldCheck :size="16"/>{{ tr('Sessions, unsaved edits and settings are stored locally. Original files are only written when you save or apply a sync.') }}</p><footer><button class="btn primary" @click="settingsOpen = false">{{ tr('Done') }}</button></footer></section></div>
    <div v-if="nameDialog" class="modal-backdrop" @click.self="nameDialog = false"><form class="modal name-modal" role="dialog" aria-modal="true" @submit.prevent="commitNamedSession"><header><h2>{{ tr('Save session') }}</h2><button type="button" class="btn-icon" @click="nameDialog = false"><X :size="19"/></button></header><label for="session-name">{{ tr('Session name') }}</label><input ref="nameInput" id="session-name" v-model="sessionName" required/><p class="muted small">{{ tr(active && isAssetKind(active.kind) ? 'File references saved; reopen browser files when restoring.' : 'Includes both documents, comparison rules and unsaved edits.') }}</p><footer><button type="button" class="btn" @click="nameDialog = false">{{ tr('Cancel') }}</button><button class="btn primary" type="submit">{{ tr('Save session') }}</button></footer></form></div>
    <div v-if="helpOpen" class="modal-backdrop" @click.self="helpOpen = false"><section class="modal help-modal" role="dialog" aria-modal="true"><header><h2>{{ tr('Keyboard shortcuts') }}</h2><button class="btn-icon" @click="helpOpen = false"><X :size="20"/></button></header><div v-for="item in [[tr('New text comparison'),'⌘ / Ctrl + N'],[tr('Open a file'),'⌘ / Ctrl + O'],[tr('Save active editor / right side'),'⌘ / Ctrl + S'],[tr('Save as'),'⌘ / Ctrl + Shift + S'],[tr('Next / previous difference'),'F7 / Shift + F7'],[tr('Find in editor'),'⌘ / Ctrl + F'],[tr('Undo merge / edit'),'⌘ / Ctrl + Z'],[tr('Settings'),'⌘ / Ctrl + ,']]" :key="item[0]" class="shortcut-row"><span>{{ item[0] }}</span><kbd>{{ item[1] }}</kbd></div><a class="source-link" href="https://github.com/xzcode/diff-ex" target="_blank" rel="noreferrer"><Github :size="16"/>xzcode/diff-ex</a></section></div></Teleport>
  </div>
</template>
