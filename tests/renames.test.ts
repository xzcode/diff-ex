import { describe, expect, it } from 'vitest'
import { findRenameCandidates } from '../src/lib/renames'
import type { DirectoryEntry, FileInfo } from '../src/contracts'
function entry(path: string, side: 'left' | 'right', fingerprint: string | null, kind: FileInfo['kind'] = 'file', size = 123): DirectoryEntry {
  const file = { kind, size, fingerprint, modifiedMs: null }
  return { relativePath: path, status: side === 'left' ? 'leftOnly' : 'rightOnly', left: side === 'left' ? file : null, right: side === 'right' ? file : null, error: null }
}
describe('possible move detection', () => {
  it('matches unique hashes across different relative paths', () => {
    const left = entry('old/a.txt','left','hash'), right = entry('new/a.txt','right','hash')
    expect(findRenameCandidates([right,left])).toEqual([{left,right}])
  })
  it('never guesses among ambiguous copies, metadata-only entries, links or differing sizes', () => {
    expect(findRenameCandidates([
      entry('a','left','hash'), entry('b','left','hash'), entry('c','right','hash'),
      entry('d','left',null), entry('e','right',null),
      entry('f','left','link','symlink'), entry('g','right','link','symlink'),
      entry('h','left','other','file',1), entry('i','right','other','file',2),
    ])).toEqual([])
  })
})
