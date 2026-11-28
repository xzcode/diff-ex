<script setup lang="ts">
import {computed,onBeforeUnmount,ref,watch} from 'vue'
import {invoke} from '@tauri-apps/api/core'
import {ListChecks,FolderOpen,Plus,Play,Square,Trash2,Download} from 'lucide-vue-next'
import type {BatchItem,CompareSession,Language,Side} from '../contracts'
import {browsePath} from '../lib/version-io'
import {cancelJob,isDesktop} from '../lib/bridge'
import {translate} from '../lib/i18n'
import {clone} from '../lib/workspace'
import {exportReport} from '../lib/report'
interface TaskResult{id:string;kind:string;left:string;right:string;status:string;differences:number;details:unknown;error:string|null}
const props=defineProps<{session:CompareSession;language:Language}>()
const emit=defineEmits<{'update-items':[items:BatchItem[]]}>()
const t=(k:string,...args:(string|number)[])=>translate(props.language,k,...args)
const items=computed(()=>props.session.batchItems||[])
const kinds:BatchItem['kind'][]=['text','structured','binary','folder','archive','document']
const label=(kind:BatchItem['kind'])=>t(({text:'Text compare',structured:'Structured compare',binary:'Binary compare',folder:'Folder compare',archive:'Archive compare',document:'Document compare'})[kind])
const statusLabel=(status:string)=>t(({equal:'Identical',different:'Changed',error:'Read error',incomplete:'Unavailable'})[status]||status)
const results=ref<TaskResult[]>([]),busy=ref(false),complete=ref(false),error=ref(''),reporting=ref(false),selected=ref<TaskResult>()
const position=ref(0),activeId=ref('')
let epoch=0,jobId='',alive=true
const ready=computed(()=>isDesktop&&items.value.length>0&&items.value.every(v=>v.left.trim()&&v.right.trim()))
function patch(id:string,change:Partial<BatchItem>){emit('update-items',items.value.map(v=>v.id===id?{...v,...change}:v))}
function add(){emit('update-items',[...items.value,{id:crypto.randomUUID(),kind:'text',left:'',right:'',format:'json'}])}
function remove(id:string){emit('update-items',items.value.filter(v=>v.id!==id))}
async function browse(item:BatchItem,side:Side){try{const path=await browsePath(item.kind==='folder');if(alive&&path&&items.value.some(v=>v.id===item.id))patch(item.id,{[side]:path})}catch(e){error.value=String(e)}}
function stop(){epoch++;if(jobId)void cancelJob(jobId).catch(()=>{});jobId='';busy.value=false;complete.value=false;activeId.value=''}
watch(()=>JSON.stringify(items.value),()=>{stop();results.value=[];selected.value=undefined;error.value='';position.value=0})
async function run(){
  if(!ready.value||busy.value)return
  const tasks=clone(items.value),id=++epoch;busy.value=true;complete.value=false;results.value=[];position.value=0;error.value='';selected.value=undefined
  try{
    for(const task of tasks){
      if(id!==epoch||!alive)return
      activeId.value=task.id;jobId=crypto.randomUUID()
      try{
        const result=await invoke<TaskResult>('run_batch_task',{task,jobId})
        if(id!==epoch||!alive)return
        results.value.push(result)
      }catch(e){if(id!==epoch||!alive)return;results.value.push({...task,status:'error',differences:0,error:String(e),details:null})}
      position.value++
    }
    if(id===epoch)complete.value=true
  }finally{if(id===epoch){busy.value=false;jobId='';activeId.value=''}}
}
const counts=computed(()=>({equal:results.value.filter(r=>r.status==='equal').length,different:results.value.filter(r=>r.status==='different').length,error:results.value.filter(r=>r.status==='error'||r.status==='incomplete').length}))
async function save(format:'json'|'html',manifest=false){
  if(reporting.value||(!manifest&&!results.value.length))return
  reporting.value=true;error.value=''
  try{await exportReport(manifest?'diff-ex-batch':'diff-ex-report',manifest?{version:1,tasks:clone(items.value)}:{version:1,complete:complete.value,results:clone(results.value)},format,path=>t('Replace the existing file?\n{0}',path),items.value.filter(v=>v.kind!=='folder').flatMap(v=>[v.left,v.right]).filter(Boolean))}
  catch(e){error.value=String(e)}finally{reporting.value=false}
}
onBeforeUnmount(()=>{alive=false;stop()})
</script>
<template>
<section class="asset-module batch-workspace">
  <div class="panel-toolbar asset-tools"><button class="btn" :disabled="busy" @click="add"><Plus :size="15"/>{{ t('Add task') }}</button><button class="btn primary" :disabled="!ready||busy" @click="run"><Play :size="15"/>{{ t('Run batch') }}</button><button v-if="busy" class="btn danger" @click="stop"><Square :size="14"/>{{ t('Cancel') }}</button><span>{{ position }} / {{ items.length }}</span><span class="spacer"/><button class="btn" :disabled="!items.length||reporting" @click="save('json',true)"><Download :size="14"/>{{ t('Export manifest') }}</button><button class="btn" :disabled="!results.length||reporting" @click="save('json')">JSON</button><button class="btn" :disabled="!results.length||reporting" @click="save('html')">HTML</button></div>
  <div class="notice">{{ t('Tasks run sequentially and read only. Export a reusable manifest for the standalone CLI or a combined report.') }}</div>
  <div v-if="!isDesktop" class="notice">{{ t('Batch execution requires the desktop app.') }}</div>
  <div v-if="error" class="error-banner" role="alert">{{ error }}</div>
  <div class="batch-tasks">
    <div v-for="(item,index) in items" :key="item.id" class="batch-task" :class="{current:item.id===activeId}">
      <span class="task-number">{{ index+1 }}</span>
      <div class="task-kind"><select :disabled="busy" :value="item.kind" @change="patch(item.id,{kind:($event.target as HTMLSelectElement).value as BatchItem['kind']})"><option v-for="k in kinds" :key="k" :value="k">{{ label(k) }}</option></select><select v-if="item.kind==='structured'" :disabled="busy" :value="item.format" @change="patch(item.id,{format:($event.target as HTMLSelectElement).value as BatchItem['format']})"><option value="json">JSON</option><option value="yaml">YAML</option><option value="xml">XML</option></select></div>
      <div class="task-paths"><label v-for="side in (['left','right'] as const)" :key="side"><span>{{ t(side==='left'?'LEFT':'RIGHT') }}</span><input :data-batch-path="side" :disabled="busy" :value="item[side]" @input="patch(item.id,{[side]:($event.target as HTMLInputElement).value})"/><button class="btn-icon" :disabled="busy||!isDesktop" @click="browse(item,side)"><FolderOpen :size="14"/></button></label></div>
      <button class="btn-icon" :disabled="busy" :title="t('Remove task')" @click="remove(item.id)"><Trash2 :size="15"/></button>
    </div>
    <div v-if="!items.length" class="asset-empty"><ListChecks :size="30"/>{{ t('Add comparisons to this batch.') }}</div>
  </div>
  <div v-if="results.length" class="batch-results"><div class="asset-status"><strong>{{ t('Batch results') }}</strong><span>{{ t('Identical') }} {{ counts.equal }}</span><span>{{ t('Changed') }} {{ counts.different }}</span><span>{{ t('Read error') }} / {{ t('Unavailable') }} {{ counts.error }}</span><span v-if="!complete" class="warning">{{ t('Partial report') }}</span></div><div class="result-scroll selectable"><div v-for="r in results" :key="r.id" class="batch-result" :class="{selected:selected===r}" tabindex="0" @click="selected=r" @keydown.enter="selected=r"><span :title="r.left">{{ r.left }}</span><span :title="r.right">{{ r.right }}</span><strong :class="'status-'+r.status">{{ statusLabel(r.status) }}</strong><span>{{ r.differences }}</span></div></div><pre v-if="selected" class="batch-detail selectable">{{ JSON.stringify(selected,null,2) }}</pre></div>
</section>
</template>
<style scoped>
.spacer{flex:1}.batch-tasks{flex:1;min-height:120px;overflow:auto}.batch-task{display:grid;grid-template-columns:26px minmax(110px,160px) minmax(180px,1fr) 34px;gap:10px;align-items:center;padding:10px 12px;border-bottom:1px solid var(--border)}.batch-task.current{background:color-mix(in srgb,var(--accent) 10%,var(--panel))}.task-number{color:var(--muted);font:12px var(--mono)}.task-kind,.task-paths{display:flex;flex-direction:column;gap:5px;min-width:0}.task-paths label{display:flex;align-items:center;gap:6px;font-size:10px;color:var(--muted)}.task-paths label span{width:40px}.task-paths input{flex:1;min-width:0;font-size:12px}.batch-results{flex:0 1 48%;min-height:100px;display:flex;flex-direction:column;overflow:hidden;border-top:1px solid var(--border)}.result-scroll{overflow:auto;min-height:40px}.batch-result{display:grid;grid-template-columns:1fr 1fr 110px 70px;gap:8px;padding:8px 12px;border-bottom:1px solid var(--border);font-size:12px}.batch-result>*{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.batch-result.selected,.batch-result:hover{background:color-mix(in srgb,var(--accent) 12%,var(--panel))}.status-equal{color:var(--green)}.status-different{color:var(--amber)}.status-error,.status-incomplete{color:var(--red)}.batch-detail{max-height:150px;min-height:60px;overflow:auto;white-space:pre-wrap;overflow-wrap:anywhere;font:11px var(--mono);padding:10px;margin:0;border-top:1px solid var(--border);user-select:text}
</style>
