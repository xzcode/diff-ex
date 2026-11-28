<script setup lang="ts">
import { computed,onBeforeUnmount,ref,watch } from 'vue'
import { Archive,File,Folder,Link,RefreshCw,Search,Square,ChevronLeft,ChevronRight } from 'lucide-vue-next'
import type { ArchiveEntry,ArchiveResult,Language } from '../contracts'
import { compareArchives,displaySize,type AssetReader } from '../lib/asset-io'
import { cancelJob,isDesktop } from '../lib/bridge'
import { translate } from '../lib/i18n'
const props=defineProps<{left?:AssetReader;right?:AssetReader;language:Language}>()
const t=(key:string,...args:(string|number)[])=>translate(props.language,key,...args)
const result=ref<ArchiveResult>(),error=ref(''),busy=ref(false),search=ref(''),filter=ref('all'),page=ref(0),selected=ref<ArchiveEntry>()
const PAGE=100
let generation=0,jobId=''
const statuses=['same','different','leftOnly','rightOnly','conflict','error'] as const
const statusText=(status:ArchiveEntry['status'])=>t(({same:'Identical',different:'Changed',leftOnly:'Left only',rightOnly:'Right only',conflict:'Type conflict',error:'Read error'})[status])
const counts=computed(()=>{const n={same:0,different:0,leftOnly:0,rightOnly:0,conflict:0,error:0};for(const e of result.value?.entries||[])n[e.status]++;return n})
const filtered=computed(()=>{const q=search.value.toLowerCase();return (result.value?.entries||[]).filter(e=>(filter.value==='all'||(filter.value==='changed'?e.status!=='same':filter.value===e.status))&&(!q||e.path.toLowerCase().includes(q)))})
const pages=computed(()=>Math.max(1,Math.ceil(filtered.value.length/PAGE)))
const visible=computed(()=>filtered.value.slice(page.value*PAGE,(page.value+1)*PAGE))
const paired=computed(()=>!!props.left?.ref.path&&!!props.right?.ref.path&&isDesktop)
function stop(){generation++;if(jobId)void cancelJob(jobId).catch(()=>{});jobId='';busy.value=false}
async function compare(){
  if(!paired.value||busy.value)return
  const id=++generation;jobId=crypto.randomUUID();busy.value=true;result.value=undefined;error.value='';selected.value=undefined
  try{const next=await compareArchives(props.left!.ref,props.right!.ref,jobId);if(id===generation)result.value=next}
  catch(e){if(id===generation)error.value=String(e)}
  finally{if(id===generation){busy.value=false;jobId=''}}
}
watch([()=>props.left,()=>props.right],()=>{stop();result.value=undefined;error.value='';selected.value=undefined})
watch([filter,search,result],()=>{page.value=0;selected.value=undefined})
onBeforeUnmount(stop)
const icon=(entry:ArchiveEntry)=>{const k=(entry.left||entry.right)?.kind;return k==='directory'?Folder:k==='link'?Link:File}
</script>
<template>
  <section class="asset-module archive-workspace">
    <div class="panel-toolbar asset-tools">
      <button class="btn primary" :disabled="!paired||busy" @click="compare"><RefreshCw :size="15" :class="{spinning:busy}"/>{{ t('Compare archives') }}</button><button v-if="busy" class="btn danger" @click="stop"><Square :size="13"/>{{ t('Cancel') }}</button>
      <label>{{ t('Filter status') }} <select v-model="filter"><option value="all">{{ t('All files') }}</option><option value="changed">{{ t('All differences') }}</option><option v-for="s in statuses" :key="s" :value="s">{{ statusText(s) }}</option></select></label>
      <label class="archive-search"><Search :size="14"/><input v-model="search" :placeholder="t('Filter by file path…')"/></label>
      <span v-if="result" class="muted">{{ result.leftFormat }} ↔ {{ result.rightFormat }}</span>
    </div>
    <div class="notice">{{ t('Compare decompressed file hashes without extracting. Links, special entries and unsafe names are not treated as equal.') }}</div>
    <div v-if="!isDesktop" class="notice">{{ t('Archive comparison requires the desktop app.') }}</div>
    <div v-if="error" class="error-banner" role="alert">{{ error }}</div>
    <div v-if="busy" class="asset-empty">{{ t('Comparing…') }}</div>
    <div v-else-if="!result" class="asset-empty"><Archive :size="32"/>{{ t('Open two files to compare.') }}</div>
    <template v-else>
      <div class="archive-counts"><span v-for="s in statuses" :key="s" :class="'archive-'+s">{{ statusText(s) }}: <b>{{ counts[s] }}</b></span></div>
      <div class="archive-table selectable">
        <div class="archive-row archive-heading"><span>{{ t('NAME / RELATIVE PATH') }}</span><span>{{ t('STATUS') }}</span><span>{{ t('LEFT SIZE') }}</span><span>{{ t('RIGHT SIZE') }}</span></div>
        <div v-for="entry in visible" :key="entry.path" class="archive-row archive-item" :class="{selected:selected?.path===entry.path}" tabindex="0" @click="selected=entry" @keydown.enter="selected=entry">
          <span class="archive-path" :title="entry.path"><component :is="icon(entry)" :size="15"/><code>{{ entry.path }}</code></span><span :class="'archive-'+entry.status">{{ statusText(entry.status) }}</span><span>{{ entry.left?displaySize(entry.left.size):'—' }}</span><span>{{ entry.right?displaySize(entry.right.size):'—' }}</span>
        </div>
        <div v-if="!visible.length" class="asset-empty">{{ t('No files match this filter.') }}</div>
      </div>
      <div v-if="selected" class="archive-detail"><strong>{{ selected.path }}</strong><div v-for="side in (['left','right'] as const)" :key="side"><b>{{ t(side==='left'?'LEFT':'RIGHT') }}</b><code>{{ selected[side]?.error||selected[side]?.fingerprint||selected[side]?.kind||t('Absent') }}</code></div></div>
      <footer class="asset-status"><span>{{ filtered.length }} {{ t('Rows') }}</span><button class="btn-icon" :disabled="page===0" :title="t('Previous page')" @click="page--"><ChevronLeft :size="17"/></button><span>{{ page+1 }} / {{ pages }}</span><button class="btn-icon" :disabled="page+1>=pages" :title="t('Next page')" @click="page++"><ChevronRight :size="17"/></button></footer>
    </template>
  </section>
</template>
<style scoped>
.archive-counts{display:flex;gap:16px;flex-wrap:wrap;font-size:11px;padding:10px 13px;background:var(--panel-alt);border-bottom:1px solid var(--border)}.archive-search{margin-left:auto}.archive-search input{width:170px}.archive-table{min-height:0;flex:1;overflow:auto}.archive-row{display:grid;grid-template-columns:minmax(230px,1fr) 140px 110px 110px;min-width:600px;gap:12px;align-items:center;padding:0 12px;min-height:35px;border-bottom:1px solid var(--border);font-size:12px}.archive-heading{position:sticky;top:0;background:var(--panel-alt);z-index:1;color:var(--muted);font-weight:600}.archive-item:hover,.archive-item.selected{background:color-mix(in srgb,var(--accent) 10%,var(--panel))}.archive-path{display:flex;align-items:center;min-width:0;gap:8px}.archive-path code{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.archive-same{color:var(--muted)}.archive-different,.archive-conflict{color:var(--amber)}.archive-leftOnly,.archive-error{color:var(--red)}.archive-rightOnly{color:var(--green)}.archive-detail{padding:10px 13px;font-size:11px;border-top:1px solid var(--border);display:flex;flex-direction:column;gap:7px;max-height:140px;overflow:auto;user-select:text}.archive-detail div{display:flex;gap:10px;align-items:start}.archive-detail code{overflow-wrap:anywhere}
</style>
