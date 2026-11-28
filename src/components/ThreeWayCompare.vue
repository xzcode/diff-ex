<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ArrowLeft, ArrowRight, Check, FileDiff, FolderOpen, RefreshCw, Save, Undo2 } from 'lucide-vue-next'
import type { CompareSession, Language, MergeChoice, MergeDraft, MergeResolution, MergeSegment, TextDocument } from '../contracts'
import { translate } from '../lib/i18n'
import { clone } from '../lib/workspace'
import { mergeText, planThreeWay } from '../lib/three-way'
import { confirmAction } from '../lib/bridge'
type Source = 'base'|'left'|'right'
const props=defineProps<{session:CompareSession;language:Language}>()
const emit=defineEmits<{'open-source':[side:Source];'change-source':[side:Source,text:string];'update-merge':[draft:MergeDraft];'save-result':[as:boolean]}>()
const t=(key:string,...args:(string|number)[])=>translate(props.language,key,...args)
const draft=computed(()=>props.session.merge!)
const sources:Source[]=['base','left','right']
const doc=(side:Source):TextDocument=>side==='base'?draft.value.base:props.session[side]
const title=(side:Source)=>t(({base:'Base version',left:'LEFT',right:'RIGHT'})[side])
const localError=ref('')
const plan=ref<MergeSegment[]>([])
watch([()=>draft.value.prepared,()=>draft.value.base.content,()=>props.session.left.content,()=>props.session.right.content],([prepared,base,left,right])=>{try{plan.value=prepared?planThreeWay(base,left,right):[]}catch(e){plan.value=[];localError.value=String(e)}})
const conflicts=computed(()=>plan.value.filter(s=>s.kind==='conflict'))
const autoCount=computed(()=>plan.value.filter(s=>s.kind!=='same'&&s.kind!=='conflict').length)
const unresolved=computed(()=>conflicts.value.filter(s=>!Object.hasOwn(draft.value.resolutions,s.id)).length)
const ready=computed(()=>draft.value.prepared&&!localError.value&&unresolved.value===0)
const selected=ref(0),editingManual=ref(false),manualValue=ref('')
try{if(draft.value.prepared)plan.value=planThreeWay(draft.value.base.content,props.session.left.content,props.session.right.content)}catch(e){localError.value=String(e)}
const current=computed(()=>conflicts.value[Math.min(selected.value,Math.max(0,conflicts.value.length-1))])
const currentChoice=computed(()=>current.value?draft.value.resolutions[current.value.id]:undefined)
function refreshManual(){
  editingManual.value=false
  manualValue.value=currentChoice.value?.choice==='manual'?currentChoice.value.text:current.value?.left||''
}
watch([current,currentChoice],refreshManual,{immediate:true})
function commit(next:MergeDraft){
  const text=mergeText(plan.value,next.resolutions)
  if(text!==null){next.output={...next.output,content:text};next.manual=false}
  emit('update-merge',next)
}
async function regenerate(){
  localError.value=''
  const snapshot=JSON.stringify(draft.value)
  const base=draft.value.base.content,left=props.session.left.content,right=props.session.right.content
  if((draft.value.prepared||draft.value.output.content!==draft.value.savedOutput)&&!(await confirmAction(t('Regenerate and replace the current merge result and conflict choices?'),t('Three-way merge'))))return
  if(snapshot!==JSON.stringify(draft.value)||base!==draft.value.base.content||left!==props.session.left.content||right!==props.session.right.content)return
  try{
    const segments=planThreeWay(base,left,right)
    const next=clone(draft.value)
    next.prepared=true;next.manual=false;next.resolutions={}
    // The result uses an independent UTF-8 document, never a source path.
    next.output={...next.output,content:mergeText(segments,{})??''}
    selected.value=0;refreshManual()
    emit('update-merge',next)
  }catch(e){localError.value=String(e)}
}
async function resolve(choice:MergeChoice,text=''){
  const segment=current.value
  if(!segment||!draft.value.prepared)return
  const snapshot=JSON.stringify(draft.value)
  if(draft.value.manual&&!(await confirmAction(t('Replace manual result edits with the resolved merge?'),t('Three-way merge'))))return
  if(snapshot!==JSON.stringify(draft.value)||current.value?.id!==segment.id)return
  const next=clone(draft.value)
  next.resolutions={...next.resolutions,[segment.id]:{choice,text}}
  commit(next)
  editingManual.value=false
  if(unresolved.value>1){const following=conflicts.value.findIndex((s,i)=>i>selected.value&&s.id!==segment.id&&!Object.hasOwn(next.resolutions,s.id));if(following>=0)selected.value=following}
}
async function unresolve(){
  const segment=current.value
  if(!segment)return
  const snapshot=JSON.stringify(draft.value)
  if(draft.value.manual&&!(await confirmAction(t('Replace manual result edits with the resolved merge?'),t('Three-way merge'))))return
  if(snapshot!==JSON.stringify(draft.value)||current.value?.id!==segment.id)return
  const next=clone(draft.value);delete next.resolutions[segment.id];next.manual=false;commit(next)
}
function editOutput(event:Event){emit('update-merge',{...draft.value,manual:true,output:{...draft.value.output,content:(event.target as HTMLTextAreaElement).value}})}
function metadata(field:'encoding'|'lineEnding'|'bom',value:string|boolean){
  emit('update-merge',{...draft.value,output:{...draft.value.output,[field]:value}})
}
function choiceText(choice:MergeResolution|undefined){return choice?t(({left:'Use left',right:'Use right',base:'Use base',manual:'Manual resolution'})[choice.choice]):t('Unresolved')}
function span(s:MergeSegment){return s.start===s.end?String(s.start):String(s.start+1)+'–'+String(s.end)}
</script>
<template>
  <section class="merge-workspace">
    <div class="panel-toolbar asset-tools">
      <FileDiff :size="19"/><strong>{{ t('Three-way merge') }}</strong>
      <button class="btn primary" @click="regenerate"><RefreshCw :size="15"/>{{ t('Generate merge') }}</button>
      <span v-if="draft.prepared" class="muted">{{ t('Automatic changes') }}: {{ autoCount }}</span>
      <span v-if="draft.prepared" :class="unresolved?'warning':'positive'">{{ t('Unresolved conflicts') }}: {{ unresolved }} / {{ conflicts.length }}</span>
      <span class="spacer"/>
      <button class="btn" :disabled="!ready" @click="emit('save-result',true)"><Save :size="15"/>{{ t('Save as') }}</button>
      <button class="btn primary" :disabled="!ready" @click="emit('save-result',false)"><Save :size="15"/>{{ t('Save file') }}</button>
    </div>
    <div class="notice">{{ t('Only independent or identical changes are merged automatically. Sources are never written by this workspace.') }}</div>
    <div v-if="localError" class="error-banner" role="alert">{{ localError }}</div>
    <div class="merge-sources">
      <div v-for="side in sources" :key="side" class="merge-source">
        <header><span class="side-mark">{{ title(side) }}</span><strong :title="doc(side).path||doc(side).name">{{ doc(side).name==='Untitled'?t('Untitled'):doc(side).name }}</strong><button class="btn-icon" :aria-label="title(side)+' · '+t('Open file')" :title="t('Open file')" @click="emit('open-source',side)"><FolderOpen :size="16"/></button></header>
        <textarea class="source-text" :data-merge-source="side" :aria-label="title(side)" spellcheck="false" :value="doc(side).content" @input="emit('change-source',side,($event.target as HTMLTextAreaElement).value)"/>
      </div>
    </div>
    <div v-if="!draft.prepared" class="asset-empty merge-empty">{{ t('Open or paste three versions, then generate a merge.') }}<span v-if="draft.output.content" class="warning">{{ t('The previous result is retained until you regenerate.') }}</span></div>
    <div v-else class="merge-lower">
      <div v-if="conflicts.length" class="conflict-panel">
        <div class="conflict-bar">
          <strong>{{ t('Conflicts') }} {{ Math.min(selected+1,conflicts.length) }} / {{ conflicts.length }}</strong>
          <button class="btn-icon" :disabled="selected===0" :title="t('Previous difference')" @click="selected--"><ArrowLeft :size="16"/></button>
          <button class="btn-icon" :disabled="selected+1>=conflicts.length" :title="t('Next difference')" @click="selected++"><ArrowRight :size="16"/></button>
          <select v-model.number="selected" :aria-label="t('Conflicts')"><option v-for="(s,i) in conflicts" :key="s.id" :value="i">{{ i+1 }} · {{ span(s) }} · {{ choiceText(draft.resolutions[s.id]) }}</option></select>
          <span class="spacer"/><span :class="currentChoice?'positive':'warning'">{{ choiceText(currentChoice) }}</span>
        </div>
        <template v-if="current">
          <div class="conflict-versions">
            <div v-for="side in sources" :key="side"><strong>{{ title(side) }}</strong><pre>{{ current[side]||'∅' }}</pre><button class="btn" @click="resolve(side)">{{ t(({base:'Use base',left:'Use left',right:'Use right'})[side]) }}</button></div>
          </div>
          <div class="conflict-actions"><button class="btn" @click="editingManual=!editingManual">{{ t('Manual resolution') }}</button><button class="btn" :disabled="!currentChoice" @click="unresolve"><Undo2 :size="14"/>{{ t('Reset resolution') }}</button></div>
          <div v-if="editingManual" class="manual-resolution"><textarea v-model="manualValue" :aria-label="t('Manual resolution')" spellcheck="false"/><button class="btn primary" @click="resolve('manual',manualValue)"><Check :size="14"/>{{ t('Apply resolution') }}</button></div>
        </template>
      </div>
      <div class="merge-output">
        <header><strong>{{ t('Merge result') }}</strong><span class="muted">{{ draft.output.name }}</span><span class="spacer"/><label>{{ t('Save encoding') }} <select :value="draft.output.encoding" @change="metadata('encoding',($event.target as HTMLSelectElement).value)"><option v-for="enc in ['UTF-8','UTF-16LE','UTF-16BE','GBK','GB18030']" :key="enc">{{ enc }}</option></select></label><label>{{ t('Save line endings') }} <select :value="draft.output.lineEnding" @change="metadata('lineEnding',($event.target as HTMLSelectElement).value)"><option value="lf">LF</option><option value="crlf">CRLF</option><option value="cr">CR</option></select></label><label><input type="checkbox" :checked="draft.output.bom" @change="metadata('bom',($event.target as HTMLInputElement).checked)"/> BOM</label></header>
        <textarea v-if="ready" class="result-text" :aria-label="t('Merge result')" spellcheck="false" :value="draft.output.content" @input="editOutput"/>
        <div v-else class="asset-empty">{{ t('Resolve every conflict before editing or saving the result.') }}</div>
      </div>
    </div>
  </section>
</template>
<style scoped>
.merge-workspace{height:100%;min-height:0;display:flex;flex-direction:column;overflow:auto;background:var(--panel)}
.merge-workspace .spacer{flex:1}.merge-sources{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));min-height:140px;flex:0 0 35%;border-bottom:1px solid var(--border)}.merge-source{min-width:0;min-height:0;display:flex;flex-direction:column;border-right:1px solid var(--border)}.merge-source:last-child{border-right:0}
.merge-source header,.merge-output header{display:flex;align-items:center;gap:8px;min-height:38px;padding:6px 10px;background:var(--panel-alt);border-bottom:1px solid var(--border);font-size:11px}.merge-source header strong{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.side-mark{font-size:10px;white-space:nowrap;color:var(--muted);font-weight:600}
.source-text,.result-text{flex:1;min-height:0;border:0;border-radius:0;resize:none;padding:10px;background:var(--panel);font:13px/1.55 var(--mono);white-space:pre;overflow:auto;tab-size:4}
.merge-empty{min-height:120px}.merge-lower{display:flex;flex:1;flex-direction:column;min-height:240px;overflow:auto}.conflict-panel{border-bottom:1px solid var(--border);flex-shrink:0;background:var(--panel-alt)}.conflict-bar,.conflict-actions{display:flex;align-items:center;gap:6px;padding:7px 10px;font-size:11px;flex-wrap:wrap}.conflict-bar select{min-height:28px;max-width:260px}
.conflict-versions{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:1px;background:var(--border)}.conflict-versions>div{min-width:0;display:flex;flex-direction:column;padding:8px 10px;gap:8px;background:var(--panel);font-size:11px}.conflict-versions pre{max-height:120px;overflow:auto;user-select:text;white-space:pre-wrap;overflow-wrap:anywhere;min-height:35px;flex:1;margin:0;font:12px/1.5 var(--mono)}.conflict-versions button{align-self:flex-start;height:28px;min-height:28px}.manual-resolution{display:flex;gap:8px;padding:8px}.manual-resolution textarea{flex:1;font:12px var(--mono);min-height:65px}.merge-output{display:flex;flex:1;min-height:160px;flex-direction:column}.merge-output header{flex-wrap:wrap}.merge-output header label{display:flex;align-items:center;gap:5px;color:var(--muted)}.merge-output header select{font-size:11px;min-height:28px;padding:3px 6px}.result-text{color:var(--text)}@media(max-width:850px){.merge-source header{gap:3px;padding:4px}.merge-source .side-mark{display:none}.conflict-versions>div{padding:5px}}
</style>
