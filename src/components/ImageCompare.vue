<script setup lang="ts">
import {computed,nextTick,onBeforeUnmount,ref,shallowRef,watch} from 'vue'
import {ImageIcon,Layers,SplitSquareHorizontal,Columns2,ScanLine} from 'lucide-vue-next'
import type {AssetSettings,Language} from '../contracts'
import type {AssetReader} from '../lib/asset-io'
import {compareRaster,decodeRaster,rasterCanvas,type Raster,type ImageStats} from '../lib/image-diff'
import {translate} from '../lib/i18n'
const props=defineProps<{left?:AssetReader;right?:AssetReader;language:Language;settings:AssetSettings}>()
const emit=defineEmits<{'update-settings':[patch:Partial<AssetSettings>]}>()
const t=(key:string,...args:(string|number)[])=>translate(props.language,key,...args)
const a=shallowRef<Raster>(),b=shallowRef<Raster>()
const leftCanvas=ref<HTMLCanvasElement>(),rightCanvas=ref<HTMLCanvasElement>(),composite=ref<HTMLCanvasElement>()
const stats=shallowRef<ImageStats>(),loading=ref(false),error=ref(''),zoom=ref('fit')
let generation=0,paintGeneration=0,sourceA:HTMLCanvasElement|undefined,sourceB:HTMLCanvasElement|undefined
const modes=[{value:'side',key:'Side by side',icon:Columns2},{value:'overlay',key:'Overlay',icon:Layers},{value:'split',key:'Split view',icon:SplitSquareHorizontal},{value:'heat',key:'Heatmap',icon:ScanLine}] as const
const mode=computed(()=>props.settings.imageMode),width=computed(()=>Math.max(a.value?.width||0,b.value?.width||0)),height=computed(()=>Math.max(a.value?.height||0,b.value?.height||0))
const differenceCount=computed(()=>stats.value?stats.value.changed+stats.value.leftOnly+stats.value.rightOnly:0)
function draw(target:HTMLCanvasElement|undefined,source:HTMLCanvasElement|undefined){if(!target||!source)return;target.width=source.width;target.height=source.height;const ctx=target.getContext('2d');if(!ctx)throw new Error('Image canvas allocation failed.');ctx.drawImage(source,0,0)}
async function paint(){
  const id=++paintGeneration;await nextTick();if(id!==paintGeneration)return
  try{
    if(mode.value==='side'){draw(leftCanvas.value,sourceA);draw(rightCanvas.value,sourceB);return}
    const target=composite.value;if(!target||!sourceA||!sourceB)return
    target.width=width.value;target.height=height.value
    const ctx=target.getContext('2d');if(!ctx)throw new Error('Image canvas allocation failed.')
    if(mode.value==='heat'){if(stats.value)ctx.putImageData(new ImageData(new Uint8ClampedArray(stats.value.heat),stats.value.width,stats.value.height),0,0);return}
    if(mode.value==='overlay'){const mix=props.settings.imageMix/100;ctx.globalAlpha=1-mix;ctx.drawImage(sourceA,0,0);ctx.globalCompositeOperation='lighter';ctx.globalAlpha=mix;ctx.drawImage(sourceB,0,0);ctx.globalAlpha=1;ctx.globalCompositeOperation='source-over'}
    else{const x=Math.round(width.value*props.settings.imageMix/100);ctx.save();ctx.beginPath();ctx.rect(0,0,x,height.value);ctx.clip();ctx.drawImage(sourceA,0,0);ctx.restore();ctx.save();ctx.beginPath();ctx.rect(x,0,width.value-x,height.value);ctx.clip();ctx.drawImage(sourceB,0,0);ctx.restore()}
  }catch(e){error.value=String(e)}
}
function compare(){stats.value=undefined;if(a.value&&b.value){try{stats.value=compareRaster(a.value,b.value,props.settings.imageTolerance)}catch(e){error.value=String(e)}}void paint()}
async function load(){
  const id=++generation;paintGeneration++;error.value='';loading.value=true;stats.value=undefined;a.value=undefined;b.value=undefined;sourceA=undefined;sourceB=undefined
  const active=()=>id===generation
  try{const [l,r]=await Promise.all([props.left?decodeRaster(props.left,active):undefined,props.right?decodeRaster(props.right,active):undefined]);if(!active())return;a.value=l;b.value=r;if(l)sourceA=rasterCanvas(l);if(r)sourceB=rasterCanvas(r);compare()}
  catch(e){if(active())error.value=String(e)}
  finally{if(active()){loading.value=false;void paint()}}
}
watch([()=>props.left,()=>props.right],load,{immediate:true})
watch(()=>props.settings.imageTolerance,compare)
watch([mode,()=>props.settings.imageMix],()=>void paint())
onBeforeUnmount(()=>{generation++;paintGeneration++;sourceA=undefined;sourceB=undefined})
</script>
<template>
  <section class="asset-module image-workspace">
    <div class="panel-toolbar asset-tools">
      <div class="mode-switch"><button v-for="m in modes" :key="m.value" class="btn" :class="{active:mode===m.value}" :aria-pressed="mode===m.value" @click="emit('update-settings',{imageMode:m.value})"><component :is="m.icon" :size="15"/>{{ t(m.key) }}</button></div>
      <label>{{ t('Tolerance') }} <input type="range" min="0" max="255" step="1" :aria-label="t('Tolerance')" :value="settings.imageTolerance" @change="emit('update-settings',{imageTolerance:Number(($event.target as HTMLInputElement).value)})"/> <code>{{ settings.imageTolerance }}</code></label>
      <label v-if="mode==='overlay'||mode==='split'">{{ t(mode==='overlay'?'Opacity':'Split position') }} <input type="range" min="0" max="100" :value="settings.imageMix" @input="emit('update-settings',{imageMix:Number(($event.target as HTMLInputElement).value)})"/> {{ settings.imageMix }}%</label>
      <label>{{ t('Zoom') }} <select v-model="zoom"><option value="fit">{{ t('Fit') }}</option><option value="actual">100%</option><option value="double">200%</option></select></label>
    </div>
    <div class="notice">{{ t('Decoded raster pixels, first frame only; size differences count as unmatched pixels.') }}</div>
    <div v-if="error" class="error-banner" role="alert">{{ error }}</div>
    <div v-if="loading" class="asset-empty">{{ t('Comparing…') }}</div><div v-else-if="!a&&!b" class="asset-empty"><ImageIcon :size="32"/>{{ t('Open two files to compare.') }}</div>
    <div v-else-if="mode==='side'" class="image-panes" :class="'zoom-'+zoom">
      <div class="image-pane"><header>{{ t('LEFT') }} <span v-if="a">{{ a.width }} × {{ a.height }}</span></header><div class="image-stage"><canvas v-if="a" ref="leftCanvas" :style="zoom==='double'?{width:(a.width*2)+'px'}:{}"/><span v-else class="muted">{{ t('Absent') }}</span></div></div>
      <div class="image-pane"><header>{{ t('RIGHT') }} <span v-if="b">{{ b.width }} × {{ b.height }}</span></header><div class="image-stage"><canvas v-if="b" ref="rightCanvas" :style="zoom==='double'?{width:(b.width*2)+'px'}:{}"/><span v-else class="muted">{{ t('Absent') }}</span></div></div>
    </div>
    <div v-else-if="a&&b" class="image-single" :class="'zoom-'+zoom"><div class="image-stage"><canvas ref="composite" :style="zoom==='double'?{width:(width*2)+'px'}:{}"/></div></div><div v-else class="asset-empty">{{ t('Open two files to compare.') }}</div>
    <footer class="asset-status"><span v-if="stats" :class="differenceCount?'warning':'positive'">{{ differenceCount?t('Pixel differences')+': '+differenceCount.toLocaleString(language):t('Equal under current rules') }}</span><span v-if="stats">{{ t('Changed') }}: {{ stats.changed.toLocaleString(language) }} · {{ t('Left only') }}: {{ stats.leftOnly.toLocaleString(language) }} · {{ t('Right only') }}: {{ stats.rightOnly.toLocaleString(language) }}</span><span v-if="stats">{{ t('Maximum channel difference') }}: {{ stats.maxDelta }}</span></footer>
  </section>
</template>
<style scoped>
.mode-switch{display:flex;gap:3px;flex-wrap:wrap}.mode-switch .btn.active{background:color-mix(in srgb,var(--accent) 16%,var(--panel));color:var(--accent);border-color:var(--accent)}.asset-tools input[type=range]{padding:0;width:100px;min-height:20px;accent-color:var(--accent);border:0;box-shadow:none;background:transparent}
.image-panes{display:grid;grid-template-columns:1fr 1fr;flex:1;min-height:0;overflow:hidden}.image-pane{display:flex;flex-direction:column;min-width:0;min-height:0}.image-pane:first-child{border-right:1px solid var(--border)}.image-pane header{display:flex;justify-content:space-between;padding:8px 12px;background:var(--panel-alt);color:var(--muted);font-size:11px;font-weight:600;border-bottom:1px solid var(--border)}
.image-single{flex:1;min-height:0;overflow:hidden;display:flex}.image-stage{display:flex;align-items:center;justify-content:center;flex:1;min-width:0;min-height:0;overflow:auto;padding:16px;background:repeating-conic-gradient(var(--panel-alt) 0% 25%,var(--bg) 0% 50%) 50% / 20px 20px}.image-stage canvas{display:block;object-fit:contain;flex-shrink:0;image-rendering:pixelated;box-shadow:0 1px 12px #0003}.zoom-fit .image-stage canvas{max-width:100%;max-height:100%;image-rendering:auto}.zoom-actual .image-stage,.zoom-double .image-stage{display:block}.zoom-actual canvas,.zoom-double canvas{margin:auto}
</style>
