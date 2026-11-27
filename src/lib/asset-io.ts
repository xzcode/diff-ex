import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import { isDesktop } from './bridge'
import type { AssetRef, BinaryStats, Side, TableSheet } from '../contracts'

export const ASSET_CHUNK=1024*1024
export interface AssetReader {
  ref:AssetRef
  file?:File
  read(offset:number,length:number):Promise<Uint8Array>
}
export const browserAsset=(file:File):AssetRef=>({path:null,name:file.name,size:file.size,modified:String(file.lastModified)})
export async function chooseNativeAsset():Promise<AssetRef|null>{
  const path=await open({multiple:false,directory:false})
  return typeof path==='string'?invoke<AssetRef>('inspect_asset',{path}):null
}
export function makeReader(ref:AssetRef|null,file?:File):AssetReader|undefined{
  if(!ref||(!ref.path&&(!file||file.name!==ref.name||file.size!==ref.size||String(file.lastModified)!==ref.modified)))return
  if(ref.path&&!isDesktop)return
  return {ref,file:ref.path?undefined:file,async read(offset,length){
    if(!Number.isSafeInteger(offset)||offset<0||!Number.isSafeInteger(length)||length<0||length>ASSET_CHUNK)throw new Error('Invalid asset read range.')
    const at=Math.min(offset,ref.size),n=Math.min(length,ref.size-at)
    const raw=ref.path?await invoke<ArrayBuffer|number[]>('read_asset_chunk',{asset:ref,offset:at,length:n}):await file!.slice(at,at+n).arrayBuffer()
    const bytes=new Uint8Array(raw as ArrayBuffer)
    if(bytes.length!==n)throw new Error('The file changed while being read. Open it again.')
    return bytes
  }}
}
export async function readAssetBlob(reader:AssetReader,active:()=>boolean=()=>true):Promise<Blob>{
  if(reader.file)return reader.file
  const chunks:ArrayBuffer[]=[]
  if(!reader.ref.size)await reader.read(0,0)
  for(let offset=0;offset<reader.ref.size;offset+=ASSET_CHUNK){
    if(!active())throw new Error('Comparison cancelled.')
    chunks.push(Uint8Array.from(await reader.read(offset,ASSET_CHUNK)).buffer)
  }
  return new Blob(chunks)
}
export const emptyStats=():BinaryStats=>({compared:0,changed:0,leftOnly:0,rightOnly:0,firstDifference:null,lastDifference:null})
export function countBytes(stats:BinaryStats,left:Uint8Array,right:Uint8Array,offset:number){
  const length=Math.max(left.length,right.length)
  for(let i=0;i<length;i++){
    if(i>=left.length)stats.rightOnly++
    else if(i>=right.length)stats.leftOnly++
    else if(left[i]!==right[i])stats.changed++
    else continue
    stats.firstDifference??=offset+i
    stats.lastDifference=offset+i
  }
  stats.compared+=length
}
export async function scanBytes(left:AssetReader,right:AssetReader,jobId:string,active:()=>boolean,progress:(done:number)=>void):Promise<BinaryStats>{
  if(left.ref.path&&right.ref.path&&isDesktop)return invoke('compare_binary_assets',{left:left.ref,right:right.ref,jobId})
  const stats=emptyStats(),size=Math.max(left.ref.size,right.ref.size)
  for(let offset=0;offset<size;offset+=ASSET_CHUNK){
    if(!active())throw new Error('Comparison cancelled.')
    const [a,b]=await Promise.all([left.read(offset,ASSET_CHUNK),right.read(offset,ASSET_CHUNK)])
    countBytes(stats,a,b,offset)
    progress(stats.compared)
    await new Promise<void>(resolve=>setTimeout(resolve,0))
  }
  if(!active())throw new Error('Comparison cancelled.')
  return stats
}
export const workbookNames=(asset:AssetRef)=>invoke<string[]>('workbook_sheets',{asset})
export const workbookSheet=(asset:AssetRef,sheet:string)=>invoke<TableSheet>('workbook_sheet',{asset,sheet})
export const assetSides:Side[]=['left','right']
export function displaySize(size:number):string{
  return size<1024?size+' B':size<1048576?(size/1024).toFixed(1)+' KiB':size<1073741824?(size/1048576).toFixed(1)+' MiB':(size/1073741824).toFixed(2)+' GiB'
}
