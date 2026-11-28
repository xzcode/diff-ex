<script setup lang="ts">
import {computed,onBeforeUnmount,ref,shallowRef} from 'vue'
import {FolderOpen,File,X} from 'lucide-vue-next'
import type {AssetRef,AssetSettings,CompareSession,Language,Side} from '../contracts'
import {assetSides,browserAsset,chooseNativeAsset,displaySize,makeReader} from '../lib/asset-io'
import {isDesktop} from '../lib/bridge'
import {translate} from '../lib/i18n'
import {defaultAssetSettings} from '../lib/workspace'
import BinaryCompare from './BinaryCompare.vue'
import ImageCompare from './ImageCompare.vue'
import TableCompare from './TableCompare.vue'
import ArchiveCompare from './ArchiveCompare.vue'

const props=defineProps<{session:CompareSession;language:Language}>()
const emit=defineEmits<{'update-source':[side:Side,value:AssetRef|null];'update-settings':[patch:Partial<AssetSettings>]}>()
const t=(key:string,...args:(string|number)[])=>translate(props.language,key,...args)
const browserFiles=shallowRef<Partial<Record<Side,File>>>({})
const inputs:Partial<Record<Side,HTMLInputElement>>={}
const sourceError=ref(''),picking=ref(false)
let alive=true
const left=computed(()=>makeReader(props.session.assetSources?.left??null,browserFiles.value.left))
const right=computed(()=>makeReader(props.session.assetSources?.right??null,browserFiles.value.right))
const settings=computed(()=>({...defaultAssetSettings(),...props.session.assetSettings}))
const readerFor=(side:Side)=>side==='left'?left.value:right.value
const sourceFor=(side:Side)=>props.session.assetSources?.[side]??null
const accept=computed(()=>props.session.kind==='image'?'image/png,image/jpeg,image/webp,image/gif,image/bmp,image/avif,.ico':props.session.kind==='archive'?'.zip,.jar,.apk,.war,.tar,.tar.gz,.tgz,.gz':props.session.kind==='table'?'.csv,.tsv,.tab,.txt,.xlsx,.xls,.xlsm,.xlsb,.ods':'*')
function register(side:Side,value:unknown){if(value instanceof HTMLInputElement)inputs[side]=value}
function setSource(side:Side,value:AssetRef|null){
  emit('update-source',side,value)
  emit('update-settings',side==='left'?{leftSheet:'',binaryOffset:0}:{rightSheet:'',binaryOffset:0})
}
async function choose(side:Side){
  sourceError.value=''
  if(!isDesktop){inputs[side]?.click();return}
  if(picking.value)return
  picking.value=true
  try{const asset=await chooseNativeAsset();if(asset&&alive)setSource(side,asset)}
  catch(error){if(alive)sourceError.value=String(error)}
  finally{picking.value=false}
}
function onBrowserFile(side:Side,event:Event){
  const input=event.target as HTMLInputElement,file=input.files?.[0]
  input.value=''
  if(!file)return
  browserFiles.value={...browserFiles.value,[side]:file};setSource(side,browserAsset(file))
}
function clear(side:Side){
  const next={...browserFiles.value};delete next[side];browserFiles.value=next;setSource(side,null)
}
onBeforeUnmount(()=>{alive=false;browserFiles.value={}})
</script>
<template>
  <div class="asset-workspace">
    <div class="asset-source-bar">
      <div v-for="side in assetSides" :key="side" class="asset-source">
        <span class="side-mark">{{ t(side==='left'?'LEFT':'RIGHT') }}</span><File :size="19"/>
        <div class="asset-source-name">
          <strong :title="sourceFor(side)?.path||sourceFor(side)?.name">{{ sourceFor(side)?.name||t('Open file') }}</strong>
          <span v-if="sourceFor(side)&&!readerFor(side)" class="warning">{{ t('Open this file again to restore the comparison.') }}</span>
          <span v-else>{{ sourceFor(side)?displaySize(sourceFor(side)!.size):'—' }}</span>
        </div>
        <input v-if="!isDesktop" :ref="v=>register(side,v)" :data-asset-side="side" type="file" :accept="accept" hidden @change="onBrowserFile(side,$event)"/>
        <button class="btn" :disabled="picking" :title="t(side==='left'?'Open left file':'Open right file')" @click="choose(side)"><FolderOpen :size="15"/>{{ t('Open file') }}</button>
        <button v-if="sourceFor(side)" class="btn-icon" :title="t('Remove file')" @click="clear(side)"><X :size="15"/></button>
      </div>
    </div>
    <div v-if="sourceError" class="error-banner" role="alert"><span>{{ sourceError }}</span><button @click="sourceError=''"><X :size="15"/></button></div>
    <BinaryCompare v-if="session.kind==='binary'" :left="left" :right="right" :language="language" :settings="settings" @update-settings="emit('update-settings',$event)"/>
    <ImageCompare v-else-if="session.kind==='image'" :left="left" :right="right" :language="language" :settings="settings" @update-settings="emit('update-settings',$event)"/>
    <ArchiveCompare v-else-if="session.kind==='archive'" :left="left" :right="right" :language="language"/>
    <TableCompare v-else :left="left" :right="right" :language="language" :settings="settings" @update-settings="emit('update-settings',$event)"/>
  </div>
</template>
<style scoped>
.asset-workspace{height:100%;min-height:0;display:flex;flex-direction:column;overflow:hidden}
.asset-source-bar{display:grid;grid-template-columns:1fr 1fr;background:var(--panel);border-bottom:1px solid var(--border)}
.asset-source{min-width:0;display:flex;align-items:center;gap:9px;padding:10px 13px}.asset-source:first-child{border-right:1px solid var(--border)}
.side-mark{font-size:10px;font-weight:700;letter-spacing:.5px;color:var(--muted)}.asset-source>svg{color:var(--accent)}
.asset-source-name{min-width:0;flex:1;display:flex;flex-direction:column;gap:4px}.asset-source-name strong,.asset-source-name span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.asset-source-name span{font-size:11px;color:var(--muted)}.asset-source-name span.warning{color:var(--amber)}
@media(max-width:950px){.asset-source{gap:6px;padding:8px}.asset-source .btn{font-size:11px;padding:0 6px}.asset-source>.side-mark{display:none}}
</style>
