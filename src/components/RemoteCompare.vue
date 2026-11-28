<script setup lang="ts">
import {computed,onBeforeUnmount,ref,watch} from 'vue'
import {FolderOpen,KeyRound,RefreshCw,Server,Square,Trash2} from 'lucide-vue-next'
import type {CompareSession,Language,RemoteRequest,RemoteResult,RemoteSelection,Side,SnapshotEntry,SshProfile,TextDocument} from '../contracts'
import {browsePath,compareRemote,probeSsh,readRemote} from '../lib/version-io'
import {cancelJob,isDesktop,confirmAction} from '../lib/bridge'
import {clone} from '../lib/workspace'
import {translate} from '../lib/i18n'
import SnapshotList from './SnapshotList.vue'
const props=defineProps<{session:CompareSession;language:Language;profiles:SshProfile[]}>()
const emit=defineEmits<{'update-remote':[side:Side,source:RemoteSelection];'save-profile':[profile:SshProfile];'delete-profile':[name:string];'open-pair':[left:TextDocument|null,right:TextDocument|null,title:string]}>()
const t=(key:string)=>translate(props.language,key)
const sides:Side[]=['left','right']
const source=(side:Side)=>props.session.remoteSources![side]
// Authentication secrets deliberately do not enter session data, settings,
// connection profiles, logs or named snapshots.
const secrets=ref<Record<Side,string>>({left:'',right:''})
const presented=ref<Record<Side,string>>({left:'',right:''})
const probing=ref<Record<Side,boolean>>({left:false,right:false})
const result=ref<RemoteResult>(),error=ref(''),busy=ref(false),opening=ref(false)
let epoch=0,jobId='',alive=true
const identity=(p:SshProfile)=>JSON.stringify([p.host,p.port,p.username,p.auth,p.keyPath,p.fingerprint])
function patch(side:Side,partial:Partial<RemoteSelection>){emit('update-remote',side,{...source(side),...partial})}
function profile(side:Side,partial:Partial<SshProfile>){patch(side,{profile:{...source(side).profile,...partial}})}
function selectProfile(side:Side,name:string){const p=props.profiles.find(v=>v.name===name);if(p){secrets.value[side]='';presented.value[side]='';patch(side,{profile:clone(p),kind:'sftp'})}}
async function saveProfile(side:Side){
  const p=clone(source(side).profile);p.name=p.name.trim()
  if(!p.name||!p.host||!p.username||!Number.isInteger(p.port)||p.port<1||p.port>65535){error.value=t('Enter a connection name, host, port and username.');return}
  if(props.profiles.some(v=>v.name===p.name)&&!await confirmAction(t('Replace the saved connection?'),t('Save connection')))return
  if(alive)emit('save-profile',p)
}
async function removeProfile(side:Side){const name=source(side).profile.name;if(props.profiles.some(p=>p.name===name)&&await confirmAction(t('Remove the saved connection?'),t('Remove connection'))&&alive)emit('delete-profile',name)}
function request(side:Side,selection=source(side)):RemoteRequest{return {source:clone(selection),secret:selection.kind==='sftp'&&selection.profile.auth!=='agent'?secrets.value[side]||null:null}}
const ready=computed(()=>isDesktop&&sides.every(side=>{
  const s=source(side),p=s.profile
  return !!s.path.trim()&&(s.kind==='local'||!!p.host.trim()&&!!p.username.trim()&&Number.isInteger(p.port)&&p.port>0&&p.port<=65535&&p.fingerprint.trim().startsWith('SHA256:')&&(p.auth!=='password'||!!secrets.value[side])&&(p.auth!=='key'||!!p.keyPath))
}))
async function browse(side:Side,directory:boolean,key=false){
  try{const path=await browsePath(directory);if(path&&alive){if(key)profile(side,{keyPath:path});else patch(side,{path})}}
  catch(e){error.value=String(e)}
}
async function probe(side:Side){
  if(!isDesktop||probing.value[side])return
  const p=clone(source(side).profile),id=identity(p);probing.value[side]=true;presented.value[side]='';error.value=''
  try{const pin=await probeSsh(p);if(alive&&identity(source(side).profile)===id)presented.value[side]=pin}
  catch(e){if(alive)error.value=String(e)}
  finally{probing.value[side]=false}
}
function stop(){epoch++;if(jobId)void cancelJob(jobId).catch(()=>{});jobId='';busy.value=false}
watch(()=>JSON.stringify(props.session.remoteSources),()=>{stop();result.value=undefined;error.value='';opening.value=false})
for(const side of sides)watch(()=>identity(source(side).profile),()=>{secrets.value[side]='';presented.value[side]=''})
async function compare(){
  if(!ready.value||busy.value)return
  const id=++epoch;jobId=crypto.randomUUID();busy.value=true;result.value=undefined;error.value=''
  try{const r=await compareRemote(request('left'),request('right'),jobId);if(alive&&id===epoch)result.value=r}
  catch(e){if(id===epoch)error.value=String(e)}
  finally{if(id===epoch){busy.value=false;jobId=''}}
}
async function openEntry(entry:SnapshotEntry,encoding:string|null){
  if(!result.value||opening.value)return
  const id=epoch,snapshot=result.value;opening.value=true;error.value=''
  try{const [left,right]=await Promise.all(sides.map(side=>entry[side]?.fingerprint?readRemote(request(side,snapshot[side]),entry.path,entry[side]!.fingerprint!,encoding):Promise.resolve(null)));if(alive&&id===epoch)emit('open-pair',left,right,entry.path)}
  catch(e){if(id===epoch)error.value=String(e)}
  finally{if(id===epoch)opening.value=false}
}
onBeforeUnmount(()=>{alive=false;stop();secrets.value={left:'',right:''}})
</script>
<template>
  <section class="asset-module remote-workspace">
    <div class="version-sources remote-sources">
      <div v-for="side in sides" :key="side" class="version-source">
        <header><Server :size="16"/><strong>{{ t(side==='left'?'LEFT':'RIGHT') }}</strong><span class="spacer"/><select :aria-label="t('Source type')" :value="source(side).kind" @change="patch(side,{kind:($event.target as HTMLSelectElement).value as RemoteSelection['kind']})"><option value="local">{{ t('Local files') }}</option><option value="sftp">SSH/SFTP</option></select></header>
        <template v-if="source(side).kind==='sftp'">
          <label>{{ t('Saved connections') }}<select :data-profile-side="side" :value="profiles.some(p=>p.name===source(side).profile.name)?source(side).profile.name:''" @change="selectProfile(side,($event.target as HTMLSelectElement).value)"><option value="">{{ t('Choose connection') }}</option><option v-for="p in profiles" :key="p.name" :value="p.name">{{ p.name }}</option></select></label>
          <label>{{ t('Connection name') }}<span class="field-pair"><input :data-connection-name="side" :value="source(side).profile.name" @input="profile(side,{name:($event.target as HTMLInputElement).value})"/><button class="btn" :title="t('Save connection')" @click="saveProfile(side)">{{ t('Save connection') }}</button><button class="btn-icon" :disabled="!profiles.some(p=>p.name===source(side).profile.name)" :title="t('Remove connection')" @click="removeProfile(side)"><Trash2 :size="15"/></button></span></label>
          <div class="ssh-fields"><label>{{ t('Host') }}<input :data-ssh-host="side" :value="source(side).profile.host" @input="profile(side,{host:($event.target as HTMLInputElement).value})"/></label><label>{{ t('Port') }}<input type="number" min="1" max="65535" :value="source(side).profile.port" @change="profile(side,{port:Math.max(1,Math.min(65535,Number(($event.target as HTMLInputElement).value)||22))})"/></label></div>
          <div class="ssh-fields user-auth"><label>{{ t('Username') }}<input :value="source(side).profile.username" @input="profile(side,{username:($event.target as HTMLInputElement).value})"/></label><label>{{ t('Authentication') }}<select :value="source(side).profile.auth" @change="profile(side,{auth:($event.target as HTMLSelectElement).value as SshProfile['auth']})"><option value="agent">SSH agent</option><option value="key">{{ t('Private key') }}</option><option value="password">{{ t('Password') }}</option></select></label></div>
          <label v-if="source(side).profile.auth==='key'">{{ t('Private key') }}<span class="field-pair"><input :value="source(side).profile.keyPath" @input="profile(side,{keyPath:($event.target as HTMLInputElement).value})"/><button class="btn-icon" :disabled="!isDesktop" @click="browse(side,false,true)"><KeyRound :size="15"/></button></span></label>
          <label v-if="source(side).profile.auth!=='agent'">{{ t(source(side).profile.auth==='password'?'Password':'Key passphrase') }}<input v-model="secrets[side]" :data-ssh-secret="side" type="password" autocomplete="off" spellcheck="false"/></label>
          <label>{{ t('Trusted host fingerprint') }}<span class="field-pair"><input placeholder="SHA256:…" :value="source(side).profile.fingerprint" @input="profile(side,{fingerprint:($event.target as HTMLInputElement).value})"/><button class="btn" :disabled="!isDesktop||probing[side]||!source(side).profile.host" @click="probe(side)">{{ t('Probe host key') }}</button></span></label>
          <small v-if="presented[side]" class="selectable">{{ t('Presented fingerprint') }}: {{ presented[side] }} · {{ t('Verify it independently before entering the trusted fingerprint.') }}</small>
        </template>
        <label>{{ t(source(side).kind==='local'?'Local file or folder':'Remote path') }}<span class="field-pair"><input :data-remote-path="side" :placeholder="source(side).kind==='sftp'?'/path/to/file':''" :value="source(side).path" @input="patch(side,{path:($event.target as HTMLInputElement).value})"/><template v-if="source(side).kind==='local'"><button class="btn" :disabled="!isDesktop" @click="browse(side,false)">{{ t('Open file') }}</button><button class="btn-icon" :disabled="!isDesktop" :title="t('Open folder')" @click="browse(side,true)"><FolderOpen :size="16"/></button></template></span></label>
      </div>
    </div>
    <div class="panel-toolbar asset-tools"><button class="btn primary" :disabled="!ready||busy||opening" @click="compare"><RefreshCw :size="15"/>{{ t('Compare sources') }}</button><button v-if="busy" class="btn danger" @click="stop"><Square :size="14"/>{{ t('Cancel') }}</button></div>
    <div class="notice">{{ t('SFTP is read only. Passwords and key passphrases are never saved. Verify the SSH host key before authentication.') }}</div>
    <div v-if="!isDesktop" class="notice">{{ t('Git and SFTP comparison require the desktop app.') }}</div>
    <div v-if="error" class="error-banner" role="alert">{{ error }}</div>
    <div v-if="busy" class="asset-empty">{{ t('Comparing…') }}</div>
    <SnapshotList v-else-if="result" :entries="result.entries" :language="language" :busy="opening" @open-entry="openEntry"/>
    <div v-else class="asset-empty"><Server :size="30"/>{{ t('Choose two sources to compare.') }}</div>
  </section>
</template>
<style scoped>
.remote-sources{max-height:58%;overflow:auto}.ssh-fields{display:grid;grid-template-columns:minmax(0,1fr) 95px;gap:8px}.ssh-fields.user-auth{grid-template-columns:minmax(0,1fr) minmax(100px,.7fr)}.spacer{flex:1}
</style>
