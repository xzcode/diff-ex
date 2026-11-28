import { chooseSavePath, confirmAction, isDesktop, writeText, downloadText } from './bridge'
import { invoke } from '@tauri-apps/api/core'

export function reportHtml(title:string,value:unknown):string{
  const escape=(s:string)=>s.replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]!))
  return '<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>'+escape(title)+'</title><style>body{font:14px system-ui;margin:32px;color:#20242a;background:#f8f9fb}pre{white-space:pre-wrap;overflow-wrap:anywhere;padding:20px;background:#fff;border:1px solid #d9dde4;border-radius:8px}h1{font-size:24px}</style><h1>'+escape(title)+'</h1><pre>'+escape(JSON.stringify(value,null,2))+'</pre></html>'
}
export async function exportReport(name:string,value:unknown,format:'json'|'html',confirmText:(path:string)=>string):Promise<boolean>{
  const filename=name+'.'+format
  const content=format==='html'?reportHtml(name,value):JSON.stringify(value,null,2)
  if(!isDesktop){
    downloadText({name:filename,path:null,fingerprint:null,content,encoding:'UTF-8',bom:false,lineEnding:'lf',byteLength:new TextEncoder().encode(content).length})
    return true
  }
  const path=await chooseSavePath(filename)
  if(!path)return false
  const fingerprint=await invoke<string|null>('inspect_save_target',{path})
  if(fingerprint&&!(await confirmAction(confirmText(path),'Save as')))return false
  await writeText({path,content,encoding:'UTF-8',bom:false,lineEnding:'lf',expectedFingerprint:fingerprint})
  return true
}
