<script setup lang="ts">
import {computed,onBeforeUnmount,ref,watch} from 'vue'
import {FolderOpen,GitBranch,RefreshCw,Square} from 'lucide-vue-next'
import type {CompareSession,GitResult,GitSelection,Language,Side,SnapshotEntry,TextDocument} from '../contracts'
import {browsePath,compareGit,gitRefs,readGit} from '../lib/version-io'
import {cancelJob,isDesktop} from '../lib/bridge'
import {clone} from '../lib/workspace'
import {translate} from '../lib/i18n'
import SnapshotList from './SnapshotList.vue'
const props=defineProps<{session:CompareSession;language:Language}>()
const emit=defineEmits<{'update-git':[side:Side,source:GitSelection];'open-pair':[left:TextDocument|null,right:TextDocument|null,title:string]}>()
const t=(key:string)=>translate(props.language,key)
const sides:Side[]=['left','right']
const source=(side:Side)=>props.session.gitSources![side]
const refs=ref<Record<Side,string[]>>({left:[],right:[]})
const result=ref<GitResult>(),error=ref(''),busy=ref(false),opening=ref(false)
const ready=computed(()=>isDesktop&&sides.every(side=>source(side).repository.trim()&&source(side).revision.trim()))
let epoch=0,jobId='',alive=true
const refEpoch:Record<Side,number>={left:0,right:0}
function patch(side:Side,partial:Partial<GitSelection>){emit('update-git',side,{...source(side),...partial})}
async function browse(side:Side){
  try{const repository=await browsePath(true);if(!repository||!alive)return;patch(side,{repository});if(side==='left'&&!source('right').repository)patch('right',{repository})}
  catch(e){error.value=String(e)}
}
async function loadRefs(side:Side){
  const repository=source(side).repository,id=++refEpoch[side]
  if(!isDesktop||!repository.trim())return
  try{const names=await gitRefs(repository);if(alive&&id===refEpoch[side]&&source(side).repository===repository)refs.value[side]=names}
  catch(e){if(alive&&id===refEpoch[side])error.value=String(e)}
}
function stop(){epoch++;if(jobId)void cancelJob(jobId).catch(()=>{});jobId='';busy.value=false}
watch(()=>JSON.stringify(props.session.gitSources),()=>{stop();result.value=undefined;error.value='';opening.value=false})
for(const side of sides)watch(()=>source(side).repository,()=>{refEpoch[side]++;refs.value[side]=[]})
async function compare(){
  if(!ready.value||busy.value)return
  const id=++epoch;jobId=crypto.randomUUID();busy.value=true;result.value=undefined;error.value=''
  try{const r=await compareGit(clone(source('left')),clone(source('right')),jobId);if(alive&&id===epoch)result.value=r}
  catch(e){if(id===epoch)error.value=String(e)}
  finally{if(id===epoch){busy.value=false;jobId=''}}
}
async function openEntry(entry:SnapshotEntry,encoding:string|null){
  if(!result.value||opening.value)return
  const id=epoch,snapshot=result.value;opening.value=true;error.value=''
  try{
    const [left,right]=await Promise.all(sides.map(side=>entry[side]?.fingerprint?readGit(snapshot[side],entry.path,entry[side]!.fingerprint!,encoding):Promise.resolve(null)))
    if(alive&&epoch===id)emit('open-pair',left,right,entry.path)
  }catch(e){if(epoch===id)error.value=String(e)}
  finally{if(epoch===id)opening.value=false}
}
onBeforeUnmount(()=>{alive=false;stop();refEpoch.left++;refEpoch.right++})
</script>
<template>
  <section class="asset-module git-workspace">
    <div class="version-sources">
      <div v-for="side in sides" :key="side" class="version-source">
        <header><GitBranch :size="16"/><strong>{{ t(side==='left'?'LEFT':'RIGHT') }}</strong></header>
        <label>{{ t('Git repository') }}<span class="field-pair"><input :data-git-repo="side" :value="source(side).repository" @input="patch(side,{repository:($event.target as HTMLInputElement).value})"/><button class="btn-icon" :disabled="!isDesktop" :title="t('Open folder')" @click="browse(side)"><FolderOpen :size="16"/></button></span></label>
        <label>{{ t('Git revision') }}<span class="field-pair"><input :data-git-revision="side" :value="source(side).revision" :list="'git-refs-'+session.id+'-'+side" @input="patch(side,{revision:($event.target as HTMLInputElement).value})"/><button class="btn" :disabled="!isDesktop||!source(side).repository" @click="loadRefs(side)">{{ t('Load refs') }}</button></span></label>
        <datalist :id="'git-refs-'+session.id+'-'+side"><option v-for="name in refs[side]" :key="name" :value="name"/></datalist>
        <small>HEAD · WORKTREE · refs/heads/… · commit SHA</small>
      </div>
    </div>
    <div class="panel-toolbar asset-tools"><button class="btn primary" :disabled="!ready||busy||opening" @click="compare"><RefreshCw :size="15"/>{{ t('Compare versions') }}</button><button v-if="busy" class="btn danger" @click="stop"><Square :size="14"/>{{ t('Cancel') }}</button><span v-if="result" class="muted selectable">{{ result.left.revision.slice(0,12) }} ↔ {{ result.right.revision.slice(0,12) }}</span></div>
    <div class="notice">{{ t('Git objects and working trees are read only. Open a detached text copy to review changes.') }}</div>
    <div v-if="!isDesktop" class="notice">{{ t('Git and SFTP comparison require the desktop app.') }}</div>
    <div v-if="error" class="error-banner" role="alert">{{ error }}</div>
    <div v-if="busy" class="asset-empty">{{ t('Comparing…') }}</div>
    <SnapshotList v-else-if="result" :entries="result.entries" :language="language" :busy="opening" @open-entry="openEntry"/>
    <div v-else class="asset-empty"><GitBranch :size="32"/>{{ t('Choose two sources to compare.') }}</div>
  </section>
</template>
