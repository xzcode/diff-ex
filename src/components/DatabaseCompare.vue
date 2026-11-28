<script setup lang="ts">
import {computed,onBeforeUnmount,ref,watch} from 'vue'
import {invoke} from '@tauri-apps/api/core'
import {Database,FolderOpen,RefreshCw,Square,Search,Trash2,Download,ChevronLeft,ChevronRight} from 'lucide-vue-next'
import type {CompareSession,DbEngine,DbProfile,DbSource,DbOptions,DbResult,DbDifference,DbRowDifference,Side,Language} from '../contracts'
import {browsePath} from '../lib/version-io'
import {cancelJob,confirmAction,isDesktop} from '../lib/bridge'
import {clone,defaultDbProfile} from '../lib/workspace'
import {translate} from '../lib/i18n'
import {exportReport} from '../lib/report'
const props=defineProps<{session:CompareSession;profiles:DbProfile[];language:Language}>()
const emit=defineEmits<{'update-source':[side:Side,source:DbSource];'update-options':[patch:Partial<DbOptions>];'save-profile':[profile:DbProfile];'delete-profile':[name:string]}>()
const t=(key:string,...args:(string|number)[])=>translate(props.language,key,...args)
const sides:Side[]=['left','right'],engines:DbEngine[]=['sqlite','mysql','postgres','mongodb']
const labels={sqlite:'SQLite',mysql:'MySQL / MariaDB',postgres:'PostgreSQL',mongodb:'MongoDB'}
const src=(side:Side)=>props.session.dbSources![side]
const opts=computed(()=>props.session.dbOptions!)
const secrets=ref<Record<Side,string>>({left:'',right:''})
const catalogs=ref<Record<Side,string[]>>({left:[],right:[]})
const loading=ref<Record<Side,boolean>>({left:false,right:false})
const busy=ref(false),error=ref(''),result=ref<DbResult>(),reporting=ref(false),notice=ref('')
const tab=ref<'schema'|'data'>('schema'),search=ref(''),showEqual=ref(false),page=ref(0)
const selected=ref<DbDifference|DbRowDifference>()
const statusFilter=ref('all')
const PAGE=100
let epoch=0,jobId='',alive=true
function patch(side:Side,change:Partial<DbSource>){emit('update-source',side,{...src(side),...change})}
function profile(side:Side,change:Partial<DbProfile>){patch(side,{profile:{...src(side).profile,...change}})}
function engine(side:Side,e:DbEngine){patch(side,{profile:defaultDbProfile(e),relation:''})}
const identity=(p:DbProfile)=>JSON.stringify(p)
for(const side of sides)watch(()=>identity(src(side).profile),()=>{secrets.value[side]='';catalogs.value[side]=[];loading.value[side]=false})
const valid=(p:DbProfile)=>p.engine==='sqlite'?!!p.path.trim():!!p.host.trim()&&!!p.database.trim()&&Number.isInteger(p.port)&&p.port>0&&p.port<=65535
const ready=computed(()=>isDesktop&&src('left').profile.engine===src('right').profile.engine&&sides.every(side=>valid(src(side).profile)&&!!src(side).relation.trim()))
const request=(side:Side)=>({profile:clone(src(side).profile),relation:src(side).relation,secret:src(side).profile.engine==='sqlite'?null:secrets.value[side]||null})
function useProfile(side:Side,name:string){const p=props.profiles.find(v=>v.name===name);if(p){secrets.value[side]='';patch(side,{profile:clone(p),relation:''})}}
async function saveProfile(side:Side){
  const p=clone(src(side).profile);p.name=p.name.trim()
  if(!p.name||!valid(p)){error.value=t('Enter a connection name and valid database settings.');return}
  if(props.profiles.some(v=>v.name===p.name)&&!await confirmAction(t('Replace the saved connection?'),t('Save connection')))return
  if(alive)emit('save-profile',p)
}
async function removeProfile(side:Side){const name=src(side).profile.name;if(props.profiles.some(p=>p.name===name)&&await confirmAction(t('Remove the saved connection?'),t('Remove connection'))&&alive)emit('delete-profile',name)}
async function browse(side:Side,field:'path'|'caPath'){
  try{const path=await browsePath(false);if(path&&alive)profile(side,{[field]:path})}catch(e){error.value=String(e)}
}
async function authorizePlaintext(sidesToCheck:Side[]){
  if(sidesToCheck.some(side=>src(side).profile.engine!=='sqlite'&&!src(side).profile.tls))return confirmAction(t('TLS is disabled. Database credentials and data may travel unencrypted. Continue?'),t('Database compare'))
  return true
}
async function catalog(side:Side){
  if(!isDesktop||loading.value[side]||!valid(src(side).profile))return
  const snapshot=identity(src(side).profile)
  if(!await authorizePlaintext([side])||snapshot!==identity(src(side).profile)||!alive)return
  loading.value[side]=true;error.value=''
  try{const names=await invoke<string[]>('database_catalog',{request:request(side)});if(alive&&snapshot===identity(src(side).profile))catalogs.value[side]=names}
  catch(e){if(alive&&snapshot===identity(src(side).profile))error.value=String(e)}
  finally{if(alive&&snapshot===identity(src(side).profile))loading.value[side]=false}
}
function stop(){epoch++;if(jobId)void cancelJob(jobId).catch(()=>{});jobId='';busy.value=false}
watch(()=>JSON.stringify([props.session.dbSources,props.session.dbOptions]),()=>{stop();result.value=undefined;selected.value=undefined;error.value='';notice.value=''})
async function compare(){
  if(!ready.value||busy.value)return
  const snapshot=JSON.stringify([props.session.dbSources,opts.value])
  if(!await authorizePlaintext(sides)||snapshot!==JSON.stringify([props.session.dbSources,opts.value])||!alive)return
  const id=++epoch;jobId=crypto.randomUUID();busy.value=true;result.value=undefined;error.value='';notice.value=''
  try{
    const next=await invoke<DbResult>('compare_database',{left:request('left'),right:request('right'),options:clone(opts.value),jobId})
    if(id!==epoch||!alive)return
    result.value=next;tab.value=opts.value.mode==='data'?'data':'schema'
  }catch(e){if(id===epoch)error.value=String(e)}
  finally{if(id===epoch){busy.value=false;jobId=''}}
}
const status=(s:string)=>t(({same:'Identical',different:'Changed',leftOnly:'Left only',rightOnly:'Right only'})[s]||s)
const list=computed(()=>tab.value==='schema'?result.value?.schema||[]:result.value?.rows||[])
const filtered=computed(()=>{const q=search.value.toLowerCase();return list.value.filter(r=>(showEqual.value||r.status!=='same')&&(statusFilter.value==='all'||r.status===statusFilter.value)&&(!q||('key'in r?r.key+' '+r.columns.join(' '):r.name+' '+r.kind).toLowerCase().includes(q)))})
const pages=computed(()=>Math.max(1,Math.ceil(filtered.value.length/PAGE))),visible=computed(()=>filtered.value.slice(page.value*PAGE,(page.value+1)*PAGE))
watch([tab,search,showEqual,result,statusFilter],()=>{page.value=0;selected.value=undefined})
const keyText=(key:string)=>{try{return (JSON.parse(key) as {value:string}[]).map(v=>v.value).join(' | ')}catch{return key}}
const detail=(row:DbDifference|DbRowDifference,side:Side)=>{const v=row[side];return v===null?'—':typeof v==='string'?v:JSON.stringify(v,null,2)}
const counts=computed(()=>({schema:result.value?.schema.filter(v=>v.status!=='same').length||0,data:result.value?.rows.length||0}))
function arrayValue(event:Event){return (event.target as HTMLTextAreaElement).value.split('\n').map(v=>v.trim()).filter(Boolean)}
async function saveReport(format:'json'|'html'){
  if(!result.value||reporting.value)return
  reporting.value=true
  try{const saved=await exportReport('database-comparison',{version:1,kind:'database',left:{engine:src('left').profile.engine,relation:src('left').relation},right:{engine:src('right').profile.engine,relation:src('right').relation},options:clone(opts.value),result:clone(result.value)},format,path=>t('Replace existing file?',path));if(saved)notice.value=t('Report exported.')}
  catch(e){error.value=String(e)}finally{reporting.value=false}
}
onBeforeUnmount(()=>{alive=false;stop();secrets.value={left:'',right:''}})
</script>
<template>
<section class="asset-module database-workspace">
  <div class="version-sources db-sources">
    <div v-for="side in sides" :key="side" class="version-source">
      <header><Database :size="16"/><strong>{{ t(side==='left'?'LEFT':'RIGHT') }}</strong><span class="spacer"/><select :data-db-engine="side" :value="src(side).profile.engine" @change="engine(side,($event.target as HTMLSelectElement).value as DbEngine)"><option v-for="e in engines" :key="e" :value="e">{{ labels[e] }}</option></select></header>
      <label>{{ t('Saved connections') }}<select :value="profiles.some(p=>p.name===src(side).profile.name)?src(side).profile.name:''" @change="useProfile(side,($event.target as HTMLSelectElement).value)"><option value="">{{ t('Choose connection') }}</option><option v-for="p in profiles" :key="p.name" :value="p.name">{{ p.name }} · {{ labels[p.engine] }}</option></select></label>
      <label>{{ t('Connection name') }}<span class="field-pair"><input :data-db-name="side" :value="src(side).profile.name" @input="profile(side,{name:($event.target as HTMLInputElement).value})"/><button class="btn" @click="saveProfile(side)">{{ t('Save connection') }}</button><button class="btn-icon" :disabled="!profiles.some(p=>p.name===src(side).profile.name)" :title="t('Remove connection')" @click="removeProfile(side)"><Trash2 :size="14"/></button></span></label>
      <label v-if="src(side).profile.engine==='sqlite'">{{ t('Database file') }}<span class="field-pair"><input :data-db-file="side" :value="src(side).profile.path" @input="profile(side,{path:($event.target as HTMLInputElement).value})"/><button class="btn-icon" :disabled="!isDesktop" @click="browse(side,'path')"><FolderOpen :size="16"/></button></span></label>
      <template v-else>
        <div class="db-grid"><label>{{ t('Host') }}<input :value="src(side).profile.host" @input="profile(side,{host:($event.target as HTMLInputElement).value})"/></label><label>{{ t('Port') }}<input type="number" min="1" max="65535" :value="src(side).profile.port" @change="profile(side,{port:Math.max(1,Math.min(65535,Number(($event.target as HTMLInputElement).value)||1))})"/></label><label>{{ t('Database') }}<input :data-db-database="side" :value="src(side).profile.database" @input="profile(side,{database:($event.target as HTMLInputElement).value})"/></label><label>{{ t('Username') }}<input :value="src(side).profile.username" @input="profile(side,{username:($event.target as HTMLInputElement).value})"/></label></div>
        <label>{{ t('Password') }}<input v-model="secrets[side]" :data-db-secret="side" type="password" autocomplete="off"/></label>
        <label v-if="src(side).profile.engine==='postgres'">{{ t('Schema') }}<input :value="src(side).profile.schema" @input="profile(side,{schema:($event.target as HTMLInputElement).value})"/></label>
        <label v-if="src(side).profile.engine==='mongodb'">{{ t('Authentication database') }}<input :value="src(side).profile.authDatabase" @input="profile(side,{authDatabase:($event.target as HTMLInputElement).value})"/></label>
        <label class="inline-check"><input type="checkbox" :checked="src(side).profile.tls" @change="profile(side,{tls:($event.target as HTMLInputElement).checked})"/> {{ t('Verify TLS certificates') }}</label>
        <label v-if="src(side).profile.tls">{{ t('CA certificate file') }}<span class="field-pair"><input :value="src(side).profile.caPath" @input="profile(side,{caPath:($event.target as HTMLInputElement).value})"/><button class="btn-icon" :disabled="!isDesktop" @click="browse(side,'caPath')"><FolderOpen :size="14"/></button></span></label>
      </template>
      <label>{{ t('Table or collection') }}<span class="field-pair"><input :data-db-relation="side" :list="'db-tables-'+session.id+'-'+side" :value="src(side).relation" @input="patch(side,{relation:($event.target as HTMLInputElement).value})"/><button class="btn" :disabled="!isDesktop||loading[side]||!valid(src(side).profile)" @click="catalog(side)">{{ t('Load tables') }}</button></span></label>
      <datalist :id="'db-tables-'+session.id+'-'+side"><option v-for="name in catalogs[side]" :key="name" :value="name"/></datalist>
    </div>
  </div>
  <div class="panel-toolbar asset-tools db-options">
    <label>{{ t('Comparison mode') }} <select :value="opts.mode" @change="emit('update-options',{mode:($event.target as HTMLSelectElement).value as DbOptions['mode']})"><option value="both">{{ t('Schema and data') }}</option><option value="schema">{{ t('Schema only') }}</option><option value="data">{{ t('Data only') }}</option></select></label>
    <label v-if="opts.mode!=='schema'">{{ t('Key columns (one per line)') }}<textarea rows="2" :placeholder="t('Automatic primary key')" :value="opts.keys.join('\n')" @change="emit('update-options',{keys:arrayValue($event)})"/></label>
    <label v-if="opts.mode!=='schema'">{{ t('Ignored columns (one per line)') }}<textarea rows="2" :value="opts.ignoreColumns.join('\n')" @change="emit('update-options',{ignoreColumns:arrayValue($event)})"/></label>
    <button class="btn primary" :disabled="!ready||busy" @click="compare"><RefreshCw :size="15"/>{{ t('Compare databases') }}</button><button v-if="busy" class="btn danger" @click="stop"><Square :size="14"/>{{ t('Cancel') }}</button>
  </div>
  <div class="notice">{{ t('Read-only comparison. Keys must be unique and non-null. Passwords are not saved. Compare consistent snapshots or pause writes.') }}</div>
  <div v-if="!isDesktop" class="notice">{{ t('Database comparison requires the desktop app.') }}</div>
  <div v-if="error" class="error-banner" role="alert">{{ error }}</div><div v-if="notice" class="notice">{{ notice }}</div>
  <div v-if="busy" class="asset-empty">{{ t('Comparing…') }}</div>
  <template v-else-if="result">
    <div class="panel-toolbar asset-tools"><button class="btn" :class="{active:tab==='schema'}" @click="tab='schema'">{{ t('Schema differences') }} ({{ counts.schema }})</button><button class="btn" :disabled="!result.dataCompared" :class="{active:tab==='data'}" @click="tab='data'">{{ t('Record differences') }} ({{ counts.data }})</button><span v-if="result.dataCompared">{{ result.leftRows }} / {{ result.rightRows }} · {{ t('Identical') }} {{ result.equalRows }}</span><span class="spacer"/><button class="btn" :disabled="reporting" @click="saveReport('json')"><Download :size="14"/>JSON</button><button class="btn" :disabled="reporting" @click="saveReport('html')">HTML</button></div>
    <div class="panel-toolbar asset-tools"><label><Search :size="14"/><input v-model="search" :placeholder="t('Search')"/></label><select v-model="statusFilter"><option value="all">{{ t('All differences') }}</option><option v-for="v in ['different','leftOnly','rightOnly','same']" :key="v" :value="v">{{ status(v) }}</option></select><label class="inline-check"><input type="checkbox" v-model="showEqual"/>{{ t('Show identical') }}</label></div>
    <div class="db-list selectable"><div class="db-row db-heading"><span>{{ t('Name') }} / {{ t('Key column') }}</span><span>{{ t('STATUS') }}</span><span>{{ t('Changed fields') }}</span></div><div v-for="r in visible" :key="'key'in r?r.key:r.kind+':'+r.name" class="db-row" :class="{selected:selected===r}" tabindex="0" @click="selected=r" @keydown.enter="selected=r"><code>{{ 'key'in r?keyText(r.key):r.kind+' · '+r.name }}</code><span :class="'db-'+r.status">{{ status(r.status) }}</span><span>{{ 'key'in r?r.columns.join(', '):'—' }}</span></div><div v-if="!visible.length" class="asset-empty">{{ t('No changes match this filter.') }}</div></div>
    <footer class="asset-status"><span>{{ filtered.length }} {{ t('Rows') }}</span><button class="btn-icon" :disabled="page===0" @click="page--"><ChevronLeft :size="16"/></button><span>{{ page+1 }} / {{ pages }}</span><button class="btn-icon" :disabled="page+1>=pages" @click="page++"><ChevronRight :size="16"/></button></footer>
    <div v-if="selected" class="db-detail selectable"><div v-for="side in sides" :key="side"><strong>{{ t(side==='left'?'LEFT':'RIGHT') }}</strong><pre>{{ detail(selected,side) }}</pre></div></div>
  </template>
  <div v-else class="asset-empty"><Database :size="30"/>{{ t('Choose two sources to compare.') }}</div>
</section>
</template>
<style scoped>
.db-sources{max-height:49%;overflow:auto}.spacer{flex:1}.db-grid{display:grid;grid-template-columns:2fr 1fr;gap:6px}.version-source .inline-check,.inline-check{flex-direction:row;align-items:center;gap:7px}.inline-check input{width:auto;min-height:0}.db-options{align-items:center}.db-options textarea{min-width:130px;max-width:220px;padding:5px;resize:vertical;font-size:11px}.db-list{flex:1;min-height:60px;overflow:auto}.db-row{display:grid;grid-template-columns:minmax(150px,1.4fr) 120px minmax(100px,1fr);gap:10px;align-items:center;padding:8px 12px;border-bottom:1px solid var(--border);font-size:12px;min-width:450px}.db-row>*{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.db-heading{position:sticky;top:0;z-index:1;background:var(--panel-alt);color:var(--muted)}.db-row.selected,.db-row:hover{background:color-mix(in srgb,var(--accent) 12%,var(--panel))}.db-different{color:var(--amber)}.db-leftOnly{color:var(--red)}.db-rightOnly{color:var(--green)}.db-detail{display:grid;grid-template-columns:1fr 1fr;gap:10px;padding:10px 12px;max-height:29%;overflow:auto;border-top:1px solid var(--border)}.db-detail>div{min-width:0}.db-detail pre{white-space:pre-wrap;overflow-wrap:anywhere;font:12px/1.5 var(--mono);user-select:text}
</style>
