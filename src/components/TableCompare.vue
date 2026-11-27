<script setup lang="ts">
import {computed,onBeforeUnmount,ref,shallowRef,watch} from 'vue'
import {ChevronLeft,ChevronRight,Search,Table2} from 'lucide-vue-next'
import type {AssetSettings,Language,TableCell,TableSheet} from '../contracts'
import {type AssetReader,readAssetBlob,workbookNames,workbookSheet} from '../lib/asset-io'
import {decodeDelimited,parseDelimited,alignColumns,compareTables,getCell,compareCells,colLabel,tableWidth,type ColumnPair,type RowPair} from '../lib/table-diff'
import {translate} from '../lib/i18n'
const props=defineProps<{left?:AssetReader;right?:AssetReader;language:Language;settings:AssetSettings}>()
const emit=defineEmits<{'update-settings':[patch:Partial<AssetSettings>]}>()
const t=(key:string,...args:(string|number)[])=>translate(props.language,key,...args)
const a=shallowRef<TableSheet>(),b=shallowRef<TableSheet>()
const leftSheets=ref<string[]>([]),rightSheets=ref<string[]>([]),effectiveLeft=ref(''),effectiveRight=ref('')
const error=ref(''),loading=ref(false),search=ref(''),changedOnly=ref(true),rowPage=ref(0),colPage=ref(0)
const selected=ref<{row:RowPair;column:ColumnPair}>()
const ROWS=60,COLS=10
let generation=0
async function loadOne(reader:AssetReader|undefined,selection:string,active:()=>boolean):Promise<{sheet?:TableSheet;names:string[];selected:string}>{
  if(!reader)return {names:[],selected:''}
  const ext=reader.ref.name.split('.').at(-1)?.toLowerCase()
  if(['xlsx','xls','xlsm','xlsb','ods'].includes(ext||'')){
    if(!reader.ref.path)throw new Error(t('Excel and OpenDocument files require the desktop app.'))
    const names=await workbookNames(reader.ref)
    if(!active())throw new Error('Comparison cancelled.')
    const name=names.includes(selection)?selection:names[0]
    if(!name)throw new Error('Workbook has no worksheets.')
    return {sheet:await workbookSheet(reader.ref,name),names,selected:name}
  }
  if(!['csv','tsv','txt','tab'].includes(ext||''))throw new Error('Unsupported table format.')
  const blob=await readAssetBlob(reader,active)
  const text=decodeDelimited(new Uint8Array(await blob.arrayBuffer()),props.settings.tableEncoding)
  if(!active())throw new Error('Comparison cancelled.')
  const delimiter=props.settings.tableDelimiter==='auto'?(ext==='tsv'||ext==='tab'?'\t':','):props.settings.tableDelimiter
  return {sheet:parseDelimited(text,delimiter),names:[],selected:''}
}
async function load(){
  const id=++generation,active=()=>id===generation
  loading.value=true;error.value='';a.value=undefined;b.value=undefined;selected.value=undefined
  try{
    const [l,r]=await Promise.allSettled([loadOne(props.left,props.settings.leftSheet,active),loadOne(props.right,props.settings.rightSheet,active)])
    if(!active())return
    if(l.status==='fulfilled'){a.value=l.value.sheet;leftSheets.value=l.value.names;effectiveLeft.value=l.value.selected}else{leftSheets.value=[];effectiveLeft.value=''}
    if(r.status==='fulfilled'){b.value=r.value.sheet;rightSheets.value=r.value.names;effectiveRight.value=r.value.selected}else{rightSheets.value=[];effectiveRight.value=''}
    error.value=[l.status==='rejected'?t('LEFT')+': '+String(l.reason):'',r.status==='rejected'?t('RIGHT')+': '+String(r.reason):''].filter(Boolean).join('\n')
  }finally{if(active())loading.value=false}
}
watch([()=>props.left,()=>props.right,()=>props.settings.leftSheet,()=>props.settings.rightSheet,()=>props.settings.tableDelimiter,()=>props.settings.tableEncoding],load,{immediate:true})
onBeforeUnmount(()=>{generation++})
const aligned=computed(()=>{
  if(!a.value||!b.value)return {columns:[] as ColumnPair[],error:''}
  try{return {columns:alignColumns(a.value,b.value,props.settings.tableHeader,props.settings.tableColumnMode),error:''}}catch(e){return {columns:[],error:String(e)}}
})
const computedResult=computed(()=>{
  if(!a.value||!b.value||loading.value)return {result:undefined,error:''}
  try{return {result:compareTables(a.value,b.value,{header:props.settings.tableHeader,columnMode:props.settings.tableColumnMode,rowMode:props.settings.tableRowMode,keyColumn:props.settings.tableKey,ignoreType:props.settings.tableIgnoreType}),error:''}}catch(e){return {result:undefined,error:String(e)}}
})
const result=computed(()=>computedResult.value.result)
const keyChoices=computed(()=>aligned.value.columns.filter(c=>c.left!==null&&c.right!==null))
const filtered=computed(()=>{
  const q=search.value.toLocaleLowerCase()
  return (result.value?.rows||[]).filter(row=>(!changedOnly.value||row.status!=='same')&&(!q||row.key.toLocaleLowerCase().includes(q)||String((row.left??-1)+1)===q||String((row.right??-1)+1)===q))
})
const pages=computed(()=>Math.max(1,Math.ceil(filtered.value.length/ROWS))),columnPages=computed(()=>Math.max(1,Math.ceil((result.value?.columns.length||0)/COLS)))
const shownRows=computed(()=>filtered.value.slice(rowPage.value*ROWS,(rowPage.value+1)*ROWS)),shownColumns=computed(()=>(result.value?.columns||[]).slice(colPage.value*COLS,(colPage.value+1)*COLS))
watch([result,changedOnly,search],()=>{rowPage.value=0;selected.value=undefined});watch(result,()=>{colPage.value=0})
const leftCell=(row:RowPair,col:ColumnPair)=>a.value?getCell(a.value,row.left,col.left):null
const rightCell=(row:RowPair,col:ColumnPair)=>b.value?getCell(b.value,row.right,col.right):null
const status=(row:RowPair,col:ColumnPair)=>compareCells(leftCell(row,col),rightCell(row,col),props.settings.tableIgnoreType)
const statusText=(status:string)=>t(({same:'Identical',changed:'Changed',leftOnly:'Left only',rightOnly:'Right only'} as Record<string,string>)[status]||status)
const columnTitle=(col:ColumnPair)=>col.leftName===col.rightName?col.leftName:((col.leftName||'—')+' ↔ '+(col.rightName||'—'))
const display=(cell:TableCell|null)=>cell===null?'—':cell.value===''?'""':cell.value
function set<K extends keyof AssetSettings>(key:K,value:AssetSettings[K]){emit('update-settings',{[key]:value})}
</script>
<template>
  <section class="asset-module table-workspace">
    <div class="panel-toolbar asset-tools">
      <label>{{ t('Delimiter') }} <select :aria-label="t('Delimiter')" :value="settings.tableDelimiter" @change="set('tableDelimiter',($event.target as HTMLSelectElement).value as AssetSettings['tableDelimiter'])"><option value="auto">{{ t('Auto') }}</option><option value=",">,</option><option :value="'\t'">TAB</option><option value=";">;</option><option value="|">|</option></select></label>
      <label>{{ t('Open encoding') }} <select :value="settings.tableEncoding" @change="set('tableEncoding',($event.target as HTMLSelectElement).value)"><option value="auto">{{ t('Auto') }}</option><option v-for="enc in ['utf-8','utf-16le','utf-16be','gb18030','shift_jis','windows-1252']" :key="enc" :value="enc">{{ enc.toUpperCase() }}</option></select></label>
      <label><input type="checkbox" :checked="settings.tableHeader" @change="emit('update-settings',{tableHeader:($event.target as HTMLInputElement).checked,tableColumnMode:'position',tableRowMode:'position',tableKey:''})"/>{{ t('First row is header') }}</label>
      <label><input type="checkbox" :checked="settings.tableIgnoreType" @change="set('tableIgnoreType',($event.target as HTMLInputElement).checked)"/>{{ t('Compare values as text') }}</label>
    </div>
    <div v-if="leftSheets.length||rightSheets.length" class="panel-toolbar asset-tools sheet-selector">
      <label v-if="leftSheets.length">{{ t('LEFT') }} · {{ t('Worksheet') }} <select :value="effectiveLeft" @change="set('leftSheet',($event.target as HTMLSelectElement).value)"><option v-for="s in leftSheets" :key="s" :value="s">{{ s }}</option></select></label>
      <label v-if="rightSheets.length">{{ t('RIGHT') }} · {{ t('Worksheet') }} <select :value="effectiveRight" @change="set('rightSheet',($event.target as HTMLSelectElement).value)"><option v-for="s in rightSheets" :key="s" :value="s">{{ s }}</option></select></label>
    </div>
    <div class="panel-toolbar asset-tools">
      <label>{{ t('Align columns') }} <select :aria-label="t('Align columns')" :value="settings.tableColumnMode" @change="emit('update-settings',{tableColumnMode:($event.target as HTMLSelectElement).value as AssetSettings['tableColumnMode'],tableKey:''})"><option value="position">{{ t('By position') }}</option><option value="header" :disabled="!settings.tableHeader">{{ t('By header') }}</option></select></label>
      <label>{{ t('Match rows') }} <select :aria-label="t('Match rows')" :value="settings.tableRowMode" @change="set('tableRowMode',($event.target as HTMLSelectElement).value as AssetSettings['tableRowMode'])"><option value="position">{{ t('By position') }}</option><option value="key">{{ t('By key') }}</option></select></label>
      <label v-if="settings.tableRowMode==='key'">{{ t('Key column') }} <select :aria-label="t('Key column')" :value="settings.tableKey" @change="set('tableKey',($event.target as HTMLSelectElement).value)"><option value="">{{ t('Choose key column') }}</option><option v-for="c in keyChoices" :key="c.id" :value="c.id">{{ columnTitle(c)||(c.left!==null?colLabel(c.left):'—') }}</option></select></label>
      <label><input v-model="changedOnly" type="checkbox"/>{{ t('All differences') }}</label><label class="table-row-search"><Search :size="14"/><input v-model="search" :placeholder="t('Search row or key')"/></label>
    </div>
    <div class="notice">{{ t('Compares cell values, not workbook formatting or formulas. Duplicate or empty keys are rejected.') }}</div>
    <div v-if="error||computedResult.error||aligned.error" class="error-banner" role="alert">{{ error||computedResult.error||aligned.error }}</div>
    <div v-if="loading" class="asset-empty">{{ t('Comparing…') }}</div><div v-else-if="!a||!b" class="asset-empty"><Table2 :size="30"/>{{ t('Open two files to compare.') }}</div>
    <div v-else-if="result" class="table-result">
      <div class="table-summary"><span>{{ t('LEFT') }}: {{ a.rows.length }} × {{ tableWidth(a) }}</span><span>{{ t('RIGHT') }}: {{ b.rows.length }} × {{ tableWidth(b) }}</span><strong>{{ t('Cell differences') }}: {{ result.changed+result.leftOnly+result.rightOnly }}</strong><span>{{ t('Column differences') }}: {{ result.columnDifferences }}</span><span>{{ t('Row differences') }}: {{ result.changedRows+result.leftRows+result.rightRows }}</span></div>
      <div class="sheet-scroll selectable">
        <table class="sheet-grid"><thead><tr><th class="row-ids">{{ t('Match rows') }}</th><th v-for="c in shownColumns" :key="c.id" :class="{columnChanged:c.different}" :title="columnTitle(c)">{{ columnTitle(c)||'—' }}<small>{{ c.left!==null?colLabel(c.left):'—' }} ↔ {{ c.right!==null?colLabel(c.right):'—' }}</small></th></tr></thead>
          <tbody><tr v-for="row in shownRows" :key="row.id" :class="'row-'+row.status"><th class="row-ids" :title="row.key"><strong>{{ row.key }}</strong><small>{{ row.left!==null?row.left+1:'—' }} ↔ {{ row.right!==null?row.right+1:'—' }}</small><small>{{ statusText(row.status) }}</small></th><td v-for="c in shownColumns" :key="c.id" :class="'cell-'+status(row,c)" tabindex="0" :title="display(leftCell(row,c))+' ↔ '+display(rightCell(row,c))" @click="selected={row,column:c}" @keydown.enter="selected={row,column:c}"><div class="cell-pair"><span><i>L</i>{{ display(leftCell(row,c)) }}</span><span><i>R</i>{{ display(rightCell(row,c)) }}</span></div></td></tr></tbody>
        </table>
        <div v-if="!shownRows.length" class="asset-empty table-empty">{{ filtered.length===0&&(result.changed+result.leftOnly+result.rightOnly+result.columnDifferences+result.changedRows+result.leftRows+result.rightRows===0)?t('Equal under current rules'):t('No changes match this filter.') }}</div>
      </div>
      <div v-if="selected" class="cell-detail"><div><strong>{{ t('LEFT') }} · {{ leftCell(selected.row,selected.column)?.kind||t('Absent') }}</strong><pre>{{ display(leftCell(selected.row,selected.column)) }}</pre></div><div><strong>{{ t('RIGHT') }} · {{ rightCell(selected.row,selected.column)?.kind||t('Absent') }}</strong><pre>{{ display(rightCell(selected.row,selected.column)) }}</pre></div><button class="btn" @click="selected=undefined">{{ t('Close change details') }}</button></div>
      <footer class="asset-status table-pager">
        <span>{{ t('Rows') }} {{ filtered.length?rowPage*ROWS+1:0 }}–{{ Math.min(filtered.length,(rowPage+1)*ROWS) }} / {{ filtered.length }}</span><button class="btn-icon" :disabled="rowPage===0" :title="t('Previous page')" @click="rowPage--"><ChevronLeft :size="16"/></button><span>{{ rowPage+1 }} / {{ pages }}</span><button class="btn-icon" :disabled="rowPage+1>=pages" :title="t('Next page')" @click="rowPage++"><ChevronRight :size="16"/></button>
        <span>{{ t('Columns') }} {{ result.columns.length?colPage*COLS+1:0 }}–{{ Math.min(result.columns.length,(colPage+1)*COLS) }} / {{ result.columns.length }}</span><button class="btn-icon" :disabled="colPage===0" :title="t('Previous page')" @click="colPage--"><ChevronLeft :size="16"/></button><span>{{ colPage+1 }} / {{ columnPages }}</span><button class="btn-icon" :disabled="colPage+1>=columnPages" :title="t('Next page')" @click="colPage++"><ChevronRight :size="16"/></button>
      </footer>
    </div>
  </section>
</template>
<style scoped>
.table-workspace .asset-tools{gap:13px;min-height:43px!important}.table-workspace .asset-tools select{max-width:170px}.table-row-search{margin-left:auto}.table-row-search input{width:145px}.sheet-selector select{max-width:250px!important}
.table-result{flex:1;min-height:0;display:flex;flex-direction:column}.table-summary{display:flex;gap:14px;align-items:center;flex-wrap:wrap;padding:9px 12px;background:var(--panel-alt);border-bottom:1px solid var(--border);font-size:11px}.table-summary strong{color:var(--accent)}
.sheet-scroll{flex:1;overflow:auto;min-height:0;position:relative}.sheet-grid{border-collapse:separate;border-spacing:0;min-width:100%;table-layout:fixed;width:max-content;font-size:12px}.sheet-grid th,.sheet-grid td{border-bottom:1px solid var(--border);border-right:1px solid var(--border);padding:7px 9px;text-align:left;min-width:140px;max-width:250px;overflow:hidden;vertical-align:top}.sheet-grid thead th{position:sticky;top:0;z-index:1;background:var(--panel-alt);white-space:nowrap;text-overflow:ellipsis;font-weight:600}.sheet-grid th small{display:block;color:var(--muted);font-size:10px;margin-top:4px;font-weight:400}.sheet-grid .row-ids{min-width:100px;max-width:180px;position:sticky;left:0;background:var(--panel-alt);overflow:hidden;text-overflow:ellipsis}.sheet-grid thead .row-ids{z-index:2}.sheet-grid .row-ids strong{font-family:var(--mono);display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.sheet-grid td{background:var(--panel)}.sheet-grid tr:nth-child(even) td{background:color-mix(in srgb,var(--panel-alt) 22%,var(--panel))}.sheet-grid .cell-changed,.sheet-grid .columnChanged{background:color-mix(in srgb,var(--amber) 15%,var(--panel))!important}.sheet-grid .cell-leftOnly{background:color-mix(in srgb,var(--red) 14%,var(--panel))!important}.sheet-grid .cell-rightOnly{background:color-mix(in srgb,var(--green) 15%,var(--panel))!important}.cell-pair{display:flex;flex-direction:column;gap:5px}.cell-pair span{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;display:block;line-height:1.5}.cell-pair i{font:10px var(--mono);color:var(--muted);display:inline-block;margin-right:7px}.cell-pair span:first-child{border-bottom:1px solid color-mix(in srgb,var(--border) 55%,transparent);padding-bottom:4px}.table-empty{padding:32px}.table-pager{justify-content:center!important;gap:10px}.table-pager .btn-icon{height:25px;width:25px}.cell-detail{display:grid;grid-template-columns:1fr 1fr auto;gap:12px;align-items:start;padding:10px 12px;border-top:1px solid var(--border);background:var(--panel-alt);min-height:80px;max-height:150px;overflow:auto}.cell-detail>div{min-width:0}.cell-detail strong{font-size:11px;color:var(--muted)}.cell-detail pre{white-space:pre-wrap;overflow-wrap:anywhere;font:12px var(--mono);margin:6px 0 0}
</style>
