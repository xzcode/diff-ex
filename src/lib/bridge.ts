import { ref } from 'vue'
import { invoke, isTauri } from '@tauri-apps/api/core'
import { open, save, ask } from '@tauri-apps/plugin-dialog'
import type { TextDocument, SaveTextRequest, TextOptions, TextDiffResult, StructuredOptions, StructuredResult, DirectoryOptions, DirectoryResult, SyncMode, SyncPlan, SyncResult } from '../contracts'
import { previewText, previewStructured } from './preview'

export const isDesktop = isTauri()
export const activeSyncJobs = ref(0)
export const compareText = (left: string, right: string, options: TextOptions): Promise<TextDiffResult> =>
  isDesktop ? invoke('compare_texts', { left, right, options }) : Promise.resolve().then(() => previewText(left, right, options))
export const compareStructured = (left: string, right: string, options: StructuredOptions): Promise<StructuredResult> =>
  isDesktop ? invoke('compare_structures', { left, right, options }) : Promise.resolve().then(() => previewStructured(left, right, options))
export const readText = (path: string, encoding?: string): Promise<TextDocument> => invoke('read_text_file', { path, encoding: encoding || null })
export const writeText = (request: SaveTextRequest): Promise<TextDocument> => invoke('save_text_file', { request })
export const compareFolders = (leftRoot: string, rightRoot: string, options: DirectoryOptions, jobId: string): Promise<DirectoryResult> =>
  invoke('compare_folders', { leftRoot, rightRoot, options, jobId })
export const previewSync = (leftRoot: string, rightRoot: string, options: DirectoryOptions, mode: SyncMode, jobId: string): Promise<SyncPlan> =>
  invoke('preview_sync', { leftRoot, rightRoot, options, mode, jobId })
export const executeSync = async (planId: string, jobId: string): Promise<SyncResult> => {
  activeSyncJobs.value++
  try { return await invoke('execute_sync', { planId, jobId }) }
  finally { activeSyncJobs.value-- }
}
export const discardSync = (planId: string): Promise<void> => invoke('discard_sync', { planId })
export const cancelJob = (jobId: string): Promise<boolean> => invoke('cancel_job', { jobId })
export const loadWorkspaceRaw = (): Promise<string | null> => isDesktop ? invoke('load_workspace') : Promise.resolve(localStorage.getItem('diff-ex.workspace.v1'))
export const saveWorkspaceRaw = async (data: string): Promise<void> => {
  if (isDesktop) await invoke('save_workspace', { data })
  else localStorage.setItem('diff-ex.workspace.v1', data)
}
export const confirmAction = (message: string, title: string): Promise<boolean> =>
  isDesktop ? ask(message, { title, kind: 'warning' }) : Promise.resolve(window.confirm(message))
export const chooseDirectory = (): Promise<string | null> => open({ directory: true, multiple: false }).then(p => typeof p === 'string' ? p : null)
export const chooseSavePath = (defaultPath: string): Promise<string | null> => save({ defaultPath })
export async function chooseText(encoding?: string): Promise<TextDocument | null> {
  if (isDesktop) {
    const path = await open({ multiple: false, directory: false })
    return typeof path === 'string' ? readText(path, encoding) : null
  }
  const file = await new Promise<File | null>(resolve => {
    const input = document.createElement('input')
    input.type = 'file'
    input.onchange = () => { resolve(input.files?.[0] || null); input.remove() }
    input.oncancel = () => { resolve(null); input.remove() }
    input.hidden = true
    document.body.append(input)
    input.click()
  })
  return file ? browserDocument(file, encoding) : null
}
export async function browserDocument(file: File, encoding?: string): Promise<TextDocument> {
  const bytes = new Uint8Array(await file.arrayBuffer())
  const utf8Bom = bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf
  const le = bytes[0] === 0xff && bytes[1] === 0xfe
  const be = bytes[0] === 0xfe && bytes[1] === 0xff
  const detected = encoding || (le ? 'UTF-16LE' : be ? 'UTF-16BE' : 'UTF-8')
  const normalized = detected.toLowerCase().replace(/-/g, '')
  if ((utf8Bom && normalized !== 'utf8') || (le && normalized !== 'utf16le') || (be && normalized !== 'utf16be')) throw new Error('Selected encoding conflicts with the file BOM. Open with automatic detection or the matching encoding.')
  const content = new TextDecoder(detected, { fatal: true }).decode(bytes)
  if (content.includes('\0')) throw new Error('The file contains NUL characters and may be binary. Open it in Binary compare instead.')
  const endings = new Set((content.match(/\r\n|\r|\n/g) || []).map(v => v === '\r\n' ? 'crlf' : v === '\r' ? 'cr' : 'lf'))
  return { path: null, name: file.name, content, encoding: detected, bom: utf8Bom || le || be,
    lineEnding: endings.size > 1 ? 'mixed' : (Array.from(endings)[0] as TextDocument['lineEnding']) || 'none',
    fingerprint: null, byteLength: bytes.byteLength }
}
export function downloadText(doc: TextDocument): void {
  if (!/^utf-?8$/i.test(doc.encoding)) throw new Error('The browser preview exports UTF-8. Use the desktop app to save other encodings.')
  let text = doc.content
  if (doc.lineEnding === 'crlf') text = text.replace(/\r\n|\r|\n/g, '\n').replace(/\n/g, '\r\n')
  if (doc.lineEnding === 'lf') text = text.replace(/\r\n|\r/g, '\n')
  if (doc.lineEnding === 'cr') text = text.replace(/\r\n|\n/g, '\r')
  const blob = new Blob([doc.bom ? '\ufeff' : '', text], { type: 'text/plain;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = doc.name || 'untitled.txt'
  anchor.click()
  setTimeout(() => URL.revokeObjectURL(url), 1000)
}
