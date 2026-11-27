<script setup lang="ts">
import { findRenameCandidates } from '../lib/renames'
import { translate } from '../lib/i18n'
import type { Language } from '../contracts'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ArrowDown, ArrowLeftRight, ArrowRight, Check, ChevronDown, FileCode2, Folder, FolderOpen, RefreshCw, Search, ShieldCheck, SlidersHorizontal, Square, X, AlertTriangle } from 'lucide-vue-next'
import type { CompareSession, DirectoryEntry, DirectoryOptions, DirectoryResult, EntryStatus, SyncMode, SyncPlan, SyncResult } from '../contracts'
import { cancelJob, chooseDirectory, compareFolders, discardSync, executeSync, isDesktop, previewSync } from '../lib/bridge'
const props = defineProps<{ session: CompareSession; language: Language }>()
const emit = defineEmits<{
  'update-roots': [left: string, right: string]
  'update-options': [options: DirectoryOptions]
  'open-file': [entry: DirectoryEntry, leftRoot: string, rightRoot: string, rightRelative?: string]
}>()
const tr = (key: string, ...args: (string | number)[]) => translate(props.language, key, ...args)
const results = ref<DirectoryResult | null>(null)
const busy = ref('')
const error = ref('')
const job = ref('')
const query = ref('')
const filter = ref<EntryStatus | 'all' | 'changed' | 'moves'>('all')
const settings = ref(false)
const plan = ref<SyncPlan | null>(null)
const outcome = ref<SyncResult | null>(null)
const mode = ref<SyncMode>('leftToRight')
const sort = ref<'path' | 'status' | 'left' | 'right'>('path')
const direction = ref(1)
const scrollTop = ref(0)
const viewport = ref<HTMLElement>()
const viewportHeight = ref(520)
const selected = ref('')
let observer: ResizeObserver | undefined
let alive = true
let epoch = 0
const movePairs = computed(() => props.session.directoryOptions.method === 'content' ? findRenameCandidates(results.value?.entries || []) : [])
const moveByPath = computed(() => new Map(movePairs.value.flatMap(pair => [[pair.left.relativePath, pair] as const, [pair.right.relativePath, pair] as const])))
const rows = computed(() => {
  const q = query.value.toLowerCase()
  return (results.value?.entries || []).filter(e => (filter.value === 'all' || (filter.value === 'changed' ? e.status !== 'same' : filter.value === 'moves' ? moveByPath.value.has(e.relativePath) : e.status === filter.value)) && (!q || e.relativePath.toLowerCase().includes(q) || !!moveByPath.value.get(e.relativePath)?.left.relativePath.toLowerCase().includes(q) || !!moveByPath.value.get(e.relativePath)?.right.relativePath.toLowerCase().includes(q)))
    .slice().sort((a, b) => direction.value * (sort.value === 'path' ? a.relativePath.localeCompare(b.relativePath) : sort.value === 'status' ? a.status.localeCompare(b.status) || a.relativePath.localeCompare(b.relativePath) : (a[sort.value]?.size ?? -1) - (b[sort.value]?.size ?? -1)))
})
const first = computed(() => Math.max(0, Math.floor(scrollTop.value / 37) - 8))
const last = computed(() => Math.min(rows.value.length, first.value + Math.ceil(viewportHeight.value / 37) + 16))
const visibleRows = computed(() => rows.value.slice(first.value, last.value))
const counts = computed(() => {
  const c = { same: 0, different: 0, leftOnly: 0, rightOnly: 0, conflict: 0, error: 0 }
  results.value?.entries.forEach(e => c[e.status]++)
  return c
})
const executable = computed(() => plan.value?.operations.filter(o => o.action !== 'conflict').length || 0)
const conflicts = computed(() => plan.value?.operations.filter(o => o.action === 'conflict').length || 0)
const statusText = (s: EntryStatus) => ({ same: tr('Identical'), different: tr('Modified'), leftOnly: tr('Left only'), rightOnly: tr('Right only'), conflict: tr('Type conflict'), error: tr('Read error') }[s])
const actionText = (s: string) => ({ copyRight: tr('Copy → right'), copyLeft: tr('Copy ← left'), deleteRight: tr('Delete on right'), deleteLeft: tr('Delete on left'), createRightDir: tr('Create right folder'), createLeftDir: tr('Create left folder'), conflict: tr('Skip conflict') }[s] || s)
const bytes = (n: number | undefined) => n === undefined ? '—' : n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : n < 1073741824 ? `${(n / 1048576).toFixed(1)} MB` : `${(n / 1073741824).toFixed(1)} GB`
async function clearPlan() {
  const old = plan.value
  plan.value = null
  if (old && isDesktop) await discardSync(old.id).catch(() => undefined)
}
watch(() => [props.session.leftRoot, props.session.rightRoot, JSON.stringify(props.session.directoryOptions)], () => {
  epoch++; results.value = null; outcome.value = null; error.value = ''; if (filter.value === 'moves') filter.value = 'all'; void clearPlan()
})
watch(mode, () => { void clearPlan() })
watch([query, filter, sort, direction], () => { scrollTop.value = 0; if (viewport.value) viewport.value.scrollTop = 0 })
async function browse(side: 'left' | 'right') {
  try { const p = await chooseDirectory(); if (p) emit('update-roots', side === 'left' ? p : props.session.leftRoot, side === 'right' ? p : props.session.rightRoot) } catch (e) { error.value = String(e) }
}
async function compare() {
  if (!isDesktop || busy.value || !props.session.leftRoot || !props.session.rightRoot) return
  const revision = ++epoch
  busy.value = 'compare'; error.value = ''; job.value = crypto.randomUUID()
  await clearPlan()
  try {
    const value = await compareFolders(props.session.leftRoot, props.session.rightRoot, props.session.directoryOptions, job.value)
    if (alive && revision === epoch) { results.value = value; if (filter.value === 'moves' && !movePairs.value.length) filter.value = 'all'; scrollTop.value = 0; if (viewport.value) viewport.value.scrollTop = 0 }
  } catch (e) { if (alive && revision === epoch) error.value = String(e) }
  finally { if (alive) { busy.value = ''; job.value = '' } }
}
async function preview() {
  if (!isDesktop || busy.value || !props.session.leftRoot || !props.session.rightRoot) return
  const revision = ++epoch
  busy.value = 'plan'; error.value = ''; outcome.value = null; job.value = crypto.randomUUID()
  await clearPlan()
  try {
    const value = await previewSync(props.session.leftRoot, props.session.rightRoot, props.session.directoryOptions, mode.value, job.value)
    if (alive && revision === epoch) plan.value = value
    else await discardSync(value.id)
  } catch (e) { if (alive) error.value = String(e) }
  finally { if (alive) { busy.value = ''; job.value = '' } }
}
async function run() {
  if (!plan.value || busy.value) return
  busy.value = 'sync'; error.value = ''; job.value = crypto.randomUUID()
  const id = plan.value.id
  try { outcome.value = await executeSync(id, job.value); plan.value = null; results.value = null }
  catch (e) { error.value = String(e); plan.value = null }
  finally { if (alive) { busy.value = ''; job.value = '' } }
}
function setOption(key: keyof DirectoryOptions, value: unknown) { emit('update-options', { ...props.session.directoryOptions, [key]: value }) }
function changeSort(v: typeof sort.value) { if (sort.value === v) direction.value *= -1; else { sort.value = v; direction.value = 1 } }
function openEntry(e: DirectoryEntry) {
  const pair = moveByPath.value.get(e.relativePath)
  if (pair) {
    emit('open-file', { ...pair.left, right: pair.right.right, status: 'same' }, props.session.leftRoot, props.session.rightRoot, pair.right.relativePath)
    return
  }
  if (e.left?.kind === 'file' || e.right?.kind === 'file') emit('open-file', e, props.session.leftRoot, props.session.rightRoot)
}
onMounted(async () => { await nextTick(); if (viewport.value) { observer = new ResizeObserver(entries => { viewportHeight.value = entries[0].contentRect.height }); observer.observe(viewport.value) } })
onBeforeUnmount(() => { alive = false; observer?.disconnect(); if (job.value) void cancelJob(job.value).catch(() => undefined); void clearPlan() })
</script>

<template>
  <section class="folder-workspace">
    <div class="panel-toolbar folder-toolbar">
      <div class="tool-group"><button class="btn primary" :disabled="!isDesktop || !!busy || !session.leftRoot || !session.rightRoot" @click="compare"><RefreshCw :size="15" :class="{ spinning: busy === 'compare' }" />{{ tr('Compare folders') }}</button>
        <button v-if="busy" class="btn danger" @click="cancelJob(job)"><Square :size="13" />{{ tr('Cancel task') }}</button>
      </div>
      <div class="tool-group"><span class="muted small">{{ tr('Compare by') }}</span><select :value="session.directoryOptions.method" :disabled="!!busy" @change="setOption('method', ($event.target as HTMLSelectElement).value)"><option value="content">{{ tr('File content') }}</option><option value="metadata">{{ tr('Size + modified time') }}</option></select><button class="btn-icon" :class="{ active: settings }" :title="tr('Comparison rules')" @click="settings = !settings"><SlidersHorizontal :size="17" /></button></div>
    </div>
    <div v-if="!isDesktop" class="notice">{{ tr('Folder access and synchronization are available in the desktop app. The browser preview supports text files and JSON.') }}</div>
    <div class="folder-paths">
      <label class="folder-path"><span><span class="side-dot left-dot" />{{ tr('LEFT FOLDER') }}</span><div><FolderOpen :size="18" /><input :value="session.leftRoot" :disabled="!!busy" :placeholder="tr('Select or paste a folder path')" @change="emit('update-roots', ($event.target as HTMLInputElement).value, session.rightRoot)" @keydown.enter="compare"><button class="btn-icon" :disabled="!isDesktop || !!busy" :title="tr('Browse left folder')" @click="browse('left')">…</button></div></label>
      <button class="btn-icon swap-folders" :disabled="!!busy" :title="tr('Swap folders')" @click="emit('update-roots', session.rightRoot, session.leftRoot)"><ArrowLeftRight :size="17" /></button>
      <label class="folder-path"><span><span class="side-dot right-dot" />{{ tr('RIGHT FOLDER') }}</span><div><FolderOpen :size="18" /><input :value="session.rightRoot" :disabled="!!busy" :placeholder="tr('Select or paste a folder path')" @change="emit('update-roots', session.leftRoot, ($event.target as HTMLInputElement).value)" @keydown.enter="compare"><button class="btn-icon" :disabled="!isDesktop || !!busy" :title="tr('Browse right folder')" @click="browse('right')">…</button></div></label>
    </div>
    <div v-if="settings" class="folder-options"><label><input type="checkbox" :checked="session.directoryOptions.respectGitignore" :disabled="!!busy" @change="setOption('respectGitignore', ($event.target as HTMLInputElement).checked)"> {{ tr('Respect .gitignore') }}</label><label><input type="checkbox" :checked="session.directoryOptions.showHidden" :disabled="!!busy" @change="setOption('showHidden', ($event.target as HTMLInputElement).checked)"> {{ tr('Include hidden files') }}</label><label class="exclude-field">{{ tr('Exclude patterns · one per line') }}<textarea :value="session.directoryOptions.excludes.join('\n')" :disabled="!!busy" placeholder="node_modules/&#10;*.log" rows="2" @change="setOption('excludes', ($event.target as HTMLTextAreaElement).value.split('\n').map(v => v.trim()).filter(Boolean))" /></label><span class="muted small">{{ tr('Symbolic links are never traversed. Content comparison streams file hashes; metadata comparison is only a quick estimate.') }}</span></div>
    <div v-if="error" class="error-banner" role="alert"><AlertTriangle :size="16" /><span>{{ error }}</span><button class="btn-icon" @click="error = ''"><X :size="15" /></button></div>
    <div v-if="outcome" class="sync-outcome"><ShieldCheck :size="20" /><div><strong>{{ tr('Synchronization finished') }}</strong><p>{{ tr('Completed') }} {{ outcome.completed }} · {{ tr('Skipped') }} {{ outcome.skipped }} · {{ tr('Errors') }} {{ outcome.errors.length }}</p><p v-if="outcome.backupRoot" class="selectable">{{ tr('Backups:') }} {{ outcome.backupRoot }}</p><pre v-if="outcome.errors.length">{{ outcome.errors.join('\n') }}</pre></div><button class="btn" @click="compare">{{ tr('Compare again') }}</button></div>
    <div class="folder-filters"><div class="filter-tabs"><button :class="{ active: filter === 'all' }" @click="filter = 'all'">{{ tr('All files') }} <span>{{ results?.entries.length ?? '—' }}</span></button><button :class="{ active: filter === 'changed' }" @click="filter = 'changed'">{{ tr('Differences') }} <span>{{ results ? results.entries.length - counts.same : '—' }}</span></button><button v-if="movePairs.length" :class="{ active: filter === 'moves' }" @click="filter = 'moves'">{{ tr('Possible moves') }} <span>{{ movePairs.length }}</span></button><select v-model="filter" :aria-label="tr('Filter status')"><option v-if="movePairs.length" value="moves">{{ tr('Possible moves') }}</option><option value="all">{{ tr('All statuses') }}</option><option value="changed">{{ tr('All differences') }}</option><option v-for="s in (['same', 'different', 'leftOnly', 'rightOnly', 'conflict', 'error'] as const)" :key="s" :value="s">{{ statusText(s) }} ({{ counts[s] }})</option></select></div><label class="table-search"><Search :size="15" /><input v-model="query" :placeholder="tr('Filter by file path…')"></label></div>
    <div v-if="filter === 'moves'" class="notice">{{ tr('Unique equal-content paths are possible moves; synchronization is unchanged.') }}</div>
    <div class="directory-table" ref="viewport" @scroll="scrollTop = ($event.target as HTMLElement).scrollTop">
      <div class="directory-table-head directory-row"><button @click="changeSort('path')">{{ tr('NAME / RELATIVE PATH') }}<ChevronDown :size="13" /></button><button @click="changeSort('status')">{{ tr('STATUS') }}<ChevronDown :size="13" /></button><button @click="changeSort('left')">{{ tr('LEFT SIZE') }}</button><button @click="changeSort('right')">{{ tr('RIGHT SIZE') }}</button></div>
      <div v-if="!results" class="folder-empty"><div class="empty-folders"><Folder :size="42" :stroke-width="1.3"/><ArrowLeftRight :size="23"/><Folder :size="42" :stroke-width="1.3"/></div><h3>{{ tr('Two folders. A clear picture.') }}</h3><p>{{ tr('Choose your left and right folders to see what changed.') }}</p><span class="muted small">{{ tr('Recursive comparison · Content hashes · Safe synchronization') }}</span></div>
      <template v-else>
        <div :style="{ height: first * 37 + 'px' }" />
        <div v-for="entry in visibleRows" :key="entry.relativePath" class="directory-row directory-file" :class="[{ selected: selected === entry.relativePath }, 'status-' + entry.status]" tabindex="0" :title="entry.error || (moveByPath.get(entry.relativePath) ? moveByPath.get(entry.relativePath)!.left.relativePath + ' ↔ ' + moveByPath.get(entry.relativePath)!.right.relativePath : entry.relativePath)" @click="selected = entry.relativePath" @dblclick="openEntry(entry)" @keydown.enter="openEntry(entry)">
          <div class="file-name"><Folder v-if="(entry.left || entry.right)?.kind === 'directory'" :size="17" class="folder-icon" /><FileCode2 v-else :size="16" /><span>{{ entry.relativePath }}<template v-if="moveByPath.has(entry.relativePath)"> ↔ {{ entry.status === 'leftOnly' ? moveByPath.get(entry.relativePath)?.right.relativePath : moveByPath.get(entry.relativePath)?.left.relativePath }}</template></span></div><span class="status-label"><i />{{ moveByPath.has(entry.relativePath) ? tr('Possible move') : statusText(entry.status) }}</span><span class="file-size">{{ entry.left?.kind === 'directory' ? '—' : bytes(entry.left?.size) }}</span><span class="file-size">{{ entry.right?.kind === 'directory' ? '—' : bytes(entry.right?.size) }}</span>
        </div>
        <div :style="{ height: (rows.length - last) * 37 + 'px' }" />
        <div v-if="!rows.length" class="empty-result">{{ tr('No files match this filter.') }}</div>
      </template>
    </div>
    <footer class="folder-footer"><span class="muted small">{{ results ? tr('{0} items · double-click a file to compare', rows.length) : tr('Your files stay on this device') }}</span><div class="tool-group"><select v-model="mode" :disabled="!!busy" :aria-label="tr('Sync mode')"><option value="leftToRight">{{ tr('Update left → right') }}</option><option value="rightToLeft">{{ tr('Update right → left') }}</option><option value="mirrorLeft">{{ tr('Mirror left → right') }}</option><option value="mirrorRight">{{ tr('Mirror right → left') }}</option><option value="bidirectional">{{ tr('Two-way · keep conflicts') }}</option></select><button class="btn primary" :disabled="!isDesktop || !!busy || !session.leftRoot || !session.rightRoot" @click="preview">{{ tr('Preview sync') }}<ArrowRight :size="15" /></button></div></footer>
    <Teleport to="body"><div v-if="plan" class="modal-backdrop" @click.self="!busy && clearPlan()"><section class="modal sync-modal" @keydown.esc.stop="!busy && clearPlan()" role="dialog" aria-modal="true" :aria-label="tr('Synchronization preview')"><header><div><span class="eyebrow">{{ tr('REVIEW BEFORE APPLYING') }}</span><h2>{{ tr('Synchronization preview') }}</h2></div><button class="btn-icon" :disabled="!!busy" :title="tr('Close preview')" @click="clearPlan"><X :size="20" /></button></header><div class="sync-summary"><span><Check :size="16" />{{ executable }} {{ tr('operations') }}</span><span v-if="conflicts" class="warning"><AlertTriangle :size="16" />{{ conflicts }} {{ tr('conflicts will be skipped') }}</span></div><div class="sync-mode-label">{{ ({leftToRight: tr('Update left → right'), rightToLeft: tr('Update right → left'), mirrorLeft: tr('Mirror left → right'), mirrorRight: tr('Mirror right → left'), bidirectional: tr('Two-way · keep conflicts')})[plan.mode] }}</div><div class="sync-roots selectable">{{ plan.leftRoot }}<ArrowDown :size="16" />{{ plan.rightRoot }}</div><div class="sync-operations"><div v-for="(op, i) in plan.operations" :key="i" class="sync-operation"><span :class="{ warning: op.action === 'conflict', negative: op.action.startsWith('delete') }">{{ actionText(op.action) }}</span><code :title="op.reason">{{ op.relativePath }}</code><span class="muted">{{ bytes(op.bytes) }}</span></div><p v-if="!plan.operations.length" class="muted">{{ tr('Nothing to synchronize.') }}</p></div><p class="notice"><ShieldCheck :size="16" />{{ tr('Changed files are rechecked before execution. Overwritten and removed files are backed up. Conflicts require manual resolution.') }}</p><footer><button class="btn" :disabled="!!busy" @click="clearPlan">{{ tr('Cancel') }}</button><button v-if="busy === 'sync'" class="btn danger" @click="cancelJob(job)">{{ tr('Stop after current operation') }}</button><button v-else class="btn primary" :disabled="!executable" @click="run">{{ tr('Apply {0} operations', executable) }}</button></footer></section></div></Teleport>
  </section>
</template>

<style scoped>
.folder-workspace{display:flex;flex-direction:column;height:100%;min-height:0}.folder-toolbar{justify-content:space-between}.folder-paths{display:grid;grid-template-columns:1fr 36px 1fr;gap:10px;padding:20px 22px;border-bottom:1px solid var(--border);background:var(--panel)}.folder-path>span{display:flex;align-items:center;gap:8px;font-size:10px;letter-spacing:1.2px;color:var(--muted);margin-bottom:10px;font-weight:600}.folder-path>div{display:flex;align-items:center;gap:10px;background:var(--bg);padding:0 8px 0 12px;border:1px solid var(--border);border-radius:7px}.folder-path input{border:0;background:none;width:100%;padding:10px 0}.swap-folders{align-self:end;margin-bottom:3px}.folder-options{padding:14px 22px;display:flex;flex-wrap:wrap;gap:20px;align-items:center;border-bottom:1px solid var(--border)}.folder-options label{font-size:12px;display:flex;align-items:center;gap:7px}.exclude-field{flex:1;min-width:230px}.exclude-field textarea{flex:1}.folder-filters{display:flex;justify-content:space-between;gap:20px;padding:12px 22px;border-bottom:1px solid var(--border);background:var(--panel)}.filter-tabs{display:flex;gap:8px;align-items:center}.filter-tabs>button{padding:6px 10px;border-radius:6px;color:var(--muted);font-size:12px}.filter-tabs>button.active{color:var(--text);background:var(--panel-alt)}.filter-tabs button span{font-family:var(--mono);margin-left:6px;color:var(--muted)}.filter-tabs select{font-size:11px;padding:5px;min-height:29px}.table-search{display:flex;gap:8px;align-items:center;color:var(--muted);width:220px}.table-search input{background:none;border:0;padding:4px;width:100%;font-size:12px}.directory-table{flex:1;min-height:0;overflow:auto;position:relative}.directory-row{display:grid;grid-template-columns:minmax(240px,1fr) 150px 112px 112px;align-items:center;padding:0 22px;gap:16px;min-width:690px}.directory-table-head{height:38px;background:var(--panel-alt);position:sticky;top:0;z-index:1;border-bottom:1px solid var(--border);color:var(--muted)}.directory-table-head button{display:flex;gap:6px;align-items:center;font-size:10px;font-weight:600;letter-spacing:.8px;padding:0}.directory-table-head button:nth-last-child(-n+2){justify-content:flex-end}.directory-file{height:37px;border-bottom:1px solid color-mix(in srgb,var(--border) 50%,transparent);font-size:12px}.directory-file:hover,.directory-file.selected{background:var(--panel-alt)}.directory-file.selected{box-shadow:inset 2px 0 var(--accent)}.file-name{display:flex;gap:10px;align-items:center;overflow:hidden}.file-name span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.file-name svg{flex-shrink:0;color:var(--muted)}.file-name .folder-icon{color:var(--amber)}.file-size{font-family:var(--mono);font-size:11px;text-align:right;color:var(--muted)}.status-label{display:flex;align-items:center;gap:7px;font-size:11px;color:var(--muted)}.status-label i{width:5px;height:5px;border-radius:50%;background:currentColor}.status-different .status-label,.status-conflict .status-label{color:var(--amber)}.status-leftOnly .status-label{color:var(--red)}.status-rightOnly .status-label{color:var(--green)}.status-error .status-label{color:var(--red)}.folder-empty{display:flex;align-items:center;flex-direction:column;padding:92px 22px;text-align:center}.empty-folders{display:flex;align-items:center;gap:26px;color:var(--muted);margin-bottom:22px}.empty-folders svg:first-child{color:var(--accent)}.empty-folders svg:last-child{color:var(--green)}.folder-empty h3{font-size:19px;font-weight:500;margin:0 0 11px}.folder-empty p{color:var(--muted);font-size:13px;margin:0 0 22px}.folder-footer{display:flex;justify-content:space-between;gap:12px;align-items:center;padding:14px 22px;border-top:1px solid var(--border);background:var(--panel)}.folder-footer select{max-width:225px;font-size:12px}.sync-modal{width:min(900px,95vw)}.sync-summary{display:flex;gap:25px;padding:15px 0}.sync-summary>span{display:flex;gap:7px;align-items:center;font-size:13px}.sync-roots{display:flex;gap:8px;flex-direction:column;font:11px var(--mono);color:var(--muted);padding:12px;border:1px solid var(--border);border-radius:7px;overflow-wrap:anywhere}.sync-operations{overflow:auto;max-height:40vh;margin:16px 0}.sync-operation{display:grid;grid-template-columns:160px 1fr 85px;gap:14px;font-size:12px;padding:10px;border-bottom:1px solid var(--border)}.sync-operation code{font:11px var(--mono);overflow-wrap:anywhere;user-select:text}.sync-operation>span:last-child{text-align:right}.sync-outcome{display:flex;align-items:flex-start;gap:12px;margin:12px 22px;padding:14px;border:1px solid var(--border);border-radius:7px;background:var(--panel-alt)}.sync-outcome>svg{color:var(--green);flex-shrink:0}.sync-outcome>div{flex:1;min-width:0}.sync-outcome p{font-size:12px;color:var(--muted);overflow-wrap:anywhere}.sync-outcome pre{font-size:11px;white-space:pre-wrap;max-height:140px;overflow:auto;color:var(--red)}.empty-result{padding:50px;text-align:center;font-size:13px;color:var(--muted)}
.status-label{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
</style>
