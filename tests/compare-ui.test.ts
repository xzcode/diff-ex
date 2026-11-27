import { describe, expect, it } from 'vitest'
// @ts-expect-error Monaco's internal implementation is executable JS without public declarations.
import { DefaultLinesDiffComputer } from 'monaco-editor/esm/vs/editor/common/diff/defaultLinesDiffComputer/defaultLinesDiffComputer.js'
import { mergeLineChange, type RawLineChange } from '../src/lib/compare-ui-merge'
import type { Side } from '../src/contracts'

interface LineRange { startLineNumber: number; endLineNumberExclusive: number }
interface Mapping { original: LineRange; modified: LineRange }
function changes(left: string, right: string): RawLineChange[] {
  const result = new DefaultLinesDiffComputer().computeDiff(left.split(/\r\n|\r|\n/), right.split(/\r\n|\r|\n/), {
    ignoreTrimWhitespace: false, maxComputationTimeMs: 0, computeMoves: false,
  })
  // This is Monaco's public ILineChange convention, translated from its actual
  // computation rather than guessed test hunk coordinates.
  return result.changes.map((mapping: Mapping) => ({
    originalStartLineNumber: mapping.original.startLineNumber - (mapping.original.startLineNumber === mapping.original.endLineNumberExclusive ? 1 : 0),
    originalEndLineNumber: mapping.original.startLineNumber === mapping.original.endLineNumberExclusive ? 0 : mapping.original.endLineNumberExclusive - 1,
    modifiedStartLineNumber: mapping.modified.startLineNumber - (mapping.modified.startLineNumber === mapping.modified.endLineNumberExclusive ? 1 : 0),
    modifiedEndLineNumber: mapping.modified.startLineNumber === mapping.modified.endLineNumberExclusive ? 0 : mapping.modified.endLineNumberExclusive - 1,
  }))
}
function mergeAll(left: string, right: string, target: Side): string {
  let hunks = changes(left, right)
  const maximumSteps = hunks.length + 1
  let steps = 0
  while (hunks.length) {
    if (++steps > maximumSteps) throw new Error('Merging failed to converge')
    const next = mergeLineChange(left, right, hunks[0]!, target)
    if (target === 'left') left = next
    else right = next
    hunks = changes(left, right)
  }
  return target === 'left' ? left : right
}

describe('raw Monaco hunk merge', () => {
  const fixtures = [
    ['', 'hello'], ['', 'hello\n'], ['hello', ''], ['\n', ''], ['\n', '\n\n'],
    ['first\nlast', 'first\ninserted\nlast'], ['a', 'a\nb'], ['a\nb', 'a'],
    ['a\nb\n', 'a\nb'], ['a\nb', 'a\nb\n'],
    ['start\n\nkeep\nold\nend\n', 'new\nstart\n\nkeep\nchanged\nend\nextra'],
    ['行一\r\n行二\r\n', '第一行\r\n行一\r\n行二\r\n最后\r\n'],
  ]
  it.each(fixtures)('reconstructs either side without losing boundary lines: %j → %j', (left, right) => {
    expect(mergeAll(left!, right!, 'left').replaceAll('\r\n', '\n')).toBe(right!.replaceAll('\r\n', '\n'))
    expect(mergeAll(left!, right!, 'right').replaceAll('\r\n', '\n')).toBe(left!.replaceAll('\r\n', '\n'))
  })
  it('preserves untouched lines when only one separated hunk is merged', () => {
    const middle = Array.from({ length: 12 }, (_, n) => `unchanged line ${n}`).join('\n')
    const left = `old heading\n${middle}\nold tail`
    const right = `new heading\n${middle}\nnew tail`
    const hunks = changes(left, right)
    expect(hunks).toHaveLength(2)
    expect(mergeLineChange(left, right, hunks[0]!, 'left')).toBe(`new heading\n${middle}\nold tail`)
  })
  it('merges a large insertion without spread-argument limits', () => {
    const right = Array.from({ length: 150_000 }, (_, i) => `line ${i}`).join('\n')
    expect(mergeLineChange('', right, { originalStartLineNumber: 1, originalEndLineNumber: 1, modifiedStartLineNumber: 1, modifiedEndLineNumber: 150_000 }, 'left')).toBe(right)
  })
})
