<script setup lang="ts">
import {computed,ref,watch} from 'vue'
import {ChevronLeft,ChevronRight,File,Folder,Link,Search} from 'lucide-vue-next'
import type {EntryStatus,Language,SnapshotEntry} from '../contracts'
import {displaySize} from '../lib/asset-io'
import {translate} from '../lib/i18n'
const props=defineProps<{entries:SnapshotEntry[];language:Language;busy:boolean}>()
const emit=defineEmits<{'open-entry':[entry:SnapshotEntry,encoding:string|null]}>()
const t=(key:string)=>translate(props.language,key)
const statuses=['same','different','leftOnly','rightOnly','conflict','error'] as const
const label=(status:EntryStatus)=>t(({same:'Identical',different:'Changed',leftOnly:'Left only',rightOnly:'Right only',conflict:'Type conflict',error:'Read error'})[status])
const filter=ref('changed'),search=ref(''),page=ref(0),encoding=ref('auto'),selected=ref<SnapshotEntry>()
const PAGE=100
const counts=computed(()=>{const n={same:0,different:0,leftOnly:0,rightOnly:0,conflict:0,error:0};for(const e of props.entries)n[e.status]++;return n})
const filtered=computed(()=>{const q=search.value.toLowerCase();return props.entries.filter(e=>(filter.value==='all'||(filter.value==='changed'?e.status!=='same':e.status===filter.value))&&(!q||e.path.toLowerCase().includes(q)))})
const pages=computed(()=>Math.max(1,Math.ceil(filtered.value.length/PAGE)))
const visible=computed(()=>filtered.value.slice(page.value*PAGE,(page.value+1)*PAGE))
watch([()=>props.entries,filter,search],()=>{page.value=0;selected.value=undefined})
const readable=(entry:SnapshotEntry|undefined)=>!!entry&&entry.status!=='error'&&entry.status!=='conflict'&&[entry.left,entry.right].every(item=>!item||(item.kind==='file'&&!!item.fingerprint))
const icon=(entry:SnapshotEntry)=>{const kind=(entry.left||entry.right)?.kind;return kind==='directory'?Folder:kind==='link'?Link:File}
function open(entry:SnapshotEntry){if(!props.busy&&readable(entry))emit('open-entry',entry,encoding.value==='auto'?null:encoding.value)}
</script>
<template>
  <section class="snapshot-list">
    <div class="panel-toolbar asset-tools">
      <label>{{ t('Filter status') }} <select v-model="filter"><option value="all">{{ t('All files') }}</option><option value="changed">{{ t('All differences') }}</option><option v-for="s in statuses" :key="s" :value="s">{{ label(s) }} ({{ counts[s] }})</option></select></label>
      <label><Search :size="14"/><input v-model="search" :placeholder="t('Filter by file path…')"/></label>
      <span class="spacer"/><label>{{ t('Open encoding') }} <select v-model="encoding"><option value="auto">{{ t('Auto') }}</option><option v-for="enc in ['UTF-8','UTF-16LE','UTF-16BE','GBK','GB18030']" :key="enc">{{ enc }}</option></select></label><button class="btn primary" :disabled="busy||!readable(selected)" @click="selected&&open(selected)">{{ t('Open text comparison') }}</button>
    </div>
    <div class="snapshot-counts"><span v-for="s in statuses" :key="s" :class="'snapshot-'+s">{{ label(s) }}: <b>{{ counts[s] }}</b></span></div>
    <div class="snapshot-scroll selectable">
      <div class="snapshot-row snapshot-heading"><span>{{ t('NAME / RELATIVE PATH') }}</span><span>{{ t('STATUS') }}</span><span>{{ t('LEFT SIZE') }}</span><span>{{ t('RIGHT SIZE') }}</span></div>
      <div v-for="e in visible" :key="e.path" class="snapshot-row snapshot-item" :class="{selected:selected?.path===e.path}" tabindex="0" @click="selected=e" @dblclick="open(e)" @keydown.enter="open(e)">
        <span class="snapshot-path" :title="e.path"><component :is="icon(e)" :size="15"/><code>{{ e.path }}</code></span><span :class="'snapshot-'+e.status">{{ label(e.status) }}</span><span>{{ e.left?displaySize(e.left.size):'—' }}</span><span>{{ e.right?displaySize(e.right.size):'—' }}</span>
      </div><div v-if="!visible.length" class="asset-empty">{{ t('No files match this filter.') }}</div>
    </div>
    <div v-if="selected" class="snapshot-detail selectable"><strong>{{ selected.path }}</strong><div v-for="side in (['left','right'] as const)" :key="side"><b>{{ t(side==='left'?'LEFT':'RIGHT') }}</b><code>{{ selected[side]?.error||selected[side]?.fingerprint||selected[side]?.kind||t('Absent') }}</code><span v-if="selected[side]?.mode">{{ selected[side]?.mode }}</span></div></div>
    <footer class="asset-status"><span>{{ filtered.length }} {{ t('Rows') }}</span><button class="btn-icon" :disabled="page===0" :title="t('Previous page')" @click="page--"><ChevronLeft :size="17"/></button><span>{{ page+1 }} / {{ pages }}</span><button class="btn-icon" :disabled="page+1>=pages" :title="t('Next page')" @click="page++"><ChevronRight :size="17"/></button></footer>
  </section>
</template>
<style scoped>
.snapshot-list{flex:1;min-height:0;display:flex;flex-direction:column;overflow:hidden}.spacer{flex:1}.snapshot-counts{display:flex;gap:15px;flex-wrap:wrap;padding:9px 12px;background:var(--panel-alt);font-size:11px;border-bottom:1px solid var(--border)}.snapshot-scroll{flex:1;min-height:0;overflow:auto}.snapshot-row{display:grid;grid-template-columns:minmax(180px,1fr) 130px 110px 110px;min-width:550px;gap:12px;align-items:center;min-height:35px;padding:0 12px;border-bottom:1px solid var(--border);font-size:12px}.snapshot-heading{position:sticky;top:0;z-index:1;background:var(--panel-alt);font-weight:600;color:var(--muted)}.snapshot-path{min-width:0;display:flex;gap:8px;align-items:center}.snapshot-path code{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.snapshot-item:hover,.snapshot-item.selected{background:color-mix(in srgb,var(--accent) 12%,var(--panel))}.snapshot-same{color:var(--muted)}.snapshot-different,.snapshot-conflict{color:var(--amber)}.snapshot-error,.snapshot-leftOnly{color:var(--red)}.snapshot-rightOnly{color:var(--green)}.snapshot-detail{padding:8px 12px;border-top:1px solid var(--border);display:flex;flex-direction:column;gap:7px;font-size:11px;max-height:130px;overflow:auto}.snapshot-detail>div{display:flex;gap:10px}.snapshot-detail code{overflow-wrap:anywhere}
</style>
