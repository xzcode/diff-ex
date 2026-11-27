<script setup lang="ts">
import { mergeJsonNode } from '../lib/json-node-merge'
import { translate } from '../lib/i18n'
import type { Language } from '../contracts'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ArrowLeft, ArrowRight, Undo2, ArrowLeftRight, Braces, Check, ChevronDown, Code2, FileJson2, FolderOpen, ListFilter, Save, Search, X } from 'lucide-vue-next'
import type { CompareSession, Preferences, Side, StructureChange, StructuredOptions, StructuredResult, TextDocument } from '../contracts'
import { compareStructured } from '../lib/bridge'
import { isSideDirty } from '../lib/workspace'
import { monaco, setMonacoTheme } from '../lib/monaco'

const props = defineProps<{ session: CompareSession; preferences: Preferences; language: Language }>()
const emit = defineEmits<{
  change: [side: Side, content: string]
  open: [side: Side, encoding?: string]
  save: [side: Side, saveAs?: boolean]
  'update-options': [options: StructuredOptions]
  'update-document': [side: Side, patch: Partial<TextDocument>]
}>()
const t = (key: string, ...args: (string | number)[]) => translate(props.language, key, ...args)
const sides: Side[] = ['left', 'right']
const leftHost = ref<HTMLDivElement>()
const rightHost = ref<HTMLDivElement>()
const editorsHost = ref<HTMLDivElement>()
const workspace = ref<HTMLElement>()
const listHost = ref<HTMLDivElement>()
const canonical = ref(false)
const leftPercent = ref(50)
const editorPercent = ref(48)
const result = ref<StructuredResult>()
const busy = ref(false)
const error = ref('')
const search = ref('')
const kindFilter = ref('all')
const selected = ref<StructureChange>()
const mergeError = ref('')
const mergeSide = ref<Side>('right')
const canMerge = computed(() => !canonical.value && !busy.value && !!selected.value && props.session.structuredOptions.format === 'json' && !!result.value)
let comparedSources: { left: string; right: string; options: string } | undefined
const scrollTop = ref(0)
const listHeight = ref(250)
const rowHeight = 44
let editorLeft: monaco.editor.IStandaloneCodeEditor | undefined
let editorRight: monaco.editor.IStandaloneCodeEditor | undefined
let originals: Partial<Record<Side, monaco.editor.ITextModel>> = {}
let canonicals: Partial<Record<Side, monaco.editor.ITextModel>> = {}
let viewStates: Partial<Record<Side, monaco.editor.ICodeEditorViewState | null>> = {}
let disposables: monaco.IDisposable[] = []
let localizedActions: monaco.IDisposable[] = []
let resizeObserver: ResizeObserver | undefined
let timer: ReturnType<typeof setTimeout> | undefined
let requestId = 0
let externalUpdate = false
let alive = true
let dragCleanup: (() => void) | undefined
const filtered = computed(() => {
  const query = search.value.toLocaleLowerCase()
  return (result.value?.changes || []).filter(change => (kindFilter.value === 'all' || change.kind === kindFilter.value) && (!query || `${change.path} ${change.left ?? ''} ${change.right ?? ''}`.toLocaleLowerCase().includes(query)))
})
const startRow = computed(() => Math.max(0, Math.floor(scrollTop.value / rowHeight) - 5))
const visible = computed(() => filtered.value.slice(startRow.value, startRow.value + Math.ceil(listHeight.value / rowHeight) + 12))
const counts = computed(() => {
  const changes = result.value?.changes || []
  return {
    added: changes.filter(c => c.kind === 'added').length,
    removed: changes.filter(c => c.kind === 'removed').length,
    changed: changes.filter(c => c.kind === 'changed' || c.kind === 'typeChanged').length,
  }
})
const editorFor = (side: Side) => side === 'left' ? editorLeft : editorRight
const dirty = (side: Side) => isSideDirty(props.session, side)
function changeLabel(kind: string) {
  return ({ added: t('Added'), removed: t('Removed'), changed: t('Changed'), typeChanged: t('Type changed') } as Record<string, string>)[kind] || kind
}
function updateOption<K extends keyof StructuredOptions>(key: K, value: StructuredOptions[K]) {
  emit('update-options', { ...props.session.structuredOptions, [key]: value })
}
function setMetadata(side: Side, field: 'encoding' | 'lineEnding', event: Event) {
  const value = (event.target as HTMLSelectElement).value
  emit('update-document', side, { [field]: value, ...(field === 'encoding' && !value.startsWith('utf') ? { bom: false } : {}) })
}
function syncContent(side: Side, content: string) {
  const model = originals[side]
  if (!model || model.getValue() === content) return
  externalUpdate = true
  const view = editorFor(side)?.saveViewState()
  model.setValue(content)
  if (!canonical.value && view) editorFor(side)?.restoreViewState(view)
  externalUpdate = false
}
function syncMode() {
  for (const side of sides) {
    const editor = editorFor(side)
    if (!editor) continue
    if (canonical.value) viewStates[side] = editor.saveViewState()
    editor.setModel(canonical.value ? canonicals[side]! : originals[side]!)
    editor.updateOptions({ readOnly: canonical.value, domReadOnly: canonical.value })
    if (!canonical.value && viewStates[side]) editor.restoreViewState(viewStates[side]!)
  }
}
function syncSyntax() {
  for (const side of sides) if (originals[side]) monaco.editor.setModelLanguage(originals[side]!, props.session.structuredOptions.format)
}
function syncPreferences() {
  setMonacoTheme(props.preferences.theme)
  for (const side of sides) editorFor(side)?.updateOptions({ fontSize: props.preferences.fontSize, lineHeight: Math.round(props.preferences.fontSize * 1.65), wordWrap: props.preferences.wordWrap ? 'on' : 'off' })
}
function scheduleCompare() {
  if (timer) clearTimeout(timer)
  const thisRequest = ++requestId
  busy.value = true
  error.value = ''
  result.value = undefined
  comparedSources = undefined
  selected.value = undefined
  const untouchedDraft = sides.every(side => props.session[side].content === '' && props.session[side].path === null && props.session[side].name === 'Untitled')
  if (untouchedDraft) {
    busy.value = false
    canonicals.left?.setValue('')
    canonicals.right?.setValue('')
    return
  }
  timer = setTimeout(async () => {
    try {
      const left = originals.left?.getValue() ?? props.session.left.content, right = originals.right?.getValue() ?? props.session.right.content
      const options = { ...props.session.structuredOptions }
      const next = await compareStructured(left, right, options)
      if (!alive || thisRequest !== requestId) return
      comparedSources = { left, right, options: JSON.stringify(options) }
      result.value = next
      canonicals.left?.setValue(next.leftCanonical)
      canonicals.right?.setValue(next.rightCanonical)
    } catch (cause) {
      if (!alive || thisRequest !== requestId) return
      error.value = String(cause instanceof Error ? cause.message : cause)
      canonicals.left?.setValue('')
      canonicals.right?.setValue('')
    } finally {
      if (alive && thisRequest === requestId) busy.value = false
    }
  }, 280)
}
function dragSplit(event: PointerEvent, direction: 'horizontal' | 'vertical') {
  event.preventDefault()
  dragCleanup?.()
  const target = event.currentTarget as HTMLElement
  target.setPointerCapture(event.pointerId)
  const previousCursor = document.body.style.cursor
  document.body.style.cursor = direction === 'horizontal' ? 'col-resize' : 'row-resize'
  const move = (moveEvent: PointerEvent) => {
    if (direction === 'horizontal') {
      const rect = editorsHost.value!.getBoundingClientRect()
      leftPercent.value = Math.max(20, Math.min(80, (moveEvent.clientX - rect.left) / rect.width * 100))
    } else {
      const rect = workspace.value!.getBoundingClientRect()
      editorPercent.value = Math.max(20, Math.min(75, (moveEvent.clientY - rect.top - 100) / Math.max(100, rect.height - 160) * 100))
    }
  }
  const end = () => {
    target.removeEventListener('pointermove', move)
    target.removeEventListener('pointerup', end)
    target.removeEventListener('pointercancel', end)
    document.body.style.cursor = previousCursor
    if (target.hasPointerCapture(event.pointerId)) target.releasePointerCapture(event.pointerId)
    dragCleanup = undefined
  }
  target.addEventListener('pointermove', move)
  target.addEventListener('pointerup', end)
  target.addEventListener('pointercancel', end)
  dragCleanup = end
}
function selectChange(change: StructureChange) { selected.value = change; mergeError.value = '' }
function mergeSelected(target: Side) {
  if (!canMerge.value || !selected.value || !comparedSources) return
  mergeError.value = ''
  const left = originals.left?.getValue(), right = originals.right?.getValue()
  if (left === undefined || right === undefined || left !== comparedSources.left || right !== comparedSources.right || JSON.stringify(props.session.structuredOptions) !== comparedSources.options) {
    mergeError.value = t('Comparison changed. Compare again before merging.')
    scheduleCompare()
    return
  }
  const editor = editorFor(target), model = originals[target]
  if (!editor || !model) return
  try {
    const edit = mergeJsonNode(target === 'left' ? right : left, target === 'left' ? left : right, selected.value.path)
    const start = model.getPositionAt(edit.offset), end = model.getPositionAt(edit.offset + edit.length)
    editor.pushUndoStop()
    editor.executeEdits('diff-ex.node-merge', [{ range: new monaco.Range(start.lineNumber, start.column, end.lineNumber, end.column), text: edit.text, forceMoveMarkers: true }])
    editor.pushUndoStop()
    mergeSide.value = target
    editor.focus()
  } catch (cause) {
    mergeError.value = String(cause instanceof Error ? cause.message : cause)
  }
}
function undoNodeMerge() {
  if (canonical.value) return
  editorFor(mergeSide.value)?.trigger('diff-ex', 'undo', null)
}
function resetList() {
  scrollTop.value = 0
  if (listHost.value) listHost.value.scrollTop = 0
}

function syncLocalizedActions() {
  localizedActions.forEach(action => action.dispose())
  localizedActions = []
  for (const side of sides) {
    const pane = editorFor(side)
    if (!pane) continue
    localizedActions.push(pane.addAction({ id: 'diff-ex.save-file', label: t('Save original file'), keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS], run: () => emit('save', side) }))
    localizedActions.push(pane.addAction({ id: 'diff-ex.save-file-as', label: t('Save original file as'), keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyS], run: () => emit('save', side, true) }))
    localizedActions.push(pane.addAction({ id: 'diff-ex.open-file', label: t('Open file'), keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyO], run: () => emit('open', side) }))
  }
}
watch(() => props.language, syncLocalizedActions)
onMounted(async () => {
  await nextTick()
  if (!alive || !leftHost.value || !rightHost.value) return
  setMonacoTheme(props.preferences.theme)
  for (const side of sides) {
    originals[side] = monaco.editor.createModel(props.session[side].content, props.session.structuredOptions.format, monaco.Uri.parse(`inmemory://structured/${props.session.id}/${side}/source`))
    canonicals[side] = monaco.editor.createModel('', 'json', monaco.Uri.parse(`inmemory://structured/${props.session.id}/${side}/canonical.json`))
    const instance = monaco.editor.create(side === 'left' ? leftHost.value : rightHost.value, {
      model: originals[side], automaticLayout: true, minimap: { enabled: false },
      fontFamily: '"SFMono-Regular", Consolas, "Liberation Mono", monospace', fontSize: props.preferences.fontSize,
      lineHeight: Math.round(props.preferences.fontSize * 1.65), padding: { top: 14, bottom: 14 },
      scrollBeyondLastLine: false, tabSize: 2, lineNumbersMinChars: 3, wordWrap: props.preferences.wordWrap ? 'on' : 'off',
      stickyScroll: { enabled: false }, links: false,
      unicodeHighlight: { ambiguousCharacters: false, invisibleCharacters: false },
    })
    if (side === 'left') editorLeft = instance
    else editorRight = instance
    disposables.push(originals[side]!.onDidChangeContent(() => { if (!externalUpdate) emit('change', side, originals[side]!.getValue()) }))
  }
  syncLocalizedActions()
  resizeObserver = new ResizeObserver(() => { if (listHost.value) listHeight.value = listHost.value.clientHeight })
  if (listHost.value) resizeObserver.observe(listHost.value)
  scheduleCompare()
})
watch(() => props.session.left.content, content => syncContent('left', content))
watch(() => props.session.right.content, content => syncContent('right', content))
watch(() => [props.session.left.content, props.session.right.content, props.session.left.path, props.session.right.path, props.session.left.name, props.session.right.name, props.session.structuredOptions], scheduleCompare, { deep: true })
watch(() => props.session.structuredOptions.format, syncSyntax)
watch(() => [props.preferences.theme, props.preferences.fontSize, props.preferences.wordWrap], syncPreferences)
watch(canonical, syncMode)
watch([search, kindFilter, result], resetList)
watch(listHost, (element, previous) => {
  if (previous) resizeObserver?.unobserve(previous)
  if (element) {
    resizeObserver?.observe(element)
    listHeight.value = element.clientHeight
  }
})
onBeforeUnmount(() => {
  alive = false
  ++requestId
  if (timer) clearTimeout(timer)
  dragCleanup?.()
  resizeObserver?.disconnect()
  localizedActions.forEach(action => action.dispose())
  localizedActions = []
  for (const disposable of disposables) disposable.dispose()
  editorLeft?.dispose()
  editorRight?.dispose()
  for (const model of Object.values(originals)) model?.dispose()
  for (const model of Object.values(canonicals)) model?.dispose()
})
</script>

<template>
  <section ref="workspace" class="structure-workspace" :aria-label="t('Structured comparison')">
    <div class="structure-toolbar">
      <div class="format-control"><Braces :size="17" /><select :value="session.structuredOptions.format" :aria-label="t('Structured format')" @change="updateOption('format', ($event.target as HTMLSelectElement).value as StructuredOptions['format'])"><option value="json">JSON</option><option value="yaml">YAML</option><option value="xml">XML</option></select></div>
      <label class="check-label" :class="{ disabled: session.structuredOptions.format === 'xml' }" :title="t('Applies to JSON and YAML arrays. XML element order is preserved.')"><input type="checkbox" :disabled="session.structuredOptions.format === 'xml'" :checked="session.structuredOptions.ignoreArrayOrder && session.structuredOptions.format !== 'xml'" @change="updateOption('ignoreArrayOrder', ($event.target as HTMLInputElement).checked)" />{{ t('Ignore array order') }}</label>
      <label class="check-label" :title="t('Ignore case in string values; keys and element names stay case-sensitive.')"><input type="checkbox" :checked="session.structuredOptions.ignoreCase" @change="updateOption('ignoreCase', ($event.target as HTMLInputElement).checked)" />{{ t('Ignore value case') }}</label>
      <span class="spacer" />
      <button class="btn-icon" :disabled="canonical" :title="t('Undo')" :aria-label="t('Undo')" @click="undoNodeMerge"><Undo2 :size="16" /></button>
      <div class="view-switch"><button :class="{ active: !canonical }" @click="canonical = false"><Code2 :size="14" />{{ t('Original') }}</button><button :class="{ active: canonical }" @click="canonical = true"><FileJson2 :size="14" />{{ t('Canonical JSON') }}</button></div>
    </div>
    <div class="semantic-note"><ArrowLeftRight :size="13" /><span>{{ canonical ? t('Read-only comparison representation. Saving always writes the original document.') : t('Object key order is ignored. Types, values and ordered collections are compared.') }}</span></div>
    <div class="file-headers" :style="{ gridTemplateColumns: `${leftPercent}% 1fr` }"><div v-for="side in sides" :key="side" class="file-header"><span class="side-mark">{{ side === 'left' ? t('LEFT') : t('RIGHT') }}</span><div class="file-name"><strong>{{ !session[side].path && ['Untitled', '(missing on left)', '(missing on right)'].includes(session[side].name) ? t(session[side].name) : session[side].name }}<span v-if="dirty(side)" class="dirty-dot" /></strong><span :title="session[side].path || ''">{{ session[side].path || t('Unsaved document') }}</span></div><span class="encoding-badge">{{ session[side].encoding.toUpperCase() }} · {{ session[side].lineEnding.toUpperCase() }}</span><button class="btn-icon" :title="t('Open original file')" :aria-label="side === 'left' ? t('Open left file') : t('Open right file')" @click="emit('open', side)"><FolderOpen :size="15" /></button><button class="btn-icon" :title="t('Save original file')" :aria-label="side === 'left' ? t('Save left file') : t('Save right file')" @click="emit('save', side)"><Save :size="15" /></button><details class="file-menu"><summary :aria-label="t('File options')"><ChevronDown :size="13" /></summary><div class="file-menu-popover"><button @click="emit('open', side, session[side].encoding)">{{ t('Open using selected encoding…') }}</button><button @click="emit('save', side, true)">{{ t('Save original as…') }}</button><label>{{ t('Save encoding') }}<select :aria-label="t('Save encoding')" :value="session[side].encoding.toLowerCase()" @change="setMetadata(side, 'encoding', $event)"><option value="utf-8">UTF-8</option><option value="utf-16le">UTF-16 LE</option><option value="utf-16be">UTF-16 BE</option><option value="gbk">GBK</option><option value="gb18030">GB18030</option></select></label><label>{{ t('Save line endings') }}<select :aria-label="t('Save line endings')" :value="session[side].lineEnding" @change="setMetadata(side, 'lineEnding', $event)"><option value="lf">LF</option><option value="crlf">CRLF</option><option value="cr">CR</option><option v-if="session[side].lineEnding === 'mixed'" value="mixed" disabled>{{ t('Mixed — choose') }}</option><option v-if="session[side].lineEnding === 'none'" value="none">{{ t('No line ending') }}</option></select></label><label class="check-label"><input type="checkbox" :disabled="!session[side].encoding.toLowerCase().startsWith('utf')" :checked="session[side].bom" @change="emit('update-document', side, { bom: ($event.target as HTMLInputElement).checked })" />BOM</label></div></details></div></div>
    <div v-if="session.left.lineEnding === 'mixed' || session.right.lineEnding === 'mixed'" class="mixed-note">{{ t('Mixed line endings will normalize when edited. Select LF, CRLF or CR before saving.') }}</div>
    <div ref="editorsHost" class="structure-editors" :style="{ flexBasis: `${editorPercent}%`, gridTemplateColumns: `calc(${leftPercent}% - 2px) 4px 1fr` }"><div ref="leftHost" class="code-host" /><div class="horizontal-handle" role="separator" aria-orientation="vertical" :aria-valuenow="Math.round(leftPercent)" tabindex="0" :title="t('Resize editors')" @pointerdown="dragSplit($event, 'horizontal')" @keydown.left.prevent="leftPercent = Math.max(20, leftPercent - 2)" @keydown.right.prevent="leftPercent = Math.min(80, leftPercent + 2)" /><div ref="rightHost" class="code-host" /></div>
    <div class="vertical-handle" role="separator" aria-orientation="horizontal" :aria-valuenow="Math.round(editorPercent)" tabindex="0" :title="t('Resize result area')" @pointerdown="dragSplit($event, 'vertical')" @keydown.up.prevent="editorPercent = Math.max(20, editorPercent - 2)" @keydown.down.prevent="editorPercent = Math.min(75, editorPercent + 2)" />
    <div class="results-toolbar"><strong>{{ t('Semantic changes') }}</strong><span v-if="busy" class="muted">{{ t('Comparing…') }}</span><template v-else-if="result"><span v-if="result.equal" class="equal-status"><Check :size="14" />{{ t('Structurally equal') }}</span><template v-else><span class="stat added">+{{ counts.added }}</span><span class="stat removed">−{{ counts.removed }}</span><span class="stat changed">{{ counts.changed }} {{ t('changed') }}</span></template></template><span class="spacer" /><label class="filter-search"><Search :size="13" /><input v-model="search" :placeholder="t('Search paths or values')" :aria-label="t('Search semantic changes')" /><button v-if="search" :aria-label="t('Clear search')" @click="search = ''"><X :size="12" /></button></label><ListFilter :size="14" class="muted" /><select v-model="kindFilter" :aria-label="t('Filter change type')"><option value="all">{{ t('All changes') }}</option><option value="added">{{ t('Added') }}</option><option value="removed">{{ t('Removed') }}</option><option value="changed">{{ t('Changed') }}</option><option value="typeChanged">{{ t('Type changed') }}</option></select></div>
    <div v-if="error" class="parse-error" role="alert"><strong>{{ t('Cannot compare this document') }}</strong><pre>{{ error }}</pre></div>
    <template v-else><div class="change-table-head"><span>{{ t('Change') }}</span><span>{{ t('Path') }}</span><span>{{ t('Left value') }}</span><span>{{ t('Right value') }}</span></div><div ref="listHost" class="change-list" role="list" :aria-label="t('Semantic differences')" @scroll="scrollTop = ($event.target as HTMLElement).scrollTop"><div v-if="!filtered.length" class="no-changes"><template v-if="busy">{{ t('Parsing and comparing documents…') }}</template><template v-else-if="search || kindFilter !== 'all'">{{ t('No changes match this filter.') }}</template><template v-else-if="result?.equal"><Check :size="24" /><strong>{{ t('Same structure, same values') }}</strong><span>{{ t('Formatting and object key order do not affect this result.') }}</span></template><template v-else>{{ t('Open or paste two documents to compare.') }}</template></div><div v-else :style="{ height: `${filtered.length * rowHeight}px`, position: 'relative' }"><div :style="{ transform: `translateY(${startRow * rowHeight}px)` }"><button v-for="(change, index) in visible" :key="`${startRow + index}:${change.path}`" class="change-row" :class="{ selected: selected === change }" :style="{ height: `${rowHeight}px` }" role="listitem" @click="selectChange(change)"><span class="kind-pill" :class="change.kind">{{ changeLabel(change.kind) }}</span><code class="change-path" :title="change.path || '/'">{{ change.path || '/' }}</code><code class="change-value" :class="{ missing: change.left === null }" :title="change.left || ''">{{ change.left === null ? t('Absent') : change.left }}</code><code class="change-value" :class="{ missing: change.right === null }" :title="change.right || ''">{{ change.right === null ? t('Absent') : change.right }}</code></button></div></div></div></template>
    <div v-if="mergeError" class="notice" role="status">{{ mergeError }}</div>
    <div v-if="selected && !error" class="change-detail"><div class="detail-heading"><strong>{{ changeLabel(selected.kind) }}</strong><code>{{ selected.path || '/' }}</code><span class="spacer" /><template v-if="session.structuredOptions.format === 'json'"><button class="btn" :disabled="!canMerge" @click="mergeSelected('left')"><ArrowLeft :size="14" />{{ t('To left') }}</button><button class="btn" :disabled="!canMerge" @click="mergeSelected('right')">{{ t('To right') }}<ArrowRight :size="14" /></button></template><button class="btn-icon" :aria-label="t('Close change details')" @click="selected = undefined"><X :size="14" /></button></div><div class="detail-values"><div><span>{{ t('LEFT') }}</span><pre>{{ selected.left === null ? t('Absent') : selected.left }}</pre></div><div><span>{{ t('RIGHT') }}</span><pre>{{ selected.right === null ? t('Absent') : selected.right }}</pre></div></div></div>
    <div class="structure-footer"><span>{{ filtered.length.toLocaleString() }} {{ t('matching changes') }}</span><span>{{ t('Paths use JSON Pointer escaping · click a row for full values') }}</span></div>
  </section>
</template>

<style scoped>
.structure-workspace{height:100%;min-height:0;display:flex;flex-direction:column;overflow:hidden;background:var(--panel)}.structure-toolbar{display:flex;align-items:center;gap:18px;padding:10px 14px;min-height:48px;border-bottom:1px solid var(--border);flex-wrap:wrap}.format-control{display:flex;gap:8px;align-items:center;color:var(--accent)}.format-control select{font-size:12px;padding:5px 24px 5px 8px;min-height:29px}.check-label{display:flex;align-items:center;gap:7px;white-space:nowrap;font-size:11px}.check-label input{accent-color:var(--accent);width:14px;height:14px;min-height:0;margin:0}.disabled{opacity:.45}.spacer{flex:1}.view-switch{display:flex;padding:3px;background:var(--bg);border:1px solid var(--border);border-radius:6px}.view-switch button{display:flex;align-items:center;gap:6px;font-size:11px;padding:5px 9px;background:transparent;border:0;border-radius:4px;color:var(--muted)}.view-switch button.active{color:var(--text);background:var(--panel-alt);box-shadow:0 1px 4px #0001}.semantic-note{padding:7px 15px;display:flex;align-items:center;gap:7px;font-size:10px;color:var(--muted);border-bottom:1px solid var(--border);background:var(--panel-alt)}.file-headers{display:grid}.file-header{position:relative;display:flex;align-items:center;padding:9px 12px;gap:8px;min-width:0;border-bottom:1px solid var(--border);background:var(--panel-alt)}.file-header:first-child{border-right:1px solid var(--border)}.side-mark{font-size:9px;font-weight:700;letter-spacing:1px;color:var(--muted)}.file-name{display:flex;flex-direction:column;flex:1;min-width:0;gap:3px}.file-name strong{font-size:12px;font-weight:600;display:flex;align-items:center;gap:8px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.file-name>span{font-size:10px;color:var(--muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.encoding-badge{font-size:9px;color:var(--muted);white-space:nowrap}.dirty-dot{width:6px;height:6px;background:var(--amber);border-radius:50%;display:inline-block;flex-shrink:0}.btn-icon{display:flex;align-items:center;justify-content:center;gap:4px;padding:0;width:28px;height:28px;border-radius:4px;flex-shrink:0}.structure-editors{display:grid;min-height:0;flex-shrink:1;flex-grow:0;overflow:hidden}.code-host{min-height:0;min-width:0;overflow:hidden}.horizontal-handle{background:var(--border);cursor:col-resize;touch-action:none}.vertical-handle{height:5px;flex-shrink:0;cursor:row-resize;touch-action:none;background:var(--border)}.horizontal-handle:hover,.vertical-handle:hover,.horizontal-handle:focus,.vertical-handle:focus{background:var(--accent)}.results-toolbar{display:flex;align-items:center;gap:12px;padding:10px 14px;flex-wrap:wrap;border-bottom:1px solid var(--border);font-size:11px}.results-toolbar>strong{font-size:12px;font-weight:600}.results-toolbar>select{font-size:11px;padding:5px;max-width:132px;min-height:27px}.equal-status{color:var(--green);display:flex;gap:5px;align-items:center}.stat.added{color:var(--green)}.stat.removed{color:var(--red)}.stat.changed{color:var(--amber)}.muted{color:var(--muted)}.filter-search{display:flex;align-items:center;gap:6px;background:var(--bg);border:1px solid var(--border);padding:4px 7px;border-radius:5px;color:var(--muted)}.filter-search input{border:0!important;background:transparent;box-shadow:none;outline:0;min-height:20px;padding:0;font-size:11px;width:155px;color:var(--text)}.filter-search button{border:0;background:transparent;color:var(--muted);display:flex;padding:0}.change-table-head,.change-row{display:grid;grid-template-columns:100px minmax(120px,1.2fr) minmax(90px,1fr) minmax(90px,1fr);align-items:center;gap:14px;padding:0 14px}.change-table-head{font-size:10px;color:var(--muted);height:30px;min-height:30px;background:var(--panel-alt);border-bottom:1px solid var(--border)}.change-list{flex:1;min-height:0;overflow:auto}.change-row{width:100%;background:transparent;border:0;border-bottom:1px solid var(--border);text-align:left;cursor:pointer;border-radius:0;color:var(--text);font-size:11px;box-sizing:border-box}.change-row:hover,.change-row.selected{background:color-mix(in srgb,var(--accent) 8%,var(--panel))}.kind-pill{justify-self:start;font-size:9px;padding:3px 6px;border-radius:3px;font-weight:600;white-space:nowrap}.kind-pill.added{color:var(--green);background:color-mix(in srgb,var(--green) 12%,transparent)}.kind-pill.removed{color:var(--red);background:color-mix(in srgb,var(--red) 12%,transparent)}.kind-pill.changed,.kind-pill.typeChanged{color:var(--amber);background:color-mix(in srgb,var(--amber) 12%,transparent)}.change-path,.change-value{font-size:11px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0}.change-path{color:var(--accent)}.change-value.missing{color:var(--muted);font-style:italic}.no-changes{padding:25px;min-height:100%;display:flex;align-items:center;justify-content:center;flex-direction:column;gap:10px;color:var(--muted);font-size:11px;text-align:center;box-sizing:border-box}.no-changes>svg{color:var(--green)}.no-changes>strong{font-size:13px;color:var(--text);font-weight:550}.parse-error{padding:16px;color:var(--red);font-size:12px;background:color-mix(in srgb,var(--red) 7%,var(--panel));flex:1;overflow:auto}.parse-error pre{font-size:12px;white-space:pre-wrap;line-height:1.6;font-family:inherit}.change-detail{flex-shrink:0;max-height:35%;border-top:1px solid var(--border);background:var(--panel-alt);overflow:auto}.detail-heading{display:flex;gap:12px;align-items:center;padding:5px 14px;font-size:11px;border-bottom:1px solid var(--border)}.detail-heading code{color:var(--accent);overflow:hidden;text-overflow:ellipsis}.detail-values{display:grid;grid-template-columns:1fr 1fr}.detail-values>div{min-width:0;padding:10px 14px}.detail-values>div:first-child{border-right:1px solid var(--border)}.detail-values span{font-size:9px;font-weight:700;letter-spacing:1px;color:var(--muted)}.detail-values pre{white-space:pre-wrap;overflow-wrap:anywhere;font-size:12px;line-height:1.5;margin:7px 0 0;user-select:text}.structure-footer{display:flex;justify-content:space-between;gap:16px;padding:8px 14px;font-size:10px;color:var(--muted);border-top:1px solid var(--border);flex-shrink:0}.file-menu{position:static}.file-menu summary{cursor:pointer;display:flex;width:18px;height:28px;align-items:center;justify-content:center;list-style:none;border-radius:4px;color:var(--muted)}.file-menu summary::-webkit-details-marker{display:none}.file-menu-popover{position:absolute;top:calc(100% - 2px);right:8px;z-index:12;width:225px;max-width:calc(100% - 16px);max-height:35vh;overflow:auto;box-shadow:0 12px 30px #0004;background:var(--panel);border:1px solid var(--border);border-radius:8px;padding:8px;display:flex;flex-direction:column;gap:4px}.file-menu-popover button{background:transparent;border:0;padding:8px;text-align:left;font-size:12px;color:var(--text);border-radius:4px}.file-menu-popover button:hover{background:var(--panel-alt)}.file-menu-popover label:not(.check-label){display:flex;flex-direction:column;gap:6px;padding:8px;font-size:11px;color:var(--muted)}.file-menu-popover select{font-size:12px;padding:6px;color:var(--text)}.file-menu-popover .check-label{padding:8px}.mixed-note{padding:7px 14px;color:var(--amber);font-size:11px;background:color-mix(in srgb,var(--amber) 7%,var(--panel));line-height:1.5;border-bottom:1px solid var(--border)}
@media(max-width:950px){.encoding-badge{display:none}.structure-toolbar{gap:10px}.change-table-head,.change-row{grid-template-columns:85px minmax(110px,1.1fr) minmax(70px,1fr) minmax(70px,1fr);gap:9px;padding-left:10px;padding-right:10px}.structure-footer span:last-child{display:none}.side-mark{display:none}.file-header{padding:9px 8px;gap:6px}.results-toolbar{gap:7px}.filter-search input{width:125px}}
.detail-heading{flex-wrap:wrap}.detail-heading .btn{min-height:28px;height:28px;font-size:11px}.detail-heading code{max-width:40%;white-space:nowrap}
</style>
