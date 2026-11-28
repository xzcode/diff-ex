import { diffArrays } from 'diff'
import type { MergeDraft, MergeResolution, MergeSegment } from '../contracts'

// Keep line terminators attached to their original tokens. In particular, an
// empty file is not a single empty line and a final newline is significant.
export function splitExactLines(text: string): string[] {
  return text.match(/[^\r\n]*(?:\r\n|\r|\n)|[^\r\n]+$/g) || []
}
interface Edit { start: number; end: number; lines: string[]; side: 'left' | 'right' }
function edits(base: string[], version: string[], side: Edit['side']): Edit[] {
  const output: Edit[] = []
  let at = 0
  let pending: Edit | undefined
  const flush = () => { if (pending) output.push(pending); pending = undefined }
  for (const part of diffArrays(base, version)) {
    if (!part.added && !part.removed) { flush(); at += part.count; continue }
    pending ??= { start: at, end: at, lines: [], side }
    if (part.removed) { at += part.count; pending.end = at }
    else for (const line of part.value) pending.lines.push(line)
  }
  flush()
  return output
}
function apply(base: string[], start: number, end: number, changes: Edit[]): string {
  const result: string[] = []
  let at = start
  for (const edit of changes) {
    for (let i=at;i<edit.start;i++) result.push(base[i]!)
    for (const line of edit.lines) result.push(line)
    at = edit.end
  }
  for (let i=at;i<end;i++) result.push(base[i]!)
  return result.join('')
}

/** Merge independently aligned base-relative line edits. Intersecting edits,
 * including insertions on the boundaries of another edit, are treated as one
 * cluster. A cluster is automatically resolved only if both sides agree or
 * one side is exactly unchanged from base. Adjacent nonempty edits are
 * independent; concurrent unequal inserts are conflicts, never concatenated.
 */
export function planThreeWay(baseText: string, leftText: string, rightText: string): MergeSegment[] {
  const base = splitExactLines(baseText)
  const all = [...edits(base, splitExactLines(leftText), 'left'), ...edits(base, splitExactLines(rightText), 'right')]
    .sort((a,b) => a.start-b.start || a.end-b.end || a.side.localeCompare(b.side))
  const groups: Edit[][] = []
  let end = -1, insertionAtEnd = false
  for (const edit of all) {
    const insert = edit.start === edit.end
    const joins = groups.length > 0 && (edit.start < end || (edit.start === end && (insert || insertionAtEnd)))
    if (!joins) {
      groups.push([edit]); end = edit.end; insertionAtEnd = insert
    } else {
      groups[groups.length - 1]!.push(edit)
      if (edit.end > end) { end = edit.end; insertionAtEnd = insert }
      else if (edit.end === end && insert) insertionAtEnd = true
    }
  }
  const segments: MergeSegment[] = []
  let cursor = 0
  for (const group of groups) {
    const start = group[0]!.start
    const finish = group.reduce((n,e)=>Math.max(n,e.end),start)
    if (cursor < start) {
      const text = base.slice(cursor,start).join('')
      segments.push({id:'same:'+cursor,start:cursor,end:start,kind:'same',base:text,left:text,right:text,text})
    }
    const original = base.slice(start,finish).join('')
    const left = apply(base,start,finish,group.filter(e=>e.side==='left'))
    const right = apply(base,start,finish,group.filter(e=>e.side==='right'))
    const kind = left === right ? 'both' : left === original ? 'right' : right === original ? 'left' : 'conflict'
    segments.push({id:'edit:'+start+':'+finish,start,end:finish,kind,base:original,left,right,text:kind==='conflict'?'':kind==='right'?right:left})
    cursor = finish
  }
  if (cursor < base.length) {
    const text = base.slice(cursor).join('')
    segments.push({id:'same:'+cursor,start:cursor,end:base.length,kind:'same',base:text,left:text,right:text,text})
  }
  return segments
}
export function mergeText(segments: MergeSegment[], resolutions: Record<string,MergeResolution>): string | null {
  const chunks: string[] = []
  for (const segment of segments) {
    if (segment.kind !== 'conflict') { chunks.push(segment.text); continue }
    const choice = Object.hasOwn(resolutions,segment.id) ? resolutions[segment.id] : undefined
    if (!choice) return null
    chunks.push(choice.choice==='manual'?choice.text:segment[choice.choice])
  }
  return chunks.join('')
}
export function mergeReady(draft: MergeDraft | undefined, left: string, right: string): boolean {
  try { return !!draft?.prepared && mergeText(planThreeWay(draft.base.content,left,right),draft.resolutions) !== null }
  catch { return false }
}
