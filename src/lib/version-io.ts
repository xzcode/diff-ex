import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import type { AssetRef,DocumentData,GitResult,GitSelection,RemoteRequest,RemoteResult,SshProfile,TextDocument } from '../contracts'

export async function browsePath(directory:boolean):Promise<string|null>{
  const path=await open({directory,multiple:false})
  return typeof path==='string'?path:null
}
export const gitRefs=(repository:string)=>invoke<string[]>('git_refs',{repository})
export const compareGit=(left:GitSelection,right:GitSelection,jobId:string)=>invoke<GitResult>('compare_git',{left,right,jobId})
export const readGit=(selection:GitSelection,path:string,fingerprint:string,encoding:string|null)=>invoke<TextDocument>('read_git',{selection,path,fingerprint,encoding})
export const probeSsh=(profile:SshProfile)=>invoke<string>('probe_ssh',{profile})
export const compareRemote=(left:RemoteRequest,right:RemoteRequest,jobId:string)=>invoke<RemoteResult>('compare_remote',{left,right,jobId})
export const readRemote=(request:RemoteRequest,path:string,fingerprint:string,encoding:string|null)=>invoke<TextDocument>('read_remote',{request,path,fingerprint,encoding})
export const readDocument=(asset:AssetRef,jobId:string)=>invoke<DocumentData>('read_document',{asset,jobId})
export const renderPdf=(asset:AssetRef,page:number,dpi:number,jobId:string)=>invoke<ArrayBuffer|number[]>('render_pdf_page',{asset,page,dpi,jobId})
