<script setup lang="ts">
import {computed,onBeforeUnmount,ref,shallowRef,watch} from 'vue'
import {ArrowDownToLine,ChevronLeft,ChevronRight,ScanLine,Square} from 'lucide-vue-next'
import type {AssetSettings,BinaryStats,Language} from '../contracts'
import {type AssetReader,emptyStats,countBytes,scanBytes} from '../lib/asset-io'
import {cancelJob} from '../lib/bridge'
import {translate} from '../lib/i18n'
const props=defineProps<{left?:AssetReader;right?:AssetReader;language:Language;settings:AssetSettings}>()
const emit=defineEmits<{'update-settings':[patch:Partial<AssetSettings>]}>()
const t=(key:string,...args:(string|number)[])=>translate(props.language,key,...args)
const leftPage=shallowRef<Uint8Array>(new Uint8Array()),rightPage=shallowRef<Uint8Array>(new Uint8Array())
const result=ref<BinaryStats>(),error=ref(''),scanError=ref(''),scanning=ref(false),loading=ref(false),scanned=ref(0)
const size=computed(()=>Math.max(props.left?.ref.size||0,props.right?.ref.size||0)),pageSize=computed(()=>props.settings.binaryPageSize)
const maxOffset=computed(()=>size.value?Math.floor((size.value-1)/pageSize.value)*pageSize.value:0)
const offset=computed(()=>Math.max(0,Math.min(maxOffset.value,Math.floor(props.settings.binaryOffset/pageSize.value)*pageSize.value)))
const hex=(n:number,width=2)=>n.toString(16).toUpperCase().padStart(width,'0')
const offsetText=ref('0')
watch(offset,n=>{offsetText.value='0x'+hex(n,8)},{immediate:true})
let pageEpoch=0,scanEpoch=0,jobId='',nativeJob=false
const paired=computed(()=>!!props.left&&!!props.right)
const pageRows=computed(()=>Array.from({length:Math.ceil(Math.max(leftPage.value.length,rightPage.value.length)/16)},(_,row)=>Array.from({length:16},(_,col)=>row*16+col)))
const pageStats=computed(()=>{const stats=emptyStats();countBytes(stats,leftPage.value,rightPage.value,offset.value);return stats})
function go(value:number){
  if(!Number.isSafeInteger(value)||value<0||value>Math.max(0,size.value-1)){error.value=t('Invalid offset');return}
  const at=Math.floor(value/pageSize.value)*pageSize.value
  error.value='';emit('update-settings',{binaryOffset:at});offsetText.value='0x'+hex(at,8)
}
function jump(){const value=offsetText.value.trim();if(!/^(?:0x[0-9a-f]+|[0-9]+)$/i.test(value)){error.value=t('Invalid offset');return}go(Number(value))}
function symbol(bytes:Uint8Array,index:number){const b=bytes[index];return b===undefined?' ':b>=32&&b<127?String.fromCharCode(b):'.'}
function kind(index:number,side:'left'|'right'){
  if(!paired.value)return ''
  const a=leftPage.value[index],b=rightPage.value[index]
  if(a===b)return ''
  if(a===undefined)return side==='right'?'added':'missing'
  if(b===undefined)return side==='left'?'removed':'missing'
  return 'changed'
}
async function loadPage(){
  const epoch=++pageEpoch;error.value='';loading.value=true;leftPage.value=new Uint8Array();rightPage.value=new Uint8Array()
  try{const [a,b]=await Promise.all([props.left?.read(offset.value,pageSize.value),props.right?.read(offset.value,pageSize.value)]);if(epoch===pageEpoch){leftPage.value=a||new Uint8Array();rightPage.value=b||new Uint8Array()}}
  catch(e){if(epoch===pageEpoch)error.value=String(e)}
  finally{if(epoch===pageEpoch)loading.value=false}
}
function stop(){scanEpoch++;if(jobId&&nativeJob)void cancelJob(jobId).catch(()=>{});jobId='';nativeJob=false;scanning.value=false}
async function scan(){
  if(!props.left||!props.right||scanning.value)return
  const epoch=++scanEpoch;scanning.value=true;scanned.value=0;result.value=undefined;scanError.value='';jobId=crypto.randomUUID();nativeJob=!!props.left.ref.path&&!!props.right.ref.path
  try{const r=await scanBytes(props.left,props.right,jobId,()=>epoch===scanEpoch,done=>{if(epoch===scanEpoch)scanned.value=done});if(epoch===scanEpoch){result.value=r;scanned.value=r.compared}}
  catch(e){if(epoch===scanEpoch)scanError.value=String(e)}
  finally{if(epoch===scanEpoch){scanning.value=false;jobId='';nativeJob=false}}
}
watch([()=>props.left,()=>props.right],()=>{stop();result.value=undefined;scanError.value=''})
watch([()=>props.left,()=>props.right,offset,pageSize],loadPage,{immediate:true})
onBeforeUnmount(()=>{pageEpoch++;stop()})
</script>
<template>
  <section class="asset-module binary-workspace">
    <div class="panel-toolbar asset-tools">
      <button class="btn primary" :disabled="!paired||scanning" @click="scan"><ScanLine :size="16"/>{{ t('Scan whole file') }}</button>
      <button v-if="scanning" class="btn danger" @click="stop"><Square :size="13"/>{{ t('Cancel') }}</button>
      <label>{{ t('Offset') }} <input v-model="offsetText" class="offset-input" :aria-label="t('Offset')" @keydown.enter="jump"/></label>
      <button class="btn" :disabled="!size" @click="jump">{{ t('Go to') }}</button>
      <label>{{ t('Page size') }} <select :value="pageSize" @change="emit('update-settings',{binaryPageSize:Number(($event.target as HTMLSelectElement).value)})"><option v-for="n in [256,512,1024,4096]" :key="n" :value="n">{{ n }} B</option></select></label>
      <button class="btn-icon" :disabled="!offset" :title="t('Previous page')" @click="go(Math.max(0,offset-pageSize))"><ChevronLeft :size="18"/></button>
      <button class="btn-icon" :disabled="offset>=maxOffset" :title="t('Next page')" @click="go(offset+pageSize)"><ChevronRight :size="18"/></button>
    </div>
    <div class="notice">{{ t('Bytes are compared at the same offset; insertions shift later bytes. Read only.') }}</div>
    <div v-if="error||scanError" class="error-banner" role="alert">{{ error||scanError }}</div>
    <div v-if="scanning" class="notice" role="status">{{ t('Comparing…') }} <span v-if="scanned">{{ scanned.toLocaleString(language) }} / {{ size.toLocaleString(language) }} B</span></div>
    <div v-if="result" class="binary-summary">
      <strong v-if="result.firstDifference===null" class="positive">{{ t('Contents are equal') }}</strong><span v-else>{{ t('Total byte differences') }}: <b>{{ (result.changed+result.leftOnly+result.rightOnly).toLocaleString(language) }}</b></span>
      <span class="warning">{{ t('Changed') }}: {{ result.changed.toLocaleString(language) }}</span><span class="negative">{{ t('Left only') }}: {{ result.leftOnly.toLocaleString(language) }}</span><span class="positive">{{ t('Right only') }}: {{ result.rightOnly.toLocaleString(language) }}</span>
      <button v-if="result.firstDifference!==null" class="btn" @click="go(result.firstDifference!)"><ArrowDownToLine :size="14"/>{{ t('First difference') }} · 0x{{ hex(result.firstDifference,8) }}</button><button v-if="result.lastDifference!==null" class="btn" @click="go(result.lastDifference!)">{{ t('Last difference') }} · 0x{{ hex(result.lastDifference,8) }}</button>
    </div>
    <div v-if="!left&&!right" class="asset-empty">{{ t('Open two files to compare.') }}</div><div v-else-if="loading" class="asset-empty">{{ t('Comparing…') }}</div><div v-else-if="!pageRows.length" class="asset-empty">{{ t('No bytes on this page.') }}</div>
    <div v-else class="hex-scroll selectable">
      <div class="hex-grid hex-heading"><span>{{ t('Offset') }}</span><span>{{ t('LEFT') }} · HEX</span><span>ASCII</span><span>{{ t('RIGHT') }} · HEX</span><span>ASCII</span></div>
      <div v-for="(row,index) in pageRows" :key="offset+index*16" class="hex-grid hex-row">
        <code class="hex-address">{{ hex(offset+index*16,8) }}</code>
        <div class="hex-bytes"><code v-for="i in row" :key="i" :class="kind(i,'left')">{{ leftPage[i]===undefined?'··':hex(leftPage[i]) }}</code></div><div class="hex-ascii"><code v-for="i in row" :key="i" :class="kind(i,'left')">{{ symbol(leftPage,i) }}</code></div>
        <div class="hex-bytes"><code v-for="i in row" :key="i" :class="kind(i,'right')">{{ rightPage[i]===undefined?'··':hex(rightPage[i]) }}</code></div><div class="hex-ascii"><code v-for="i in row" :key="i" :class="kind(i,'right')">{{ symbol(rightPage,i) }}</code></div>
      </div>
    </div>
    <footer class="asset-status"><span>{{ t('Current page') }}: 0x{{ hex(offset,8) }} – 0x{{ hex(Math.max(offset,Math.min(size,offset+pageSize)-1),8) }}</span><span v-if="paired&&!loading">{{ t('Byte differences on page') }}: {{ pageStats.changed+pageStats.leftOnly+pageStats.rightOnly }}</span><span>{{ size.toLocaleString(language) }} B</span></footer>
  </section>
</template>
<style scoped>
.offset-input{width:125px;font-family:var(--mono);font-size:12px}.binary-summary{display:flex;align-items:center;flex-wrap:wrap;gap:14px;padding:10px 12px;border-bottom:1px solid var(--border);background:var(--panel-alt);font-size:12px}.binary-summary .btn{font-size:11px}
.hex-scroll{flex:1;overflow:auto;background:var(--panel);min-height:0}.hex-grid{display:grid;grid-template-columns:minmax(84px,auto) repeat(2,1fr 16ch);gap:10px;align-items:center;width:max-content;min-width:100%;padding:0 12px;font:12px var(--mono)}
.hex-heading{position:sticky;top:0;z-index:1;height:36px;background:var(--panel-alt);color:var(--muted);border-bottom:1px solid var(--border)}
.hex-row{height:26px;border-bottom:1px solid color-mix(in srgb,var(--border) 48%,transparent)}.hex-row:nth-child(even){background:color-mix(in srgb,var(--panel-alt) 24%,var(--panel))}
.hex-address{color:var(--muted)}.hex-bytes{display:grid;grid-template-columns:repeat(16,2.4ch);gap:2px}.hex-ascii{display:grid;grid-template-columns:repeat(16,1ch)}.hex-row code{text-align:center;padding:2px 0;white-space:pre}.hex-row .changed{background:color-mix(in srgb,var(--amber) 26%,var(--panel));color:var(--amber)}.hex-row .added{background:color-mix(in srgb,var(--green) 25%,var(--panel));color:var(--green)}.hex-row .removed{background:color-mix(in srgb,var(--red) 25%,var(--panel));color:var(--red)}.hex-row .missing{color:var(--muted);opacity:.6}
</style>
