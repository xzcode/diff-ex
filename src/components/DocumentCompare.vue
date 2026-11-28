<script setup lang="ts">
import {computed,nextTick,onBeforeUnmount,ref,shallowRef,watch} from 'vue'
import {FileText,RefreshCw,Square,ChevronLeft,ChevronRight,ScanLine,Columns2,Search} from 'lucide-vue-next'
import type {DocumentData,DocUnit,Language,Side,TextDocument} from '../contracts'
import type {AssetReader} from '../lib/asset-io'
import {readDocument,renderPdf} from '../lib/version-io'
import {compareDocuments,differs,type DocumentRow} from '../lib/document-compare'
import {cancelJob,isDesktop} from '../lib/bridge'
import {translate} from '../lib/i18n'
import {newDocument} from '../lib/workspace'
import {compareRaster,rasterCanvas,type Raster,type ImageStats} from '../lib/image-diff'
const props=defineProps<{left?:AssetReader;right?:AssetReader;language:Language}>()
const emit=defineEmits<{'open-pair':[left:TextDocument|null,right:TextDocument|null,title:string]}>()
const t=(key:string)=>translate(props.language,key)
const a=shallowRef<DocumentData>(),b=shallowRef<DocumentData>()
const error=ref(''),renderError=ref(''),busy=ref(false),rendering=ref(false)
const filter=ref('changed'),search=ref(''),page=ref(0),selected=ref<DocumentRow>()
const PAGE=100
const rows=computed(()=>a.value&&b.value?compareDocuments(a.value,b.value):[])
const filtered=computed(()=>{const q=search.value.toLowerCase();return rows.value.filter(r=>(filter.value==='all'||differs(r))&&(!q||[r.left?.label,r.right?.label,r.left?.text,r.right?.text].some(v=>v?.toLowerCase().includes(q))))})
const pages=computed(()=>Math.max(1,Math.ceil(filtered.value.length/PAGE)))
const visible=computed(()=>filtered.value.slice(page.value*PAGE,(page.value+1)*PAGE))
const counts=computed(()=>({text:rows.value.filter(r=>r.textStatus==='different'||r.textStatus==='leftOnly'||r.textStatus==='rightOnly').length,layout:rows.value.filter(r=>r.layoutStatus==='different').length,unknown:rows.value.filter(r=>r.textStatus==='unknown'||r.layoutStatus==='unknown').length}))
const canCompare=computed(()=>isDesktop&&!!props.left?.ref.path&&!!props.right?.ref.path)
const canRender=computed(()=>!!selected.value?.left?.page&&!!selected.value?.right?.page&&a.value?.format==='PDF'&&b.value?.format==='PDF'&&canCompare.value)
let generation=0,jobIds:string[]=[],renderGeneration=0,renderIds:string[]=[]
const dpi=ref(96),tolerance=ref(0),mode=ref<'side'|'heat'>('side')
const leftRaster=shallowRef<Raster>(),rightRaster=shallowRef<Raster>(),imageStats=shallowRef<ImageStats>()
const leftCanvas=ref<HTMLCanvasElement>(),rightCanvas=ref<HTMLCanvasElement>(),heatCanvas=ref<HTMLCanvasElement>()
function stopLoad(){generation++;for(const id of jobIds)void cancelJob(id).catch(()=>{});jobIds=[];busy.value=false}
function stopRender(){renderGeneration++;for(const id of renderIds)void cancelJob(id).catch(()=>{});renderIds=[];rendering.value=false}
function clearRender(){stopRender();leftRaster.value=undefined;rightRaster.value=undefined;imageStats.value=undefined;renderError.value=''}
watch([()=>props.left,()=>props.right],()=>{stopLoad();clearRender();a.value=undefined;b.value=undefined;error.value='';selected.value=undefined})
watch([rows,filter,search],()=>{page.value=0;selected.value=undefined})
watch([selected,dpi],clearRender)
async function compare(){
  if(!canCompare.value||busy.value)return
  const id=++generation;busy.value=true;error.value='';a.value=undefined;b.value=undefined;selected.value=undefined
  jobIds=[crypto.randomUUID(),crypto.randomUUID()]
  try{
    const [l,r]=await Promise.allSettled([readDocument(props.left!.ref,jobIds[0]!),readDocument(props.right!.ref,jobIds[1]!)])
    if(id!==generation)return
    if(l.status==='fulfilled')a.value=l.value
    if(r.status==='fulfilled')b.value=r.value
    error.value=[l.status==='rejected'?t('LEFT')+': '+String(l.reason):'',r.status==='rejected'?t('RIGHT')+': '+String(r.reason):''].filter(Boolean).join('\n')
  }finally{if(id===generation){busy.value=false;jobIds=[]}}
}
function unitDocument(unit:DocUnit|null,side:Side):TextDocument|null{
  if(!unit)return null
  return {...newDocument(),name:(side==='left'?props.left?.ref.name:props.right?.ref.name)+' · '+unit.label,content:unit.text,byteLength:new TextEncoder().encode(unit.text).length}
}
function openText(){const r=selected.value;if(r)emit('open-pair',unitDocument(r.left,'left'),unitDocument(r.right,'right'),r.left?.label||r.right?.label||t('Document compare'))}
async function pngRaster(raw:ArrayBuffer|number[]):Promise<Raster>{
  const bytes=Uint8Array.from(new Uint8Array(raw as ArrayBuffer))
  const bitmap=await createImageBitmap(new Blob([bytes.buffer],{type:'image/png'}))
  try{const canvas=document.createElement('canvas');canvas.width=bitmap.width;canvas.height=bitmap.height;const ctx=canvas.getContext('2d',{willReadFrequently:true});if(!ctx)throw Error('Canvas allocation failed');ctx.drawImage(bitmap,0,0);return {width:canvas.width,height:canvas.height,data:ctx.getImageData(0,0,canvas.width,canvas.height).data}}finally{bitmap.close()}
}
function draw(target:HTMLCanvasElement|undefined,r:Raster|undefined){if(!target||!r)return;target.width=r.width;target.height=r.height;target.getContext('2d')?.drawImage(rasterCanvas(r),0,0)}
async function paint(){
  await nextTick()
  if(mode.value==='side'){draw(leftCanvas.value,leftRaster.value);draw(rightCanvas.value,rightRaster.value)}
  else if(imageStats.value){const s=imageStats.value;draw(heatCanvas.value,{width:s.width,height:s.height,data:s.heat})}
}
function recalculate(){if(!leftRaster.value||!rightRaster.value)return;try{imageStats.value=compareRaster(leftRaster.value,rightRaster.value,tolerance.value);void paint()}catch(e){renderError.value=String(e)}}
watch(tolerance,recalculate);watch(mode,()=>void paint())
async function render(){
  if(!canRender.value||rendering.value)return
  clearRender();const id=++renderGeneration;rendering.value=true
  const l=selected.value!.left!.page!,r=selected.value!.right!.page!
  renderIds=[crypto.randomUUID(),crypto.randomUUID()]
  try{
    const [a,b]=await Promise.all([renderPdf(props.left!.ref,l,dpi.value,renderIds[0]!),renderPdf(props.right!.ref,r,dpi.value,renderIds[1]!)])
    const [x,y]=await Promise.all([pngRaster(a),pngRaster(b)])
    if(id!==renderGeneration)return
    leftRaster.value=x;rightRaster.value=y;recalculate()
  }catch(e){if(id===renderGeneration)renderError.value=String(e)}
  finally{if(id===renderGeneration){rendering.value=false;renderIds=[]}}
}
const textStatus=(r:DocumentRow)=>t(({same:'Identical',different:'Changed',leftOnly:'Left only',rightOnly:'Right only',unknown:'Unavailable'})[r.textStatus])
const layoutStatus=(r:DocumentRow)=>t(({same:'Identical',different:'Changed',unknown:'Unavailable'})[r.layoutStatus])
onBeforeUnmount(()=>{stopLoad();clearRender()})
</script>
<template>
  <section class="asset-module document-workspace">
    <div class="panel-toolbar asset-tools"><button class="btn primary" :disabled="!canCompare||busy" @click="compare"><RefreshCw :size="15"/>{{ t('Compare documents') }}</button><button v-if="busy" class="btn danger" @click="stopLoad"><Square :size="14"/>{{ t('Cancel') }}</button><label>{{ t('Filter status') }} <select v-model="filter"><option value="changed">{{ t('All differences') }}</option><option value="all">{{ t('All files') }}</option></select></label><label><Search :size="14"/><input v-model="search" :placeholder="t('Search document text')"/></label></div>
    <div class="notice">{{ t('Text extraction and structural signatures are not a guarantee of identical rendered appearance. Scanned PDFs may require OCR.') }}</div>
    <div v-if="!isDesktop" class="notice">{{ t('Document comparison requires the desktop app.') }}</div>
    <div v-if="error" class="error-banner" role="alert">{{ error }}</div>
    <div v-if="busy" class="asset-empty">{{ t('Comparing…') }}</div>
    <div v-else-if="!a||!b" class="asset-empty"><FileText :size="32"/>{{ t('Open two files to compare.') }}</div>
    <template v-else>
      <div class="document-summary"><span>{{ a.format }} ↔ {{ b.format }}</span><strong>{{ t('Text differences') }}: {{ counts.text }}</strong><strong>{{ t('Layout differences') }}: {{ counts.layout }}</strong><span>{{ t('Unavailable') }}: {{ counts.unknown }}</span><span>{{ t('File bytes') }}: {{ a.fingerprint===b.fingerprint?t('Identical'):t('Changed') }}</span></div>
      <div class="document-list selectable">
        <div class="document-row doc-heading"><span>{{ t('LEFT') }}</span><span>{{ t('RIGHT') }}</span><span>{{ t('Text') }}</span><span>{{ t('Layout') }}</span></div>
        <div v-for="r in visible" :key="r.id" class="document-row" :class="{selected:selected?.id===r.id}" tabindex="0" @click="selected=r" @keydown.enter="selected=r"><span :title="r.left?.label">{{ r.left?.label||'—' }}</span><span :title="r.right?.label">{{ r.right?.label||'—' }}</span><span :class="'doc-'+r.textStatus">{{ textStatus(r) }}</span><span :class="'doc-'+r.layoutStatus">{{ layoutStatus(r) }}</span></div>
        <div v-if="!visible.length" class="asset-empty">{{ t('No changes match this filter.') }}</div>
      </div>
      <div class="asset-status"><span>{{ filtered.length }} {{ t('Rows') }}</span><button class="btn-icon" :disabled="page===0" :title="t('Previous page')" @click="page--"><ChevronLeft :size="16"/></button><span>{{ page+1 }} / {{ pages }}</span><button class="btn-icon" :disabled="page+1>=pages" :title="t('Next page')" @click="page++"><ChevronRight :size="16"/></button></div>
      <div v-if="selected" class="document-detail">
        <header><strong>{{ selected.left?.label||'—' }} ↔ {{ selected.right?.label||'—' }}</strong><span class="spacer"/><button class="btn" @click="openText">{{ t('Open text comparison') }}</button><button class="btn" @click="selected=undefined">{{ t('Close change details') }}</button></header>
        <div class="document-panes"><div v-for="side in (['left','right'] as const)" :key="side"><strong>{{ t(side==='left'?'LEFT':'RIGHT') }}</strong><pre v-if="selected[side]?.warning" class="warning">{{ selected[side]?.warning }}</pre><pre class="document-text">{{ selected[side]?.text||'—' }}</pre><details><summary>{{ t('Layout') }}</summary><pre>{{ selected[side]?.layout||'—' }}</pre></details></div></div>
        <div v-if="canRender" class="render-tools"><button class="btn primary" :disabled="rendering" @click="render"><ScanLine :size="15"/>{{ t('Render PDF pages') }}</button><button v-if="rendering" class="btn danger" @click="stopRender">{{ t('Cancel') }}</button><label>DPI <select v-model.number="dpi"><option :value="96">96</option><option :value="144">144</option><option :value="200">200</option></select></label><span class="muted">{{ t('Requires local pdftoppm (Poppler).') }}</span></div>
        <div v-if="renderError" class="error-banner" role="alert">{{ renderError }}</div>
        <template v-if="imageStats">
          <div class="render-tools"><button class="btn" :class="{active:mode==='side'}" @click="mode='side'"><Columns2 :size="15"/>{{ t('Side by side') }}</button><button class="btn" :class="{active:mode==='heat'}" @click="mode='heat'">{{ t('Heatmap') }}</button><label>{{ t('Tolerance') }} <input type="range" min="0" max="255" v-model.number="tolerance"/> {{ tolerance }}</label><span>{{ t('Pixel differences') }}: {{ imageStats.changed+imageStats.leftOnly+imageStats.rightOnly }}</span></div>
          <div class="render-panes" :class="{single:mode==='heat'}"><template v-if="mode==='side'"><canvas ref="leftCanvas"/><canvas ref="rightCanvas"/></template><canvas v-else ref="heatCanvas"/></div>
        </template>
      </div>
    </template>
  </section>
</template>
<style scoped>
.document-summary{display:flex;flex-wrap:wrap;gap:16px;background:var(--panel-alt);font-size:11px;padding:10px 12px;border-bottom:1px solid var(--border)}.document-list{flex:1;min-height:100px;overflow:auto}.document-row{display:grid;grid-template-columns:1fr 1fr 110px 110px;gap:12px;padding:9px 12px;border-bottom:1px solid var(--border);font-size:12px;min-width:520px}.document-row>span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.doc-heading{position:sticky;top:0;background:var(--panel-alt);color:var(--muted);font-weight:600;z-index:1}.document-row.selected,.document-row:hover{background:color-mix(in srgb,var(--accent) 12%,var(--panel))}.doc-different,.doc-unknown{color:var(--amber)}.doc-leftOnly{color:var(--red)}.doc-rightOnly{color:var(--green)}.document-detail{max-height:62%;min-height:140px;overflow:auto;flex-shrink:0;border-top:1px solid var(--border)}.document-detail header,.render-tools{display:flex;align-items:center;gap:9px;flex-wrap:wrap;padding:8px 12px;background:var(--panel-alt);border-bottom:1px solid var(--border);font-size:11px}.spacer{flex:1}.document-panes{display:grid;grid-template-columns:1fr 1fr;gap:1px;background:var(--border)}.document-panes>div{min-width:0;padding:10px;background:var(--panel);font-size:11px}.document-panes pre{font:12px/1.5 var(--mono);white-space:pre-wrap;overflow-wrap:anywhere;max-height:140px;overflow:auto;user-select:text}.document-panes details{color:var(--muted)}.render-tools label{display:flex;align-items:center;gap:6px}.render-tools input{width:100px;min-height:20px}.render-panes{display:grid;grid-template-columns:1fr 1fr;gap:10px;padding:10px;background:var(--bg)}.render-panes.single{grid-template-columns:1fr}.render-panes canvas{max-width:100%;height:auto;min-width:0;justify-self:center;background:white}
</style>
