<script setup lang="ts">
import { translate } from '../lib/i18n'
import type { Language } from '../contracts'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ArrowDown, ArrowLeft, ArrowRight, ArrowUp, Check, ChevronDown, Columns2, Copy, FileCode2, FolderOpen, ListFilter, Rows3, Save, Search, Settings2, Undo2, WrapText } from 'lucide-vue-next'
import type { CompareSession, Preferences, Side, TextDiffResult, TextDocument, TextOptions } from '../contracts'
import { compareText } from '../lib/bridge'
import { isSideDirty } from '../lib/workspace'
import { codeLanguages, languageForFile, monaco, setMonacoTheme } from '../lib/monaco'
import { mergeLineChange } from '../lib/compare-ui-merge'

const props = defineProps<{ session: CompareSession; preferences: Preferences; language: Language }>()
const emit = defineEmits<{
  change: [side: Side, content: string]
  open: [side: Side, encoding?: string]
  save: [side: Side, saveAs?: boolean]
  'update-options': [options: TextOptions]
  'update-document': [side: Side, patch: Partial<TextDocument>]
}>()
const t = (key: string, ...args: (string | number)[]) => translate(props.language, key, ...args)
const host = ref<HTMLDivElement>()
const split = ref(true)
const headerLeftPercent = ref(50)
const collapsed = ref(false)
const wrap = ref(props.preferences.wordWrap)
const optionsVisible = ref(false)
const syntax = ref('auto')
const activeSide = ref<Side>('right')
const current = ref(-1)
const rawCount = ref(0)
const ready = ref(false)
const busy = ref(false)
const error = ref('')
const result = ref<TextDiffResult>()
const cursor = ref({ line: 1, column: 1 })
const statusNote = ref('')
let editor: monaco.editor.IStandaloneDiffEditor | undefined
let original: monaco.editor.ITextModel | undefined
let modified: monaco.editor.ITextModel | undefined
let rawChanges: monaco.editor.ILineChange[] = []
let diffVersions = [0, 0]
let disposables: monaco.IDisposable[] = []
let localizedActions: monaco.IDisposable[] = []
let timer: ReturnType<typeof setTimeout> | undefined
let requestId = 0
let externalUpdate = false
let alive = true
const sides: Side[] = ['left', 'right']
const rulesActive = computed(() => props.session.textOptions.ignoreCase || props.session.textOptions.ignoreBlankLines || props.session.textOptions.whitespace !== 'none' || !props.session.textOptions.ignoreLineEndings)
const currentLabel = computed(() => current.value < 0 ? `0 / ${rawCount.value}` : `${current.value + 1} / ${rawCount.value}`)
const lineCount = (value: string) => value.split(/\r\n|\r|\n/).length
const dirty = (side: Side) => isSideDirty(props.session, side)
const contentEditor = (side: Side) => side === 'left' ? editor?.getOriginalEditor() : editor?.getModifiedEditor()
const documentModel = (side: Side) => side === 'left' ? original : modified

function updateOption<K extends keyof TextOptions>(key: K, value: TextOptions[K]) {
  emit('update-options', { ...props.session.textOptions, [key]: value })
}
function optionSelect(event: Event, key: 'algorithm' | 'whitespace') {
  updateOption(key, (event.target as HTMLSelectElement).value as never)
}
function setMetadata(side: Side, field: 'encoding' | 'lineEnding', event: Event) {
  const value = (event.target as HTMLSelectElement).value
  emit('update-document', side, { [field]: value, ...(field === 'encoding' && !value.startsWith('utf') ? { bom: false } : {}) })
}
function syncLanguage() {
  if (original) monaco.editor.setModelLanguage(original, syntax.value === 'auto' ? languageForFile(props.session.left.name) : syntax.value)
  if (modified) monaco.editor.setModelLanguage(modified, syntax.value === 'auto' ? languageForFile(props.session.right.name) : syntax.value)
}
function syncHeaderSplit() {
  if (!split.value) { headerLeftPercent.value = 50; return }
  const hostBounds = host.value?.getBoundingClientRect()
  const rightBounds = editor?.getModifiedEditor().getDomNode()?.getBoundingClientRect()
  if (hostBounds?.width && rightBounds?.width) {
    headerLeftPercent.value = Math.max(5, Math.min(95, (rightBounds.left - hostBounds.left) / hostBounds.width * 100))
  }
}
function syncOptions() {
  editor?.updateOptions({
    renderSideBySide: split.value,
    useInlineViewWhenSpaceIsLimited: false,
    wordWrap: wrap.value ? 'on' : 'off',
    diffWordWrap: wrap.value ? 'on' : 'off',
    fontSize: props.preferences.fontSize,
    lineHeight: Math.round(props.preferences.fontSize * 1.65),
    hideUnchangedRegions: { enabled: collapsed.value, contextLineCount: 3, minimumLineCount: 4, revealLineCount: 12 },
  })
  syncHeaderSplit()
}
function syncContent(side: Side, value: string) {
  const model = documentModel(side)
  if (!model || model.getValue() === value) return
  // Parent reflections of editor events do not recreate models or reset undo/cursor.
  // A newly opened file intentionally starts its own undo history.
  externalUpdate = true
  const view = contentEditor(side)?.saveViewState()
  model.setValue(value)
  if (view) contentEditor(side)?.restoreViewState(view)
  externalUpdate = false
}
function scheduleCompare() {
  if (timer) clearTimeout(timer)
  const thisRequest = ++requestId
  result.value = undefined
  error.value = ''
  busy.value = true
  timer = setTimeout(async () => {
    try {
      const next = await compareText(props.session.left.content, props.session.right.content, { ...props.session.textOptions })
      if (alive && thisRequest === requestId) result.value = next
    } catch (cause) {
      if (alive && thisRequest === requestId) error.value = String(cause instanceof Error ? cause.message : cause)
    } finally {
      if (alive && thisRequest === requestId) busy.value = false
    }
  }, 220)
}
function selectCurrentFromCursor(side: Side, line: number) {
  if (!rawChanges.length) return
  const starts = rawChanges.map(change => side === 'left' ? change.originalStartLineNumber : change.modifiedStartLineNumber)
  let selected = starts.findIndex(start => start >= line)
  if (selected < 0) selected = starts.length - 1
  if (selected > 0 && starts[selected]! > line) selected -= 1
  current.value = selected
}
function navigate(direction: -1 | 1) {
  if (!rawChanges.length || !editor) return
  current.value = (current.value + direction + rawChanges.length) % rawChanges.length
  const change = rawChanges[current.value]!
  const originalLine = Math.max(1, change.originalStartLineNumber)
  const modifiedLine = Math.max(1, change.modifiedStartLineNumber)
  editor.getOriginalEditor().revealLineInCenter(originalLine)
  editor.getModifiedEditor().revealLineInCenter(modifiedLine)
  const target = contentEditor(activeSide.value)
  target?.setPosition({ lineNumber: activeSide.value === 'left' ? originalLine : modifiedLine, column: 1 })
  target?.focus()
}
function replaceContents(side: Side, content: string) {
  const model = documentModel(side)
  const target = contentEditor(side)
  if (!model || !target || model.getValue() === content) return
  target.pushUndoStop()
  target.executeEdits('diff-ex.merge', [{ range: model.getFullModelRange(), text: content, forceMoveMarkers: true }])
  target.pushUndoStop()
  target.focus()
  activeSide.value = side
}
function merge(target: Side) {
  if (!ready.value || !original || !modified || diffVersions[0] !== original.getVersionId() || diffVersions[1] !== modified.getVersionId()) return
  const change = rawChanges[current.value]
  if (!change) return
  const content = mergeLineChange(original.getValue(), modified.getValue(), change, target)
  replaceContents(target, content)
  statusNote.value = 'Change applied. Undo is available.'
}
function copyAll(target: Side) {
  const source = documentModel(target === 'left' ? 'right' : 'left')
  if (source) replaceContents(target, source.getValue())
  statusNote.value = 'All text copied. Undo is available.'
}
function undo() {
  contentEditor(activeSide.value)?.trigger('diff-ex', 'undo', null)
}
function find() {
  void contentEditor(activeSide.value)?.getAction('actions.find')?.run()
}

function syncLocalizedActions() {
  localizedActions.forEach(action => action.dispose())
  localizedActions = []
  for (const side of sides) {
    const pane = contentEditor(side)
    if (!pane) continue
    localizedActions.push(pane.addAction({ id: 'diff-ex.save-file', label: t('Save file'), keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS], run: () => emit('save', side) }))
    localizedActions.push(pane.addAction({ id: 'diff-ex.save-file-as', label: t('Save file as'), keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyS], run: () => emit('save', side, true) }))
    localizedActions.push(pane.addAction({ id: 'diff-ex.open-file', label: t('Open file'), keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyO], run: () => emit('open', side) }))
    localizedActions.push(pane.addAction({ id: 'diff-ex.next-change', label: t('Next difference'), keybindings: [monaco.KeyCode.F7], run: () => navigate(1) }))
    localizedActions.push(pane.addAction({ id: 'diff-ex.previous-change', label: t('Previous difference'), keybindings: [monaco.KeyMod.Shift | monaco.KeyCode.F7], run: () => navigate(-1) }))
  }
}
watch(() => props.language, syncLocalizedActions)
onMounted(async () => {
  await nextTick()
  if (!host.value || !alive) return
  setMonacoTheme(props.preferences.theme)
  original = monaco.editor.createModel(props.session.left.content, languageForFile(props.session.left.name), monaco.Uri.parse(`inmemory://diff-ex/${props.session.id}/left/${encodeURIComponent(props.session.left.name)}`))
  modified = monaco.editor.createModel(props.session.right.content, languageForFile(props.session.right.name), monaco.Uri.parse(`inmemory://diff-ex/${props.session.id}/right/${encodeURIComponent(props.session.right.name)}`))
  editor = monaco.editor.createDiffEditor(host.value, {
    originalEditable: true,
    readOnly: false,
    automaticLayout: true,
    enableSplitViewResizing: true,
    renderSideBySide: true,
    useInlineViewWhenSpaceIsLimited: false,
    ignoreTrimWhitespace: false,
    // Resource availability determines capacity; Monaco's default 50MB cap is disabled.
    maxFileSize: 0,
    maxComputationTime: 0,
    diffAlgorithm: 'advanced',
    renderMarginRevertIcon: false,
    renderGutterMenu: false,
    minimap: { enabled: false },
    fontFamily: '"SFMono-Regular", Consolas, "Liberation Mono", monospace',
    fontSize: props.preferences.fontSize,
    lineHeight: Math.round(props.preferences.fontSize * 1.65),
    scrollBeyondLastLine: false,
    padding: { top: 16, bottom: 16 },
    renderWhitespace: 'selection',
    lineNumbersMinChars: 3,
    stickyScroll: { enabled: false },
    links: false,
    unicodeHighlight: { ambiguousCharacters: false, invisibleCharacters: false },
  })
  original.updateOptions({ tabSize: 2 })
  modified.updateOptions({ tabSize: 2 })
  editor.setModel({ original, modified })
  syncOptions()
  for (const side of sides) {
    const model = documentModel(side)!
    const pane = contentEditor(side)!
    disposables.push(model.onDidChangeContent(() => {
      ready.value = false
      if (!externalUpdate) emit('change', side, model.getValue())
    }))
    disposables.push(pane.onDidLayoutChange(() => { queueMicrotask(() => { if (alive) syncHeaderSplit() }) }))
    disposables.push(pane.onDidFocusEditorText(() => { activeSide.value = side }))
    disposables.push(pane.onDidChangeCursorPosition(event => {
      activeSide.value = side
      cursor.value = { line: event.position.lineNumber, column: event.position.column }
      selectCurrentFromCursor(side, event.position.lineNumber)
    }))
  }
  syncLocalizedActions()
  disposables.push(editor.onDidUpdateDiff(() => {
    rawChanges = editor?.getLineChanges() || []
    rawCount.value = rawChanges.length
    current.value = rawChanges.length ? Math.max(0, Math.min(current.value, rawChanges.length - 1)) : -1
    diffVersions = [original!.getVersionId(), modified!.getVersionId()]
    ready.value = true
  }))
  scheduleCompare()
})
watch(() => props.session.left.content, value => syncContent('left', value))
watch(() => props.session.right.content, value => syncContent('right', value))
watch(() => [props.session.left.content, props.session.right.content, props.session.textOptions], scheduleCompare, { deep: true })
watch(() => [props.session.left.name, props.session.right.name, syntax.value], syncLanguage)
watch(() => props.preferences.theme, setMonacoTheme)
watch(() => props.preferences.wordWrap, value => { wrap.value = value })
watch(() => [props.preferences.fontSize, split.value, collapsed.value, wrap.value], syncOptions)
onBeforeUnmount(() => {
  alive = false
  requestId += 1
  if (timer) clearTimeout(timer)
  localizedActions.forEach(action => action.dispose())
  localizedActions = []
  for (const disposable of disposables) disposable.dispose()
  disposables = []
  editor?.dispose()
  original?.dispose()
  modified?.dispose()
})
</script>

<template>
  <section class="text-workspace" :aria-label="t('Text comparison')">
    <div class="text-tools">
      <div class="tool-group">
        <button class="btn-icon" :title="t('Previous difference (Shift+F7)')" :aria-label="t('Previous difference')" :disabled="!rawCount" @click="navigate(-1)"><ArrowUp :size="16" /></button>
        <span class="hunk-counter" :title="t('Visual differences')">{{ currentLabel }}</span>
        <button class="btn-icon" :title="t('Next difference (F7)')" :aria-label="t('Next difference')" :disabled="!rawCount" @click="navigate(1)"><ArrowDown :size="16" /></button>
      </div>
      <div class="tool-separator" />
      <div class="tool-group">
        <button class="btn small" :disabled="!ready || current < 0" :title="t('Apply selected original-text difference to left')" @click="merge('left')"><ArrowLeft :size="15" />{{ t('To left') }}</button>
        <button class="btn small" :disabled="!ready || current < 0" :title="t('Apply selected original-text difference to right')" @click="merge('right')">{{ t('To right') }}<ArrowRight :size="15" /></button>
        <button class="btn-icon" :title="t('Undo in focused pane')" :aria-label="t('Undo')" @click="undo"><Undo2 :size="16" /></button>
      </div>
      <div class="toolbar-spacer" />
      <select v-model="syntax" class="syntax-select" :aria-label="t('Syntax language')"><option v-for="[value, label] in codeLanguages" :key="value" :value="value">{{ value === 'auto' ? t('Auto language') : value === 'plaintext' ? t('Text') : label }}</option></select>
      <button class="btn-icon" :title="t('Find in focused pane')" :aria-label="t('Find')" @click="find"><Search :size="16" /></button>
      <button class="btn-icon" :class="{ active: wrap }" :title="t('Word wrap')" :aria-label="t('Word wrap')" :aria-pressed="wrap" @click="wrap = !wrap"><WrapText :size="16" /></button>
      <button class="btn-icon" :class="{ active: collapsed }" :title="t('Collapse unchanged regions')" :aria-label="t('Collapse unchanged regions')" :aria-pressed="collapsed" @click="collapsed = !collapsed"><ListFilter :size="16" /></button>
      <button class="btn-icon" :title="split ? t('Switch to inline view') : t('Switch to side-by-side view')" :aria-label="t('Toggle comparison layout')" @click="split = !split"><Columns2 v-if="split" :size="16" /><Rows3 v-else :size="16" /></button>
      <button class="btn small" :class="{ active: optionsVisible }" :aria-expanded="optionsVisible" @click="optionsVisible = !optionsVisible"><Settings2 :size="15" />{{ t('Rules') }}<ChevronDown :size="13" /></button>
    </div>

    <div v-if="optionsVisible" class="rules-panel">
      <label>{{ t('Algorithm') }}<select :aria-label="t('Algorithm')" :value="session.textOptions.algorithm" @change="optionSelect($event, 'algorithm')"><option value="myers">Myers</option><option value="patience">Patience</option><option value="histogram">Histogram</option></select></label>
      <label>{{ t('Whitespace') }}<select :aria-label="t('Whitespace')" :value="session.textOptions.whitespace" @change="optionSelect($event, 'whitespace')"><option value="none">{{ t('Compare all') }}</option><option value="trim">{{ t('Ignore edge whitespace') }}</option><option value="all">{{ t('Ignore all whitespace') }}</option></select></label>
      <label class="check-label"><input type="checkbox" :checked="session.textOptions.ignoreCase" @change="updateOption('ignoreCase', ($event.target as HTMLInputElement).checked)" />{{ t('Ignore case') }}</label>
      <label class="check-label"><input type="checkbox" :checked="session.textOptions.ignoreBlankLines" @change="updateOption('ignoreBlankLines', ($event.target as HTMLInputElement).checked)" />{{ t('Ignore blank lines') }}</label>
      <label class="check-label"><input type="checkbox" :checked="session.textOptions.ignoreLineEndings" @change="updateOption('ignoreLineEndings', ($event.target as HTMLInputElement).checked)" />{{ t('Ignore line endings') }}</label>
      <p class="rules-explanation">{{ t('Rules and algorithm control the result count below. The editor always shows original-text differences; merging copies original text.') }}</p>
    </div>

    <div class="file-headers" :style="{ gridTemplateColumns: `${headerLeftPercent}% 1fr` }">
      <div v-for="side in sides" :key="side" class="file-header" :class="side">
        <span class="side-mark">{{ side === 'left' ? t('LEFT') : t('RIGHT') }}</span>
        <FileCode2 :size="17" class="file-symbol" />
        <div class="file-identity"><strong :title="session[side].name">{{ !session[side].path && ['Untitled', '(missing on left)', '(missing on right)'].includes(session[side].name) ? t(session[side].name) : session[side].name }}<span v-if="dirty(side)" class="dirty-dot" :title="t('Unsaved changes')" /></strong><span :title="session[side].path || ''">{{ session[side].path || t('Unsaved document · paste or open a file') }}</span></div>
        <button class="btn-icon" :title="t('Open file')" :aria-label="side === 'left' ? t('Open left file') : t('Open right file')" @click="emit('open', side)"><FolderOpen :size="16" /></button>
        <button class="btn-icon" :title="t('Save file')" :aria-label="side === 'left' ? t('Save left file') : t('Save right file')" @click="emit('save', side)"><Save :size="16" /></button>
        <details class="file-menu"><summary :aria-label="t('File options')"><ChevronDown :size="13" /></summary><div class="file-menu-popover">
          <button @click="emit('open', side, session[side].encoding)">{{ t('Open using selected encoding…') }}</button>
          <button @click="emit('save', side, true)">{{ t('Save as…') }}</button>
          <button @click="copyAll(side)"><Copy :size="14" />{{ side === 'left' ? t('Copy all right → left') : t('Copy all left → right') }}</button>
          <label>{{ t('Save encoding') }}<select :aria-label="t('Save encoding')" :value="session[side].encoding.toLowerCase()" @change="setMetadata(side, 'encoding', $event)"><option value="utf-8">UTF-8</option><option value="utf-16le">UTF-16 LE</option><option value="utf-16be">UTF-16 BE</option><option value="gbk">GBK</option><option value="gb18030">GB18030</option></select></label>
          <label>{{ t('Save line endings') }}<select :aria-label="t('Save line endings')" :value="session[side].lineEnding" @change="setMetadata(side, 'lineEnding', $event)"><option value="lf">LF</option><option value="crlf">CRLF</option><option value="cr">CR</option><option v-if="session[side].lineEnding === 'mixed'" value="mixed" disabled>{{ t('Mixed — choose') }}</option><option v-if="session[side].lineEnding === 'none'" value="none">{{ t('No line ending') }}</option></select></label>
          <label class="check-label"><input type="checkbox" :disabled="!session[side].encoding.toLowerCase().startsWith('utf')" :checked="session[side].bom" @change="emit('update-document', side, { bom: ($event.target as HTMLInputElement).checked })" />{{ t('Write BOM') }}</label>
        </div></details>
      </div>
    </div>
    <div v-if="session.left.lineEnding === 'mixed' || session.right.lineEnding === 'mixed'" class="mixed-note">{{ t('Mixed line endings detected. Editing normalizes line endings; choose LF, CRLF or CR in file options before saving.') }}</div>
    <div v-if="error" class="compare-error" role="alert">{{ error }}</div>
    <div ref="host" class="monaco-host" />
    <div class="file-statuses" :style="{ gridTemplateColumns: `${headerLeftPercent}% 1fr` }"><div v-for="side in sides" :key="side"><span>{{ lineCount(session[side].content).toLocaleString() }} {{ t('lines') }}</span><span>{{ session[side].encoding.toUpperCase() }}{{ session[side].bom ? ' BOM' : '' }}</span><span>{{ session[side].lineEnding.toUpperCase() }}</span><span v-if="activeSide === side" class="cursor-info">{{ t('Ln') }} {{ cursor.line }}, {{ t('Col') }} {{ cursor.column }}</span></div></div>
    <div class="comparison-summary" aria-live="polite">
      <span v-if="busy" class="muted">{{ t('Comparing…') }}</span>
      <template v-else-if="result"><span v-if="result.equal" class="equal-status"><Check :size="14" />{{ rulesActive ? t('Equal under current rules') : t('Contents are equal') }}</span><template v-else><span class="stat added">+{{ result.added }} {{ t('added') }}</span><span class="stat removed">−{{ result.removed }} {{ t('removed') }}</span><span class="stat modified">{{ result.modified }} {{ t('modified') }}</span><span class="muted">{{ result.hunks.length }} {{ t('change blocks') }}</span></template></template>
      <span v-if="rulesActive" class="rule-badge">{{ t('Rules applied to count') }}</span>
      <span class="toolbar-spacer" /><span class="status-note">{{ t(statusNote || 'F7 next change · edits are saved only when you choose Save') }}</span>
    </div>
  </section>
</template>

<style scoped>
.text-workspace{height:100%;min-height:0;display:flex;flex-direction:column;overflow:hidden;background:var(--panel)}
.text-tools{display:flex;align-items:center;gap:5px;padding:9px 12px;min-height:48px;flex-wrap:wrap;border-bottom:1px solid var(--border);background:var(--panel)}
.tool-group{display:flex;align-items:center;gap:4px}.toolbar-spacer{flex:1}.tool-separator{height:20px;width:1px;background:var(--border);margin:0 5px}.hunk-counter{font-size:12px;min-width:45px;text-align:center;font-variant-numeric:tabular-nums;color:var(--muted)}
.btn,.btn-icon{display:inline-flex;align-items:center;justify-content:center;gap:6px}.small{font-size:12px;min-height:30px;padding:5px 8px}.btn-icon{width:30px;height:30px;padding:0;flex-shrink:0;border-radius:5px}.active{background:color-mix(in srgb,var(--accent) 14%,transparent);color:var(--accent);border-color:color-mix(in srgb,var(--accent) 40%,transparent)}
.syntax-select{width:132px;max-width:160px;font-size:12px;min-height:30px;padding:4px 7px}.rules-panel{padding:12px 16px;display:flex;align-items:center;gap:16px;flex-wrap:wrap;border-bottom:1px solid var(--border);background:var(--panel-alt);font-size:12px}.rules-panel label{display:flex;align-items:center;gap:8px}.rules-panel select{font-size:12px;padding:5px;max-width:185px}.check-label{display:flex;align-items:center;gap:7px;white-space:nowrap}.check-label input{accent-color:var(--accent);width:14px;height:14px;min-height:0;margin:0}.rules-explanation{flex-basis:100%;margin:0;color:var(--muted);line-height:1.55}
.file-headers,.file-statuses{display:grid;grid-template-columns:1fr 1fr;flex-shrink:0}.file-header{position:relative;display:flex;align-items:center;gap:9px;padding:12px 14px;min-width:0;border-bottom:1px solid var(--border);background:var(--panel-alt)}.file-header:first-child{border-right:1px solid var(--border)}.side-mark{font-size:9px;font-weight:750;letter-spacing:1px;color:var(--muted);flex-shrink:0}.file-symbol{color:var(--accent);flex-shrink:0}.right .file-symbol{color:var(--green)}.file-identity{display:flex;flex-direction:column;gap:4px;flex:1;min-width:0}.file-identity strong{font-size:12px;display:flex;gap:7px;align-items:center;font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.file-identity>span{font-size:10px;color:var(--muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.dirty-dot{width:6px;height:6px;background:var(--amber);border-radius:50%;display:inline-block;flex-shrink:0}.file-menu{position:static}.file-menu summary{cursor:pointer;display:flex;width:20px;height:30px;align-items:center;justify-content:center;list-style:none;border-radius:4px;color:var(--muted)}.file-menu summary::-webkit-details-marker{display:none}.file-menu[open] summary,.file-menu summary:hover{background:var(--bg);color:var(--text)}.file-menu-popover{position:absolute;top:calc(100% - 2px);right:8px;z-index:12;width:240px;max-width:calc(100% - 16px);max-height:35vh;overflow:auto;box-shadow:0 12px 30px #0004;background:var(--panel);border:1px solid var(--border);border-radius:8px;padding:8px;display:flex;flex-direction:column;gap:4px}.file-menu-popover button{display:flex;gap:8px;align-items:center;background:transparent;border:0;padding:8px;text-align:left;font-size:12px;color:var(--text);border-radius:4px}.file-menu-popover button:hover{background:var(--panel-alt)}.file-menu-popover label:not(.check-label){display:flex;flex-direction:column;gap:6px;padding:8px;font-size:11px;color:var(--muted)}.file-menu-popover select{font-size:12px;padding:6px;color:var(--text)}.file-menu-popover .check-label{padding:8px;font-size:12px}.monaco-host{flex:1;min-height:0;overflow:hidden}.file-statuses{border-top:1px solid var(--border);background:var(--panel-alt)}.file-statuses>div{display:flex;align-items:center;gap:15px;min-width:0;padding:7px 14px;font-size:10px;color:var(--muted)}.file-statuses>div:first-child{border-right:1px solid var(--border)}.cursor-info{margin-left:auto;white-space:nowrap}.comparison-summary{display:flex;align-items:center;gap:15px;padding:9px 14px;min-height:36px;font-size:11px;border-top:1px solid var(--border)}.equal-status{display:flex;align-items:center;gap:5px;color:var(--green)}.stat.added{color:var(--green)}.stat.removed{color:var(--red)}.stat.modified{color:var(--amber)}.muted,.status-note{color:var(--muted)}.status-note{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.rule-badge{font-size:10px;background:var(--panel-alt);color:var(--muted);padding:3px 6px;border-radius:3px}.compare-error{padding:10px 16px;color:var(--red);background:color-mix(in srgb,var(--red) 8%,var(--panel));font-size:12px;border-bottom:1px solid var(--border);white-space:pre-wrap}.mixed-note{padding:8px 16px;color:var(--amber);font-size:11px;background:color-mix(in srgb,var(--amber) 7%,var(--panel));line-height:1.5;border-bottom:1px solid var(--border)}
@media(max-width:850px){.text-tools{gap:3px;padding:7px}.syntax-select{width:90px}.file-header{padding:10px 8px;gap:5px}.side-mark{display:none}.file-statuses>div{padding:6px 8px;gap:8px}.comparison-summary{flex-wrap:wrap;gap:8px}.status-note{display:none}.rules-panel{gap:10px}.small{padding:4px 6px}}
</style>
