import type { Side } from '../contracts'

export interface RawLineChange {
  originalStartLineNumber: number
  originalEndLineNumber: number
  modifiedStartLineNumber: number
  modifiedEndLineNumber: number
}

// Monaco represents an insertion with endLineNumber=0 and startLineNumber equal
// to the preceding line. Otherwise its line coordinates are one-based inclusive.
function span(start: number, end: number): [number, number] {
  return end === 0 ? [start, start] : [start - 1, end]
}

export function mergeLineChange(left: string, right: string, change: RawLineChange, target: Side): string {
  const source = target === 'left' ? right : left
  const destination = target === 'left' ? left : right
  const sourceRange = target === 'left'
    ? span(change.modifiedStartLineNumber, change.modifiedEndLineNumber)
    : span(change.originalStartLineNumber, change.originalEndLineNumber)
  const destinationRange = target === 'left'
    ? span(change.originalStartLineNumber, change.originalEndLineNumber)
    : span(change.modifiedStartLineNumber, change.modifiedEndLineNumber)
  const sourceLines = source === '' ? [] : source.split(/\r\n|\r|\n/)
  const destinationLines = destination === '' ? [] : destination.split(/\r\n|\r|\n/)
  const merged = destinationLines.slice(0, destinationRange[0]).concat(
    sourceLines.slice(sourceRange[0], sourceRange[1]),
    destinationLines.slice(destinationRange[1]),
  )
  const eol = destination.includes('\r\n') ? '\r\n' : destination.includes('\r') && !destination.includes('\n') ? '\r' : '\n'
  return merged.join(eol)
}
